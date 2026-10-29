//! Mutable game entities. OpenCity's City snapshot is static geography, not
//! the owner of vehicles, players, traffic, or future mission objects.
mod buildings;
pub(crate) mod catalog;
mod contacts;
mod lineup;
mod player;
mod props;
pub use props::PropBatch;
mod spawn;
mod suspension;
mod traffic;
pub use spawn::SpawnPoint;
#[cfg(test)]
mod handbrake_tests;
mod vehicle;
use crate::hermes::Hermes;
use glam::Vec3;
use opencity::runtime::Mesh;
use player::Pedestrian;
pub use player::WalkControls;
pub use vehicle::{Controls, Vehicle, METERS_PER_UNIT};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VehicleId(usize);
pub struct DynamicWorld {
    vehicles: Vec<Vehicle>,
    player_vehicle: VehicleId,
    pedestrian: Option<Pedestrian>,
    vehicle_spawns: Vec<SpawnPoint>,
    props: props::Props,
    traffic: traffic::Traffic,
    buildings: buildings::Buildings,
    building_candidates: Vec<usize>,
}
impl DynamicWorld {
    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub fn new(hermes: &Hermes) -> Self {
        Self::with_start(hermes, None).expect("default map spawn")
    }
    pub fn with_start(hermes: &Hermes, start: Option<SpawnPoint>) -> Result<Self, String> {
        let mut vehicle = match start {
            Some(point) => Vehicle::spawn_at(hermes.city(), point)?,
            None => Vehicle::spawn(hermes.city()),
        };
        vehicle.definition = Some(0);
        let mut world = Self {
            traffic: traffic::Traffic::new(hermes.traffic_paths()),
            buildings: buildings::Buildings::new(hermes.city()),
            building_candidates: Vec::new(),
            vehicles: vec![vehicle],
            player_vehicle: VehicleId(0),
            pedestrian: None,
            vehicle_spawns: Vec::new(),
            props: props::Props::from_groups(hermes.prop_groups()),
        };
        world.add_vehicle_lineup(hermes);
        world.vehicle_spawns = world
            .vehicles
            .iter()
            .map(|v| SpawnPoint {
                point: (v.position.truncate() / METERS_PER_UNIT).to_array(),
                heading: Some(v.yaw),
            })
            .collect();
        world.populate_traffic(hermes);
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
    pub fn player(&self) -> &Vehicle {
        &self.vehicles[self.player_vehicle.0]
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
    pub fn toggle_vehicle(&mut self, hermes: &Hermes) -> bool {
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
                self.player_vehicle = VehicleId(index);
                self.pedestrian = None;
                return true;
            }
        } else if self.player().velocity.length() <= 3. {
            self.pedestrian = Pedestrian::beside_if(hermes.city(), self.player(), |position| {
                self.props.blocked(position)
            });
            return self.pedestrian.is_some();
        }
        false
    }
    pub fn reset_player(&mut self, hermes: &Hermes) {
        let definition = self.player().definition;
        self.vehicles[self.player_vehicle.0] =
            Vehicle::spawn_at(hermes.city(), self.vehicle_spawns[self.player_vehicle.0])
                .expect("validated parking position");
        self.vehicles[self.player_vehicle.0].definition = definition;
        self.pedestrian = None;
    }
    pub fn step(&mut self, hermes: &Hermes, input: Controls, walk: WalkControls, dt: f32) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        if self.traffic.needs_refill(dt) {
            self.populate_traffic(hermes);
        }
        let ai = self.traffic.controls(
            hermes.city(),
            &self.vehicles,
            self.pedestrian.as_ref().map(|p| p.position),
            dt,
        );
        let driving = self.is_driving();
        for (i, vehicle) in self.vehicles.iter_mut().enumerate() {
            if self.traffic.inactive(i) {
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
                if driving && i == self.player_vehicle.0 {
                    input
                } else if let Some((_, controls)) = ai.iter().find(|(id, _)| *id == i) {
                    *controls
                } else {
                    Controls {
                        handbrake: true,
                        ..Controls::default()
                    }
                },
                dt,
                Some(&self.building_candidates),
            );
            self.props.vehicle_move(vehicle, before, hermes.city());
        }
        contacts::resolve(&mut self.vehicles);
        if let Some(player) = &mut self.pedestrian {
            let before = player.position;
            player.step(
                hermes.city(),
                &self.vehicles[self.player_vehicle.0],
                walk,
                dt,
            );
            if self.props.blocked(player.position)
                || self
                    .vehicles
                    .iter()
                    .any(|v| v.entry_distance(player.position) < 0.42)
            {
                player.position = before;
            }
        }
        self.props.step(dt);
        if self.player_position().z < -40. {
            self.reset_player(hermes);
        }
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
    pub fn write_prop_batch(&self, batch: &PropBatch, output: &mut Mesh) {
        self.props.write_batch(batch, output);
    }
    /// Vehicles, pedestrian and moving debris; intact props use cached batches.
    pub fn write_moving_mesh(&self, mesh: &mut Mesh) {
        self.write_non_vehicle_actors(mesh);
        self.props.append_moving(mesh, self.player_position());
    }
    #[cfg(test)]
    fn write_actors(&self, mesh: &mut Mesh) {
        self.write_non_vehicle_actors(mesh);
        for vehicle in &self.vehicles {
            if vehicle.position.distance(self.player_position()) <= 450. {
                vehicle.append_mesh(mesh);
            }
        }
    }
    pub(crate) fn vehicle_instances(&self) -> impl Iterator<Item = (usize, Vec<glam::Mat4>)> + '_ {
        self.vehicles
            .iter()
            .filter(|v| v.position.distance(self.player_position()) <= 450.)
            .filter_map(|v| {
                v.definition
                    .map(|id| (id, catalog::definitions()[id].part_transforms(v)))
            })
    }
    fn write_non_vehicle_actors(&self, mesh: &mut Mesh) {
        mesh.vertices.clear();
        self.traffic.append_signals(mesh, self.player_position());
        if let Some(player) = &self.pedestrian {
            mesh.vertices.extend(player.mesh().vertices);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entering_requires_proximity_and_fast_exit_is_rejected() {
        let hermes = Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        world.vehicles[0].velocity = Vec3::X * 10.;
        assert!(!world.toggle_vehicle(&hermes));
        assert!(world.is_driving());
        world.vehicles[0].velocity = Vec3::ZERO;
        assert!(world.toggle_vehicle(&hermes));
        assert!(!world.is_driving());
        assert_ne!(world.player_position(), world.player().position);
        assert!(world.toggle_vehicle(&hermes));
        assert!(world.is_driving());
        assert!(world.toggle_vehicle(&hermes));
        world.pedestrian.as_mut().unwrap().position += Vec3::X * 20.;
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
        world.reset_player(&hermes);
        assert!(world.is_driving());
        assert_eq!(world.player_position(), expected);
        assert_eq!(world.player_yaw(), 0.73);
        assert_eq!(world.player().velocity, Vec3::ZERO);
    }
}
