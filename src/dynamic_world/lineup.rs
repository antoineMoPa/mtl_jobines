//! Park the road catalog on supported, unoccupied ground near the selected spawn.
use super::{catalog, DynamicWorld, SpawnPoint, Vehicle, METERS_PER_UNIT};
use crate::hermes::Hermes;
use glam::Vec2;
use opencity::runtime::SurfaceKind;
impl DynamicWorld {
    pub(super) fn add_vehicle_lineup(&mut self, hermes: &Hermes) {
        let origin = self.vehicles[0].position;
        let yaw = self.vehicles[0].yaw;
        let forward = Vec2::new(yaw.cos(), yaw.sin());
        let left = Vec2::new(-forward.y, forward.x);
        let mut offsets = Vec::new();
        // 5 m parking pitch; all candidates are deterministic and searched nearest first.
        for row in -24i32..=24 {
            for col in -24i32..=24 {
                if row == 0 && col == 0 {
                    continue;
                }
                offsets.push((row, col));
            }
        }
        offsets.sort_by_key(|(row, col)| row * row + col * col);
        for (id, def) in catalog::definitions()
            .iter()
            .enumerate()
            .filter(|(i, d)| *i != 0 && d.config.road_vehicle)
        {
            let dims = &def.config.dimensions;
            let found = offsets.iter().find_map(|(row, col)| {
                let point =
                    origin.truncate() + forward * (*row as f32 * 5.) + left * (*col as f32 * 5.);
                let mut vehicle = Vehicle::spawn_at(
                    hermes.city(),
                    SpawnPoint {
                        point: (point / METERS_PER_UNIT).to_array(),
                        heading: Some(yaw),
                    },
                )
                .ok()?;
                vehicle.definition = Some(id);
                for other in &self.vehicles {
                    let delta = point - other.position.truncate();
                    if delta.dot(forward).abs() < (dims.length + other.length()) * 0.5 + 2.
                        && delta.dot(left).abs() < (dims.width + other.width()) * 0.5 + 2.
                    {
                        return None;
                    }
                }
                // Cover the full footprint, including the bus and blades.
                let checks = (dims.length / 1.).ceil() as usize;
                for step in 0..=checks {
                    let along = (step as f32 / checks as f32 - 0.5) * dims.length;
                    for across in [-dims.width * 0.5, 0., dims.width * 0.5] {
                        let p = point + forward * along + left * across;
                        let sample = hermes.city().surface_at(
                            (p / METERS_PER_UNIT).to_array(),
                            (vehicle.position.z + 0.4) / METERS_PER_UNIT,
                        )?;
                        let ground = sample.height * METERS_PER_UNIT;
                        if sample.kind == SurfaceKind::Water
                            || (ground - (vehicle.position.z - vehicle.ride_height())).abs() > 0.35
                            || hermes.city().building_collision(
                                (p / METERS_PER_UNIT).to_array(),
                                0.004,
                                (ground + 0.1) / METERS_PER_UNIT,
                                (ground + dims.height) / METERS_PER_UNIT,
                            )
                            || hermes
                                .city()
                                .road_barrier_contact(
                                    (p / METERS_PER_UNIT).to_array(),
                                    0.004,
                                    (ground + 0.1) / METERS_PER_UNIT,
                                    (ground + dims.height) / METERS_PER_UNIT,
                                )
                                .is_some()
                            || self.props.blocked(p.extend(ground + 0.65))
                        {
                            return None;
                        }
                    }
                }
                Some(vehicle)
            });
            if let Some(vehicle) = found {
                self.vehicles.push(vehicle);
            } else {
                eprintln!(
                    "No supported parking space near spawn for {}",
                    def.config.name
                );
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_map_has_every_road_vehicle_near_spawn() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("opencity/examples/default_map.road");
        let hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let world = DynamicWorld::new(&hermes);
        assert_eq!(
            world.vehicles.len() - world.traffic.count(),
            15,
            "default map must offer all road vehicles"
        );
        let origin = world.vehicles[0].position;
        let rows = world
            .vehicles
            .iter()
            .take(15)
            .map(|v| {
                assert!(
                    v.position.distance(origin) < 120.,
                    "parking must remain near spawn"
                );
                serde_json::json!({
                    "name":v.name(),
                    "positionMeters":v.position.to_array(),
                    "distanceFromSpawnMeters":v.position.distance(origin),
                    "yawRadians":v.yaw
                })
            })
            .collect::<Vec<_>>();
        if let Ok(path) = std::env::var("VEHICLE_LINEUP_QA") {
            std::fs::write(path, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
        }
    }
    #[test]
    fn all_road_vehicles_spawn_separately_and_reset_retains_selected_model() {
        let hermes = Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        assert_eq!(world.vehicles.len() - world.traffic.count(), 15);
        for (i, a) in world.vehicles.iter().take(15).enumerate() {
            assert_eq!(a.definition, Some(i));
            assert!(a.definition().unwrap().config.road_vehicle);
            for b in &world.vehicles[..i] {
                assert!(a.position.distance(b.position) > 3.);
            }
        }
        world.player_vehicle = super::super::VehicleId(9);
        world.reset_player(&hermes);
        assert_eq!(world.player().definition, Some(9));
    }
    #[test]
    fn every_road_model_drives_and_renders_steering_wheels() {
        let hermes = Hermes::test_map();
        for (id, def) in catalog::definitions()
            .iter()
            .enumerate()
            .filter(|(_, d)| d.config.road_vehicle)
        {
            let mut vehicle = Vehicle::spawn(hermes.city());
            vehicle.definition = Some(id);
            let initial = vehicle.mesh();
            vehicle.steering = 0.3;
            vehicle.wheel_angle = 0.7;
            let turned = vehicle.mesh();
            assert_eq!(initial.vertices.len(), turned.vertices.len());
            assert!(initial
                .vertices
                .iter()
                .zip(&turned.vertices)
                .any(|(a, b)| a.position != b.position));
            vehicle.steering = 0.;
            for _ in 0..120 {
                vehicle.step(
                    hermes.city(),
                    super::super::Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    1. / 120.,
                );
            }
            assert!(
                vehicle.velocity.truncate().length() > 0.1,
                "{} failed to accelerate",
                def.config.name
            );
        }
    }
}
