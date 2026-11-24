//! Mutable game entities. OpenCity's City snapshot is static geography, not
//! the owner of vehicles, players, traffic, or future mission objects.
mod auto_drive;
pub use auto_drive::{AutoDrive, DriveAvailability, DriveStep};
mod aircraft;
pub(crate) mod airport;
mod baggage;
mod baggage_shift;
pub(crate) mod bikeshare;
mod boats;
pub use boats::Wake;
mod water_hull;
pub use water_hull::Waterline;
mod buildings;
pub mod catalog;
mod contacts;
mod crash;
pub use crash::Crash;
mod crowd;
mod cycling_pose;
mod cyclists;
mod doors;
mod drivers;
mod illumination;
pub mod pedestrian_catalog;
pub mod pedestrian_gait;
mod weapon_query;
pub use illumination::LocalLight;
#[cfg(any(test, feature = "test-support"))]
pub use illumination::RearLight;
pub use weapon_query::{ground_shot_contact, ShotHit, ShotSurface};
mod landmark_respawn;
mod lineup;
mod melee;
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
pub use traffic::{ChaseStatus, Crime, SignalBatch};
#[cfg(test)]
mod handbrake_tests;
mod vehicle;
mod vehicle_entry;
mod vehicle_lights;
mod vehicle_switch;
use crate::hermes::Hermes;
use glam::Vec3;
use opencity::runtime::Mesh;
use player::Pedestrian;
pub const PEDESTRIAN_CAPACITY: usize =
    crowd::CAPACITY + crowd::KIOSK_CAPACITY + cyclists::CAPACITY + traffic::TARGET + 1 + 9 + 4;
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
pub mod rescue_cat;
pub mod wildlife;

// Recheck audible vehicles and the player immediately. Discover the rest in
// rotating batches so a larger fleet changes discovery latency only.
const MAX_SOUND_SOURCES: usize = 45;
const SOUND_SCAN: usize = MAX_SOUND_SOURCES - crate::audio::CAR_VOICES - 1;

fn sound_batch(cursor: &mut usize, count: usize) -> impl Iterator<Item = usize> + Clone {
    let start = if *cursor < count { *cursor } else { 0 };
    let end = start + SOUND_SCAN.min(count - start);
    let wrap = SOUND_SCAN.min(count) - (end - start);
    *cursor = if end == count { wrap } else { end };
    (start..end).chain(0..wrap)
}

fn sound_slots(
    cursor: &mut usize,
    count: usize,
    player: Option<usize>,
    retained: [usize; crate::audio::CAR_VOICES],
) -> impl Iterator<Item = usize> + Clone {
    player
        .into_iter()
        .chain(
            retained
                .into_iter()
                .filter(move |&id| id < count && Some(id) != player),
        )
        .chain(
            sound_batch(cursor, count)
                .filter(move |id| Some(*id) != player && !retained.contains(id)),
        )
}

pub struct DynamicWorld {
    baggage_shift: Option<baggage_shift::Shift>,
    pub rescue_cat: Option<rescue_cat::RescueCat>,
    wildlife_homes: Vec<wildlife::Home>,
    wildlife_population: wildlife::Population,
    wildlife_night: bool,
    wildlife_vehicle_starts: Vec<Vec3>,
    wildlife_time: f32,
    season: crate::season::Season,
    pub snow: snow::Tracks,
    snow_timer: f32,
    player_tire_pops: Vec<ShotHit>,
    crash: Option<Crash>,
    sound_impacts: [crate::audio::Impact; crate::audio::IMPACT_VOICES],
    sound_serial: u64,
    sound_cursor: usize,
    impact_cursor: usize,
    audible_slots: [usize; crate::audio::CAR_VOICES],
    sound_foliage: f32,
    sound_geography_timer: f32,
    metro: metro::Metro,
    airport: airport::Airport,
    boat_routes: Vec<boats::Patrol>,
    wakes: boats::Wakes,
    pub water_level: Option<f32>,
    vehicles: Vec<Vehicle>,
    parked_carts: Vec<baggage::Cart>,
    // Authored parking occupies the slots after the initial player, before traffic.
    parked_vehicle_count: usize,
    player_vehicle: VehicleId,
    health: u8,
    pedestrian: Option<Pedestrian>,
    entry: Option<Box<vehicle_entry::Entry>>,
    displaced: Vec<vehicle_entry::DisplacedOccupant>,
    exit_on_right: bool,
    walking_spawn: Option<SpawnPoint>,
    vehicle_spawns: Vec<SpawnPoint>,
    snow_events: Vec<crate::kairos::SnowCell>,
    props: props::Props,
    traffic: traffic::Traffic,
    dispatch_view: Option<glam::Mat4>,
    crowd: crowd::Crowd,
    cyclists: cyclists::Cyclists,
    service_passengers: service_passengers::ServicePassengers,
    buildings: buildings::Buildings,
    building_candidates: Vec<usize>,
    driver_controls: Vec<Option<Controls>>,
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
        // Keep bicycle handling starts on local streets; autoroutes start in a car.
        let road_role = hermes
            .city()
            .surface_at(
                (vehicle.position.truncate() / METERS_PER_UNIT).to_array(),
                vehicle.position.z / METERS_PER_UNIT,
            )
            .and_then(|surface| surface.road)
            .map(|id| hermes.city().roads[id].definition.role);
        let initial_model = match road_role {
            Some(
                opencity::RoadRole::Highway | opencity::RoadRole::Ramp | opencity::RoadRole::Merge,
            ) => "vehicle-v2-09",
            _ => catalog::BICYCLE_ID,
        };
        vehicle.set_catalog_model(Some(
            catalog::definitions()
                .iter()
                .position(|definition| definition.config.research_id == initial_model)
                .expect("bundled initial player vehicle"),
        ));
        let prop_center = walking_start.map_or(vehicle.position, |point| {
            Vec3::new(
                point.point[0] * METERS_PER_UNIT,
                point.point[1] * METERS_PER_UNIT,
                vehicle.position.z,
            )
        });
        let mut world = Self {
            rescue_cat: None,
            wildlife_homes: hermes.wildlife.clone(),
            wildlife_population: Default::default(),
            wildlife_night: false,
            wildlife_vehicle_starts: Vec::new(),
            wildlife_time: 0.,
            season: Default::default(),
            snow: snow::Tracks::default(),
            snow_timer: 0.,
            player_tire_pops: Vec::new(),
            crash: None,
            sound_impacts: [crate::audio::Impact::default(); crate::audio::IMPACT_VOICES],
            sound_serial: 0,
            sound_cursor: 0,
            impact_cursor: 0,
            audible_slots: [usize::MAX; crate::audio::CAR_VOICES],
            sound_foliage: 0.15,
            sound_geography_timer: 0.,
            dispatch_view: None,
            traffic: traffic::Traffic::new_near(hermes.traffic_paths(), prop_center),
            crowd: crowd::Crowd::new(hermes.walking_paths()),
            cyclists: cyclists::Cyclists::new(hermes),
            service_passengers: Default::default(),
            buildings: buildings::Buildings::new(hermes.city()),
            building_candidates: Vec::new(),
            driver_controls: Vec::new(),
            metro: metro::Metro::new(hermes),
            airport: airport::Airport::new(hermes.city())?,
            vehicles: vec![vehicle],
            baggage_shift: None,
            parked_carts: Vec::new(),
            parked_vehicle_count: 0,
            boat_routes: Vec::new(),
            wakes: Default::default(),
            water_level: hermes
                .city()
                .elevation
                .as_ref()
                .and_then(|e| e.spec.water_level),
            player_vehicle: VehicleId(0),
            health: melee::PLAYER_HEALTH,
            pedestrian: None,
            entry: None,
            displaced: Vec::new(),
            exit_on_right: false,
            walking_spawn: None,
            vehicle_spawns: Vec::new(),
            snow_events: Vec::new(),
            props: props::Props::from_map(hermes, prop_center),
        };
        world.traffic.update_prop_focus(prop_center);
        world.add_parked_vehicles(hermes)?;
        if vehicle_lineup {
            world.add_vehicle_lineup(hermes);
        }
        world
            .cyclists
            .install_stations(hermes, &mut world.vehicles)?;
        world
            .cyclists
            .populate(hermes, &mut world.vehicles, prop_center);
        world.populate_boats(hermes);
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
        world.update_riders();
        world.update_wildlife_population(hermes, 0.);
        Ok(world)
    }
    fn populate_traffic(&mut self, hermes: &Hermes) {
        self.populate_traffic_limited(hermes, usize::MAX);
    }
    /// Returns whether the spawn limit left vehicles still to place.
    fn populate_traffic_limited(&mut self, hermes: &Hermes, limit: usize) -> bool {
        let center = self.player_position();
        let more = self
            .traffic
            .populate_limited(hermes.city(), &mut self.vehicles, center, limit);
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
        more
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
            .begin(board, alight, self.player_vehicle.0, vehicle, sidewalk, bus)
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
        self.update_riders();
    }
    pub fn player(&self) -> &Vehicle {
        &self.vehicles[self.player_vehicle.0]
    }
    pub fn player_vehicle_id(&self) -> VehicleId {
        self.player_vehicle
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
        self.pedestrian
            .as_ref()
            .filter(|p| !p.swimming)
            .map(Pedestrian::foot_motion)
    }
    pub fn is_driving(&self) -> bool {
        self.pedestrian.is_none()
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
    pub fn metro_camera(&self) -> Option<(Vec3, Vec3)> {
        if !self.metro.aboard {
            return None;
        }
        let player = self.pedestrian.as_ref()?;
        let forward = Vec3::new(player.yaw.cos(), player.yaw.sin(), 0.);
        let eye = (player.position + Vec3::Z * 0.8 + forward * 0.2) / METERS_PER_UNIT;
        Some((eye, eye + forward * 0.1))
    }
    pub fn camera_eye(&self, hermes: &Hermes, anchor: Vec3, desired: Vec3) -> Vec3 {
        if let Some((eye, _)) = self.metro_camera() {
            return eye;
        }
        let eye = self.buildings.camera_eye(hermes.city(), anchor, desired);
        self.metro.camera_eye(anchor, eye)
    }
    /// Snapshot only nearby active vehicles. Parked lineup cars have no engine.
    /// Slots are bounded, ordered by distance; slot zero reserves the player.
    pub fn sound_frame(
        &mut self,
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
        let player_slot = self.is_driving().then_some(self.player_vehicle.0);
        let fleet = self.vehicles.len();
        let planes = self.airport.plane_slice();
        for id in sound_slots(
            &mut self.sound_cursor,
            fleet + planes.len(),
            player_slot,
            self.audible_slots,
        ) {
            let v = if id < fleet {
                &self.vehicles[id]
            } else {
                &planes[id - fleet]
            };
            let siren = self.traffic.is_patrol(id);
            if self.traffic.inactive(id)
                || (!siren && !v.sound_running && v.velocity.length_squared() < 0.25)
            {
                continue;
            }
            let distance = v.position.distance_squared(eye);
            if distance > 140. * 140. {
                continue;
            }
            let player = Some(id) == player_slot;
            let rank = if player {
                -40_000.
            } else if siren {
                distance - 20_000.
            } else {
                distance
            };
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
                boat_max_speed: v
                    .definition()
                    .filter(|d| d.config.vehicle_kind == catalog::VehicleKind::Boat)
                    .map(|d| d.config.physics.max_speed),
                wheel_radius: v
                    .definition()
                    .map_or(0.32, |definition| definition.config.physics.wheel_radius),
                horn: false,
                siren,
                player,
                engine_on: v.sound_running,
            };
        }
        self.audible_slots = frame.cars.map(|car| car.id);
        frame
    }
    fn nearest_enterable_vehicle(&self, position: Vec3) -> Option<usize> {
        self.vehicles
            .iter()
            .enumerate()
            .filter(|(index, vehicle)| {
                !self.traffic.inactive(*index)
                    && (!vehicle.is_aircraft() || vehicle.grounded)
                    && vehicle.velocity.length() <= 3.
                    && vehicle.entry_distance(position) <= 2.
            })
            .min_by(|(_, a), (_, b)| {
                a.entry_distance(position)
                    .total_cmp(&b.entry_distance(position))
            })
            .map(|(index, _)| index)
    }
    pub fn toggle_vehicle(&mut self, hermes: &Hermes) -> bool {
        if self.crash.is_some() || self.metro.aboard || self.health == 0 {
            return false;
        }
        if let Some(player) = &self.pedestrian {
            if let Some(index) = self.nearest_enterable_vehicle(player.position) {
                let car = &self.vehicles[index];
                let entry = vehicle_entry::Entry::new(
                    car,
                    player,
                    self.season,
                    car.nearest_front_door(player.position),
                );
                let occupied = self.traffic.driven_slots().any(|(id, _)| id == index);
                let model = self.cyclists.rider_model(index).or_else(|| {
                    (occupied && !self.officer_out_of(index))
                        .then(|| drivers::traffic_model(index, car))
                });
                if let (Some(model), Some(motion)) = (model, &entry) {
                    let right = !motion.on_right();
                    let outward =
                        glam::Vec2::from_angle(car.yaw).perp() * if right { -1. } else { 1. };
                    let landing =
                        Pedestrian::beside_on_side(hermes.city(), car, model, right, |p| {
                            (p - car.position).truncate().dot(outward) <= 0.
                                || self.props.blocked(p)
                                || self
                                    .vehicles
                                    .iter()
                                    .any(|v| player::blocked_by_vehicle(p, v))
                        });
                    let Some(occupant) = landing.and_then(|person| {
                        vehicle_entry::DisplacedOccupant::new(
                            index,
                            &mut self.vehicles[index],
                            person,
                            self.season,
                        )
                    }) else {
                        return false;
                    };
                    self.displaced.push(occupant);
                }
                self.exit_on_right = entry.as_ref().is_some_and(|entry| entry.on_right());
                self.entry = entry;
                self.vehicles[index].doors.claim();
                self.traffic.release(index);
                self.cyclists.undock(index);
                self.cyclists.release(index);
                self.boat_routes.retain(|route| route.vehicle != index);
                self.traffic.claim_player_slot(index);
                self.player_vehicle = VehicleId(index);
                self.pedestrian = None;
                if occupied {
                    self.report_crime(Crime::VehicleTheft);
                }
                self.update_riders();
                return true;
            }
        } else if !self.player().is_aircraft() || self.player().grounded {
            let dock = self.bike_return_dock();
            let docked_bike = dock.map(|(station, slot)| {
                self.cyclists.stations[station].parked_pose(slot, self.player())
            });
            let exit_vehicle = docked_bike.as_ref().unwrap_or_else(|| self.player());
            let steer = self.player().sound_controls.steering;
            let prefer_right = if steer.abs() > 0.2 {
                steer < 0.
            } else {
                self.exit_on_right
            };
            self.pedestrian = Pedestrian::beside_on_side(
                hermes.city(),
                exit_vehicle,
                pedestrian_catalog::PLAYER_SUMMER_MODEL,
                prefer_right,
                |position| {
                    self.props.blocked(position)
                        || player::blocked_by_vehicle(position, exit_vehicle)
                        || self.vehicles.iter().enumerate().any(|(id, v)| {
                            id != self.player_vehicle.0 && player::blocked_by_vehicle(position, v)
                        })
                },
            );
            if let Some(player) = &self.pedestrian {
                self.entry = None;
                if let (Some((station, slot)), Some(bike)) = (dock, docked_bike) {
                    let id = self.player_vehicle.0;
                    self.vehicles[id] = bike;
                    self.cyclists.stations[station].docks[slot].bike = Some(id);
                }
                let car = &mut self.vehicles[self.player_vehicle.0];
                if car.velocity.length_squared() <= 9. {
                    self.entry = vehicle_entry::Entry::new(
                        car,
                        player,
                        self.season,
                        car.nearest_door(player.position),
                    );
                }
                if let Some(exit) = &mut self.entry {
                    car.doors.claim();
                    exit.begin_exit(car);
                } else if let Some(door) = car.nearest_door(player.position) {
                    car.doors.use_door(door);
                }
                self.update_riders();
                return true;
            }
        }
        false
    }
    pub fn reset_player(&mut self, hermes: &Hermes) {
        self.invalidate_baggage_shift();
        self.crash = None;
        self.displaced.clear();
        self.entry = None;
        self.exit_on_right = false;
        self.clear_chase();
        self.health = melee::PLAYER_HEALTH;
        self.crowd.cancel_bites();
        self.metro.aboard = false;
        self.cyclists.undock(self.player_vehicle.0);
        if self.traffic.inactive(self.player_vehicle.0) {
            if let Some(start) = self.walking_spawn {
                self.pedestrian =
                    Some(Pedestrian::at(hermes.city(), start).expect("validated walking start"));
                self.update_riders();
                return;
            }
            self.traffic.claim_player_slot(self.player_vehicle.0);
        }
        let definition = self.player().definition;
        let cart_count = self.player().baggage_links.len();
        self.vehicles[self.player_vehicle.0] = Vehicle::spawn_supported(
            hermes.city(),
            self.vehicle_spawns[self.player_vehicle.0],
            true,
        )
        .expect("validated parking position");
        self.vehicles[self.player_vehicle.0].set_catalog_model(definition);
        self.vehicles[self.player_vehicle.0]
            .baggage_links
            .resize(cart_count, Default::default());
        self.pedestrian = self
            .walking_spawn
            .map(|start| Pedestrian::at(hermes.city(), start).expect("validated walking start"));
        self.update_riders();
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
        if self.crash.is_some() {
            self.advance_crash(hermes, dt);
            return;
        }
        if self.chase_status() == ChaseStatus::Arrested {
            self.step_chase(hermes, self.player_position(), dt);
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
        let phase = crate::telemetry::span("npc.wildlife");
        self.update_wildlife_population(hermes, dt);
        self.step_wildlife_reactions(hermes.city(), dt);
        drop(phase);
        let phase = crate::telemetry::span("npc.transit");
        self.metro.step(dt, self.pedestrian.as_mut());
        self.airport.step(hermes.city(), dt);
        drop(phase);
        let phase = crate::telemetry::span("traffic.population_and_lod");
        if self.traffic.needs_refill(dt) {
            self.refresh_abandoned_cars(hermes.city(), self.zombie_mode());
            // Spend only a per-frame share of spawns; finish the fleet over the next frames.
            if self.populate_traffic_limited(hermes, crate::performance::traffic_spawns()) {
                self.traffic.refill_in(crate::performance::SPAWN_CONTINUE);
            }
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
        if self
            .entry
            .as_ref()
            .is_some_and(|entry| entry.yields_to_input(input, walk))
        {
            self.entry = None;
        }
        let driving = self.is_driving();
        let center = self.player_position();
        let center_velocity = if driving {
            self.player().velocity
        } else {
            Vec3::ZERO
        };
        if !self.zombie_mode() {
            self.cyclists
                .visit_stations(&mut self.vehicles, center, self.player_vehicle.0, dt);
            self.cyclists
                .refresh(hermes, &mut self.vehicles, center, dt);
        }
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
        drop(phase);
        let phase = crate::telemetry::span("traffic.controls");
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
        ai.extend(
            self.boat_routes
                .iter_mut()
                .map(|route| (route.vehicle, route.controls(&self.vehicles[route.vehicle]))),
        );
        self.driver_controls.clear();
        self.driver_controls.resize(self.vehicles.len(), None);
        for (id, controls) in ai {
            self.driver_controls[id].get_or_insert(controls);
        }
        stage!(1);
        drop(phase);
        let phase = crate::telemetry::span("physics.vehicles");
        self.capture_wildlife_vehicle_starts(center, center_velocity, dt);
        let fleet = self.vehicles.len();
        let impact_slots = sound_slots(
            &mut self.impact_cursor,
            fleet,
            Some(self.player_vehicle.0),
            self.audible_slots,
        );
        // A newly sampled slot can still contain impacts from an earlier tick.
        // Start this tick with empty buffers before physics generates contacts.
        if fleet > SOUND_SCAN {
            for id in impact_slots.clone() {
                self.vehicles[id].sound_impact_count = 0;
            }
        }
        for (i, vehicle) in self.vehicles.iter_mut().enumerate() {
            vehicle.season = self.season;
            let signal = if driving && i == self.player_vehicle.0 {
                hermes.route_signal()
            } else {
                crate::hermes::RouteSignal::Off
            };
            if let Some(damage) = &mut vehicle.damage {
                damage.tick(dt);
            }
            vehicle.doors.step(dt);
            vehicle.update_route_signal(signal, dt);
            vehicle.severe_contact = None;
            vehicle.sound_running = false;
            vehicle.sound_cooldown = (vehicle.sound_cooldown - dt).max(0.);
            if self.traffic.inactive(i) || self.cyclists.inactive(i) || self.cyclists.is_docked(i) {
                continue;
            }
            let controls = if driving && i == self.player_vehicle.0 {
                if self.entry.is_some() {
                    Controls {
                        brake: true,
                        handbrake: true,
                        ..Default::default()
                    }
                } else {
                    input
                }
            } else if let Some(controls) = self.driver_controls[i] {
                controls
            } else {
                Controls {
                    handbrake: true,
                    ..Controls::default()
                }
            };
            vehicle.sound_controls = controls;
            vehicle.sound_running =
                (driving && i == self.player_vehicle.0) || self.driver_controls[i].is_some();
            if vehicle.is_boat() {
                vehicle.step(hermes.city(), controls, dt);
                continue;
            }
            if i != self.player_vehicle.0
                && (self.traffic.step_lane(i, vehicle, controls, dt)
                    || self
                        .traffic
                        .sleep_if_settled(i, vehicle, controls, center, dt))
            {
                continue;
            }
            if vehicle.is_aircraft() {
                vehicle.step(hermes.city(), controls, dt);
                continue;
            }
            let before = vehicle.position;
            let reach = vehicle.contact_reach() + vehicle.velocity.length() * dt + 1.;
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
        if let (Some(motion), Some(person)) = (&self.entry, &self.pedestrian) {
            if motion.lost_landing(person) {
                self.entry = None;
            }
        }
        if let Some(entry) = &mut self.entry {
            let vehicle = &mut self.vehicles[entry.vehicle(self.player_vehicle.0)];
            if let Some(landing) = &self.pedestrian {
                entry.update_landing(vehicle, landing, self.season);
            }
            if entry.step(vehicle, dt) {
                self.entry = None;
            }
        }
        self.displaced.retain_mut(|occupant| {
            if let Some(model) = occupant.step(&mut self.vehicles, self.season, dt) {
                if self
                    .crowd
                    .accept_occupant(hermes.city(), model, &occupant.person)
                {
                    return false;
                }
            }
            occupant.person.position.distance_squared(center) < 140_f32.powi(2)
        });
        stage!(2);
        drop(phase);
        let phase = crate::telemetry::span("physics.contacts");
        self.check_wildlife_vehicle_contacts(dt);
        contacts::parked_cart_contacts(&mut self.vehicles, &self.parked_carts);
        let contacts = contacts::resolve_with_contacts(&mut self.vehicles);
        self.cyclists.anchor_docked_bikes(&mut self.vehicles);
        self.patrol_contact(&contacts);
        self.traffic
            .record_contacts(&contacts, self.player_vehicle.0);
        stage!(3);
        drop(phase);
        let phase = crate::telemetry::span("physics.player_and_impacts");
        for hit in &mut self.sound_impacts {
            hit.age += dt;
        }
        for id in impact_slots {
            let vehicle = &mut self.vehicles[id];
            for hit in &vehicle.sound_impacts[..vehicle.sound_impact_count] {
                hit.publish(&mut self.sound_impacts, &mut self.sound_serial);
            }
            vehicle.sound_impact_count = 0;
        }
        self.begin_player_crash();
        if self.crash.is_some() {
            return;
        }
        let mut walking = None;
        if let Some(player) = self
            .pedestrian
            .as_mut()
            .filter(|_| self.health > 0 && self.entry.is_none())
        {
            let before = player.position;
            self.metro.walk(hermes, player, walk, dt, |position| {
                self.parked_carts
                    .iter()
                    .any(|c| c.blocks(position, player::RADIUS))
                    || self.props.blocked(position)
                    || self
                        .vehicles
                        .iter()
                        .any(|v| player::blocked_by_vehicle(position, v))
            });
            self.crowd
                .player_move(hermes.city(), before, player.position, dt, &self.vehicles);
            let after = player.position;
            if !self.metro.aboard {
                walking = Some((before, after));
                self.check_wildlife_walking_contacts(before, after, dt);
            }
        }
        self.step_melee(hermes.city(), dt);
        self.step_wakes(dt);
        stage!(4);
        drop(phase);
        let phase = crate::telemetry::span("physics.props");
        self.props.step(dt);
        stage!(5);
        drop(phase);
        let phase = crate::telemetry::span("npc.crowd");
        // Both populations sweep against the vehicle positions from the previous crowd step.
        let officer_hit = self.step_officer_impacts(hermes.city(), walking, dt);
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
        drop(phase);
        let phase = crate::telemetry::span("npc.pursuit");
        if officer_hit
            || (driving
                && self
                    .crowd
                    .vehicle_knockdowns
                    .contains(&self.player_vehicle.0))
        {
            self.report_crime(Crime::PedestrianHit);
        }
        self.step_chase(hermes, center, dt);
        stage!(6);
        drop(phase);
        let _phase = crate::telemetry::span("simulation.passengers_debris_snow");
        if let Some((index, door)) = self.service_passengers.access {
            self.vehicles[index].doors.use_door(door);
        }
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
        if self.player_position().z < -40. && !(driving && self.player().is_aircraft()) {
            self.reset_player(hermes);
        }
        self.update_riders();
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
        self.update_riders();
    }
    fn update_riders(&mut self) {
        self.cyclists.update_poses(
            &self.vehicles,
            self.player_vehicle.0,
            self.is_driving() && self.entry.is_none(),
            self.season,
        );
    }
    pub fn pedestrian_instances(&self) -> impl Iterator<Item = crowd::Instance<'_>> + '_ {
        self.pedestrian
            .iter()
            .map(|player| match &self.entry {
                Some(motion) => {
                    motion.instance(&self.vehicles[motion.vehicle(self.player_vehicle.0)])
                }
                None => player.instance(self.season),
            })
            .chain(self.crowd.instances(self.player_position()))
            .chain(self.cyclists.waiting_instances(self.player_position()))
            .chain(self.service_passengers.instances())
            .chain(
                self.entry
                    .iter()
                    .filter(|_| self.is_driving() && self.player().is_bicycle())
                    .map(|entry| entry.instance(self.player())),
            )
            .chain(
                self.displaced
                    .iter()
                    .map(|npc| npc.instance(&self.vehicles, self.season)),
            )
            .chain(self.seated_drivers())
            .chain(self.officer_instances())
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
    /// Intact airfield lens positions in map units and their emissive colors.
    pub fn airport_lights(&self) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
        self.props.airport_lights()
    }
    /// Exact visible intact-prop selections, without copying their vertices.
    pub fn visible_prop_batches(&self) -> Vec<PropBatch> {
        self.props.visible_batches(self.player_position())
    }
    pub fn prop_center(&self, id: usize) -> Vec3 {
        self.props.center(id)
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
        if self.traffic.inactive(index) || self.cyclists.inactive(index) {
            return false;
        }
        let distance = if vehicle.is_aircraft() {
            2300.
        } else if index != self.player_vehicle.0 && (1..=self.parked_vehicle_count).contains(&index)
        {
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
        self.vehicle_instances_with_patrol()
            .map(|(instance, _)| instance)
    }
    pub fn vehicle_instances_with_patrol(
        &self,
    ) -> impl Iterator<Item = ((usize, Vec<glam::Mat4>, f32, bool, bool, u8), bool)> + '_ {
        self.vehicle_render_instances()
            .map(|(instance, patrol, _)| (instance, patrol))
    }
    pub fn vehicle_render_instances(
        &self,
    ) -> impl Iterator<
        Item = (
            (usize, Vec<glam::Mat4>, f32, bool, bool, u8),
            bool,
            &Vehicle,
        ),
    > + '_ {
        self.vehicles
            .iter()
            .enumerate()
            .filter(|(index, v)| self.vehicle_in_render_range(*index, v))
            .filter_map(|(index, v)| {
                v.definition.map(|id| {
                    (
                        (
                            id,
                            catalog::definitions()[id].part_transforms(v),
                            self.traffic.opacity(index),
                            v.sound_running,
                            index == self.player_vehicle.0,
                            catalog::definitions()[id].paint_variant(index),
                        ),
                        self.traffic.is_patrol(index),
                        v,
                    )
                })
            })
            .chain(
                self.metro
                    .cars()
                    .filter(|train| train.position.distance(self.player_position()) < 450.)
                    .chain(self.airport.planes().filter(|plane| {
                        plane.position.distance_squared(self.player_position())
                            < crate::performance::draw_radius(2300.).powi(2)
                    }))
                    .map(|v| {
                        let id = v.definition.expect("authored transport model");
                        (
                            (
                                id,
                                catalog::definitions()[id].part_transforms(v),
                                1.,
                                false,
                                false,
                                0,
                            ),
                            false,
                            v,
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
        self.cyclists.append_stations(mesh, self.player_position());
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
    fn autoroute_starts_use_cars_and_have_no_cycling_paths() {
        for role in ["highway", "ramp", "merge"] {
            let source = format!(
                "road autoroute: verge carlane middle carlane verge; role {role}; end
                a=map_point(-1,0); b=map_point(1,0);
                autoroute(a,-0.333333,0,0.333333,0,b);
                ground=land(-2,-1,2,-1,2,1,-2,1); player_start(0,-0.035,0);"
            );
            let hermes = Hermes::test_map_from_source(&source);
            let world = DynamicWorld::new(&hermes);
            assert!(
                !world.player().is_bicycle(),
                "bicycle player start on {role}"
            );
            assert_eq!(
                world.player().definition().unwrap().config.research_id,
                "vehicle-v2-09"
            );
            let (cycling, _) =
                crate::hermes::traffic_paths::TrafficPaths::for_bicycles(hermes.city());
            assert!(cycling.paths.is_empty(), "bicycle routes on {role}");

            assert!(
                world
                    .vehicle_render_instances()
                    .all(|(_, _, vehicle)| !vehicle.is_bicycle()),
                "bicycle spawned on {role}"
            );
        }
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
    fn overloaded_sound_selection_has_a_fixed_budget_and_reaches_every_slot() {
        use crate::audio::CAR_VOICES;
        let count = SOUND_SCAN * 23 + 1;
        let retained = std::array::from_fn(|i| count - i - 2);
        let player = count - 1;
        let mut seen = vec![false; count];
        let mut cursor = 0;
        for _ in 0..count.div_ceil(SOUND_SCAN) {
            let ids: Vec<_> = sound_slots(&mut cursor, count, Some(player), retained).collect();
            assert_eq!(ids[0], player);
            assert_eq!(&ids[1..=CAR_VOICES], &retained);
            assert!(ids.len() <= 45);
            for (i, id) in ids.iter().enumerate() {
                assert!(!ids[..i].contains(id));
                seen[*id] = true;
            }
        }
        assert!(seen.into_iter().all(|visited| visited));
        assert_eq!(
            sound_slots(&mut cursor, 2, None, retained).collect::<Vec<_>>(),
            [0, 1]
        );
    }

    #[test]
    fn newly_discovered_nearby_car_keeps_its_voice_between_sweeps() {
        let mut world = DynamicWorld::new(&Hermes::test_map());
        let far = Vec3::X * 300.;
        let mut car = world.player().clone();
        car.position = far;
        car.sound_running = true;
        world.vehicles = vec![car; SOUND_SCAN * 3];
        let player = world.vehicles.len() - 1;
        world.player_vehicle = VehicleId(player);
        world.vehicles[player].position = Vec3::ZERO;
        let nearby = SOUND_SCAN + 1;
        world.vehicles[nearby].position = Vec3::X;
        let mut snapshot =
            || world.sound_frame(Vec3::ZERO, Vec3::X, Vec3::ZERO, Default::default());
        assert_eq!(snapshot().cars[0].id, player);
        assert_eq!(snapshot().cars[1].id, nearby);
        assert_eq!(snapshot().cars[1].id, nearby);
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
    fn moving_vehicle_can_be_left_but_boarding_requires_low_speed_and_proximity() {
        let hermes = Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        world.vehicles[0].velocity = Vec3::X * 10.;
        assert!(world.toggle_vehicle(&hermes));
        assert!(!world.is_driving());
        assert_ne!(world.player_position(), world.player().position);
        assert_eq!(world.player().velocity, Vec3::X * 10.);
        assert!(!world.toggle_vehicle(&hermes));
        world.vehicles[0].velocity = Vec3::ZERO;
        assert!(world.toggle_vehicle(&hermes));
        assert!(world.is_driving());
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
