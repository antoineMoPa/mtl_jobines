use super::frame_rate::FrameRate;
use super::palette::{Palette, PaletteAction, TimeCommand};
use super::prop_scenery::PropScenery;
use super::render::{GpuMesh, Renderer};
#[cfg(not(target_arch = "wasm32"))]
use super::smoke::{self, SkyOverrides, SmokeOptions};
use super::vehicle_scenery::VehicleScenery;
use super::{
    hud::{self, MenuItem, Overlay},
    scenery::Scenery,
};
use crate::dynamic_world::{
    AutoDrive, Controls, DriveStep, DynamicWorld, SpawnPoint, WalkControls, METERS_PER_UNIT,
    WALK_TURN_SPEED,
};
use crate::hermes::{Hermes, MinimapScale};
use crate::kairos::{Kairos, MissionEvent};
use glam::{Mat4, Quat, Vec3};
use opencity::runtime::Mesh;
#[cfg(not(target_arch = "wasm32"))]
use opencity::runtime::Region;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::{collections::HashSet, sync::Arc};
use web_time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, KeyCode, PhysicalKey},
    window::{Window, WindowId},
};
pub(super) const DT: f32 = 1. / 120.;
const DEFAULT_CHASE_ZOOM: f32 = 0.45;
const AIRCRAFT_CHASE_ZOOM: f32 = 0.30;
const MIN_AIRCRAFT_CHASE_ZOOM: f32 = 0.12;
const AIM_LOOK_SENSITIVITY: f32 = 0.0016;
const CAMERA_ORBIT_SENSITIVITY: f32 = 0.005;
const CAMERA_PITCH_SENSITIVITY: f32 = 0.003;
const CAMERA_RETURN_RATE: f32 = 1.0;
const CURSOR_IDLE_DELAY: Duration = Duration::from_secs(2);
mod camera_input;
#[cfg(not(target_arch = "wasm32"))]
mod construction;
mod cutscenes;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod cutscenes_preview;
mod focus;
#[cfg(any(target_arch = "wasm32", test))]
mod mobile;
mod weapon_camera;
mod weapons;

pub(super) fn mobile_enabled() -> bool {
    #[cfg(target_arch = "wasm32")]
    return mobile::enabled();
    #[cfg(not(target_arch = "wasm32"))]
    false
}
use cutscenes::AfterCutscene;
mod location;
mod missions;
#[cfg(not(target_arch = "wasm32"))]
mod native_startup;
#[cfg(not(target_arch = "wasm32"))]
mod reload;
mod startup;
/// At default zoom the taxi sits 6.3 m ahead and 2.55 m below the eye.
fn ground_chase(p: Vec3, forward: Vec3, zoom: f32, pitch: f32) -> (Vec3, Vec3) {
    let scale = zoom / DEFAULT_CHASE_ZOOM / METERS_PER_UNIT;
    let eye = p + (-forward * (6.3 * pitch.cos()) + Vec3::Z * (2.55 + 6.3 * pitch.sin())) * scale;
    let aim = p + (forward * 6.0 + Vec3::Z * 1.4) / METERS_PER_UNIT;
    (eye, aim)
}

/// Eye and aim offsets in map units. Size the view by airframe length so the
/// jet and airliner occupy a similar portion of the frame.
fn aircraft_chase(length: f32, forward: Vec3, zoom: f32, orbit_pitch: f32) -> (Vec3, Vec3) {
    let scale = length / METERS_PER_UNIT;
    let distance = 1.43 * scale * zoom / AIRCRAFT_CHASE_ZOOM;
    let elevation = 0.15 + orbit_pitch;
    let eye = (-forward * elevation.cos() + Vec3::Z * elevation.sin()) * distance;
    let aim = (forward * 2.8 + Vec3::Z * 0.10) * scale;
    (eye, aim)
}

/// Turn the taxi chase aim along the shorter arc, independent of render frame rate.
fn taxi_aim_yaw(previous: f32, nose: f32, elapsed: f32) -> f32 {
    let turn = nose - previous;
    previous + turn.sin().atan2(turn.cos()) * (1. - (-4. * elapsed).exp())
}

/// Ease sideways from the current lens, pulling back just enough to frame the arrest.
fn arrest_view(start: (Vec3, Vec3), focus: Vec3, forward: Vec3, progress: f32) -> (Vec3, Vec3) {
    let t = progress.clamp(0., 1.);
    let glide = t * t * (3. - 2. * t);
    let arm = (start.0 - focus).truncate();
    let heading = if arm.length() * METERS_PER_UNIT > 2. {
        arm.y.atan2(arm.x)
    } else {
        (-forward.y).atan2(-forward.x)
    };
    let distance = (arm.length() * METERS_PER_UNIT + 2.).clamp(6., 11.);
    let end =
        focus + (glam::Vec2::from_angle(heading + 0.68) * distance).extend(2.7) / METERS_PER_UNIT;
    (start.0.lerp(end, glide), start.1.lerp(focus, glide))
}

fn movement_controls(keys: &HashSet<KeyCode>, speed: f32) -> (Controls, WalkControls) {
    let held = |key| keys.contains(&key);
    let forward = held(KeyCode::KeyW) || held(KeyCode::ArrowUp);
    let backward = held(KeyCode::KeyS) || held(KeyCode::ArrowDown);
    let direction = forward as u8 as f32 - backward as u8 as f32;
    let brake = direction * speed < -0.5;
    let driving = Controls {
        throttle: if brake { 0. } else { direction },
        brake,
        steering: (held(KeyCode::KeyA) || held(KeyCode::ArrowLeft)) as u8 as f32
            - (held(KeyCode::KeyD) || held(KeyCode::ArrowRight)) as u8 as f32,
        handbrake: held(KeyCode::Space),
        boost: held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight),
        ..Controls::default()
    };
    let walking = WalkControls {
        forward: direction,
        turn: driving.steering,
        run: held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight),
        jump: held(KeyCode::Space),
        ..WalkControls::default()
    };
    (driving, walking)
}
fn aircraft_controls(keys: &HashSet<KeyCode>) -> Controls {
    let held = |key| f32::from(u8::from(keys.contains(&key)));
    Controls {
        throttle: held(KeyCode::KeyW),
        brake: keys.contains(&KeyCode::KeyS),
        steering: held(KeyCode::KeyA) - held(KeyCode::KeyD),
        aircraft_pitch: held(KeyCode::ArrowDown) - held(KeyCode::ArrowUp),
        aircraft_yaw: held(KeyCode::ArrowLeft) - held(KeyCode::ArrowRight),
        handbrake: keys.contains(&KeyCode::Space),
        ..Controls::default()
    }
}

enum GameEvent {
    #[cfg(target_arch = "wasm32")]
    Mobile(mobile::Event),
    #[cfg(target_arch = "wasm32")]
    RendererReady(Box<Result<Renderer, String>>),
    #[cfg(not(target_arch = "wasm32"))]
    MapLoaded,
}
struct Game {
    #[cfg(not(target_arch = "wasm32"))]
    live_reload: Option<reload::LiveReload>,
    #[cfg(not(target_arch = "wasm32"))]
    profile_shutdown: bool,
    audio: crate::audio::Audio,
    sound_settings: crate::audio::Settings,
    sound_eye: Option<Vec3>,
    focused: bool,
    suspended: bool,
    hidden: bool,
    pending_resize: Option<winit::dpi::PhysicalSize<u32>>,
    minimized: bool,
    #[cfg(target_arch = "wasm32")]
    proxy: winit::event_loop::EventLoopProxy<GameEvent>,
    hermes: Hermes,
    kairos: Kairos,
    mission_vehicle: Option<crate::dynamic_world::VehicleId>,
    world: DynamicWorld,
    auto_drive: Option<AutoDrive>,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    scenery: Option<Scenery>,
    prop_scenery: PropScenery,
    vehicle_scenery: Option<VehicleScenery>,
    dynamic_mesh: Option<GpuMesh>,
    dynamic_vertices: Mesh,
    loadout: weapons::Loadout,
    officer_weapons: Vec<super::muzzle::Equipment>,
    arrest_camera_start: Option<(Vec3, Vec3)>,
    aim_mouse: camera_input::AimMouse,
    palette: Option<Palette>,
    keys: HashSet<KeyCode>,
    #[cfg(target_arch = "wasm32")]
    mobile: mobile::Input,
    last: Option<Instant>,
    game_time: f32,
    accumulator: f32,
    previous_car_position: Vec3,
    previous_car_orientation: Quat,
    previous_car_yaw: f32,
    mesh_center: [f32; 2],
    eye: Vec3,
    camera_anchor: Vec3,
    taxi_aim_yaw: Option<f32>,
    zoom: f32,
    orbit: f32,
    camera_pitch: f32,
    dragging: bool,
    orbit_drag: Option<camera_input::OrbitDrag>,
    cursor: Option<[f32; 2]>,
    cursor_hide_at: Option<Instant>,
    cursor_hidden: bool,
    overlay: Overlay,
    help_visible: bool,
    fabula: super::fabula::Fabula,
    after_cutscene: Option<AfterCutscene>,
    intro_played: bool,
    full_map: hud::FullMapView,
    menu_selection: MenuItem,
    hood: bool,
    first_frame: bool,
    first_frame_attempts: u32,
    surface_retry: Option<Instant>,
    scene_dirty: bool,
    route_revision: u64,
    frame_rate: FrameRate,
}
impl Game {
    fn show_cursor(&mut self) {
        self.cursor_hide_at = None;
        if self.cursor_hidden {
            self.cursor_hidden = false;
            if let Some(window) = &self.window {
                window.set_cursor_visible(true);
            }
        }
    }

    fn cursor_activity(&mut self, now: Instant) {
        if self.aim_mouse.active() || self.orbit_drag.as_ref().is_some_and(|drag| drag.captured) {
            return;
        }
        self.show_cursor();
        #[cfg(not(target_arch = "wasm32"))]
        if self.simulation_paused() {
            return;
        }
        self.cursor_hide_at = Some(now + CURSOR_IDLE_DELAY);
    }

    fn cursor_wait(&mut self, now: Instant) -> ControlFlow {
        // A panel or cutscene can open after gameplay armed the idle timer.
        #[cfg(not(target_arch = "wasm32"))]
        if self.simulation_paused() {
            self.show_cursor();
            return ControlFlow::Wait;
        }
        if self.aim_mouse.active() {
            return ControlFlow::Wait;
        }
        if let Some(deadline) = self.cursor_hide_at {
            if now < deadline {
                return ControlFlow::WaitUntil(deadline);
            }
            self.cursor_hide_at = None;
            self.cursor_hidden = true;
            if let Some(window) = &self.window {
                window.set_cursor_visible(false);
            }
        }
        ControlFlow::Wait
    }

    fn retry_surface(&mut self, delay: Duration) {
        self.surface_retry = Some(Instant::now() + delay);
    }
    fn wake_surface(&mut self, now: Instant) -> bool {
        if self.surface_retry.is_some_and(|deadline| deadline <= now) {
            self.surface_retry = None;
            true
        } else {
            false
        }
    }
    fn wait_for_event(&mut self, now: Instant) -> ControlFlow {
        if !self.can_present_frame() {
            return ControlFlow::Wait;
        }
        let cursor = self.cursor_wait(now);
        let waiting = match (cursor, self.surface_retry) {
            (ControlFlow::WaitUntil(deadline), Some(retry)) => {
                ControlFlow::WaitUntil(deadline.min(retry))
            }
            (_, Some(retry)) => ControlFlow::WaitUntil(retry),
            _ => cursor,
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(reload) = &self.live_reload {
            return ControlFlow::WaitUntil(match waiting {
                ControlFlow::WaitUntil(deadline) => deadline.min(reload.next_check()),
                _ => reload.next_check(),
            });
        }
        waiting
    }
    fn exit_game(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(not(target_arch = "wasm32"))]
        event_loop.exit();
        // Browsers cannot close a tab they did not open. Return to the title.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = event_loop;
            self.world.reset_player(&self.hermes);
            self.scene_dirty = true;
            self.set_overlay(Overlay::Start);
            self.help_visible = false;
        }
    }
    fn set_overlay(&mut self, overlay: Overlay) {
        self.audio.silence();
        self.sound_eye = None;
        self.auto_drive = None;
        self.overlay = overlay;
        if overlay.paused() {
            self.release_aim_mouse();
            self.loadout.projection = None;
        }
        self.help_visible = false;
        if self.cursor.is_some() {
            self.cursor_activity(Instant::now());
        }
        self.full_map.selection = hud::MapSelection::Route;
        if overlay == Overlay::Driving {
            self.audio.user_gesture();
        }
        self.menu_selection = MenuItem::Resume;
        self.keys.clear();
        #[cfg(target_arch = "wasm32")]
        self.mobile.clear();
        self.stop_orbit_drag();
        self.accumulator = 0.;
        self.last = None;
        self.frame_rate = FrameRate::default();
        if let Some(renderer) = &mut self.renderer {
            renderer.fps = None;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn menu_key(&mut self, key: KeyCode, state: ElementState, repeat: bool) -> Option<MenuItem> {
        if !matches!(self.overlay, Overlay::Start | Overlay::Menu)
            || state != ElementState::Pressed
            || repeat
        {
            return None;
        }
        match key {
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                self.menu_selection = if key == KeyCode::ArrowUp {
                    self.menu_selection.previous()
                } else {
                    self.menu_selection.other()
                };
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                None
            }
            KeyCode::Enter => Some(self.menu_selection),
            KeyCode::KeyM => Some(MenuItem::Map),
            KeyCode::KeyQ => Some(MenuItem::Exit),
            _ => None,
        }
    }

    #[cfg(test)]
    fn new(
        hermes: Hermes,
        #[cfg(target_arch = "wasm32")] proxy: winit::event_loop::EventLoopProxy<GameEvent>,
    ) -> Self {
        Self::new_at(
            hermes,
            None,
            #[cfg(target_arch = "wasm32")]
            proxy,
        )
        .expect("default map spawn")
    }
    fn new_at(
        hermes: Hermes,
        start: Option<SpawnPoint>,
        #[cfg(target_arch = "wasm32")] proxy: winit::event_loop::EventLoopProxy<GameEvent>,
    ) -> Result<Self, String> {
        let world = DynamicWorld::with_start(&hermes, start)?;
        let p = world.player_position() / METERS_PER_UNIT;
        let yaw = world.player_yaw();
        let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        let previous_car_position = world.player().position;
        let previous_car_orientation = world.player().orientation();
        let previous_car_yaw = world.player().yaw;
        Ok(Self {
            #[cfg(not(target_arch = "wasm32"))]
            live_reload: None,
            #[cfg(not(target_arch = "wasm32"))]
            profile_shutdown: false,
            audio: crate::audio::Audio::default(),
            sound_settings: crate::audio::Settings::default(),
            sound_eye: None,
            focused: true,
            suspended: false,
            hidden: false,
            pending_resize: None,
            minimized: false,
            #[cfg(target_arch = "wasm32")]
            proxy,
            hermes,
            mission_vehicle: None,
            kairos: {
                #[cfg(test)]
                {
                    Kairos::test_session()
                }
                #[cfg(not(test))]
                {
                    Kairos::load()
                }
            },
            world,
            auto_drive: None,
            window: None,
            renderer: None,
            scenery: None,
            prop_scenery: PropScenery::default(),
            vehicle_scenery: None,
            dynamic_mesh: None,
            dynamic_vertices: Mesh::default(),
            loadout: Default::default(),
            officer_weapons: Vec::new(),
            arrest_camera_start: None,
            aim_mouse: Default::default(),
            palette: None,
            keys: HashSet::new(),
            #[cfg(target_arch = "wasm32")]
            mobile: mobile::Input::default(),
            last: None,
            game_time: 0.,
            accumulator: 0.,
            previous_car_position,
            previous_car_orientation,
            previous_car_yaw,
            mesh_center: [p.x, p.y],
            eye: ground_chase(p, forward, DEFAULT_CHASE_ZOOM, 0.).0,
            camera_anchor: p,
            taxi_aim_yaw: None,
            zoom: DEFAULT_CHASE_ZOOM,
            orbit: 0.,
            camera_pitch: 0.,
            dragging: false,
            orbit_drag: None,
            cursor: None,
            cursor_hide_at: None,
            cursor_hidden: false,
            overlay: if start.is_some() {
                Overlay::Driving
            } else {
                Overlay::Start
            },
            help_visible: false,
            fabula: Default::default(),
            after_cutscene: None,
            intro_played: start.is_some(),
            menu_selection: MenuItem::Resume,
            full_map: Default::default(),
            hood: false,
            first_frame: true,
            first_frame_attempts: 0,
            surface_retry: None,
            scene_dirty: true,
            route_revision: 0,
            frame_rate: FrameRate::default(),
        })
    }
    fn simulation_paused(&self) -> bool {
        !self.window_active()
            || self.overlay.paused()
            || self.help_visible
            || self.fabula.active()
            || self.palette.as_ref().is_some_and(|p| p.is_open())
    }
    fn recovery_in_progress(&self) -> bool {
        self.world.arrest_progress().is_some() || self.world.crash().is_some()
    }
    fn horn_requested(&self) -> bool {
        !self.simulation_paused()
            && self.world.is_driving()
            && (self.keys.contains(&KeyCode::Enter) || self.keys.contains(&KeyCode::NumpadEnter))
    }
    fn refresh_scene(&self) -> bool {
        !self.simulation_paused() || self.scene_dirty || self.fabula.active()
    }
    fn simulate(&mut self, elapsed: f32) {
        let _span = mtl_world::telemetry::span("simulation");
        if self.simulation_paused() {
            self.accumulator = 0.;
            return;
        }
        if let Some(renderer) = &self.renderer {
            let sky = renderer.sky_settings;
            let solar_hour = sky.hour(self.game_time + elapsed, renderer.daylight);
            self.world
                .set_wildlife_night(sky.sun_direction(solar_hour).z < 0.);
        }
        if self.recovery_in_progress() {
            self.game_time += elapsed;
            self.loadout.effects.step(elapsed);
            self.world.step(
                &self.hermes,
                Controls::default(),
                WalkControls::default(),
                elapsed,
            );
            if !self.recovery_in_progress() {
                self.arrest_camera_start = None;
                self.previous_car_position = self.world.player().position;
                self.previous_car_orientation = self.world.player().orientation();
                self.previous_car_yaw = self.world.player().yaw;
                self.finish_location_change();
            }
            return;
        }
        if !self.world.zombie_mode() && !self.world.can_equip_weapon() {
            self.loadout.equip(None);
        }
        if self.kairos.active_id() == Some(crate::kairos::MissionId::Baggage) {
            self.world
                .update_baggage_shift(&self.hermes, self.kairos.target(), elapsed);
            if self.world.baggage_shift_invalid() && !self.kairos.is_complete() {
                self.kairos.event(MissionEvent::BaggageRestarted);
            }
        }
        let driving_mission_vehicle = self.driving_mission_vehicle();
        if self
            .kairos
            .vehicle_return_expired(driving_mission_vehicle, elapsed)
        {
            self.abandon_mission();
        }
        let steps = super::frame_rate::physics_steps(&mut self.accumulator, elapsed, DT);
        self.game_time += elapsed;
        self.loadout.advance(self.game_time);
        self.loadout.effects.step(elapsed.min(0.1));
        let f = glam::Vec2::new(self.world.player().yaw.cos(), self.world.player().yaw.sin());
        let speed = self.world.player().velocity.truncate().dot(f);
        #[cfg(not(target_arch = "wasm32"))]
        let keys = &self.keys;
        #[cfg(target_arch = "wasm32")]
        let merged = self.mobile.with_keyboard(&self.keys);
        #[cfg(target_arch = "wasm32")]
        let keys = &merged;
        let (mut input, keyboard_walk) = movement_controls(keys, speed);
        let turn_key_held = [
            KeyCode::KeyA,
            KeyCode::ArrowLeft,
            KeyCode::KeyD,
            KeyCode::ArrowRight,
        ]
        .iter()
        .any(|key| keys.contains(key));
        if self.world.is_driving() && self.world.player().is_aircraft() {
            #[cfg(not(target_arch = "wasm32"))]
            {
                input = aircraft_controls(keys);
            }
            #[cfg(target_arch = "wasm32")]
            {
                input = aircraft_controls(&self.mobile.with_aircraft_keyboard(&self.keys));
            }
        }
        if input.throttle != 0.
            || input.steering != 0.
            || input.brake
            || input.handbrake
            || input.boost
        {
            self.auto_drive = None;
        }
        for _ in 0..steps {
            self.previous_car_position = self.world.player().position;
            self.previous_car_orientation = self.world.player().orientation();
            self.previous_car_yaw = self.world.player().yaw;
            let mut walk = keyboard_walk;
            let aiming = self.loadout.equipped.is_some() && self.world.can_equip_weapon();
            let yaw = self.world.player_yaw();
            if aiming {
                let forward = glam::Vec2::from_angle(yaw + self.orbit);
                walk.aim_move = Some(
                    (forward * walk.forward + forward.perp() * walk.turn).clamp_length_max(1.),
                );
                walk.turn = (self.orbit / (WALK_TURN_SPEED * DT)).clamp(-1., 1.);
            } else if !self.world.is_driving() && !turn_key_held {
                let step_turn = self
                    .orbit
                    .clamp(-WALK_TURN_SPEED * DT, WALK_TURN_SPEED * DT);
                walk.turn = step_turn / (WALK_TURN_SPEED * DT);
                self.orbit -= step_turn;
            }
            let mut driving = input;
            if let Some(drive) = &mut self.auto_drive {
                let position = self.world.player_position() / METERS_PER_UNIT;
                self.hermes.update_navigation(position.to_array());
                match drive.step(&mut self.world, &self.hermes, DT) {
                    DriveStep::Apply(controls) => driving = controls,
                    DriveStep::Finished => self.auto_drive = None,
                    DriveStep::Blocked => {
                        self.set_overlay(Overlay::Map);
                        self.full_map.selection = hud::MapSelection::DriveBlocked;
                        return;
                    }
                }
            }
            self.world.step(&self.hermes, driving, walk, DT);
            for hit in self.world.drain_player_tire_pops() {
                self.loadout.effects.tire_pop(hit.point, hit.normal);
            }
            if self.recovery_in_progress() {
                self.auto_drive = None;
                self.keys.clear();
                self.stop_orbit_drag();
                self.release_aim_mouse();
                self.loadout.equip(None);
                self.accumulator = 0.;
                return;
            }
            if self.world.player_ready_to_respawn() {
                self.world.reset_player(&self.hermes);
                self.finish_location_change();
                return;
            }
            if aiming {
                self.orbit -= self.world.player_yaw() - yaw;
                self.orbit = self.orbit.sin().atan2(self.orbit.cos());
            }
            if driving_mission_vehicle {
                self.try_service();
            }
        }
        if self.world.take_service_finished() {
            self.service_completed();
        }
        let cleared = self.world.take_snow_events();
        if driving_mission_vehicle
            && !cleared.is_empty()
            && self.kairos.active_id() == Some(crate::kairos::MissionId::Snowplow)
        {
            let was_complete = self.kairos.is_complete();
            self.kairos.event(MissionEvent::SnowCleared(cleared));
            if !was_complete && self.kairos.is_complete() {
                self.begin_cutscene(
                    super::fabula::Cue::AfterMission(crate::kairos::MissionId::Snowplow),
                    AfterCutscene::Resume,
                );
            }
            #[cfg(target_arch = "wasm32")]
            self.kairos.save_now();
        }
    }
    fn player_render_pose(&self) -> (Vec3, Quat, f32) {
        let car = self.world.player();
        if !self.world.is_driving()
            || self.world.crash().is_some()
            || self.simulation_paused()
            || car.position.distance_squared(self.previous_car_position) > 4.
        {
            return (car.position, car.orientation(), car.yaw);
        }
        let alpha = (self.accumulator / DT).clamp(0., 1.);
        let yaw_delta = (car.yaw - self.previous_car_yaw)
            .sin()
            .atan2((car.yaw - self.previous_car_yaw).cos());
        (
            self.previous_car_position.lerp(car.position, alpha),
            self.previous_car_orientation
                .slerp(car.orientation(), alpha),
            self.previous_car_yaw + yaw_delta * alpha,
        )
    }
    fn apply_palette_action(&mut self, action: PaletteAction) {
        match action {
            PaletteAction::PlayIntro => self.replay_intro(),
            PaletteAction::SelectMission(id) => self.activate_mission(id),
            PaletteAction::AbandonMission => self.abandon_mission(),
            PaletteAction::SwitchVehicle(model) => {
                if self.world.switch_vehicle(model) {
                    self.finish_location_change();
                }
            }
            PaletteAction::Season(season) => {
                self.change_season(season);
            }
            #[cfg(not(target_arch = "wasm32"))]
            PaletteAction::BuildBuildings
            | PaletteAction::PlaceProps(_)
            | PaletteAction::ExpandNetwork
            | PaletteAction::GrowNetwork => self.open_construction(action),
            PaletteAction::RestartBaggage => {
                if self.kairos.active_id() == Some(crate::kairos::MissionId::Baggage)
                    && self.world.restart_baggage_shift(&self.hermes)
                {
                    self.kairos.accept(crate::kairos::MissionId::Baggage);
                    self.refresh_mission_route();
                }
            }
            PaletteAction::ServiceBaggage => self.service_baggage(),
            PaletteAction::AttachCart => {
                self.world.attach_baggage_cart();
            }
            PaletteAction::DetachCart => {
                self.world.detach_baggage_cart();
            }
            PaletteAction::UseBikeshare => {
                if self.world.bikeshare_hint().is_some() && self.world.toggle_vehicle(&self.hermes)
                {
                    self.reset_boarding_camera();
                }
            }
            PaletteAction::InteractWithCat => {
                self.interact_with_cat();
            }
            PaletteAction::ResumeDriving => self.auto_drive = None,
            PaletteAction::JumpToLocation => {
                self.set_overlay(Overlay::Map);
                self.full_map.selection = hud::MapSelection::Jump;
            }
            #[cfg(not(target_arch = "wasm32"))]
            PaletteAction::BugReport => {
                unreachable!("bug report capture is handled before present")
            }
            PaletteAction::Time(command) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.daylight = command.daylight();
                    renderer.sky_settings.solar_hour = None;
                }
                if let Some(window) = &self.window {
                    window.set_title(match command {
                        TimeCommand::Day => "MTL Jobines - day",
                        TimeCommand::Night => "MTL Jobines - night",
                        TimeCommand::Automatic => "MTL Jobines - automatic daylight",
                    });
                    window.request_redraw();
                }
            }
            PaletteAction::Respawn(id) => {
                let Some(landmark) = self.hermes.landmarks().into_iter().find(|l| l.id == id)
                else {
                    return;
                };
                match self.world.respawn_at_landmark(&self.hermes, &landmark) {
                    Ok(()) => self.finish_location_change(),
                    Err(error) => eprintln!("Cannot respawn near {}: {error}", landmark.title),
                }
            }
        }
    }
    fn acquire_frame(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> Option<(wgpu::SurfaceTexture, bool)> {
        let renderer = self.renderer.as_mut()?;
        if let Some(size) = self.pending_resize.take() {
            if (size.width, size.height) != (renderer.config.width, renderer.config.height) {
                renderer.resize(size.width, size.height);
            }
        }
        if self.first_frame {
            self.first_frame_attempts += 1;
            if self.first_frame_attempts == 1 {
                eprintln!("Drawing first frame...");
            }
        }
        use wgpu::CurrentSurfaceTexture::*;
        let retry = match renderer.surface.as_ref()?.get_current_texture() {
            Success(frame) => return Some((frame, false)),
            Suboptimal(frame) => return Some((frame, true)),
            Outdated => {
                renderer.resize(renderer.config.width, renderer.config.height);
                Duration::from_millis(100)
            }
            Lost => {
                if let Err(error) = renderer.recreate_surface(self.window.as_ref().unwrap().clone())
                {
                    eprintln!("Cannot recreate game surface: {error}");
                    event_loop.exit();
                    return None;
                }
                Duration::from_millis(100)
            }
            Timeout => Duration::from_millis(100),
            Occluded => Duration::from_millis(250),
            Validation => {
                eprintln!("Cannot acquire game surface: GPU validation failed");
                event_loop.exit();
                return None;
            }
        };
        self.reset_frame_timing();
        self.audio.silence();
        self.retry_surface(retry);
        None
    }

    fn frame_elapsed(&mut self, now: Instant) -> f32 {
        self.last
            .replace(now)
            .map_or(0., |last| (now - last).as_secs_f32())
    }

    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if !self.can_present_frame()
            || self
                .surface_retry
                .is_some_and(|deadline| Instant::now() < deadline)
        {
            return;
        }
        let _frame = mtl_world::telemetry::frame();
        let acquired = {
            let _span = mtl_world::telemetry::span("surface.acquire");
            self.acquire_frame(event_loop)
        };
        let Some((frame, suboptimal)) = acquired else {
            return;
        };
        let cutscene_waiting_for_gesture =
            self.cutscene_can_play() && self.audio.cutscene_waiting_for_gesture();
        let frame_elapsed = self.frame_elapsed(Instant::now());
        let elapsed = frame_elapsed.min(0.1);
        if self.cutscene_can_play()
            && self
                .fabula
                .advance(frame_elapsed, self.audio.cutscene_time())
        {
            self.finish_cutscene(true);
        }
        let previous_position = self.world.player_position();
        self.simulate(elapsed);
        let scene = mtl_world::telemetry::span("scene.prepare");
        let paused = self.simulation_paused();
        let refresh_scene = self.refresh_scene();

        let (render_position, render_orientation, render_yaw) = self.player_render_pose();
        let p = if self.world.is_driving() {
            render_position
        } else {
            self.world.player_position()
        } / METERS_PER_UNIT;
        let build_camera = self
            .palette
            .as_ref()
            .and_then(|palette| palette.build_camera());
        let center = build_camera.map_or([p.x, p.y], |(_, _, center)| center);
        if !paused && self.auto_drive.is_none() {
            self.hermes.update_navigation(p.to_array());
        }
        let horn = self.horn_requested();
        let armed = !paused && self.loadout.equipped.is_some() && self.world.can_equip_weapon();
        let scene_cursor = self.scene_cursor(armed);
        let renderer = self.renderer.as_mut().unwrap();
        if self.route_revision != self.hermes.route_revision() {
            renderer.update_route(&self.hermes.route_uvs());
            self.route_revision = self.hermes.route_revision();
        }
        if !paused
            && self.world.is_driving()
            && !self.dragging
            && self
                .world
                .player_position()
                .distance_squared(previous_position)
                > 0.000001
        {
            self.orbit *= (-elapsed * CAMERA_RETURN_RATE).exp();
            self.camera_pitch *= (-elapsed * CAMERA_RETURN_RATE).exp();
        }
        let plane = self.world.player();
        let flying = self.world.is_driving() && plane.is_aircraft() && !plane.grounded;
        let flight_chase = flying && !self.hood;
        let nose = glam::Vec2::new(render_yaw.cos(), render_yaw.sin());
        let heading = if flight_chase && plane.velocity.truncate().dot(nose) > 2. {
            plane.velocity.y.atan2(plane.velocity.x)
        } else if self.world.is_driving() {
            render_yaw
        } else {
            self.world.player_yaw()
        } + self.orbit;
        let forward = Vec3::new(heading.cos(), heading.sin(), 0.);
        let aircraft = self
            .world
            .is_driving()
            .then(|| self.world.player())
            .filter(|vehicle| vehicle.is_aircraft());
        // Gain one street-level visibility range per 100 m climbed, up to four ranges.
        renderer.flight_visibility = if aircraft.is_some_and(|plane| !plane.grounded)
            && build_camera.is_none()
            && !self.fabula.active()
        {
            1. + (p.z - 0.1).clamp(0., crate::performance::draw_radius(3.))
        } else {
            1.
        };
        let previous_taxi_yaw = self.taxi_aim_yaw.take();
        let (target_eye, look) = if let Some(crash) = self.world.crash() {
            let focus = (plane.position.lerp(crash.origin, 0.45) + Vec3::Z * 1.5 * crash.scale)
                / METERS_PER_UNIT;
            location::drive_view(focus, render_yaw, plane.length().max(5.2), crash.age)
        } else if let Some(drive) = &self.auto_drive {
            location::drive_view(p, render_yaw, plane.length(), drive.elapsed)
        } else if let Some(plane) = aircraft {
            if self.hood {
                let cockpit = Vec3::new(plane.length() * 0.38, 0., plane.height() * 0.16);
                let eye = p + render_orientation * cockpit / METERS_PER_UNIT;
                (eye, eye + render_orientation * Vec3::X * 0.25)
            } else {
                let (eye, aim) =
                    aircraft_chase(plane.length(), forward, self.zoom, self.camera_pitch);
                let aim = if plane.grounded {
                    let yaw = taxi_aim_yaw(
                        previous_taxi_yaw.unwrap_or(render_yaw),
                        render_yaw,
                        if paused { 0. } else { elapsed },
                    );
                    self.taxi_aim_yaw = Some(yaw);
                    // Orbit is already in the aim. Only ease the aircraft's own turn.
                    Quat::from_rotation_z(yaw - render_yaw) * aim
                } else {
                    aim
                };
                (p + eye, p + aim)
            }
        } else if !self.world.is_driving() {
            weapon_camera::on_foot_view(p, forward, self.zoom, self.camera_pitch)
        } else if self.hood {
            (
                p + forward * 0.009 + Vec3::Z * 0.011,
                p + Vec3::Z * 0.014 + forward * 0.045,
            )
        } else {
            ground_chase(p, forward, self.zoom, self.camera_pitch)
        };
        if !self.world.is_driving() {
            self.eye = target_eye;
        } else if !paused {
            let travel = p - self.camera_anchor;
            if flight_chase {
                // Follow horizontal travel while allowing a little heave through a climb.
                self.eye += Vec3::new(travel.x, travel.y, travel.z * 0.7);
                self.eye = self.eye.lerp(target_eye, 1. - (-elapsed * 2.8).exp());
            } else {
                self.eye += travel;
                self.eye = self.eye.lerp(target_eye, 1. - (-elapsed * 6.).exp());
            }
        }
        if !self.fabula.active() {
            self.camera_anchor = p;
            self.eye =
                camera_input::chase_eye(&self.world, &self.hermes, p, self.eye, refresh_scene);
        }
        let look = if self.world.is_driving() {
            look
        } else {
            self.eye + (look - target_eye)
        };
        let (eye, look) = if let Some(playback) = &self.fabula.playback {
            let (eye, look) = playback.camera();
            let eye = self.world.camera_eye(&self.hermes, look, eye);
            (eye, look)
        } else {
            self.world.metro_camera().unwrap_or_else(|| {
                build_camera.map_or((self.eye, look), |(eye, look, _)| (eye, look))
            })
        };
        let arrest = self.world.arrest_progress();
        let (eye, look) = if let Some(t) = arrest {
            let start = *self.arrest_camera_start.get_or_insert((eye, look));
            let focus = self.world.arrest_focus() / METERS_PER_UNIT;
            let (desired, look) = arrest_view(start, focus, forward, t);
            (
                self.world
                    .camera_eye(&self.hermes, p + Vec3::Z * (0.5 / METERS_PER_UNIT), desired),
                look,
            )
        } else {
            self.arrest_camera_start = None;
            (eye, look)
        };
        let view_rotation = Quat::from_mat4(&Mat4::look_at_rh(eye, look, Vec3::Z).inverse())
            * Quat::from_mat3(&glam::Mat3::from_cols(-Vec3::Z, -Vec3::X, Vec3::Y));
        let mut shot_sprites =
            self.loadout
                .effects
                .sprites(|_| None, view_rotation, eye * METERS_PER_UNIT);
        if build_camera.is_none() {
            let lighting = super::lighting::Lighting::with_settings(
                self.game_time,
                renderer.daylight,
                renderer.sky_settings,
            );
            super::airport_lights::append(
                &mut shot_sprites,
                self.world.airport_lights(),
                eye,
                view_rotation,
                renderer.scene_height(),
                renderer.flight_visibility * 10.,
                lighting.daylight,
            );
        }
        if let Some(crash) = self.world.crash() {
            super::crash::sprites(crash, view_rotation, &mut shot_sprites);
        }
        let view_projection = renderer.view_projection(eye, look);
        self.world.set_dispatch_view(view_projection);
        self.loadout.projection = armed.then(|| view_projection.inverse());
        let weapon_target = self
            .loadout
            .projection
            .and_then(weapons::sight_line)
            .map(|(from, end)| self.world.weapon_aim_point(self.hermes.city(), from, end));
        self.world.aim_weapon(
            weapon_target,
            self.loadout.equipped == Some(weapons::Weapon::Rifle),
        );
        let sound_eye = eye * METERS_PER_UNIT;
        let sound_velocity = self.sound_eye.map_or(Vec3::ZERO, |previous| {
            if elapsed > 0.0001 {
                ((sound_eye - previous) / elapsed).clamp_length_max(100.)
            } else {
                Vec3::ZERO
            }
        });
        self.sound_eye = if paused { None } else { Some(sound_eye) };
        if !self.fabula.active() {
            let _span = mtl_world::telemetry::span("scene.audio");
            let mut sound_frame =
                self.world
                    .sound_frame(sound_eye, look - eye, sound_velocity, self.sound_settings);
            if let Some(car) = sound_frame.cars.iter_mut().find(|car| car.player) {
                car.horn = horn;
            }
            self.audio.update(sound_frame, !paused);
        }
        let stage = self.fabula.playback.as_ref().map(|p| p.stage());
        let moving_mesh = self.dynamic_mesh.as_mut().unwrap();
        if refresh_scene {
            if stage.is_none() || self.scene_dirty {
                let _span = mtl_world::telemetry::span("scene.props_and_wildlife");
                self.prop_scenery
                    .update_with_stage(renderer, &self.world, stage);
            }
            let car = self.world.player();
            let player_pose_delta = Mat4::from_rotation_translation(
                render_orientation,
                render_position / METERS_PER_UNIT,
            ) * Mat4::from_rotation_translation(
                car.orientation(),
                car.position / METERS_PER_UNIT,
            )
            .inverse();
            if stage.is_none() || self.scene_dirty {
                let _span = mtl_world::telemetry::span("scene.moving_mesh");
                self.world.write_moving_mesh_with_player_pose(
                    &mut self.dynamic_vertices,
                    player_pose_delta,
                );
                self.loadout.effects.solids(&mut self.dynamic_vertices);
                renderer.update_dynamic(moving_mesh, &self.dynamic_vertices);
            }
            self.vehicle_scenery.as_mut().unwrap().update_with_stage(
                renderer,
                &self.world,
                player_pose_delta,
                stage,
            );
            self.scene_dirty = false;
        }
        self.hermes.set_minimap_scale(if flying {
            MinimapScale::Flight
        } else {
            MinimapScale::Ground
        });
        if let Err(error) = renderer.update_vector_map(self.hermes.city()) {
            eprintln!("Vector map: {error}");
            let view = (self.overlay == Overlay::Map).then(|| {
                self.full_map
                    .raster_view(renderer.config.width as f32, renderer.config.height as f32)
            });
            if let Some(pixels) = self.hermes.update_minimap_view(center, view) {
                renderer.update_minimap(pixels);
            }
        }
        self.mesh_center = center;
        let mut navigation = self.hermes.minimap_view(center, self.world.player_yaw());
        renderer.set_job_marker(self.world.cat_marker().map(|point| {
            self.hermes
                .map_uv((point / METERS_PER_UNIT).truncate().to_array())
        }));
        renderer.set_police_markers(self.world.nearby_police().map(|(point, walking)| {
            (
                self.hermes
                    .map_uv((point.truncate() / METERS_PER_UNIT).to_array()),
                walking,
            )
        }));
        if build_camera.is_some() {
            navigation.player_uv = self
                .hermes
                .minimap_view([p.x, p.y], self.world.player_yaw())
                .player_uv;
        }
        renderer.overlay = if matches!(self.overlay, Overlay::Start | Overlay::Menu) {
            Overlay::Driving
        } else {
            self.overlay
        };
        renderer.cinematic = self.fabula.active() || arrest.is_some() || self.auto_drive.is_some();
        renderer.zombie_mode = self.world.zombie_mode();
        renderer.full_map = self.full_map;
        if self.overlay == Overlay::Map {
            renderer.full_map.drive = self.world.drive_availability(&self.hermes);
        }
        renderer.menu_selection = self.menu_selection;
        renderer.fps = self.frame_rate.value;
        renderer.reserved_bottom = if renderer.cinematic {
            0
        } else {
            self.palette.as_ref().map_or(0, |palette| {
                palette.reserved_bottom(renderer.config.width, renderer.config.height)
            })
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            renderer.construction_span = self.palette.as_ref().and_then(|p| p.orthographic_span());
        }
        #[cfg(not(target_arch = "wasm32"))]
        let surface_preview = self.palette.as_mut().and_then(|p| p.road_preview(renderer));
        #[cfg(not(target_arch = "wasm32"))]
        let prop_preview = self
            .palette
            .as_mut()
            .and_then(|p| p.props.as_mut())
            .and_then(|d| d.preview(renderer, self.hermes.city()));
        #[cfg(not(target_arch = "wasm32"))]
        let authored_visible = !self
            .palette
            .as_ref()
            .and_then(|p| p.props.as_ref())
            .is_some_and(|d| d.open);
        #[cfg(target_arch = "wasm32")]
        let (prop_preview, authored_visible): (
            Option<Vec<std::sync::Arc<super::render::GpuMesh>>>,
            bool,
        ) = (None, true);
        #[cfg(target_arch = "wasm32")]
        let surface_preview: Option<std::sync::Arc<super::render::GpuMesh>> = None;
        self.scenery.as_mut().unwrap().update_winter(
            renderer,
            &self.world,
            self.hermes.city(),
            eye,
            self.game_time,
        );
        renderer.prepare_grass(
            &mut self.scenery.as_mut().unwrap().grass,
            self.hermes.city(),
            eye,
            look,
        );
        self.scenery
            .as_mut()
            .unwrap()
            .update_graffiti(renderer, center);
        let (mut meshes, metro) =
            self.scenery
                .as_mut()
                .unwrap()
                .for_frame(renderer, center, surface_preview.is_some());
        meshes.extend(surface_preview.iter().map(|mesh| mesh.as_ref()));
        meshes.extend(prop_preview.iter().flatten().map(|mesh| mesh.as_ref()));
        meshes.push(moving_mesh);
        renderer.local_lights.flash.set(None);
        if let Some(target) = weapon_target {
            meshes.extend(self.loadout.visible(
                renderer,
                &self.world,
                target,
                view_rotation,
                &mut shot_sprites,
            ));
        }
        let mut armed_officers = 0;
        for (i, (hand, target, age)) in self.world.officer_handguns().enumerate() {
            armed_officers += 1;
            if i == self.officer_weapons.len() {
                self.officer_weapons.push(super::muzzle::Equipment::upload(
                    renderer,
                    &weapons::Weapon::Handgun.model(),
                ));
            }
            let gun = &mut self.officer_weapons[i];
            let flash = super::muzzle::MuzzleFlash::handgun_at(age);
            gun.place(
                renderer,
                weapons::weapon_pose(hand, target),
                view_rotation,
                &flash,
                &mut shot_sprites,
            );
        }
        meshes.extend(
            self.officer_weapons[..armed_officers]
                .iter()
                .map(|gun| &gun.gpu),
        );
        renderer.update_impacts(&shot_sprites);
        meshes.extend(self.prop_scenery.visible_with_authored(authored_visible));
        meshes.extend(self.vehicle_scenery.as_ref().unwrap().visible());
        renderer.underground_exits =
            (renderer.construction_span.is_none() && metro.enclosed_view(eye)).then(|| {
                metro
                    .stations
                    .iter()
                    .map(|s| s.point(-0.40, 0.06, 0.).truncate())
                    .collect()
            });
        if let Some(exits) = renderer.underground_exits.as_deref() {
            meshes.retain(|mesh| mesh.visible_underground(exits));
        }
        renderer.mission_markers.update(
            &renderer.queue,
            self.world
                .waiting_service_passengers()
                .chain(self.world.cat_marker())
                .chain(
                    self.world
                        .baggage_markers(&self.hermes, self.kairos.target()),
                ),
            self.world.baggage_bay(&self.hermes, self.kairos.target()),
        );
        let mut palette_action = None;
        let mut menu_clicked = None;
        let mut skip_cutscene = false;
        let help_before_draw = self.help_visible;
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(renderer.config.format.add_srgb_suffix()),
            ..Default::default()
        });
        drop(scene);
        renderer.render(&view, &meshes, eye, look, self.game_time, navigation);
        if let Some(palette) = &mut self.palette {
            let _span = mtl_world::telemetry::span("ui.palette");
            palette_action = palette.draw(
                self.window.as_ref().unwrap(),
                renderer,
                &view,
                &mut self.sound_settings,
                renderer.view_projection(eye, look),
                self.hermes.city(),
                &self.kairos,
                &self.world,
                scene_cursor,
                &mut self.help_visible,
                self.overlay,
                &mut self.menu_selection,
                self.fabula.playback.as_ref(),
                cutscene_waiting_for_gesture,
                palette
                    .foot_motion_enabled()
                    .then(|| self.world.player_foot_motion())
                    .flatten(),
            );
            menu_clicked = palette.menu_clicked.take();
            skip_cutscene = palette.cutscene_skip;
            #[cfg(not(target_arch = "wasm32"))]
            if palette_action == Some(PaletteAction::BugReport) {
                let screenshot = renderer.report_screenshot_with_overlay(
                    &meshes,
                    eye,
                    look,
                    self.game_time,
                    navigation,
                    |target| palette.paint_report_overlay(renderer, target),
                );
                palette.bug_report.begin(
                    super::bug_report::Report::new(
                        self.hermes.city(),
                        p.to_array(),
                        eye.to_array(),
                        look.to_array(),
                    ),
                    screenshot,
                );
                palette_action = None;
                self.window.as_ref().unwrap().request_redraw();
            }
        }
        {
            let _span = mtl_world::telemetry::span("surface.present");
            renderer.queue.present(frame);
        }
        if let Some(playback) = &mut self.fabula.playback {
            playback.presented = true;
        }
        self.surface_retry = None;
        if suboptimal {
            // The texture is usable: present it before reconfiguring the surface.
            renderer.resize(renderer.config.width, renderer.config.height);
        }
        if !paused {
            self.frame_rate.presented(Instant::now());
        }
        if self.first_frame {
            eprintln!("Game ready: first frame presented");
            self.first_frame = false;
            #[cfg(target_arch = "wasm32")]
            web_status(None);
            #[cfg(not(target_arch = "wasm32"))]
            if self.profile_shutdown {
                event_loop.exit();
            }
        }
        if skip_cutscene {
            self.finish_cutscene(true);
        }
        if let Some(action) = palette_action {
            self.apply_palette_action(action);
        }
        match menu_clicked {
            Some(MenuItem::Resume) => self.start_playing(),
            Some(MenuItem::Map) => self.set_overlay(Overlay::Map),
            Some(MenuItem::Settings) => {
                if let Some(palette) = &mut self.palette {
                    palette.settings_open = true;
                }
            }
            Some(MenuItem::Exit) => self.exit_game(event_loop),
            None => {}
        }
        if self.help_visible != help_before_draw {
            self.window.as_ref().unwrap().request_redraw();
        }
        if self.help_visible {
            self.audio.silence();
            self.keys.clear();
            self.stop_orbit_drag();
            self.accumulator = 0.;
            self.cursor_hide_at = None;
            self.cursor_hidden = false;
            if let Some(window) = &self.window {
                window.set_cursor_visible(true);
            }
        } else if help_before_draw && self.overlay == Overlay::Start {
            self.start_playing();
        }
        let cutscene_playing = self.cutscene_can_play();
        if self.fabula.active() {
            self.audio.cutscene_active(
                cutscene_playing && self.fabula.playback.as_ref().is_some_and(|p| p.presented),
            );
        }
        if (!self.simulation_paused() || cutscene_playing) && self.surface_retry.is_none() {
            self.window.as_ref().unwrap().request_redraw();
        }
    }
}
impl ApplicationHandler<GameEvent> for Game {
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta } = event {
            let motion = [delta.0 as f32, delta.1 as f32];
            if self.aim_mouse.relative() && self.armed_aim() {
                self.aim_motion(motion);
            } else if self.orbit_drag.as_ref().is_some_and(|drag| drag.captured) {
                self.orbit_motion(motion);
            }
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.release_aim_mouse();
        self.stop_orbit_drag();
        if self.fabula.active() {
            self.finish_cutscene(false);
        }
        self.kairos.save_now();
        let started = Instant::now();
        if let Some(window) = &self.window {
            window.set_visible(false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.profile_shutdown {
            eprintln!("Shutdown hide window: {:?}", started.elapsed());
        }
        #[cfg(target_arch = "wasm32")]
        let _ = started;
    }
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.suspended = true;
        self.window_activity_changed();
        self.kairos.save_now();
        if self.overlay != Overlay::Start && !self.fabula.active() {
            self.set_overlay(Overlay::Menu);
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_arch = "wasm32")]
        self.sync_mobile_controls();
        self.sync_aim_mouse();
        if !self.can_present_frame() {
            event_loop.set_control_flow(ControlFlow::Wait);
            return;
        }
        self.kairos.save_if_due();
        #[cfg(not(target_arch = "wasm32"))]
        self.poll_map_reload(Instant::now());
        if self.simulation_paused() && !self.cutscene_can_play() {
            self.audio.silence();
        }
        let now = Instant::now();
        if self.wake_surface(now) {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(self.wait_for_event(now));
    }

    #[cfg(target_arch = "wasm32")]
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: GameEvent) {
        match event {
            GameEvent::Mobile(event) => self.mobile_event(event),
            GameEvent::RendererReady(result) => match *result {
                Ok(renderer) => {
                    self.finish_startup(renderer);
                }
                Err(error) => {
                    web_status(Some(&format!("Could not start the game: {error}")));
                    event_loop.exit();
                }
            },
        }
    }
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        self.suspended = false;
        self.window_activity_changed();
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;
            if self.window.is_some() {
                return;
            }
            let canvas = web_sys::window()
                .unwrap()
                .document()
                .unwrap()
                .get_element_by_id("game")
                .unwrap()
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .unwrap();
            let width = canvas.client_width().max(1) as f64;
            let height = canvas.client_height().max(1) as f64;
            let attributes = Window::default_attributes()
                .with_title("MTL Jobines")
                .with_canvas(Some(canvas))
                .with_prevent_default(true)
                .with_inner_size(winit::dpi::LogicalSize::new(width, height));
            let window = Arc::new(_event_loop.create_window(attributes).expect("window"));
            let size = window.inner_size();
            self.window = Some(window.clone());
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                web_loading_stage("Preparing graphics and textures…").await;
                let result = Renderer::new(Some(window), size.width, size.height).await;
                if result.is_ok() {
                    web_loading_stage("Building city scenery…").await;
                }
                let _ = proxy.send_event(GameEvent::RendererReady(Box::new(result)));
            });
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // egui marks RedrawRequested as needing a repaint. Feeding it back here would
        // schedule another frame even in a hidden window or an idle pause menu.
        if matches!(event, WindowEvent::RedrawRequested) {
            self.frame(event_loop);
            return;
        }
        if (!self.window_active() || self.recovery_in_progress())
            && matches!(
                event,
                WindowEvent::KeyboardInput { .. }
                    | WindowEvent::Ime(_)
                    | WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::CursorMoved { .. }
                    | WindowEvent::Touch(_)
            )
        {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if matches!(
            event,
            WindowEvent::Focused(false) | WindowEvent::Occluded(true) | WindowEvent::Resized(_)
        ) {
            self.mobile.clear();
        }
        // Release the camera grab even when UI consumes button-up. Only an
        // unconsumed release from a scene press can reach the punch handler.
        let scene_click = match event {
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.release_orbit_drag(),
            WindowEvent::Focused(false) | WindowEvent::Occluded(true) => {
                self.stop_orbit_drag();
                false
            }
            _ => false,
        };
        // Unlock before cutscene/UI handlers consume input. Touch release is
        // needed on browsers that do not grant activation on touch start.
        let audio_gesture = match &event {
            WindowEvent::KeyboardInput { event, .. } => {
                event.state == ElementState::Pressed && !event.repeat
            }
            WindowEvent::MouseInput { .. } => true,
            WindowEvent::Touch(touch) => matches!(
                touch.phase,
                winit::event::TouchPhase::Started | winit::event::TouchPhase::Ended
            ),
            _ => false,
        };
        if audio_gesture {
            self.audio.user_gesture();
            // A fresh key press returns the wheel immediately. The same key can
            // then steer, brake, open the menu or exit the vehicle as usual.
            if matches!(&event, WindowEvent::KeyboardInput { event, .. } if !event.repeat) {
                self.auto_drive = None;
            }
        }
        if self.armed_aim() {
            match event {
                WindowEvent::MouseInput {
                    state,
                    button: MouseButton::Left | MouseButton::Right,
                    ..
                } => {
                    if state == ElementState::Pressed {
                        #[cfg(target_arch = "wasm32")]
                        if !self.aim_mouse.relative() {
                            self.release_aim_mouse();
                        }
                        self.sync_aim_mouse();
                        self.fire_weapon();
                    }
                    return;
                }
                WindowEvent::CursorMoved { position, .. } => {
                    self.move_camera_with_cursor([position.x as f32, position.y as f32]);
                    return;
                }
                _ => {}
            }
        }
        if self.fabula.active() {
            if let WindowEvent::KeyboardInput { event: key, .. } = &event {
                if key.state == ElementState::Pressed
                    && !key.repeat
                    && matches!(
                        key.physical_key,
                        PhysicalKey::Code(KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter)
                    )
                {
                    self.finish_cutscene(true);
                }
                return;
            }
            if matches!(
                &event,
                WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::CursorMoved { .. }
                    | WindowEvent::Touch(_)
                    | WindowEvent::Ime(_)
            ) {
                if let (Some(palette), Some(window)) = (&mut self.palette, &self.window) {
                    palette.event(window, &event);
                    window.request_redraw();
                }
                return;
            }
        }
        if let WindowEvent::KeyboardInput {
            event: key_event, ..
        } = &event
        {
            let question = key_event.text.as_deref() == Some("?")
                || matches!(&key_event.logical_key, Key::Character(text) if text.as_str() == "?");
            if key_event.state == ElementState::Pressed && !key_event.repeat {
                if self.help_visible
                    && (question
                        || matches!(
                            key_event.physical_key,
                            PhysicalKey::Code(
                                KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter
                            )
                        ))
                {
                    self.help_visible = false;
                    self.accumulator = 0.;
                    self.last = None;
                    if self.overlay == Overlay::Start {
                        self.start_playing();
                    } else if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    return;
                }
                if question
                    && self.overlay != Overlay::Start
                    && !self
                        .palette
                        .as_ref()
                        .is_some_and(|palette| palette.is_open())
                {
                    self.help_visible = true;
                    self.keys.clear();
                    self.stop_orbit_drag();
                    self.accumulator = 0.;
                    self.audio.silence();
                    self.show_cursor();
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    return;
                }
            }
            if self.help_visible {
                return;
            }
        }
        if matches!(
            event,
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
        ) && self.cursor.is_some()
        {
            self.cursor_activity(Instant::now());
        }
        if let (Some(palette), Some(window)) = (&mut self.palette, &self.window) {
            #[cfg(not(target_arch = "wasm32"))]
            if palette.building_drop(&event, self.hermes.city()) {
                window.request_redraw();
                return;
            }
            let consumed = palette.event(window, &event);
            let open = palette.is_open();
            if self.help_visible
                && matches!(
                    event,
                    WindowEvent::Ime(_)
                        | WindowEvent::MouseInput { .. }
                        | WindowEvent::MouseWheel { .. }
                        | WindowEvent::CursorMoved { .. }
                        | WindowEvent::Touch(_)
                )
            {
                if let WindowEvent::CursorMoved { position, .. } = &event {
                    self.cursor = Some([position.x as f32, position.y as f32]);
                    self.cursor_hide_at = None;
                    self.cursor_hidden = false;
                    window.set_cursor_visible(true);
                }
                window.request_redraw();
                return;
            }
            let phone_shortcut = matches!(&event, WindowEvent::KeyboardInput { event, .. }
                if event.physical_key == PhysicalKey::Code(KeyCode::KeyP)
                    && event.state == ElementState::Pressed && !event.repeat
                    && !palette.shortcut() && !open
                    && self.overlay == Overlay::Driving);
            if phone_shortcut {
                palette.open_job_feed();
                self.keys.clear();
                self.stop_orbit_drag();
                self.accumulator = 0.;
                self.show_cursor();
                self.window.as_ref().unwrap().request_redraw();
                return;
            }
            let shortcut = matches!(&event, WindowEvent::KeyboardInput { event, .. }
                if event.physical_key == PhysicalKey::Code(KeyCode::KeyP)
                    && event.state == ElementState::Pressed && !event.repeat && palette.shortcut());
            if shortcut {
                self.audio.silence();
                self.sound_eye = None;
                if open {
                    palette.dismiss();
                } else {
                    palette.menu.open();
                }
                self.keys.clear();
                self.stop_orbit_drag();
                self.accumulator = 0.;
                self.show_cursor();
                self.window.as_ref().unwrap().request_redraw();
                return;
            }
            if open {
                // Keep platform lifecycle events flowing, but capture all game input.
                if matches!(
                    event,
                    WindowEvent::KeyboardInput { .. }
                        | WindowEvent::Ime(_)
                        | WindowEvent::MouseInput { .. }
                        | WindowEvent::MouseWheel { .. }
                        | WindowEvent::CursorMoved { .. }
                        | WindowEvent::Touch(_)
                ) {
                    if let WindowEvent::CursorMoved { position, .. } = &event {
                        self.cursor = Some([position.x as f32, position.y as f32]);
                    }
                    self.stop_orbit_drag();
                    self.show_cursor();
                    self.window.as_ref().unwrap().request_redraw();
                    return;
                }
            }
            if consumed
                && matches!(
                    event,
                    WindowEvent::MouseInput { .. }
                        | WindowEvent::MouseWheel { .. }
                        | WindowEvent::Touch(_)
                )
            {
                window.request_redraw();
                return;
            }
        }
        // The cinematic HUD owns pointer input, including the touch takeover
        // button. Do not grab the cursor or open the hidden minimap beneath it.
        if self.auto_drive.is_some()
            && matches!(
                event,
                WindowEvent::MouseInput { .. }
                    | WindowEvent::MouseWheel { .. }
                    | WindowEvent::Touch(_)
            )
        {
            return;
        }
        if scene_click && self.loadout.equipped.is_none() && !self.simulation_paused() {
            self.world.start_punch();
        }
        if self.handle_window_activity(&event) {
            return;
        }
        match event {
            #[cfg(target_arch = "wasm32")]
            WindowEvent::Touch(touch) if mobile_enabled() => self.mobile_scene_touch(touch),
            WindowEvent::CloseRequested => {
                self.kairos.save_now();
                event_loop.exit();
            }
            WindowEvent::CursorEntered { .. } => self.cursor_activity(Instant::now()),
            WindowEvent::CursorLeft { .. }
                if self.aim_mouse.relative()
                    || self.orbit_drag.as_ref().is_some_and(|drag| drag.captured) => {}
            WindowEvent::CursorLeft { .. } => {
                self.show_cursor();
                self.cursor = None;
                self.stop_orbit_drag();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if key == KeyCode::Escape {
                        if event.state == ElementState::Pressed && !event.repeat {
                            let next = self.overlay.escape();
                            if next == Overlay::Driving {
                                self.start_playing();
                            } else {
                                self.set_overlay(next);
                            }
                        }
                        return;
                    }
                    if self.overlay.paused() {
                        if self.overlay == Overlay::Map && event.state == ElementState::Pressed {
                            let steps = match key {
                                KeyCode::Equal | KeyCode::NumpadAdd => Some(1.),
                                KeyCode::Minus | KeyCode::NumpadSubtract => Some(-1.),
                                KeyCode::Digit0 | KeyCode::Numpad0 => {
                                    self.full_map.reset_zoom();
                                    Some(0.)
                                }
                                _ => None,
                            };
                            if let (Some(steps), Some(window)) = (steps, &self.window) {
                                let size = window.inner_size();
                                let (w, h) = (size.width as f32, size.height as f32);
                                self.full_map.zoom_at(w, h, [w * 0.5, h * 0.5 + 30.], steps);
                                window.request_redraw();
                                return;
                            }
                        }
                        match self.menu_key(key, event.state, event.repeat) {
                            Some(MenuItem::Map) => self.set_overlay(Overlay::Map),
                            Some(MenuItem::Settings) => {
                                if let Some(palette) = &mut self.palette {
                                    palette.settings_open = true;
                                }
                            }
                            Some(MenuItem::Exit) => self.exit_game(event_loop),
                            Some(MenuItem::Resume) => self.start_playing(),
                            None => {}
                        }
                        return;
                    }
                    if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter)
                        && event.state == ElementState::Pressed
                        && !event.repeat
                        && self.interact_with_cat()
                    {
                        return;
                    }
                    if event.state == ElementState::Pressed {
                        self.keys.insert(key);
                        if !event.repeat {
                            match key {
                                KeyCode::Digit1
                                | KeyCode::Digit2
                                | KeyCode::Digit0
                                | KeyCode::KeyR
                                    if !self.simulation_paused()
                                        && self.world.can_equip_weapon() =>
                                {
                                    match key {
                                        KeyCode::Digit1 => {
                                            self.loadout.equip(Some(weapons::Weapon::Rifle))
                                        }
                                        KeyCode::Digit2 => {
                                            self.loadout.equip(Some(weapons::Weapon::Handgun))
                                        }
                                        KeyCode::KeyR => {
                                            if !self.world.zombie_mode() {
                                                self.loadout.reload(self.game_time);
                                            }
                                        }
                                        _ => self.loadout.equip(None),
                                    }
                                    if self.loadout.equipped.is_some() {
                                        self.world.cancel_punch();
                                    }
                                    self.sync_aim_mouse();
                                }
                                KeyCode::KeyJ if !self.simulation_paused() => {
                                    self.world.attach_baggage_cart();
                                }
                                KeyCode::KeyK if !self.simulation_paused() => {
                                    self.world.detach_baggage_cart();
                                }
                                KeyCode::KeyO if !self.simulation_paused() => {
                                    self.world.toggle_door();
                                }
                                KeyCode::KeyF => {
                                    if self.world.toggle_vehicle(&self.hermes) {
                                        self.reset_boarding_camera();
                                    }
                                }
                                KeyCode::Tab => self.hood = !self.hood,
                                KeyCode::BracketLeft | KeyCode::BracketRight => {
                                    if let Some(renderer) = &mut self.renderer {
                                        let hour = renderer
                                            .sky_settings
                                            .hour(self.game_time, renderer.daylight);
                                        let current = renderer
                                            .daylight
                                            .unwrap_or((hour.min(24. - hour) / 12.).clamp(0., 1.));
                                        let delta = if key == KeyCode::BracketRight {
                                            0.1
                                        } else {
                                            -0.1
                                        };
                                        let daylight = (current + delta).clamp(0., 1.);
                                        renderer.daylight = Some(daylight);
                                        renderer.sky_settings.solar_hour = None;
                                        if let Some(window) = &self.window {
                                            window.set_title(&format!(
                                                "MTL Jobines - daylight {daylight:.1}"
                                            ));
                                        }
                                    }
                                }
                                KeyCode::KeyT => {
                                    if let Some(renderer) = &mut self.renderer {
                                        renderer.daylight = None;
                                        renderer.sky_settings.solar_hour = None;
                                    }
                                    if let Some(window) = &self.window {
                                        window.set_title("MTL Jobines - automatic daylight");
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else {
                        self.keys.remove(&key);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. }
                if self.overlay == Overlay::Map || !self.overlay.paused() =>
            {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 50.,
                };
                if self.overlay == Overlay::Map {
                    if let (Some(point), Some(window)) = (self.cursor, &self.window) {
                        let size = window.inner_size();
                        self.full_map
                            .zoom_at(size.width as f32, size.height as f32, point, y);
                        window.request_redraw();
                    }
                } else {
                    self.scroll_chase_camera(y);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => {
                if self.overlay == Overlay::Map && state == ElementState::Pressed {
                    if self.full_map.selection.is_route() {
                        self.full_map.selection = hud::MapSelection::Route;
                        self.hermes.clear_destination();
                    } else {
                        self.start_playing();
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(window) = &self.window {
                    let size = window.inner_size();
                    let (w, h) = (size.width as f32, size.height as f32);
                    let Some(point) = self.cursor.or_else(|| {
                        (self.overlay == Overlay::Driving).then_some([w * 0.5, h * 0.5])
                    }) else {
                        return;
                    };
                    match self.overlay {
                        Overlay::Driving
                            if !mobile_enabled() && hud::minimap_contains(w, h, point) =>
                        {
                            self.set_overlay(Overlay::Map)
                        }
                        Overlay::Map
                            if hud::contains(hud::full_map_close_rect(w), point)
                                || !hud::contains([20., 20., w - 40., h - 40.], point) =>
                        {
                            self.start_playing()
                        }
                        Overlay::Map => {
                            self.select_map_point(w, h, point);
                        }
                        Overlay::Driving if !mobile_enabled() => self.begin_orbit_drag(point),
                        _ => {}
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_activity(Instant::now());
                self.move_camera_with_cursor([position.x as f32, position.y as f32]);
            }
            _ => {}
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub fn run() -> Result<(), String> {
    run_with_client(None)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_with_client(rule: Option<crate::VehicleReturnFn>) -> Result<(), String> {
    let _capture = mtl_world::telemetry::start(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/traces"),
    )?;
    let args: Vec<_> = std::env::args().collect();
    let start = args
        .iter()
        .position(|a| a == "--start")
        .map(|i| {
            args.get(i + 1)
                .ok_or_else(|| "--start requires x,y[,heading-degrees]".to_string())?
                .parse::<SpawnPoint>()
        })
        .transpose()?;
    let option = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let path = option("--map").unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/map.road")
    });
    if !args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--build-game-data" | "--replay-reports" | "--export-minimap" | "--smoke"
        )
    }) {
        return native_startup::run(
            path,
            option("--game-data"),
            rule,
            start,
            args.iter().any(|arg| arg == "--profile-shutdown"),
        );
    }
    let started = Instant::now();
    eprintln!("Loading map: {}", path.display());
    let mut hermes = Hermes::load_with_game_data(&path, option("--game-data").as_deref())?;
    let city = hermes.city();
    eprintln!(
        "Loaded {} roads, {} buildings, {} routes in {:?}",
        city.roads.len(),
        city.query(Region::WholeMap).buildings.len(),
        city.paths.len(),
        started.elapsed()
    );
    if let Some(output) = option("--build-game-data") {
        hermes.write_game_data(&output)?;
        eprintln!(
            "Game data ready: {} ({:.1} MiB)",
            output.display(),
            std::fs::metadata(&output).map_err(|e| e.to_string())?.len() as f64 / 1048576.
        );
        return Ok(());
    }
    if let Some(reports) = option("--replay-reports") {
        let output = option("--replay-output").ok_or("--replay-output requires a directory")?;
        let landmark = args
            .iter()
            .position(|a| a == "--replay-landmark")
            .and_then(|i| args.get(i + 1))
            .map(String::as_str);
        return super::report_replay::run(&mut hermes, &reports, &output, landmark);
    }
    if let Some(path) = option("--export-minimap") {
        std::fs::write(&path, hermes.minimap_svg()).map_err(|e| e.to_string())?;
        if option("--smoke").is_none() {
            return Ok(());
        }
    }
    if let Some(output) = option("--smoke") {
        let size = args
            .iter()
            .position(|a| a == "--smoke-size")
            .map(|i| {
                let value = args
                    .get(i + 1)
                    .ok_or("--smoke-size requires WIDTHxHEIGHT")?;
                let (width, height) = value
                    .split_once('x')
                    .ok_or("--smoke-size requires WIDTHxHEIGHT")?;
                let width = width.parse::<u32>().map_err(|_| "invalid smoke width")?;
                let height = height.parse::<u32>().map_err(|_| "invalid smoke height")?;
                if width == 0 || height == 0 || width > 8192 || height > 8192 {
                    return Err("smoke dimensions must be between 1 and 8192");
                }
                Ok((width, height))
            })
            .transpose()?
            .unwrap_or((1280, 800));
        return smoke::run(
            &mut hermes,
            output,
            SmokeOptions {
                overview: args.iter().any(|a| a == "--overview"),
                vehicle_lineup: args.iter().any(|a| a == "--vehicle-lineup"),
                vehicle_lamps: args.iter().any(|a| a == "--vehicle-lamps"),
                hood: args.iter().any(|a| a == "--hood"),
                profile: args
                    .iter()
                    .any(|a| a == "--profile" || a == "--profile-120"),
                profile_cpu: args.iter().any(|a| a == "--profile-cpu"),
                realtime: args.iter().any(|a| a == "--profile-realtime"),
                target_120: args.iter().any(|a| a == "--profile-120"),
                night: args.iter().any(|a| a == "--night"),
                start,
                size,
                sky: SkyOverrides::from_args(&args)?,
            },
        );
    }
    unreachable!("normal startup is handled before loading the map")
}

#[cfg(target_arch = "wasm32")]
async fn web_loading_stage(message: &str) {
    let Some(window) = web_sys::window() else {
        return;
    };
    if let Some(label) = window
        .document()
        .and_then(|d| d.get_element_by_id("loading-message"))
    {
        label.set_text_content(Some(message));
        // Leave time for the browser to paint before the next synchronous Wasm phase.
        // A timer also resolves in tabs where animation frames are suspended.
        let delay = js_sys::Promise::new(&mut |resolve, reject| {
            if let Err(error) =
                window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 32)
            {
                let _ = reject.call1(&wasm_bindgen::JsValue::UNDEFINED, &error);
            }
        });
        let _ = wasm_bindgen_futures::JsFuture::from(delay).await;
    }
}

#[cfg(target_arch = "wasm32")]
fn web_status(error: Option<&str>) {
    if let Some(element) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("loading"))
    {
        if let Some(message) = error {
            if let Some(label) = element.query_selector("#loading-message").ok().flatten() {
                label.set_text_content(Some(message));
            }
        }
        element
            .set_attribute(
                "data-state",
                if error.is_some() { "error" } else { "ready" },
            )
            .ok();
    }
}

#[cfg(target_arch = "wasm32")]
pub fn start_web(rule: Option<js_sys::Function>) -> Result<(), String> {
    use winit::platform::web::EventLoopExtWebSys;
    let hermes = Hermes::embedded()?;
    let search = web_sys::window()
        .ok_or("Browser window unavailable")?
        .location()
        .search()
        .map_err(|_| "Could not read launch position")?;
    let start = search
        .trim_start_matches('?')
        .split('&')
        .find_map(|part| part.strip_prefix("start="))
        .map(str::parse::<SpawnPoint>)
        .transpose()?;
    let event_loop = EventLoop::<GameEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let proxy = event_loop.create_proxy();
    mobile::connect(proxy.clone());
    let mut game = Game::new_at(hermes, start, proxy)?;
    if let Some(rule) = rule {
        game.kairos
            .set_vehicle_return_rule(move |away, previous, elapsed| {
                let value = rule
                    .call3(
                        &wasm_bindgen::JsValue::UNDEFINED,
                        &u32::from(away).into(),
                        &previous.into(),
                        &elapsed.into(),
                    )
                    .unwrap_or_else(|error| wasm_bindgen::throw_val(error));
                value
                    .as_f64()
                    .unwrap_or_else(|| wasm_bindgen::throw_str("client rule must return seconds"))
                    as f32
            });
    }
    event_loop.spawn_app(game);
    Ok(())
}

#[cfg(test)]
mod tests;
