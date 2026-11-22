//! Map-authored parked vehicles share driving, collision, entry and reset behavior.
use super::{catalog, landmark_respawn::vehicles_overlap, DynamicWorld, SpawnPoint, Vehicle};
use crate::hermes::Hermes;
use opencity::{Argument, Program, Statement};

pub(super) const RENDER_DISTANCE: f32 = 180.;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ParkedVehicle {
    model: usize,
    start: SpawnPoint,
}

impl ParkedVehicle {
    pub(crate) fn from_program(program: &Program) -> Result<Vec<Self>, String> {
        let mut parked = Vec::new();
        for statement in &program.statements {
            let Statement::Call(call) = statement else {
                continue;
            };
            if call.name != "parked_vehicle" {
                continue;
            }
            let [Argument::String(id), Argument::Number(x), Argument::Number(y), Argument::Number(heading)] =
                call.args.as_slice()
            else {
                return Err(
                    "parked_vehicle expects a quoted model ID, x, y, heading-degrees".into(),
                );
            };
            let id: String = serde_json::from_str(id)
                .map_err(|error| format!("Invalid parked vehicle model literal: {error}"))?;
            let model = catalog::definitions()
                .iter()
                .position(|d| {
                    d.config.research_id == id
                        && (d.config.road_vehicle
                            || id == super::baggage::MODEL_ID
                            || d.config.vehicle_kind == catalog::VehicleKind::Aircraft)
                })
                .ok_or_else(|| format!("Unknown parked vehicle: {id}"))?;
            let start = format!("{x},{y},{heading}").parse::<SpawnPoint>()?;
            parked.push(Self { model, start });
        }
        Ok(parked)
    }
}

impl DynamicWorld {
    pub(super) fn add_parked_vehicles(&mut self, hermes: &Hermes) -> Result<(), String> {
        for parked in hermes.parked_vehicles() {
            let mut vehicle = Vehicle::spawn_at(hermes.city(), parked.start)?;
            vehicle.set_catalog_model(Some(parked.model));
            // An explicit launch/reload position has priority over a parked visitor.
            if vehicles_overlap(&vehicle, self.player()) {
                continue;
            }
            if !self.clear_vehicle_position(hermes.city(), &vehicle, None) {
                return Err(format!(
                    "Parked vehicle at {:?} needs clear supported ground",
                    parked.start.point
                ));
            }
            self.vehicles.push(vehicle);
            self.parked_vehicle_count += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{Controls, VehicleId, WalkControls, METERS_PER_UNIT};
    use glam::{Vec2, Vec3};

    #[test]
    fn parked_metadata_requires_exact_road_model_and_finite_pose() {
        let read = |source| ParkedVehicle::from_program(&opencity::parse(source).unwrap());
        assert!(read("").unwrap().is_empty());
        let parked = read("parked_vehicle(\"vehicle-v2-09\",1,2,90);").unwrap();
        assert_eq!(parked[0].model, parked_model());
        assert_eq!(parked[0].start.point, [1., 2.]);
        assert_eq!(parked[0].start.heading, Some(std::f32::consts::FRAC_PI_2));
        for invalid in [
            "parked_vehicle(\"missing\",1,2,90);",
            "parked_vehicle(\"vehicle-r01\",1,2,90);",
            "parked_vehicle(\"vehicle-v2-09\",1,2);",
            "parked_vehicle(vehicle,1,2,90);",
            "parked_vehicle(\"vehicle-v2-09\",1,2,9999999999999999999999999999999999999999);",
        ] {
            assert!(read(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn parked_visitors_stay_put_can_be_entered_and_reset_to_their_spot() {
        let hermes = Hermes::test_map_from_source(
            "pad=concrete(-1,-1,1,-1,1,1,-1,1); elevation(pad,0.1);
             player_start(-0.5,0,0); parked_vehicle(\"vehicle-v2-09\",0,0,90);",
        );
        let mut world = DynamicWorld::new(&hermes);
        assert_eq!(world.vehicles.len(), 2);
        let original = world.vehicles[1].position;
        for _ in 0..240 {
            world.step(
                &hermes,
                Controls::default(),
                WalkControls::default(),
                1. / 120.,
            );
        }
        assert!(world.vehicles[1].position.distance(original) < 0.05);
        assert!(!world.vehicles[1].sound_running);
        assert!(world.vehicles[1].velocity.length() < 0.01);
        assert!(world.toggle_vehicle(&hermes));
        world.pedestrian.as_mut().unwrap().position = original + Vec3::X * 1.5;
        assert!(world.toggle_vehicle(&hermes));
        assert_eq!(world.player_vehicle, VehicleId(1));
        world.vehicles[1].position += Vec3::X * 10.;
        world.reset_player(&hermes);
        assert!(world.player().position.distance(original) < 0.001);
        assert_eq!(world.player().definition, Some(parked_model()));
        assert_eq!(world.player().yaw, std::f32::consts::FRAC_PI_2);

        let overridden = DynamicWorld::with_start(&hermes, Some("0,0,0".parse().unwrap())).unwrap();
        assert_eq!(overridden.vehicles.len(), 1);
    }

    fn parked_model() -> usize {
        catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-v2-09")
            .unwrap()
    }

    #[test]
    fn distant_parking_skips_body_shadow_instances_and_lamps_then_returns_when_near() {
        let hermes = Hermes::test_map_from_source(
            "pad=concrete(-5,-5,5,-5,5,5,-5,5); elevation(pad,0.1);
             player_start(-0.5,0,0); parked_vehicle(\"vehicle-v2-09\",0,0,90);",
        );
        let mut world = DynamicWorld::new(&hermes);
        let parked = world.vehicles[1].position;
        let player_lenses = world.player().definition().unwrap().rear_lenses().len();
        let parked_lenses = world.vehicles[1].definition().unwrap().rear_lenses().len();
        assert!(parked_lenses > 0);
        for (distance, visible) in [
            (179.99, true),
            (180., true),
            (180.01, false),
            (350., false),
            (50., true),
        ] {
            world.vehicles[0].position = parked + Vec3::X * distance;
            assert_eq!(world.vehicle_instances().count(), 1 + usize::from(visible));
            let mut moving = opencity::runtime::Mesh::default();
            world.write_moving_mesh(&mut moving);
            assert_eq!(
                moving.vertices.len(),
                player_lenses + usize::from(visible) * parked_lenses
            );
            assert_eq!(world.vehicles[1].position, parked);
        }
        // Entering the authored car keeps the driven vehicle visible everywhere.
        world.player_vehicle = VehicleId(1);
        world.vehicles[1].position += Vec3::X * 1000.;
        assert_eq!(world.vehicle_instances().count(), 1);
        assert!(world.vehicle_in_render_range(1, world.player()));
    }

    #[test]
    fn sphere_lot_has_twelve_supported_radial_visitors_and_a_clear_forecourt() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let mut world = DynamicWorld::new(&hermes);
        assert_eq!(hermes.parked_vehicles().len(), 88);
        // The first twelve visitors are at the sphere, then the driveway cars
        // and the two airport aircraft and baggage tractor. Cyclists and traffic follow these slots.
        assert_eq!(world.parked_vehicle_count, 88);
        assert!(world.vehicles[86..=87].iter().all(Vehicle::is_aircraft));
        for (i, vehicle) in world.vehicles.iter().enumerate().skip(1).take(85) {
            assert!(!vehicle.is_aircraft());
            assert!(
                world.clear_vehicle_position(hermes.city(), vehicle, Some(i)),
                "blocked driveway car {i}"
            );
        }
        let forecourt = [-1.10, -1.2];
        let mut probe = Vehicle::spawn_at(
            hermes.city(),
            SpawnPoint {
                point: forecourt,
                heading: Some(-std::f32::consts::FRAC_PI_2),
            },
        )
        .unwrap();
        probe.set_catalog_model(Some(0));
        assert!(world.clear_vehicle_position(hermes.city(), &probe, None));
        for (i, vehicle) in world.vehicles.iter().take(13).enumerate().skip(1) {
            assert!(world.clear_vehicle_position(hermes.city(), vehicle, Some(i)));
            let p = vehicle.position / METERS_PER_UNIT;
            assert!((-1.6..-0.85).contains(&p.y) && (-1.39..-0.74).contains(&p.x));
            let to_sphere = Vec2::new(-1.28, -1.2) - p.truncate();
            assert!((to_sphere.length() - 0.27).abs() < 0.00001);
            let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
            assert!(forward.dot(to_sphere) > to_sphere.length() * 0.9999);
            let support = hermes
                .city()
                .surface_at(p.truncate().to_array(), f32::INFINITY)
                .unwrap();
            assert_eq!(support.kind, opencity::runtime::SurfaceKind::Sidewalk);
            assert!(support.road.is_none());
            assert!(
                (p.z - vehicle.ride_height() / METERS_PER_UNIT - support.height).abs() < 0.0001
            );
        }
        let poses: Vec<_> = world.vehicles[1..13].iter().map(|v| v.position).collect();
        // Visiting another district only recycles moving traffic, never parked visitors.
        world.respawn_at_point(&hermes, [-1., 3.]).unwrap();
        for (vehicle, original) in world.vehicles[1..13].iter().zip(poses) {
            assert_eq!(vehicle.position, original);
        }
    }
}
