//! Mutable game entities. OpenCity's City snapshot is static geography, not
//! the owner of vehicles, players, traffic, or future mission objects.
mod player;
mod props;
mod spawn;
pub use spawn::SpawnPoint;
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
    start: Option<SpawnPoint>,
    props: props::Props,
}
impl DynamicWorld {
    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub fn new(hermes: &Hermes) -> Self {
        Self::with_start(hermes, None).expect("default map spawn")
    }
    pub fn with_start(hermes: &Hermes, start: Option<SpawnPoint>) -> Result<Self, String> {
        let vehicle = match start {
            Some(point) => Vehicle::spawn_at(hermes.city(), point)?,
            None => Vehicle::spawn(hermes.city()),
        };
        Ok(Self {
            vehicles: vec![vehicle],
            player_vehicle: VehicleId(0),
            pedestrian: None,
            start,
            props: props::Props::new(hermes.prop_meshes()),
        })
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
                    vehicle.velocity.length() <= 3.
                        && vehicle.position.distance(player.position) <= 4.
                })
                .min_by(|(_, a), (_, b)| {
                    a.position
                        .distance_squared(player.position)
                        .total_cmp(&b.position.distance_squared(player.position))
                })
            {
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
        self.vehicles[self.player_vehicle.0] = match self.start {
            Some(point) => Vehicle::spawn_at(hermes.city(), point).expect("validated start"),
            None => Vehicle::spawn(hermes.city()),
        };
        self.pedestrian = None;
    }
    pub fn step(&mut self, hermes: &Hermes, input: Controls, walk: WalkControls, dt: f32) {
        let driving = self.is_driving();
        for (i, vehicle) in self.vehicles.iter_mut().enumerate() {
            let before = vehicle.position;
            vehicle.step(
                hermes.city(),
                if driving && i == self.player_vehicle.0 {
                    input
                } else {
                    Controls {
                        handbrake: true,
                        ..Controls::default()
                    }
                },
                dt,
            );
            self.props.vehicle_move(vehicle, before, hermes.city());
        }
        if let Some(player) = &mut self.pedestrian {
            let before = player.position;
            player.step(
                hermes.city(),
                &self.vehicles[self.player_vehicle.0],
                walk,
                dt,
            );
            if self.props.blocked(player.position) {
                player.position = before;
            }
        }
        self.props.step(dt);
        if self.player_position().z < -40. {
            self.reset_player(hermes);
        }
    }
    pub fn mesh(&self) -> Mesh {
        let mut mesh = Mesh::default();
        for vehicle in &self.vehicles {
            mesh.vertices.extend(vehicle.mesh().vertices);
        }
        if let Some(player) = &self.pedestrian {
            mesh.vertices.extend(player.mesh().vertices);
        }
        self.props.append_visible(&mut mesh, self.player_position());
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn entering_requires_proximity_and_fast_exit_is_rejected() {
        let hermes = Hermes::embedded().unwrap();
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
        let hermes = Hermes::embedded().unwrap();
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
