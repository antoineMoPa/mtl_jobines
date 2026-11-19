//! Resolve map jumps and landmark visits onto supported, unobstructed spots.
use super::{DynamicWorld, SpawnPoint, Vehicle, METERS_PER_UNIT};
use crate::hermes::{traffic_paths::SpeedClass, Hermes, Landmark};
use glam::{Vec2, Vec3};
use opencity::runtime::SurfaceKind;

pub(super) fn vehicles_overlap(a: &Vehicle, b: &Vehicle) -> bool {
    let bottom_a = a.position.z - a.ride_height();
    let bottom_b = b.position.z - b.ride_height();
    if bottom_a > bottom_b + b.height() || bottom_b > bottom_a + a.height() {
        return false;
    }
    let forward_a = Vec2::new(a.yaw.cos(), a.yaw.sin());
    let left_a = Vec2::new(-forward_a.y, forward_a.x);
    let forward_b = Vec2::new(b.yaw.cos(), b.yaw.sin());
    let left_b = Vec2::new(-forward_b.y, forward_b.x);
    let delta = a.position.truncate() - b.position.truncate();
    [forward_a, left_a, forward_b, left_b]
        .into_iter()
        .all(|axis| {
            let extent_a = axis.dot(forward_a).abs() * a.length() * 0.5
                + axis.dot(left_a).abs() * a.width() * 0.5;
            let extent_b = axis.dot(forward_b).abs() * b.length() * 0.5
                + axis.dot(left_b).abs() * b.width() * 0.5;
            delta.dot(axis).abs() < extent_a + extent_b + 0.75
        })
}

impl DynamicWorld {
    pub fn respawn_at_point(&mut self, hermes: &Hermes, point: [f32; 2]) -> Result<(), String> {
        let bounds = hermes.city().bounds;
        if point.iter().any(|value| !value.is_finite())
            || point[0] < bounds[0]
            || point[0] > bounds[2]
            || point[1] < bounds[1]
            || point[1] > bounds[3]
        {
            return Err("Choose a point inside the map".into());
        }
        let spawn = SpawnPoint {
            point,
            heading: None,
        };
        let mut car = Vehicle::spawn_at(hermes.city(), spawn)?;
        car.set_catalog_model(self.player().definition);
        if !self.clear_respawn(hermes, &car) {
            return Err("Choose clear ground with room for your car".into());
        }
        self.finish_respawn(hermes, car, spawn);
        Ok(())
    }

    pub fn respawn_at_landmark(
        &mut self,
        hermes: &Hermes,
        landmark: &Landmark,
    ) -> Result<(), String> {
        // A lane can be on a bridge or behind a whole block. First look for a
        // supported viewing spot beside the landmark with a clear chase-camera ray.
        let origin = Vec2::from_array(landmark.point);
        for extra in [0.12, 0.17, 0.22, 0.30, 0.40, 0.55] {
            let radius = landmark.radius + extra;
            for step in 0..48 {
                let angle = step as f32 * std::f32::consts::TAU / 48.;
                let direction = Vec2::new(angle.cos(), angle.sin());
                let point = origin - direction * radius;
                let spawn = SpawnPoint {
                    point: point.to_array(),
                    heading: Some(angle),
                };
                let Ok(mut car) = Vehicle::spawn_at(hermes.city(), spawn) else {
                    continue;
                };
                car.set_catalog_model(self.player().definition);
                if !self.clear_respawn(hermes, &car) {
                    continue;
                }
                let p = car.position / METERS_PER_UNIT;
                let eye = p - direction.extend(0.) * 0.15 + Vec3::Z * 0.075;
                let target = origin.extend(landmark.target_height);
                let mut visible = true;
                for sample in 0..=64 {
                    let t = sample as f32 / 64.;
                    let ray = eye.lerp(target, t);
                    if ray.truncate().distance(origin) <= landmark.radius + 0.005 {
                        break;
                    }
                    let xy = ray.truncate().to_array();
                    if hermes
                        .city()
                        .building_collision(xy, 0.003, ray.z - 0.003, ray.z + 0.003)
                        || hermes
                            .city()
                            .surface_at(xy, f32::INFINITY)
                            .is_some_and(|s| s.height > ray.z - 0.003)
                    {
                        visible = false;
                        break;
                    }
                }
                if visible {
                    self.finish_respawn(hermes, car, spawn);
                    return Ok(());
                }
            }
        }
        self.respawn_near(hermes, landmark.point)
    }

    pub fn respawn_near(&mut self, hermes: &Hermes, point: [f32; 2]) -> Result<(), String> {
        self.respawn_near_within(hermes, point, 500.)
    }

    pub fn respawn_near_within(
        &mut self,
        hermes: &Hermes,
        point: [f32; 2],
        max_distance: f32,
    ) -> Result<(), String> {
        let center = Vec2::from_array(point) * METERS_PER_UNIT;
        let paths = hermes.traffic_paths();
        let margin = self.player().length() * 0.5 + 3.;
        let mut candidates = Vec::new();
        for path in &paths.paths {
            if path.speed == SpeedClass::Junction || path.length < margin * 2. {
                continue;
            }
            let mut along = 0.;
            let mut closest = (0., f32::INFINITY);
            for pair in path.points.windows(2) {
                let delta = pair[1] - pair[0];
                let xy = delta.truncate();
                let t = ((center - pair[0].truncate()).dot(xy) / xy.length_squared().max(1e-10))
                    .clamp(0., 1.);
                let error = center.distance_squared((pair[0] + delta * t).truncate());
                if error < closest.1 {
                    closest = (along + delta.length() * t, error);
                }
                along += delta.length();
            }
            if closest.1 > max_distance.powi(2) {
                continue;
            }
            for offset in -100..=100 {
                let distance = (closest.0 + offset as f32 * 5.).clamp(margin, path.length - margin);
                let (position, direction) = path.sample(distance);
                let error = center.distance_squared(position.truncate());
                if error <= max_distance.powi(2) {
                    let priority = match path.speed {
                        SpeedClass::Local | SpeedClass::Arterial => 0,
                        _ => 1,
                    };
                    candidates.push((priority, error, position, direction));
                }
            }
        }
        candidates.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        for (_, _, position, direction) in candidates {
            let spawn = SpawnPoint {
                point: (position.truncate() / METERS_PER_UNIT).to_array(),
                heading: Some(direction.y.atan2(direction.x)),
            };
            let Ok(mut vehicle) = Vehicle::spawn_at(hermes.city(), spawn) else {
                continue;
            };
            vehicle.set_catalog_model(self.player().definition);
            // spawn_at chooses the uppermost support; reject a different stacked deck.
            if (vehicle.position.z - vehicle.ride_height() - position.z).abs() > 0.5
                || !self.clear_respawn(hermes, &vehicle)
            {
                continue;
            }
            self.finish_respawn(hermes, vehicle, spawn);
            return Ok(());
        }
        Err(format!("No clear driving lane within {max_distance} m"))
    }

    fn finish_respawn(&mut self, hermes: &Hermes, vehicle: Vehicle, spawn: SpawnPoint) {
        self.vehicles[self.player_vehicle.0] = vehicle;
        self.vehicle_spawns[self.player_vehicle.0] = spawn;
        self.pedestrian = None;
        self.walking_spawn = None;
        self.vehicle_notice = 4.;
        self.sound_geography_timer = 0.;
        self.populate_traffic(hermes);
    }

    fn clear_respawn(&self, hermes: &Hermes, vehicle: &Vehicle) -> bool {
        self.clear_vehicle_position(hermes, vehicle, Some(self.player_vehicle.0))
    }

    pub(super) fn clear_vehicle_position(
        &self,
        hermes: &Hermes,
        vehicle: &Vehicle,
        ignored: Option<usize>,
    ) -> bool {
        if self
            .vehicles
            .iter()
            .enumerate()
            .any(|(id, other)| Some(id) != ignored && vehicles_overlap(vehicle, other))
        {
            return false;
        }
        let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
        let left = Vec2::new(-forward.y, forward.x);
        let base = vehicle.position.z - vehicle.ride_height();
        let steps = vehicle.length().ceil() as usize;
        for step in 0..=steps {
            let along = (step as f32 / steps as f32 - 0.5) * vehicle.length();
            for across in [-vehicle.width() * 0.5, 0., vehicle.width() * 0.5] {
                let p = vehicle.position.truncate() + forward * along + left * across;
                let xy = (p / METERS_PER_UNIT).to_array();
                let expected = base + along * vehicle.pitch.tan() + across * vehicle.roll.tan();
                let Some(support) = hermes
                    .city()
                    .surface_at(xy, (expected + 0.5) / METERS_PER_UNIT)
                else {
                    return false;
                };
                let ground = support.height * METERS_PER_UNIT;
                let supported = matches!(
                    support.kind,
                    SurfaceKind::Asphalt | SurfaceKind::Grass | SurfaceKind::Sand
                ) || (support.kind == SurfaceKind::Sidewalk
                    && support.road.is_none());
                if !supported
                    || (ground - expected).abs() > 0.5
                    || hermes.city().building_collision(
                        xy,
                        0.004,
                        (ground + 0.1) / METERS_PER_UNIT,
                        (ground + vehicle.height()) / METERS_PER_UNIT,
                    )
                    || hermes
                        .city()
                        .road_barrier_contact(
                            xy,
                            0.004,
                            (ground + 0.1) / METERS_PER_UNIT,
                            (ground + vehicle.height()) / METERS_PER_UNIT,
                        )
                        .is_some()
                    || self.props.blocked(p.extend(ground + 0.65))
                {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn map_jump_preserves_car_and_uses_elevated_ground_and_reset() {
        let hermes = Hermes::test_map_from_source(
            "land(-2,-2,2,-2,2,2,-2,2); player_start(-1,0);
             pad=concrete(0.2,-0.3,0.8,-0.3,0.8,0.3,0.2,0.3); elevation(pad,0.1);",
        );
        let mut world = DynamicWorld::new(&hermes);
        let model = super::super::catalog::definitions()
            .iter()
            .position(|d| d.config.category == "compact")
            .unwrap();
        world.vehicles[world.player_vehicle.0].set_catalog_model(Some(model));
        assert!(world.toggle_vehicle(&hermes));
        world.respawn_at_point(&hermes, [0.5, 0.]).unwrap();
        let visited = world.player_position();
        assert!(world.is_driving());
        assert_eq!(world.player().definition, Some(model));
        assert_eq!(visited.truncate(), Vec2::new(50., 0.));
        let support = hermes.city().surface_at([0.5, 0.], f32::INFINITY).unwrap();
        assert!(support.height > 0.07);
        assert!(
            (visited.z - world.player().ride_height() - support.height * METERS_PER_UNIT).abs()
                < 0.01
        );
        assert_eq!(world.player().velocity, Vec3::ZERO);
        assert!(world.clear_respawn(&hermes, world.player()));
        world.vehicles[world.player_vehicle.0].position += Vec3::splat(5.);
        world.reset_player(&hermes);
        assert!(world.player_position().distance(visited) < 0.001);
    }

    #[test]
    fn map_jump_rejects_water_buildings_other_cars_and_invalid_points_atomically() {
        let hermes = Hermes::test_map_from_source(
            "land(-2,-2,2,-2,2,2,-2,2); player_start(-1,0);
             wall=building(0.2,{family:legacy},-0.2,-0.2,0.2,-0.2,0.2,0.2,-0.2,0.2);
             elevation(wall,-0.02);
             lake(0.5,0.5,1,0.5,1,1,0.5,1);",
        );
        let mut world = DynamicWorld::new(&hermes);
        world
            .vehicles
            .push(Vehicle::spawn_at(hermes.city(), "1,0".parse().unwrap()).unwrap());
        // Ground the solid shell on the terrain. An unelevated shell starts 2 m
        // above this land surface and legitimately admits shorter vehicles.
        let ground = hermes
            .city()
            .surface_at([0., 0.], f32::INFINITY)
            .unwrap()
            .height;
        assert!(hermes
            .city()
            .building_collision([0., 0.], 0.004, ground + 0.001, ground + 0.01));
        for (model, definition) in super::super::catalog::definitions().iter().enumerate() {
            if !definition.config.road_vehicle {
                continue;
            }
            world.vehicles[world.player_vehicle.0].set_catalog_model(Some(model));
            let spawn = world.player_position();
            for point in [[0., 0.], [0.75, 0.75], [1., 0.], [10., 10.], [f32::NAN, 0.]] {
                let before = world.player_position();
                assert!(
                    world.respawn_at_point(&hermes, point).is_err(),
                    "{}: {point:?}",
                    definition.config.name
                );
                assert_eq!(world.player_position(), before);
                world.reset_player(&hermes);
                assert!(world.player_position().distance(spawn) < 0.001);
            }
        }
    }

    #[test]
    fn respawn_brings_the_current_car_and_restores_driving_and_reset() {
        let hermes = Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        let model = super::super::catalog::definitions()
            .iter()
            .position(|d| d.config.category == "compact")
            .unwrap();
        world.vehicles[world.player_vehicle.0].set_catalog_model(Some(model));
        let definition = world.player().definition;
        assert!(world.toggle_vehicle(&hermes));
        world.respawn_near(&hermes, [0.5, 0.]).unwrap();
        assert!(world.is_driving());
        assert_eq!(world.player().definition, definition);
        assert_eq!(world.player().velocity, Vec3::ZERO);
        assert!((world.player().position.x - 50.).abs() < 50.);
        assert!(world.clear_respawn(&hermes, world.player()));
        let visited = world.player().position;
        world.vehicles[world.player_vehicle.0].position += Vec3::splat(5.);
        world.reset_player(&hermes);
        assert!(world.player().position.distance(visited) < 0.001);
    }

    #[test]
    fn missing_lane_leaves_player_and_reset_unchanged() {
        let hermes = Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        let before = world.player().position;
        assert!(world.respawn_near(&hermes, [100., 100.]).is_err());
        assert_eq!(world.player().position, before);
        world.reset_player(&hermes);
        assert!(world.player().position.distance(before) < 0.001);
    }

    #[test]
    fn authored_concrete_parking_supports_landmark_visits_and_driving() {
        let hermes = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane sidewalk; end
             street(-1,-1,1,-1); land(-2,-2,2,-2,2,2,-2,2);
             parking=concrete(-0.8,-0.8,0.8,-0.8,0.8,0.8,-0.8,0.8);
             elevation(parking,0.02); player_start(-0.6,-1,0);
             sphere=building(0.12,\"study_01\",0.2,-0.06,0.32,-0.06,0.32,0.06,0.2,0.06);",
        );
        let mut world = DynamicWorld::new(&hermes);
        // This parking regression measures car launch distance, not bicycle acceleration.
        world.switch_vehicle(1);
        world
            .respawn_at_landmark(&hermes, &hermes.landmarks()[0])
            .unwrap();
        let initial = world.player_position();
        assert_eq!(world.player().surface, SurfaceKind::Sidewalk);
        assert!(world.clear_respawn(&hermes, world.player()));
        for _ in 0..120 {
            world.step(
                &hermes,
                super::super::Controls {
                    throttle: 1.,
                    ..Default::default()
                },
                Default::default(),
                1. / 120.,
            );
        }
        assert!(world.player_position().distance(initial) > 2.);
        assert_eq!(world.player().collisions, 0);
        assert_eq!(world.player().surface, SurfaceKind::Sidewalk);
    }

    #[test]
    fn clearance_handles_adjacent_rotated_and_stacked_vehicles() {
        let hermes = Hermes::test_map();
        let a = Vehicle::spawn(hermes.city());
        let mut b = Vehicle::spawn(hermes.city());
        assert!(vehicles_overlap(&a, &b));
        b.position.y += a.width() + 1.;
        assert!(!vehicles_overlap(&a, &b));
        b.position = a.position;
        b.yaw += std::f32::consts::FRAC_PI_2;
        assert!(vehicles_overlap(&a, &b));
        b.position.z += a.height() + 1.;
        assert!(!vehicles_overlap(&a, &b));
    }

    #[test]
    fn every_placed_landmark_has_a_clear_lane_spawn_in_the_current_map() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let landmarks = hermes.landmarks();
        let models = hermes
            .city()
            .landscape
            .iter()
            .filter(|f| {
                f.name.as_deref() == Some("airport_concourse")
                    || f.model
                        .as_deref()
                        .is_some_and(|id| id.starts_with("study_"))
            })
            .count();
        assert_eq!(landmarks.len(), models);
        let mut world = DynamicWorld::new(&hermes);
        let mut failures = Vec::new();
        for landmark in landmarks {
            if let Err(error) = world.respawn_at_landmark(&hermes, &landmark) {
                failures.push(format!("{}: {error}", landmark.title));
                continue;
            }
            assert!(world.is_driving());
            assert!(world.clear_respawn(&hermes, world.player()));
            let distance = world
                .player()
                .position
                .truncate()
                .distance(Vec2::from_array(landmark.point) * METERS_PER_UNIT);
            eprintln!("{}: {distance:.1} m", landmark.title);
            assert!(distance <= 500.);
            if landmark.id == "site_model_01" {
                assert!(
                    landmark.radius >= 0.08,
                    "sphere must retain its enlarged footprint"
                );
                assert_eq!(world.player().surface, SurfaceKind::Sidewalk);
                assert!(distance < 40., "sphere visit must stay beside the model");
                let toward = Vec2::from_array(landmark.point) * METERS_PER_UNIT
                    - world.player().position.truncate();
                let forward = Vec2::new(world.player_yaw().cos(), world.player_yaw().sin());
                assert!(forward.dot(toward) > toward.length() * 0.99);
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
