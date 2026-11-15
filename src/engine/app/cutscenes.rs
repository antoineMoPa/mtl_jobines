use super::super::fabula::{self, Anchor, Cue};
use super::*;
use crate::kairos::MissionId;

#[derive(Clone, Copy)]
pub(super) enum AfterCutscene {
    Help,
    ActivateMission(MissionId),
    ServiceFinished { bus_circuit: bool },
    Resume,
}
impl Game {
    pub(super) fn replay_intro(&mut self) {
        self.intro_played = true;
        if !self.begin_cutscene(Cue::GameIntro, AfterCutscene::Help) {
            self.help_visible = true;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    pub(super) fn start_playing(&mut self) {
        self.audio.user_gesture();
        if !self.intro_played && !self.kairos.intro_viewed() {
            self.intro_played = true;
            if !self.begin_cutscene(Cue::GameIntro, AfterCutscene::Help) {
                self.help_visible = true;
            }
        } else {
            self.set_overlay(Overlay::Driving);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    pub(super) fn begin_cutscene(&mut self, cue: Cue, after: AfterCutscene) -> bool {
        if self.fabula.active() {
            return false;
        }
        let Some(scene) = fabula::scene_for(cue) else {
            return false;
        };
        self.audio.user_gesture();
        self.audio.load_cutscene(
            scene.voice.clone(),
            scene.definition.voice_start,
            scene.definition.duration,
            self.sound_settings.master,
        );
        self.fabula.play(
            scene,
            Anchor {
                position: self.world.player_position(),
                yaw: self.world.player_yaw(),
            },
        );
        self.after_cutscene = Some(after);
        if let Some(palette) = &mut self.palette {
            palette.dismiss();
        }
        self.help_visible = false;
        self.keys.clear();
        self.dragging = false;
        self.accumulator = 0.;
        self.sound_eye = None;
        self.last = Instant::now();
        self.scene_dirty = true;
        self.show_cursor();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        true
    }
    pub(super) fn cutscene_can_play(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if web_sys::window()
            .and_then(|w| w.document())
            .is_some_and(|d| d.hidden())
        {
            return false;
        }
        self.fabula.active() && self.cutscene_focused && !self.hidden && !self.minimized
    }
    pub(super) fn finish_cutscene(&mut self, continue_game: bool) {
        self.audio.stop_cutscene();
        self.fabula.clear();
        self.scene_dirty = true;
        self.keys.clear();
        self.dragging = false;
        self.cursor = None;
        self.accumulator = 0.;
        self.last = Instant::now();
        self.sound_eye = None;
        if let Some(renderer) = &mut self.renderer {
            renderer.cinematic = false;
        }
        let after = self.after_cutscene.take();
        if continue_game {
            match after {
                Some(AfterCutscene::Help) => {
                    self.kairos.mark_intro_viewed();
                    self.overlay = Overlay::Start;
                    self.help_visible = true;
                }
                Some(AfterCutscene::ActivateMission(id)) => self.activate_mission_now(id),
                Some(AfterCutscene::ServiceFinished { bus_circuit }) => {
                    self.finish_service_transition(bus_circuit)
                }
                Some(AfterCutscene::Resume) | None => {}
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
    /// Service completion is a one-shot event, including each repeating bus circuit.
    pub(super) fn service_completed(&mut self) {
        let id = self.kairos.active_id();
        let bus_circuit = id == Some(MissionId::Bus)
            && self.kairos.target() == Some(crate::kairos::SiteId::BusStop3);
        self.kairos.service_finished();
        let completed = bus_circuit || self.kairos.is_complete();
        self.kairos.save_now();
        if completed
            && id.is_some_and(|id| {
                self.begin_cutscene(
                    Cue::AfterMission(id),
                    AfterCutscene::ServiceFinished { bus_circuit },
                )
            })
        {
            return;
        }
        self.finish_service_transition(bus_circuit);
    }
    pub(super) fn finish_service_transition(&mut self, bus_circuit: bool) {
        if bus_circuit {
            self.restart_bus_passengers();
        }
        if self.kairos.active_id() == Some(MissionId::Taxi) && self.kairos.is_complete() {
            self.start_next_taxi_run();
        } else {
            self.refresh_mission_route();
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn intro_runs_once_and_can_still_be_replayed() {
        let mut game = Game::new(Hermes::test_map());
        game.start_playing();
        assert!(game.fabula.active());
        assert!(!game.kairos.intro_viewed());
        game.finish_cutscene(true);
        assert!(game.kairos.intro_viewed());

        // A fresh app session has no in-memory playback flag.
        game.intro_played = false;
        game.help_visible = false;
        game.start_playing();
        assert!(!game.fabula.active());
        assert_eq!(game.overlay, Overlay::Driving);
        game.replay_intro();
        assert!(game.fabula.active());
    }

    #[test]
    fn fabula_skip_restores_loaded_world_then_opens_help() {
        let mut game = Game::new(Hermes::test_map());
        let position = game.world.player_position();
        let camera = game.eye;
        let city = game.hermes.city() as *const _;
        game.fabula.play(
            fabula::scene_for(Cue::GameIntro).unwrap(),
            Anchor {
                position,
                yaw: game.world.player_yaw(),
            },
        );
        game.after_cutscene = Some(AfterCutscene::Help);
        game.keys.insert(KeyCode::ArrowUp);
        game.simulate(10.);
        assert_eq!(game.world.player_position(), position);
        game.finish_cutscene(true);
        assert!(!game.fabula.active());
        assert!(game.help_visible);
        assert!(game.keys.is_empty());
        assert_eq!(game.eye, camera);
        assert_eq!(game.hermes.city() as *const _, city);
        assert_eq!(game.world.player_position(), position);
        assert!(game.scene_dirty);
    }
    #[test]
    fn fabula_cancel_discards_followup_and_focus_loss_preserves_scene() {
        let mut game = Game::new(Hermes::test_map());
        game.overlay = Overlay::Driving;
        game.fabula.play(
            fabula::scene_for(Cue::GameIntro).unwrap(),
            Anchor {
                position: game.world.player_position(),
                yaw: 0.,
            },
        );
        game.after_cutscene = Some(AfterCutscene::Help);
        game.window_focus_lost();
        assert!(game.fabula.active());
        assert!(!game.cutscene_can_play());
        assert_eq!(game.overlay, Overlay::Driving);
        game.finish_cutscene(false);
        assert!(!game.kairos.intro_viewed());
        assert!(!game.help_visible);
        assert!(game.after_cutscene.is_none());
    }
}
