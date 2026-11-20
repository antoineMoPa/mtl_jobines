use super::*;
use crate::kairos::{MissionId, SiteId};

const LARGE_VEHICLE_CHASE_ZOOM: f32 = 0.9;
const BUS_PASSENGERS: &[(u32, SiteId)] = &[
    (1, SiteId::BusStop1),
    (2, SiteId::BusStop1),
    (3, SiteId::BusStop2),
];

impl Game {
    pub(super) fn abandon_mission(&mut self) {
        let winter = self.kairos.active_id() == Some(MissionId::Snowplow);
        self.kairos.abandon();
        self.mission_vehicle = None;
        self.world.set_zombie_mode(self.hermes.city(), false);
        self.world.clear_service_passengers();
        self.hermes.clear_destination();
        self.hermes.clear_taxi_run();
        if winter {
            self.change_season(crate::season::Season::Summer);
        }
        self.scene_dirty = true;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    pub(super) fn change_season(&mut self, season: crate::season::Season) {
        if self
            .kairos
            .active_id()
            .and_then(MissionId::required_season)
            .is_some_and(|required| required != season)
        {
            self.kairos.abandon();
            self.mission_vehicle = None;
            self.world.clear_service_passengers();
            self.hermes.clear_destination();
            self.hermes.clear_taxi_run();
        }
        self.world.set_season(&self.hermes, season);
        if let Some(renderer) = &mut self.renderer {
            renderer.season = season;
        }
        self.scene_dirty = true;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    pub(super) fn activate_mission(&mut self, id: MissionId) {
        if !self.begin_cutscene(
            super::super::fabula::Cue::BeforeMission(id),
            AfterCutscene::ActivateMission(id),
        ) {
            self.activate_mission_now(id);
        }
    }
    pub(super) fn activate_mission_now(&mut self, id: MissionId) {
        if id == MissionId::Taxi {
            let player = self.world.player_position() / METERS_PER_UNIT;
            if !self.hermes.choose_taxi_run(player) {
                eprintln!("Taxi route unavailable on the current road network");
                return;
            }
        }
        let required: &[(u32, SiteId)] = match id {
            MissionId::Taxi => &[(1, SiteId::TaxiPickup)],
            MissionId::Bus => BUS_PASSENGERS,
            MissionId::Snowplow | MissionId::Zombie => &[],
        };
        let Some(passengers) = required
            .iter()
            .map(|(id, site)| {
                self.hermes
                    .mission_waiting_point(*site)
                    .map(|point| (*id, point))
            })
            .collect::<Option<Vec<_>>>()
        else {
            eprintln!("Mission waiting point unavailable: {}", id.title());
            return;
        };
        let destinations: &[SiteId] = match id {
            MissionId::Taxi => &[SiteId::TaxiPickup, SiteId::TaxiDropoff],
            MissionId::Bus => &[SiteId::BusStop1, SiteId::BusStop2, SiteId::BusStop3],
            MissionId::Snowplow | MissionId::Zombie => &[],
        };
        if destinations
            .iter()
            .any(|site| self.hermes.mission_site(*site).is_none())
        {
            eprintln!("Mission destination unavailable: {}", id.title());
            return;
        }
        if let Some(vehicle_id) = id.vehicle_id() {
            if !self.assign_mission_vehicle(id, vehicle_id) {
                return;
            }
            self.mission_vehicle = Some(self.world.player_vehicle_id());
        } else {
            self.mission_vehicle = None;
        }
        let continuing_snow =
            id == MissionId::Snowplow && self.kairos.active_id() == Some(MissionId::Snowplow);
        if !continuing_snow {
            self.kairos.accept(id);
        }
        self.world.clear_service_passengers();
        for (index, (actor_id, point)) in passengers.into_iter().enumerate() {
            self.world
                .spawn_service_passenger(actor_id, point + Vec3::X * (index as f32 * 0.6));
        }
        if id == MissionId::Snowplow {
            self.finish_location_change();
            self.zoom = LARGE_VEHICLE_CHASE_ZOOM;
        }
        if id != MissionId::Taxi {
            self.hermes.clear_taxi_run();
        }
        self.world
            .set_zombie_mode(self.hermes.city(), id == MissionId::Zombie);
        if let Some(season) = id.required_season() {
            self.change_season(season);
        }
        self.refresh_mission_route();
        self.keys.clear();
        self.accumulator = 0.;
        self.last = None;
        self.scene_dirty = true;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn assign_mission_vehicle(&mut self, id: MissionId, vehicle_id: &str) -> bool {
        let Some(model) = crate::dynamic_world::catalog::definitions()
            .iter()
            .position(|definition| definition.config.research_id == vehicle_id)
        else {
            eprintln!("Mission vehicle unavailable: {}", vehicle_id);
            return false;
        };
        if id == MissionId::Bus {
            let stop = self.hermes.mission_site(SiteId::BusStop1).unwrap();
            let previous = self.world.player_model_id().and_then(|research_id| {
                crate::dynamic_world::catalog::definitions()
                    .iter()
                    .position(|definition| definition.config.research_id == research_id)
            });
            self.world.switch_vehicle(model);
            if let Err(error) = self.world.respawn_near_within(&self.hermes, stop, 9.) {
                if let Some(previous) = previous {
                    self.world.switch_vehicle(previous);
                }
                eprintln!("Bus start unavailable: {error}");
                return false;
            }
            self.finish_location_change();
            self.zoom = LARGE_VEHICLE_CHASE_ZOOM;
        } else {
            self.world.switch_vehicle(model);
        }
        true
    }

    pub(super) fn refresh_mission_route(&mut self) {
        let Some(site) = self.kairos.target() else {
            self.hermes.clear_destination();
            return;
        };
        let point = (self.world.player_position() / METERS_PER_UNIT).to_array();
        if !self.hermes.select_mission_destination(site, point) {
            eprintln!("Mission route unreachable: {}", site.map_name());
        }
    }

    pub(super) fn driving_mission_vehicle(&self) -> bool {
        self.world.is_driving()
            && self.mission_vehicle == Some(self.world.player_vehicle_id())
            && self
                .kairos
                .active_id()
                .and_then(MissionId::vehicle_id)
                .is_some_and(|vehicle_id| self.world.player_model_id() == Some(vehicle_id))
    }

    pub(super) fn try_service(&mut self) {
        let (Some(id), Some(site), Some(plan)) = (
            self.kairos.active_id(),
            self.kairos.target(),
            self.kairos.service_plan(),
        ) else {
            return;
        };
        if self.world.player().velocity.length() >= 1. {
            return;
        }
        let Some(target) = self.hermes.mission_site(site) else {
            return;
        };
        let player = self.world.player_position() / METERS_PER_UNIT;
        if player.truncate().distance(glam::Vec2::from_array(target)) > 0.10 {
            return;
        }
        let Some(sidewalk) = self.hermes.mission_waiting_point(site) else {
            return;
        };
        if self
            .world
            .begin_service(plan.board, plan.alight, sidewalk, id == MissionId::Bus)
        {
            if id == MissionId::Taxi && site == SiteId::TaxiDropoff {
                self.kairos
                    .event(crate::kairos::MissionEvent::PassengerAlighted);
                self.kairos.save_now();
                self.hermes.clear_destination();
            }
            self.scene_dirty = true;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
    }

    pub(super) fn start_next_taxi_run(&mut self) {
        let player = self.world.player_position() / METERS_PER_UNIT;
        if !self.hermes.choose_taxi_run(player) {
            eprintln!("Next taxi route unavailable on the current road network");
            return;
        }
        let Some(waiting) = self.hermes.mission_waiting_point(SiteId::TaxiPickup) else {
            return;
        };
        self.world.clear_service_passengers();
        self.world.spawn_service_passenger(1, waiting);
        self.kairos.accept(MissionId::Taxi);
        self.refresh_mission_route();
        self.scene_dirty = true;
    }

    pub(super) fn restart_bus_passengers(&mut self) {
        let Some(passengers) = BUS_PASSENGERS
            .iter()
            .map(|(id, site)| {
                self.hermes
                    .mission_waiting_point(*site)
                    .map(|point| (*id, point))
            })
            .collect::<Option<Vec<_>>>()
        else {
            eprintln!("Bus waiting point unavailable for the next circuit");
            return;
        };
        self.world.clear_service_passengers();
        for (index, (id, point)) in passengers.into_iter().enumerate() {
            self.world
                .spawn_service_passenger(id, point + Vec3::X * (index as f32 * 0.6));
        }
        self.scene_dirty = true;
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn zombie_job_can_be_chosen_replaced_and_abandoned_from_the_phone() {
        let mut game = Game::new(Hermes::test_map());
        let choose = PaletteAction::SelectMission;
        game.apply_palette_action(choose(MissionId::Snowplow));
        assert!(game.world.toggle_vehicle(&game.hermes));
        let position = game.world.player_position();

        game.apply_palette_action(choose(MissionId::Zombie));
        assert_eq!(game.kairos.active_id(), Some(MissionId::Zombie));
        assert!(game.world.zombie_mode());
        assert!(!game.world.is_driving());
        assert_eq!(game.world.player_position(), position);
        assert_eq!(game.world.season(), crate::season::Season::Winter);
        assert!(game.kairos.target().is_none());
        assert!(game.mission_vehicle.is_none());
        assert!(!game.kairos.vehicle_return_expired(false, 11.));
        assert_eq!(game.kairos.vehicle_return_seconds(), None);

        game.apply_palette_action(choose(MissionId::Snowplow));
        assert!(!game.world.zombie_mode());
        assert!(game.driving_mission_vehicle());

        game.apply_palette_action(choose(MissionId::Zombie));
        assert!(game.world.is_driving());
        assert!(game.world.zombie_mode());
        game.apply_palette_action(PaletteAction::AbandonMission);
        assert!(!game.world.zombie_mode());
        assert_eq!(game.kairos.active_id(), None);
    }

    #[test]
    fn mission_vehicle_return_resumes_or_expires_during_play() {
        let mut game = Game::new(Hermes::test_map());
        game.activate_mission_now(MissionId::Snowplow);
        assert!(game.driving_mission_vehicle());
        assert!(game.world.toggle_vehicle(&game.hermes));
        game.simulate(0.25);
        assert_eq!(game.kairos.vehicle_return_seconds(), Some(10));

        game.set_overlay(Overlay::Menu);
        game.simulate(10.);
        assert_eq!(game.kairos.vehicle_return_seconds(), Some(10));
        game.set_overlay(Overlay::Driving);
        assert!(game.world.toggle_vehicle(&game.hermes));
        game.simulate(0.1);
        assert_eq!(game.kairos.vehicle_return_seconds(), None);
        assert_eq!(game.kairos.active_id(), Some(MissionId::Snowplow));

        assert!(game.world.toggle_vehicle(&game.hermes));
        game.simulate(1.);
        assert_eq!(game.kairos.vehicle_return_seconds(), Some(9));
        game.simulate(9.);
        assert_eq!(game.kairos.active_id(), None);
        assert_eq!(game.kairos.vehicle_return_seconds(), None);
        assert_eq!(game.world.season(), crate::season::Season::Summer);
    }
}
