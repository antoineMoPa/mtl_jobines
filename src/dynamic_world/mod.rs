//! Mutable game entities. OpenCity's City snapshot is static geography, not
//! the owner of vehicles, players, traffic, or future mission objects.
mod buildings;
pub mod catalog;
mod contacts;
mod crowd;
mod cycling_pose;
mod cyclists;
mod illumination;
pub mod pedestrian_catalog;
pub mod pedestrian_gait;
pub use illumination::LocalLight;
#[cfg(any(test, feature = "test-support"))]
pub use illumination::RearLight;
mod landmark_respawn;
mod lineup;
mod metro;
pub(crate) mod parked;
mod player;
mod props;
mod ragdoll;
mod service_passengers;
pub use props::PropBatch;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod physics_lod_tests;
pub mod snow;
mod spawn;
#[cfg(test)]
mod street_tests;
mod suspension;
mod traffic;
pub use spawn::SpawnPoint;
pub use traffic::SignalBatch;
#[cfg(test)]
mod handbrake_tests;
mod vehicle;
mod vehicle_lights;
mod vehicle_switch;
use crate::hermes::Hermes;
use glam::Vec3;
use opencity::runtime::Mesh;
use player::Pedestrian;
pub const PEDESTRIAN_CAPACITY: usize =
    crowd::CAPACITY + crowd::KIOSK_CAPACITY + cyclists::CAPACITY + 5;
pub use player::WalkControls;
pub use player::TURN_SPEED as WALK_TURN_SPEED;
pub use vehicle::{Controls, Vehicle, METERS_PER_UNIT};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VehicleId(usize);
#[derive(Default)]
pub struct StepProfile {
    pub stages_ns: [u64; 8],
    pub crowd_ns: [u64; 13],
}
pub struct DynamicWorld {
    season: crate::season::Season,
    pub snow: snow::Tracks,
    snow_timer: f32,
    sound_impacts: [crate::audio::Impact; crate::audio::IMPACT_VOICES],
    sound_serial: u64,
    sound_foliage: f32,
    sound_geography_timer: f32,
    metro: metro::Metro,
    vehicles: Vec<Vehicle>,
    // Authored parking occupies the slots after the initial player, before traffic.
    parked_vehicle_count: usize,
    player_vehicle: VehicleId,
    vehicle_notice: f32,
    pedestrian: Option<Pedestrian>,
    walking_spawn: Option<SpawnPoint>,
    vehicle_spawns: Vec<SpawnPoint>,
    snow_events: Vec<crate::kairos::SnowCell>,
    props: props::Props,
    traffic: traffic::Traffic,
    crowd: crowd::Crowd,
    cyclists: cyclists::Cyclists,
    service_passengers: service_passengers::ServicePassengers,
    buildings: buildings::Buildings,
    building_candidates: Vec<usize>,
}
impl DynamicWorld {
    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub fn new(hermes: &Hermes) -> Self {
        Self::with_start(hermes, None).expect("default map spawn")
    }
    pub fn with_start(hermes: &Hermes, start: Option<SpawnPoint>) -> Result<Self, String> {
        Self::with_vehicle_lineup(hermes, start, false)
    }
    pub fn with_vehicle_lineup(
        hermes: &Hermes,
        start: Option<SpawnPoint>,
        vehicle_lineup: bool,
    ) -> Result<Self, String> {
        let walking_start = start.is_none().then(|| hermes.walking_start()).flatten();
        let start = start.or(hermes.default_start()).or_else(|| {
            let city = hermes.city();
            (!city.roads.iter().any(|road| {
                road.definition
                    .elements
                    .iter()
                    .any(|element| element.kind == opencity::RoadElementKind::CarLane)
            }))
            .then(|| SpawnPoint {
                point: [
                    (city.bounds[0] + city.bounds[2]) * 0.5,
                    (city.bounds[1] + city.bounds[3]) * 0.5,
                ],
                heading: Some(0.),
            })
        });
        let mut vehicle = match start {
            Some(point) => Vehicle::spawn_at(hermes.city(), point)?,
            None => Vehicle::spawn(hermes.city()),
        };
        //TEMPORARY: start on the shared bicycle while testing its handling.
        vehicle.set_catalog_model(Some(
            catalog::definitions()
                .iter()
                .position(|definition| definition.config.research_id == catalog::BICYCLE_ID)
                .expect("bundled shared bicycle test vehicle"),
        ));
        let prop_center = walking_start.map_or(vehicle.position, |point| {
            Vec3::new(
                point.point[0] * METERS_PER_UNIT,
                point.point[1] * METERS_PER_UNIT,
                vehicle.position.z,
            )
        });
        let mut world = Self {
            season: Default::default(),
            snow: snow::Tracks::default(),
            snow_timer: 0.,
            sound_impacts: [crate::audio::Impact::default(); crate::audio::IMPACT_VOICES],
            sound_serial: 0,
            sound_foliage: 0.15,
            sound_geography_timer: 0.,
            traffic: traffic::Traffic::new_near(hermes.traffic_paths(), prop_center),
            crowd: crowd::Crowd::new(hermes.walking_paths()),
            cyclists: cyclists::Cyclists::new(hermes),
            service_passengers: Default::default(),
            buildings: buildings::Buildings::new(hermes.city()),
            building_candidates: Vec::new(),
            metro: metro::Metro::new(hermes),
            vehicles: vec![vehicle],
            parked_vehicle_count: 0,
            player_vehicle: VehicleId(0),
            vehicle_notice: 4.,
            pedestrian: None,
            walking_spawn: None,
            vehicle_spawns: Vec::new(),
            snow_events: Vec::new(),
            props: props::Props::from_parts_near(hermes.prop_parts(), prop_center),
        };
        world.traffic.update_prop_focus(prop_center);
        world.add_parked_vehicles(hermes)?;
        if vehicle_lineup {
            world.add_vehicle_lineup(hermes);
        }
        world
            .cyclists
            .populate(hermes, &mut world.vehicles, prop_center);
        world.vehicle_spawns = world
            .vehicles
            .iter()
            .map(|v| SpawnPoint {
                point: (v.position.truncate() / METERS_PER_UNIT).to_array(),
                heading: Some(v.yaw),
            })
            .collect();
        if let Some(start) = start {
            // Preserve authored coordinates instead of rounding through game metres.
            world.vehicle_spawns[0].point = start.point;
        }
        if let Some(point) = walking_start {
            let pedestrian = Pedestrian::at(hermes.city(), point)?;
            if world.props.blocked(pedestrian.position)
                || world
                    .vehicles
                    .iter()
                    .any(|vehicle| player::blocked_by_vehicle(pedestrian.position, vehicle))
            {
                return Err("Selected walking start overlaps a prop or vehicle".into());
            }
            world.pedestrian = Some(pedestrian);
            world.walking_spawn = Some(point);
        }
        world.populate_traffic(hermes);
        world.crowd.populate(
            hermes.city(),
            world.player_position(),
            &world.vehicles,
            &world.props,
        );
        world.cyclists.update_poses(
            &world.vehicles,
            world.player_vehicle.0,
            world.is_driving(),
            world.season,
        );
        Ok(world)
    }
    fn populate_traffic(&mut self, hermes: &Hermes) {
        let center = self.player_position();
        self.traffic
            .populate(hermes.city(), &mut self.vehicles, center);
        // NPCs can be entered when stopped; reset must retain a valid lane spawn.
        while self.vehicle_spawns.len() < self.vehicles.len() {
            let v = &self.vehicles[self.vehicle_spawns.len()];
            self.vehicle_spawns.push(SpawnPoint {
                point: (v.position.truncate() / METERS_PER_UNIT).to_array(),
                heading: Some(v.yaw),
            });
        }
        for d in self.traffic.take_spawns() {
            self.vehicle_spawns[d.0] = d.1;
        }
    }
    pub fn season(&self) -> crate::season::Season {
        self.season
    }
    pub fn take_snow_events(&mut self) -> Vec<crate::kairos::SnowCell> {
        std::mem::take(&mut self.snow_events)
    }
    pub fn clear_service_passengers(&mut self) {
        self.service_passengers.clear();
    }
    pub fn spawn_service_passenger(&mut self, id: u32, point: Vec3) {
        self.service_passengers.spawn(id, point);
    }
    pub fn begin_service(
        &mut self,
        board: &[u32],
        alight: &[u32],
        sidewalk: Vec3,
        bus: bool,
    ) -> bool {
        let vehicle = &self.vehicles[self.player_vehicle.0];
        self.service_passengers
            .begin(board, alight, vehicle, sidewalk, bus)
    }
    pub fn take_service_finished(&mut self) -> bool {
        self.service_passengers.take_finished()
    }
    pub fn set_season(&mut self, hermes: &Hermes, season: crate::season::Season) {
        if self.season == season {
            return;
        }
        self.season = season;
        self.snow.reset();
        self.snow_timer = 0.;
        self.traffic
            .set_season(season, &mut self.vehicles, self.player_vehicle.0);
        self.populate_traffic(hermes);
    }
    pub fn player(&self) -> &Vehicle {
        &self.vehicles[self.player_vehicle.0]
    }
    pub fn player_model_id(&self) -> Option<&str> {
        self.player()
            .definition()
            .map(|model| model.config.research_id.as_str())
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn retain_player_vehicle(&mut self, previous: &Self) {
        self.vehicles[self.player_vehicle.0].set_catalog_model(previous.player().definition);
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn preview_vehicle_lamps(&mut self, model: usize, controls: Controls) {
        for (id, vehicle) in self.vehicles.iter_mut().enumerate() {
            if id == self.player_vehicle.0 {
                vehicle.set_catalog_model(Some(model));
                vehicle.sound_controls = controls;
                vehicle.sound_running = true;
                vehicle.velocity = Vec3::ZERO;
            } else {
                vehicle.position.z = -1000.;
            }
        }
    }
    pub fn player_foot_motion(&self) -> Option<pedestrian_gait::FootMotion> {
        self.pedestrian.as_ref().map(Pedestrian::foot_motion)
    }
    pub fn is_driving(&self) -> bool {
        self.pedestrian.is_none()
    }
    pub fn vehicle_name_notice(&self) -> Option<&str> {
        (self.is_driving() && self.vehicle_notice > 0.)
            .then(|| self.player().definition().map(|d| d.config.name.as_str()))
            .flatten()
    }
    pub fn player_position(&self) -> Vec3 {
        self.pedestrian
            .as_ref()
            .map_or(self.player().position, |p| p.position)
    }
    pub fn player_yaw(&self) -> f32 {
        self.pedestrian
            .as_ref()
            .map_or(self.player().yaw, |p| p.yaw)
    }
    pub fn camera_eye(&self, hermes: &Hermes, anchor: Vec3, desired: Vec3) -> Vec3 {
        let eye = self.buildings.camera_eye(hermes.city(), anchor, desired);
        self.metro.camera_eye(anchor, eye)
    }
    /// Snapshot only nearby active vehicles. Parked lineup cars have no engine.
    /// Slots are bounded, ordered by distance; slot zero reserves the player.
    pub fn sound_frame(
        &self,
        eye: Vec3,
        forward: Vec3,
        velocity: Vec3,
        settings: crate::audio::Settings,
    ) -> crate::audio::Frame {
        use crate::audio::{Car, Frame, CAR_VOICES};
        let mut frame = Frame {
            eye,
            forward,
            velocity,
            settings,
            impacts: self.sound_impacts,
            ..Frame::default()
        };
        frame.foliage = self.sound_foliage;
        frame.wind_speed = if self.is_driving() {
            self.player().velocity.length()
        } else {
            0.
        };
        let mut distances = [f32::INFINITY; CAR_VOICES];
        for (id, v) in self.vehicles.iter().enumerate() {
            if self.traffic.inactive(id) || (!v.sound_running && v.velocity.length_squared() < 0.25)
            {
                continue;
            }
            let distance = v.position.distance_squared(eye);
            if distance > 140. * 140. {
                continue;
            }
            let player = self.is_driving() && id == self.player_vehicle.0;
            let rank = if player { -1. } else { distance };
            let Some(slot) = distances.iter().position(|d| rank < *d) else {
                continue;
            };
            for i in (slot + 1..CAR_VOICES).rev() {
                distances[i] = distances[i - 1];
                frame.cars[i] = frame.cars[i - 1];
            }
            distances[slot] = rank;
            frame.cars[slot] = Car {
                id,
                position: v.position,
                velocity: v.velocity,
                yaw: v.yaw,
                throttle: v.sound_controls.throttle,
                brake: v.sound_controls.brake,
                handbrake: v.sound_controls.handbrake,
                grounded: v.grounded,
                surface: v.surface,
                sound: v
                    .definition()
                    .map_or_else(crate::audio::VehicleSound::default, |definition| {
                        definition.config.sound
                    }),
                wheel_radius: v
                    .definition()
                    .map_or(0.32, |definition| definition.config.physics.wheel_radius),
                horn: false,
                player,
                engine_on: v.sound_running,
            };
        }
        frame
    }
    pub fn toggle_vehicle(&mut self, hermes: &Hermes) -> bool {
        if self.metro.aboard {
            return false;
        }
        if let Some(player) = &self.pedestrian {
            if let Some((index, _)) = self
                .vehicles
                .iter()
                .enumerate()
                .filter(|(_, vehicle)| {
                    vehicle.velocity.length() <= 3. && vehicle.entry_distance(player.position) <= 2.
                })
                .min_by(|(_, a), (_, b)| {
                    a.entry_distance(player.position)
                        .total_cmp(&b.entry_distance(player.position))
                })
            {
                self.traffic.release(index);
                self.cyclists.release(index);
                self.player_vehicle = VehicleId(index);
                self.pedestrian = None;
                self.vehicle_notice = 4.;
                return true;
            }
        } else if self.player().velocity.length() <= 3. {
            self.pedestrian = Pedestrian::beside_if(hermes.city(), self.player(), |position| {
                self.props.blocked(position)
                    || self
                        .vehicles
                        .iter()
                        .any(|v| player::blocked_by_vehicle(position, v))
            });
            return self.pedestrian.is_some();
        }
        false
    }
    pub fn reset_player(&mut self, hermes: &Hermes) {
        self.metro.aboard = false;
        let definition = self.player().definition;
        self.vehicles[self.player_vehicle.0] =
            Vehicle::spawn_at(hermes.city(), self.vehicle_spawns[self.player_vehicle.0])
                .expect("validated parking position");
        self.vehicles[self.player_vehicle.0].set_catalog_model(definition);
        self.pedestrian = self
            .walking_spawn
            .map(|start| Pedestrian::at(hermes.city(), start).expect("validated walking start"));
        self.vehicle_notice = 4.;
    }
    pub fn step(&mut self, hermes: &Hermes, input: Controls, walk: WalkControls, dt: f32) {
        self.step_inner(hermes, input, walk, dt, None);
    }
    pub fn step_profiled(
        &mut self,
        hermes: &Hermes,
        input: Controls,
        walk: WalkControls,
        dt: f32,
        profile: &mut StepProfile,
    ) {
        self.step_inner(hermes, input, walk, dt, Some(profile));
    }
    fn step_inner(
        &mut self,
        hermes: &Hermes,
        input: Controls,
        walk: WalkControls,
        dt: f32,
        mut profile: Option<&mut StepProfile>,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let mut mark = profile.as_ref().map(|_| web_time::Instant::now());
        macro_rules! stage {
            ($index:expr) => {
                if let (Some(profile), Some(start)) = (profile.as_deref_mut(), mark.as_mut()) {
                    profile.stages_ns[$index] += start.elapsed().as_nanos() as u64;
                    *start = web_time::Instant::now();
                }
            };
        }
        self.metro.step(dt, self.pedestrian.as_mut());
        self.vehicle_notice = (self.vehicle_notice - dt).max(0.);
        if self.traffic.needs_refill(dt) {
            self.populate_traffic(hermes);
        }
        self.sound_geography_timer -= dt;
        if self.sound_geography_timer <= 0. {
            self.sound_geography_timer = 0.5;
            let p = self.player_position() / METERS_PER_UNIT;
            let mut foliage = 0.;
            for offset in [
                glam::Vec2::ZERO,
                glam::Vec2::X * 0.3,
                -glam::Vec2::X * 0.3,
                glam::Vec2::Y * 0.3,
                -glam::Vec2::Y * 0.3,
            ] {
                let sample = hermes
                    .city()
                    .surface_at((p.truncate() + offset).to_array(), p.z + 0.1);
                foliage += match sample.map(|s| s.kind) {
                    Some(opencity::runtime::SurfaceKind::Grass) => 1.,
                    Some(opencity::runtime::SurfaceKind::Sand) => 0.5,
                    _ => 0.,
                };
            }
            self.sound_foliage = foliage / 5.;
        }
        let driving = self.is_driving();
        let center = self.player_position();
        let center_velocity = if driving {
            self.player().velocity
        } else {
            Vec3::ZERO
        };
        self.cyclists
            .refresh(hermes, &mut self.vehicles, center, dt);
        while self.vehicle_spawns.len() < self.vehicles.len() {
            let v = &self.vehicles[self.vehicle_spawns.len()];
            self.vehicle_spawns.push(SpawnPoint {
                point: (v.position.truncate() / METERS_PER_UNIT).to_array(),
                heading: Some(v.yaw),
            });
        }
        for (id, start) in self.cyclists.take_spawns() {
            self.vehicle_spawns[id] = start;
        }
        self.props.update_focus(center);
        self.traffic.update_prop_focus(center);
        self.traffic.select_physics(
            &self.vehicles,
            self.player_vehicle.0,
            center,
            center_velocity,
        );
        stage!(0);
        let mut ai = self.traffic.controls(
            hermes.city(),
            &self.vehicles,
            self.pedestrian.as_ref().map(|p| p.position),
            dt,
        );
        ai.extend(self.cyclists.controls(
            &self.traffic,
            &self.vehicles,
            center,
            self.player_vehicle.0,
        ));
        stage!(1);
        for (i, vehicle) in self.vehicles.iter_mut().enumerate() {
            let signal = if driving && i == self.player_vehicle.0 {
                hermes.route_signal()
            } else {
                crate::hermes::RouteSignal::Off
            };
            vehicle.update_route_signal(signal, dt);
            vehicle.sound_running = false;
            vehicle.sound_cooldown = (vehicle.sound_cooldown - dt).max(0.);
            if self.traffic.inactive(i) || self.cyclists.inactive(i) {
                continue;
            }
            let controls = if driving && i == self.player_vehicle.0 {
                input
            } else if let Some((_, controls)) = ai.iter().find(|(id, _)| *id == i) {
                *controls
            } else {
                Controls {
                    handbrake: true,
                    ..Controls::default()
                }
            };
            vehicle.sound_controls = controls;
            vehicle.sound_running =
                (driving && i == self.player_vehicle.0) || ai.iter().any(|(id, _)| *id == i);
            if i != self.player_vehicle.0
                && (self.traffic.step_lane(i, vehicle, controls, dt)
                    || self
                        .traffic
                        .sleep_if_settled(i, vehicle, controls, center, dt))
            {
                continue;
            }
            let before = vehicle.position;
            let reach =
                vehicle.length().hypot(vehicle.width()) * 0.5 + vehicle.velocity.length() * dt + 1.;
            self.buildings.nearby(
                vehicle.position.truncate(),
                reach,
                &mut self.building_candidates,
            );
            vehicle.step_with_buildings(
                hermes.city(),
                controls,
                dt,
                Some(&self.building_candidates),
            );
            self.props.vehicle_move(vehicle, before, hermes.city());
            self.traffic.vehicle_move(vehicle, before, hermes.city());
        }
        stage!(2);
        let contacts = contacts::resolve_with_contacts(&mut self.vehicles);
        self.traffic
            .record_contacts(&contacts, self.player_vehicle.0);
        stage!(3);
        for hit in &mut self.sound_impacts {
            hit.age += dt;
        }
        for vehicle in &mut self.vehicles {
            for hit in &vehicle.sound_impacts[..vehicle.sound_impact_count] {
                self.sound_serial = self.sound_serial.wrapping_add(1).max(1);
                let slot = (self.sound_serial as usize - 1) % crate::audio::IMPACT_VOICES;
                self.sound_impacts[slot] = crate::audio::Impact {
                    serial: self.sound_serial,
                    ..*hit
                };
            }
            vehicle.sound_impact_count = 0;
        }
        if let Some(player) = &mut self.pedestrian {
            let before = player.position;
            self.metro.walk(hermes, player, walk, dt, |position| {
                self.props.blocked(position)
                    || self
                        .vehicles
                        .iter()
                        .any(|v| player::blocked_by_vehicle(position, v))
            });
            self.crowd
                .player_move(hermes.city(), before, player.position, dt, &self.vehicles);
        }
        stage!(4);
        self.props.step(dt);
        stage!(5);
        if let Some(profile) = profile.as_deref_mut() {
            self.crowd.step_profiled(
                hermes.city(),
                self.player_position(),
                self.pedestrian.is_some(),
                &self.vehicles,
                &self.props,
                dt,
                &mut profile.crowd_ns,
            );
        } else {
            self.crowd.step(
                hermes.city(),
                self.player_position(),
                self.pedestrian.is_some(),
                &self.vehicles,
                &self.props,
                dt,
            );
        }
        stage!(6);
        self.service_passengers.step(dt);
        self.traffic.step_debris(dt);
        if self.season == crate::season::Season::Winter {
            self.snow_timer += dt;
            if self.snow_timer >= 0.1 {
                self.snow_timer = 0.;
                self.snow_events.extend(self.snow.update(
                    hermes.city(),
                    &self.vehicles,
                    self.player_vehicle.0,
                ));
            }
        }
        if self.player_position().z < -40. {
            self.reset_player(hermes);
        }
        self.cyclists.update_poses(
            &self.vehicles,
            self.player_vehicle.0,
            self.is_driving(),
            self.season,
        );
        stage!(7);
    }
    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub fn use_full_traffic_physics(&mut self) {
        self.traffic.use_full_physics();
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn traffic_physics_counts(&self) -> [usize; 3] {
        self.traffic.physics_counts(self.vehicles.len())
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn preview_bicycle_pose(&mut self, pedal: f32, steering: f32, standing: f32) {
        self.vehicles[self.player_vehicle.0].riding_stand = standing;
        self.vehicles[self.player_vehicle.0].pedal_angle = pedal;
        self.vehicles[self.player_vehicle.0].steering = steering;
        self.cyclists.update_poses(
            &self.vehicles,
            self.player_vehicle.0,
            self.is_driving(),
            self.season,
        );
    }
    pub fn pedestrian_instances(&self) -> impl Iterator<Item = crowd::Instance<'_>> + '_ {
        self.pedestrian
            .iter()
            .map(|player| player.instance(self.season))
            .chain(self.crowd.instances(self.player_position()))
            .chain(self.service_passengers.instances())
            .chain(
                self.cyclists
                    .instances(&self.vehicles, self.player_position()),
            )
    }
    pub fn waiting_service_passengers(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.service_passengers.waiting_positions()
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn prepare_pedestrian_collision(&mut self) -> glam::Vec3 {
        let (target, yaw) = self.crowd.prepare_collision();
        let direction = glam::Vec3::new(yaw.cos(), yaw.sin(), 0.);
        let vehicle = &mut self.vehicles[self.player_vehicle.0];
        vehicle.position = target - direction * 5. + glam::Vec3::Z * vehicle.ride_height();
        vehicle.yaw = yaw;
        vehicle.velocity = direction * 8.;
        target
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn prepare_player_pedestrian_collision(&mut self, hermes: &Hermes) -> Vec3 {
        let (target, yaw) = self.crowd.prepare_collision();
        let direction = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        self.pedestrian = Some(Pedestrian::new(
            hermes.city(),
            target - direction * 1.6 + Vec3::Z * player::HALF_HEIGHT,
            yaw,
        ));
        target
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn pedestrian_reactions(&self) -> [usize; 3] {
        self.crowd.reaction_counts()
    }
    #[cfg(test)]
    pub fn mesh(&self) -> Mesh {
        let mut mesh = Mesh::default();
        self.write_mesh(&mut mesh);
        mesh
    }
    /// Replace the current frame's vertices while retaining the caller's capacity.
    #[cfg(test)]
    pub fn write_mesh(&self, mesh: &mut Mesh) {
        self.write_actors(mesh);
        self.props.append_visible(mesh, self.player_position());
    }
    /// Exact visible intact-prop selections, without copying their vertices.
    pub fn visible_prop_batches(&self) -> Vec<PropBatch> {
        self.props.visible_batches(self.player_position())
    }
    pub fn authored_prop_instances(&self) -> Vec<crate::props::Instance> {
        self.props.authored_instances(self.player_position())
    }
    pub fn write_prop_batch(&self, batch: &PropBatch, output: &mut Mesh) {
        self.props.write_batch(batch, output);
    }
    /// Vehicle lamps, pedestrian and moving debris. Standing scenery uses cached batches.
    pub fn write_moving_mesh(&self, mesh: &mut Mesh) {
        self.write_moving_mesh_with_player_pose(mesh, glam::Mat4::IDENTITY);
    }
    pub fn write_moving_mesh_with_player_pose(
        &self,
        mesh: &mut Mesh,
        player_pose_delta: glam::Mat4,
    ) {
        self.write_non_vehicle_actors_with_player_pose(mesh, player_pose_delta);
        self.props.append_moving(mesh, self.player_position());
    }
    pub fn standing_signal_batches(&self) -> impl Iterator<Item = SignalBatch> + '_ {
        self.traffic.standing_signal_batches(self.player_position())
    }
    pub fn write_signal_batch(&self, batch: &SignalBatch, output: &mut Mesh) {
        output.vertices.clear();
        self.traffic.append_signal_batch(batch, output);
    }
    #[cfg(test)]
    fn write_actors(&self, mesh: &mut Mesh) {
        self.write_non_vehicle_actors(mesh);
        for batch in self.standing_signal_batches() {
            self.traffic.append_signal_batch(&batch, mesh);
        }
        for (index, vehicle) in self.vehicles.iter().enumerate() {
            if self.vehicle_in_render_range(index, vehicle) {
                vehicle.append_mesh(mesh);
            }
        }
    }
    fn vehicle_in_render_range(&self, index: usize, vehicle: &Vehicle) -> bool {
        if self.cyclists.inactive(index) {
            return false;
        }
        let distance =
            if index != self.player_vehicle.0 && (1..=self.parked_vehicle_count).contains(&index) {
                parked::RENDER_DISTANCE
            } else {
                450.
            };
        vehicle.position.distance_squared(self.player_position())
            <= crate::performance::draw_radius(distance).powi(2)
    }
    pub fn vehicle_instances(
        &self,
    ) -> impl Iterator<Item = (usize, Vec<glam::Mat4>, f32, bool, bool, u8)> + '_ {
        self.vehicles
            .iter()
            .enumerate()
            .filter(|(index, v)| self.vehicle_in_render_range(*index, v))
            .filter_map(|(index, v)| {
                v.definition.map(|id| {
                    (
                        id,
                        catalog::definitions()[id].part_transforms(v),
                        self.traffic.opacity(index),
                        v.sound_running,
                        index == self.player_vehicle.0,
                        catalog::definitions()[id].paint_variant(index),
                    )
                })
            })
            .chain(
                self.metro
                    .train
                    .iter()
                    .filter(|train| train.position.distance(self.player_position()) < 450.)
                    .map(|v| {
                        let id = v.definition.expect("metro model");
                        (
                            id,
                            catalog::definitions()[id].part_transforms(v),
                            1.,
                            false,
                            false,
                            0,
                        )
                    }),
            )
    }
    #[cfg(test)]
    fn write_non_vehicle_actors(&self, mesh: &mut Mesh) {
        self.write_non_vehicle_actors_with_player_pose(mesh, glam::Mat4::IDENTITY);
    }
    fn write_non_vehicle_actors_with_player_pose(
        &self,
        mesh: &mut Mesh,
        player_pose_delta: glam::Mat4,
    ) {
        mesh.vertices.clear();
        for (index, vehicle) in self.vehicles.iter().enumerate() {
            if self.vehicle_in_render_range(index, vehicle) {
                let start = mesh.vertices.len();
                vehicle.append_lights(mesh);
                if index == self.player_vehicle.0 {
                    // Rear lenses share the body's interpolated render pose.
                    for vertex in &mut mesh.vertices[start..] {
                        vertex.position = player_pose_delta
                            .transform_point3(Vec3::from_array(vertex.position))
                            .to_array();
                        vertex.normal = player_pose_delta
                            .transform_vector3(Vec3::from_array(vertex.normal))
                            .to_array();
                    }
                }
            }
        }
        self.traffic
            .append_signal_debris(mesh, self.player_position());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temporary_default_player_vehicle_is_shared_bicycle() {
        let program = opencity::parse(include_str!("../../data/maps/mtl/map.road")).unwrap();
        assert!(
            SpawnPoint::walking_from_program(&program)
                .unwrap()
                .is_none(),
            "test map must begin mounted, not at pedestrian_start"
        );
        assert!(SpawnPoint::from_program(&program).unwrap().is_some());
        let world = DynamicWorld::new(&Hermes::test_map());
        assert!(world.is_driving());
        assert_eq!(
            world.player().definition().unwrap().config.name,
            "Vélo partagé"
        );
    }
    #[test]
    fn on_foot_world_steps_knock_down_contacted_pedestrians() {
        let hermes = Hermes::test_map();
        for run in [false, true] {
            let mut world = DynamicWorld::new(&hermes);
            world.prepare_player_pedestrian_collision(&hermes);
            assert!(!world.is_driving());
            let mut falling = false;
            for _ in 0..120 {
                world.step(
                    &hermes,
                    Controls::default(),
                    WalkControls {
                        forward: 1.,
                        run,
                        ..Default::default()
                    },
                    1. / 120.,
                );
                falling |= world.pedestrian_reactions()[1] == 1;
                if falling {
                    break;
                }
            }
            assert!(
                falling,
                "walking contact did not reach the ragdoll system (run={run})"
            );
        }
    }
    #[test]
    fn sound_snapshot_uses_real_drivers_controls_and_bounded_recent_events() {
        let hermes = Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        world.step(
            &hermes,
            Controls {
                throttle: 0.7,
                handbrake: true,
                ..Controls::default()
            },
            WalkControls::default(),
            1. / 120.,
        );
        let eye = world.player_position();
        let frame = world.sound_frame(eye, Vec3::X, Vec3::ZERO, crate::audio::Settings::default());
        assert_eq!(frame.cars[0].id, world.player_vehicle.0);
        assert!(frame.cars[0].player && frame.cars[0].engine_on);
        assert_eq!(frame.cars[0].throttle, 0.7);
        assert!(frame.cars[0].handbrake);
        assert_eq!(frame.cars[0].position, world.player_position());
        assert!(frame
            .cars
            .iter()
            .filter(|car| car.id != usize::MAX)
            .all(|car| world.vehicles[car.id].sound_running
                || world.vehicles[car.id].velocity.length_squared() >= 0.25));
        for _ in 0..5 {
            world.vehicles[0].sound_impact(12., crate::audio::Material::Fence, true);
            world.step(
                &hermes,
                Controls::default(),
                WalkControls::default(),
                1. / 120.,
            );
        }
        let hits = world
            .sound_frame(eye, Vec3::X, Vec3::ZERO, crate::audio::Settings::default())
            .impacts;
        assert_eq!(hits.len(), 3);
        assert!(hits.iter().all(|hit| hit.serial > 0
            && hit.broken
            && hit.material == crate::audio::Material::Fence));
        for (slot, hit) in hits.iter().enumerate() {
            assert_eq!((hit.serial as usize - 1) % 3, slot);
        }
    }
    #[test]
    fn entering_requires_proximity_and_fast_exit_is_rejected() {
        let hermes = Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        let expected_name = world.player().definition().unwrap().config.name.clone();
        assert_eq!(world.vehicle_name_notice(), Some(expected_name.as_str()));
        world.vehicle_notice = 0.;
        assert!(world.vehicle_name_notice().is_none());
        world.vehicles[0].velocity = Vec3::X * 10.;
        assert!(!world.toggle_vehicle(&hermes));
        assert!(world.is_driving());
        world.vehicles[0].velocity = Vec3::ZERO;
        assert!(world.toggle_vehicle(&hermes));
        assert!(!world.is_driving());
        assert!(world.vehicle_name_notice().is_none());
        assert_ne!(world.player_position(), world.player().position);
        assert!(world.toggle_vehicle(&hermes));
        assert!(world.is_driving());
        assert_eq!(world.vehicle_name_notice(), Some(expected_name.as_str()));
        assert!(world.toggle_vehicle(&hermes));
        let beyond_lineup = world
            .vehicles
            .iter()
            .map(|v| v.position.x)
            .fold(f32::NEG_INFINITY, f32::max)
            + 100.;
        world.pedestrian.as_mut().unwrap().position.x = beyond_lineup;
        assert!(!world.toggle_vehicle(&hermes));
        assert!(!world.is_driving());
        world.reset_player(&hermes);
        assert!(world.is_driving());
    }
    #[test]
    fn reset_preserves_explicit_start_and_restores_driving() {
        let hermes = Hermes::test_map();
        let initial = Vehicle::spawn(hermes.city());
        let point = SpawnPoint {
            point: (initial.position.truncate() / METERS_PER_UNIT).to_array(),
            heading: Some(0.73),
        };
        let mut world = DynamicWorld::with_start(&hermes, Some(point)).unwrap();
        let expected = world.player_position();
        world.vehicles[0].position += Vec3::new(5., 6., 7.);
        world.vehicles[0].velocity = Vec3::X * 12.;
        world.vehicles[0].angular_momentum = Vec3::new(1., -2., 3.);
        world.vehicles[0].roll = std::f32::consts::PI;
        world.reset_player(&hermes);
        assert!(world.is_driving());
        assert_eq!(world.player_position(), expected);
        assert_eq!(world.player_yaw(), 0.73);
        assert_eq!(world.player().velocity, Vec3::ZERO);
        assert_eq!(world.player().angular_momentum, Vec3::ZERO);
        assert!(world.player().roll.abs() < 0.01);
    }
}
