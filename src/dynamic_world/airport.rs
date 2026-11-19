//! YULPET's three aircraft share a timed circuit on the west
//! runway, with the east runway available for player departures and arrivals.
mod props;
pub(crate) use props::groups as prop_groups;

use super::{aircraft, SpawnPoint, Vehicle, METERS_PER_UNIT};
use glam::{Vec2, Vec3};
use opencity::runtime::{City, Mesh, SurfaceKind};

const WEST: &str = "airport_west_runway";
const EAST: &str = "airport_east_runway";

struct Runway {
    min: Vec2,
    max: Vec2,
    height: f32,
}

impl Runway {
    fn find(city: &City, name: &str) -> Option<Self> {
        let feature = city
            .landscape
            .iter()
            .find(|f| f.name.as_deref() == Some(name))?;
        let min = feature
            .points
            .iter()
            .copied()
            .map(Vec2::from_array)
            .reduce(Vec2::min)?;
        let max = feature
            .points
            .iter()
            .copied()
            .map(Vec2::from_array)
            .reduce(Vec2::max)?;
        let height = city
            .surface_at(((min + max) * 0.5).to_array(), f32::INFINITY)?
            .height;
        Some(Self { min, max, height })
    }

    fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }
}

#[derive(Default)]
pub(super) struct Airport {
    center: Vec3,
    clock: f32,
    traffic: Vec<Vehicle>,
}

impl Airport {
    pub(super) fn new(city: &City) -> Result<Self, String> {
        let Some(runway) = Runway::find(city, WEST) else {
            return Ok(Self::default());
        };
        let center = runway.center().extend(runway.height) * METERS_PER_UNIT;
        let mut airport = Self {
            center,
            ..Default::default()
        };
        for model in [
            aircraft::Model::Estuaire,
            aircraft::Model::Fleche,
            aircraft::Model::Estuaire,
        ] {
            let mut plane = Vehicle::spawn_at(
                city,
                SpawnPoint {
                    point: runway.center().to_array(),
                    heading: Some(-std::f32::consts::FRAC_PI_2),
                },
            )?;
            plane.set_catalog_model(Some(model.catalog_index()));
            airport.traffic.push(plane);
        }
        airport.step(city, 0.);
        Ok(airport)
    }

    pub(super) fn planes(&self) -> impl Iterator<Item = &Vehicle> {
        self.traffic.iter()
    }

    pub(super) fn step(&mut self, city: &City, dt: f32) {
        self.clock = (self.clock + dt) % PERIOD;
        for (slot, plane) in self.traffic.iter_mut().enumerate() {
            let time = self.clock + 7. + (PERIOD / 3.) * slot as f32;
            let (point, motion) = circuit(time);
            let world_angle = |velocity: Vec3| -velocity.x.atan2(velocity.y);
            let heading = world_angle(motion);
            let next_heading = world_angle(circuit(time + 0.25).1);
            let turn = (next_heading - heading).sin();
            plane.position =
                self.center + Vec3::new(point.y, -point.x, point.z + plane.ride_height());
            plane.velocity = Vec3::new(motion.y, -motion.x, motion.z);
            plane.yaw = heading;
            plane.pitch = motion.z.atan2(motion.truncate().length());
            plane.grounded = point.z <= 0.05;
            plane.roll = if plane.grounded {
                0.
            } else {
                (-turn * 5.).clamp(-0.44, 0.44)
            };
            plane.aircraft_surfaces = Vec3::new(
                -plane.roll / 0.72,
                (plane.pitch / 0.23).clamp(-1., 1.),
                (turn * 3.).clamp(-1., 1.),
            );
            let ground = city
                .surface_at(
                    (plane.position.truncate() / METERS_PER_UNIT).to_array(),
                    f32::INFINITY,
                )
                .map_or(self.center.z, |surface| surface.height * METERS_PER_UNIT);
            plane.aircraft_gear =
                aircraft::gear_target(plane.position.z - plane.ride_height() - ground);
            plane.sound_running = true;
            plane.sound_controls.throttle = (plane.velocity.length() / 75.).clamp(0.15, 1.);
        }
    }
}

const PERIOD: f32 = 244.;
// Coordinates are metres relative to the runway centre: south, east, above
// the pavement. Tangents are per second so transitions preserve speed.
const CIRCUIT: &[(f32, [f32; 3], [f32; 3])] = &[
    (0., [-740., 0., 0.], [12., 0., 0.]),
    (16., [-280., 0., 0.], [51., 0., 0.]),
    (25., [245., 0., 34.], [65., 0., 7.]),
    (39., [1110., -105., 150.], [60., -28., 8.]),
    (54., [1650., -780., 235.], [0., -66., 3.]),
    (76., [700., -1760., 252.], [-72., -17., 0.]),
    (97., [-870., -1540., 245.], [-66., 33., -1.]),
    (114., [-1690., -610., 219.], [-11., 72., -3.]),
    (126., [-1420., -22., 144.], [52., 15., -9.]),
    (141., [-460., 0., 0.], [66., 0., 0.]),
    (157., [494., 0., 0.], [43., 0., 0.]),
    (165., [720., 0., 0.], [12., 0., 0.]),
    (173., [744., 80., 0.], [-1., 10., 0.]),
    (179., [665., 97., 0.], [-25., 0., 0.]),
    (225., [-668., 97., 0.], [-24., 0., 0.]),
    (233., [-742., 64., 0.], [-3., -10., 0.]),
    (239., [-752., 10., 0.], [1., -7., 0.]),
    (PERIOD, [-740., 0., 0.], [12., 0., 0.]),
];

fn circuit(time: f32) -> (Vec3, Vec3) {
    let time = time % PERIOD;
    let end = CIRCUIT.partition_point(|&(t, _, _)| t <= time);
    let (t0, a, av) = CIRCUIT[end - 1];
    let (t1, b, bv) = CIRCUIT[end];
    let duration = t1 - t0;
    let u = (time - t0) / duration;
    let a = Vec3::from_array(a);
    let b = Vec3::from_array(b);
    let av = Vec3::from_array(av) * duration;
    let bv = Vec3::from_array(bv) * duration;
    let c = (b - a) * 3. - 2. * av - bv;
    let d = (a - b) * 2. + av + bv;
    let point = a + u * (av + u * (c + u * d));
    (point, (av + u * (2. * c + 3. * u * d)) / duration)
}

pub(crate) fn append_scenery(city: &City, mesh: &mut Mesh) {
    if let Some(shore) = city
        .landscape
        .iter()
        .find(|f| f.name.as_deref() == Some("airport_shore"))
    {
        // The authored land supplies flat support. Clad its exposed perimeter
        // down to the water so the runways read as a reclaimed shoreline.
        let top = shore.elevation - 0.02;
        for ([ax, ay], [bx, by]) in shore
            .points
            .iter()
            .copied()
            .zip(shore.points.iter().copied().cycle().skip(1))
            .take(shore.points.len())
        {
            let a = [ax, ay, top];
            let b = [bx, by, top];
            let low_a = [ax, ay, -0.005];
            let low_b = [bx, by, -0.005];
            let color = [0.27, 0.31, 0.30];
            let kind = SurfaceKind::Prop;
            mesh.triangle([a, b, low_b], color, kind, [0.; 3]);
            mesh.triangle([a, low_b, low_a], color, kind, [0.; 3]);
        }
    }

    for name in [WEST, EAST] {
        let Some(r) = Runway::find(city, name) else {
            continue;
        };
        let center = r.center().x;
        let base = r.height;
        let quad = |mesh: &mut Mesh, min: Vec2, max: Vec2, height, color, kind| {
            let a = min.extend(height).to_array();
            let b = Vec3::new(max.x, min.y, height).to_array();
            let c = max.extend(height).to_array();
            let d = Vec3::new(min.x, max.y, height).to_array();
            mesh.triangle([a, b, c], color, kind, [0.; 3]);
            mesh.triangle([a, c, d], color, kind, [0.; 3]);
        };
        // Reuse the actual runway faces so there are no coplanar concrete
        // and asphalt layers at long viewing distances.
        let margin = Vec2::splat(0.00004);
        for triangle in mesh.vertices.chunks_exact_mut(3) {
            if triangle.iter().all(|v| {
                let [x, y, z] = v.position;
                let p = Vec2::new(x, y);
                v.surface[0] == SurfaceKind::Sidewalk as u32 as f32
                    && (z - base).abs() < 0.00004
                    && p.cmpge(r.min - margin).all()
                    && p.cmple(r.max + margin).all()
            }) {
                for vertex in triangle {
                    vertex.color = [0.22, 0.25, 0.27];
                    vertex.surface = [SurfaceKind::Asphalt as u32 as f32, 0., 0., 0.];
                }
            }
        }
        let mut paint = |min: Vec2, max: Vec2| {
            quad(
                mesh,
                min,
                max,
                base + 0.00045,
                [0.9, 0.92, 0.9],
                SurfaceKind::Marking,
            );
        };
        for x in [r.min.x + 0.034, r.max.x - 0.038] {
            paint(
                Vec2::new(x, r.min.y + 0.095),
                Vec2::new(x + 0.004, r.max.y - 0.095),
            );
        }
        let mut y = r.min.y + 0.50;
        while y + 0.26 < r.max.y - 0.50 {
            paint(
                Vec2::new(center - 0.005, y),
                Vec2::new(center + 0.005, y + 0.26),
            );
            y += 0.49;
        }
        for y in [r.min.y + 0.095, r.max.y - 0.26] {
            for offset in [-0.245, -0.186, -0.127, -0.068, 0.038, 0.097, 0.156, 0.215] {
                paint(
                    Vec2::new(center + offset, y),
                    Vec2::new(center + offset + 0.03, y + 0.165),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{Controls, DynamicWorld};
    use crate::hermes::Hermes;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn airport_map_has_two_runways_and_three_separated_circuits() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/mtl/map.road");
        let hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let city = hermes.city();
        let west = Runway::find(city, WEST).unwrap();
        let east = Runway::find(city, EAST).unwrap();
        assert_eq!(west.center().y, east.center().y);
        for y in [-7., -13.15, -21.5] {
            let infield = city.surface_at([0., y], f32::INFINITY).unwrap();
            assert_eq!(infield.kind, SurfaceKind::Grass);
            assert!((infield.height - west.height).abs() < 0.002);
        }

        assert!((east.center().x - west.center().x - 8.).abs() < 0.001);
        let mut world = DynamicWorld::with_start(
            &hermes,
            Some(SpawnPoint {
                point: [0.41, -4.55],
                heading: Some(-FRAC_PI_2),
            }),
        )
        .unwrap();
        assert_eq!(world.airport.planes().count(), 3);
        assert_eq!(world.vehicles.iter().filter(|v| v.is_aircraft()).count(), 2);
        assert!(
            world
                .vehicle_instances()
                .filter(|(id, ..)| {
                    aircraft::Model::from_id(
                        &super::super::catalog::definitions()[*id].config.research_id,
                    )
                    .is_some()
                })
                .count()
                >= 4
        );

        for model in aircraft::Model::ALL {
            let parked = world
                .vehicles
                .iter()
                .find(|v| v.aircraft_model() == Some(model))
                .unwrap();
            let door = parked.aircraft_boarding_point().unwrap();
            let start = SpawnPoint {
                point: (door / METERS_PER_UNIT).to_array(),
                heading: Some(0.),
            };
            world.pedestrian = Some(super::super::player::Pedestrian::at(city, start).unwrap());
            assert!(world.toggle_vehicle(&hermes));
            assert_eq!(world.player().aircraft_model(), Some(model));
            assert!(world.toggle_vehicle(&hermes));
            assert!(world.player_position().truncate().distance(door) < 0.1);
        }

        let mut traffic = Airport::new(city).unwrap();
        let mut ground_poses = 0;
        let mut retracted = 0;
        for _ in 0..(PERIOD * 4.) as usize {
            traffic.step(city, 0.25);
            for (i, plane) in traffic.planes().enumerate() {
                for other in traffic.planes().skip(i + 1) {
                    assert!(plane.position.distance(other.position) > 36.);
                }
                if plane.grounded {
                    let p = plane.position / METERS_PER_UNIT;
                    let surface = city.surface_at([p.x, p.y], f32::INFINITY).unwrap();
                    assert_eq!(surface.kind, SurfaceKind::Sidewalk, "taxi pose {p:?}");
                    assert!(
                        (surface.height * METERS_PER_UNIT + plane.ride_height() - plane.position.z)
                            .abs()
                            < 0.06
                    );
                    ground_poses += 1;
                } else if plane.aircraft_gear == 1. {
                    retracted += 1;
                }
            }
        }
        assert!(ground_poses > 500 && retracted > 500);
    }

    #[test]
    fn aircraft_brake_back_up_steer_and_resume_forward_taxi() {
        let hermes = Hermes::test_map_from_source(
            "field=concrete(-4,-4,4,-4,4,4,-4,4); elevation(field,0.122001); player_start(0,0,0);",
        );
        let city = hermes.city();
        for model in aircraft::Model::ALL {
            let mut plane = Vehicle::spawn_at(
                city,
                SpawnPoint {
                    point: [0., 0.],
                    heading: Some(0.),
                },
            )
            .unwrap();
            plane.set_catalog_model(Some(model.catalog_index()));
            plane.velocity = Vec3::X * 2.;
            let back = Controls {
                brake: true,
                ..Controls::default()
            };
            plane.step(city, back, 0.1);
            assert!(plane.velocity.x > 0. && plane.velocity.x < 2.);
            for _ in 0..300 {
                plane.step(city, back, 1. / 60.);
            }
            assert!(plane.grounded && plane.position.x < -4.);
            assert!((plane.velocity.x + 3.).abs() < 0.001);
            plane.step(
                city,
                Controls {
                    aircraft_yaw: 1.,
                    ..back
                },
                0.1,
            );
            assert!(plane.yaw < 0.);
            for _ in 0..60 {
                plane.step(
                    city,
                    Controls {
                        handbrake: true,
                        ..back
                    },
                    1. / 60.,
                );
            }
            assert_eq!(plane.velocity, Vec3::ZERO);
            plane.step(
                city,
                Controls {
                    throttle: 1.,
                    ..Controls::default()
                },
                0.1,
            );
            assert!(plane.velocity.x > 0.);
            plane.position.z += 50.;
            plane.grounded = false;
            plane.velocity = Vec3::X * 0.01;
            plane.step(city, back, 0.1);
            assert_eq!(plane.velocity.truncate().length(), 0.);
        }
    }

    #[test]
    fn aircraft_surfaces_lead_the_bank_and_the_airliner_rolls_more_slowly() {
        let hermes = Hermes::test_map();
        let mut bank = Vec::new();
        for model in aircraft::Model::ALL {
            let mut plane = Vehicle::spawn(hermes.city());
            plane.set_catalog_model(Some(model.catalog_index()));
            plane.position = Vec3::Z * 100.;
            plane.grounded = false;
            plane.yaw = 0.;
            plane.velocity = Vec3::X * 70.;
            let dt = 1. / 120.;
            let left = Controls {
                steering: 1.,
                ..Controls::default()
            };
            plane.step(hermes.city(), left, dt);
            assert!(plane.aircraft_surfaces.x > 0.04 && plane.roll.abs() < 0.001);
            for _ in 1..60 {
                plane.step(hermes.city(), left, dt);
            }
            bank.push(-plane.roll);
            let before = plane.roll;
            plane.step(
                hermes.city(),
                Controls {
                    steering: -1.,
                    ..Controls::default()
                },
                dt,
            );
            assert!(
                plane.roll < before,
                "reversing ailerons should take time to arrest the roll"
            );
            assert!(plane.yaw > plane.velocity.y.atan2(plane.velocity.x));
        }
        assert!(bank[0] > 0.05 && bank[1] > bank[0]);
    }

    #[test]
    fn aircraft_keep_bank_and_pitch_after_release_and_respond_to_counter_input() {
        let hermes = Hermes::test_map();
        for model in aircraft::Model::ALL {
            let mut plane = Vehicle::spawn(hermes.city());
            plane.set_catalog_model(Some(model.catalog_index()));
            plane.position = Vec3::Z * 200.;
            plane.grounded = false;
            plane.yaw = 0.;
            plane.velocity = Vec3::X * 70.;
            let dt = 1. / 120.;
            let maneuver = Controls {
                steering: 1.,
                aircraft_pitch: 1.,
                ..Default::default()
            };
            for _ in 0..72 {
                plane.step(hermes.city(), maneuver, dt);
            }
            assert!(plane.roll < -0.1 && plane.pitch > 0.03);
            let release = (plane.roll, plane.pitch);
            for _ in 0..360 {
                plane.step(hermes.city(), Default::default(), dt);
            }
            assert!(plane.roll < release.0 && plane.pitch > release.1);
            let settled = (plane.roll, plane.pitch, plane.yaw);
            for _ in 0..120 {
                plane.step(hermes.city(), Default::default(), dt);
            }
            assert!(
                (plane.roll - settled.0).abs() < 0.005 && (plane.pitch - settled.1).abs() < 0.005
            );
            assert!(plane.yaw > settled.2 && !plane.grounded);
            let recover = Controls {
                steering: -1.,
                aircraft_pitch: -1.,
                ..Default::default()
            };
            for _ in 0..72 {
                plane.step(hermes.city(), recover, dt);
            }
            assert!(plane.roll > settled.0 + 0.05 && plane.pitch < settled.1 - 0.02);
        }
    }

    #[test]
    fn aircraft_rotate_progressively_and_lift_off_at_different_speeds_with_different_pitch() {
        let hermes = Hermes::test_map_from_source(
            "field=concrete(-35,-8,35,-8,35,8,-35,8); player_start(-24,0,0);",
        );
        let city = hermes.city();
        for model in aircraft::Model::ALL {
            let mut lift_speeds = Vec::new();
            for elevator in [0.55, 1.] {
                let mut plane = Vehicle::spawn_at(
                    city,
                    SpawnPoint {
                        point: [-24., 0.],
                        heading: Some(0.),
                    },
                )
                .unwrap();
                plane.set_catalog_model(Some(model.catalog_index()));
                let mut rolling_pitch = 0_f32;
                for _ in 0..3000 {
                    plane.step(
                        city,
                        Controls {
                            throttle: 1.,
                            aircraft_pitch: elevator,
                            ..Default::default()
                        },
                        1. / 120.,
                    );
                    if !plane.grounded {
                        assert!(plane.velocity.z >= 0. && plane.velocity.z < 0.1);
                        break;
                    }
                    rolling_pitch = rolling_pitch.max(plane.pitch);
                }
                assert!(
                    !plane.grounded && rolling_pitch > 0.04,
                    "{model:?} {elevator}: {rolling_pitch}"
                );
                lift_speeds.push(plane.velocity.x);
            }
            assert!(lift_speeds[0] > lift_speeds[1] + 1.);
        }
    }

    #[test]
    fn aircraft_rebound_at_buildings_and_clear_the_roof() {
        let hermes = Hermes::test_map_from_source("field=concrete(-4,-4,4,-4,4,4,-4,4); elevation(field,0.01); wall=building(0.40,0,-1,0.2,-1,0.2,1,0,1); player_start(-1,0,0);");
        let city = hermes.city();
        for model in aircraft::Model::ALL {
            for (altitude, sideways, impact) in [(18., 0., true), (18., 8., true), (65., 0., false)]
            {
                let mut plane = Vehicle::spawn_at(
                    city,
                    SpawnPoint {
                        point: [-1., 0.],
                        heading: Some(0.),
                    },
                )
                .unwrap();
                plane.set_catalog_model(Some(model.catalog_index()));
                plane.position = Vec3::new(-30., 0., altitude);
                plane.grounded = false;
                plane.velocity = Vec3::new(60., sideways, 0.);
                plane.yaw = plane.velocity.y.atan2(plane.velocity.x);
                for _ in 0..120 {
                    plane.step(city, Default::default(), 1. / 120.);
                    if plane.collisions > 0 {
                        break;
                    }
                }
                assert_eq!(plane.collisions > 0, impact);
                if impact {
                    assert!(
                        plane.position.x < -0.40 * plane.length()
                            && (plane.position.z - altitude).abs() < 2.
                    );
                    assert!(plane.velocity.x < -5.);
                    if sideways > 0. {
                        assert!(plane.velocity.y > 4.);
                    }
                    let hit = plane.position;
                    plane.step(city, Default::default(), 1. / 60.);
                    assert!(plane.position.x < hit.x);
                } else {
                    assert!(plane.position.x > 20.);
                }
            }
        }
    }

    #[test]
    fn aircraft_stop_at_a_wall_while_taxiing_and_rebound_off_a_roof() {
        let hermes = Hermes::test_map_from_source("field=concrete(-4,-4,4,-4,4,4,-4,4); elevation(field,0.01); wall=building(0.40,0,-1,0.80,-1,0.80,1,0,1); player_start(-1,0,0);");
        let city = hermes.city();
        for model in aircraft::Model::ALL {
            let mut plane = Vehicle::spawn_at(
                city,
                SpawnPoint {
                    point: [-1., 0.],
                    heading: Some(0.),
                },
            )
            .unwrap();
            plane.set_catalog_model(Some(model.catalog_index()));
            plane.position.x = -plane.length() * 0.50 - 0.10;
            plane.velocity = Vec3::X * 1.5;
            for _ in 0..120 {
                plane.step(city, Default::default(), 1. / 120.);
                if plane.collisions > 0 {
                    break;
                }
            }
            assert!(
                plane.collisions > 0 && plane.grounded && plane.velocity.x.abs() < 0.05,
                "{model:?} hits={} grounded={} position={:?} velocity={:?}",
                plane.collisions,
                plane.grounded,
                plane.position,
                plane.velocity
            );
            plane.collisions = 0;
            plane.position = Vec3::new(40., 0., 47.);
            plane.velocity = -Vec3::Z * 9.;
            plane.grounded = false;
            for _ in 0..120 {
                plane.step(city, Default::default(), 1. / 120.);
                if plane.collisions > 0 {
                    break;
                }
            }
            assert!(plane.collisions > 0 && plane.position.z > 40. && plane.velocity.z > 0.);
        }
    }

    #[test]
    fn aircraft_stay_in_flight_across_map_bounds_and_ditch_at_the_touchdown_site() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/mtl/map.road");
        let hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let city = hermes.city();
        let mut world = DynamicWorld::new(&hermes);
        for model in aircraft::Model::ALL {
            world.switch_vehicle(model.catalog_index());
            let id = world.player_vehicle.0;
            let edge = city.bounds[2] * METERS_PER_UNIT;
            let plane = &mut world.vehicles[id];
            plane.yaw = 0.;
            plane.position = Vec3::new(edge - 0.1, 0., 90.);
            plane.velocity = Vec3::X * 60.;
            plane.grounded = false;
            world.step(&hermes, Default::default(), Default::default(), 1. / 60.);
            assert!(world.player().position.x > edge && world.player().position.z > 89.);
            let plane = &mut world.vehicles[id];
            plane.velocity.x = -60.;
            plane.yaw = std::f32::consts::PI;
            for _ in 0..2 {
                world.step(&hermes, Default::default(), Default::default(), 1. / 60.);
            }
            assert!(world.player().position.x < edge && world.player().position.z > 89.);

            let sea = Vec2::new(20., -35.);
            assert_eq!(
                city.surface_at(sea.to_array(), f32::INFINITY).unwrap().kind,
                opencity::runtime::SurfaceKind::Water
            );
            let plane = &mut world.vehicles[id];
            let waterline =
                city.elevation.as_ref().unwrap().spec.water_level.unwrap() * METERS_PER_UNIT;
            plane.position = (sea * METERS_PER_UNIT).extend(waterline + plane.ride_height() + 0.05);
            plane.yaw = 0.;
            plane.velocity = Vec3::new(30., 0., -6.);
            for _ in 0..24 {
                world.step(&hermes, Default::default(), Default::default(), 1. / 60.);
            }
            let plane = world.player();
            assert!(plane.grounded && plane.surface == opencity::runtime::SurfaceKind::Water);
            assert!((plane.position.z - plane.ride_height() - waterline).abs() < 0.01);
            assert!(plane.position.truncate().distance(sea * METERS_PER_UNIT) < 13.);
            assert!(plane.velocity.x < 22.);
        }
    }

    #[test]
    fn aircraft_take_off_retract_and_extend_the_gear_then_land() {
        let hermes = Hermes::test_map_from_source(
            "field=concrete(-30,-8,30,-8,30,8,-30,8); elevation(field,0.122001);
             player_start(-24,0,0);",
        );
        let city = hermes.city();
        for model in aircraft::Model::ALL {
            let mut plane = Vehicle::spawn_at(
                city,
                SpawnPoint {
                    point: [-24., 0.],
                    heading: Some(0.),
                },
            )
            .unwrap();
            plane.set_catalog_model(Some(model.catalog_index()));
            let start = plane.position;
            let dt = 1. / 60.;
            for _ in 0..1200 {
                plane.step(
                    city,
                    Controls {
                        throttle: 1.,
                        aircraft_pitch: 1.,
                        ..Controls::default()
                    },
                    dt,
                );
                if plane.aircraft_gear > 0.95 {
                    break;
                }
            }
            assert!(!plane.grounded && plane.position.z > start.z + 24.);
            assert!(plane.aircraft_gear > 0.95);
            plane.step(
                city,
                Controls {
                    throttle: 1.,
                    steering: 1.,
                    aircraft_yaw: 1.,
                    ..Controls::default()
                },
                dt,
            );
            assert!(
                plane.yaw > 0. && plane.roll < 0.,
                "left input banks into a left turn"
            );
            for _ in 0..1000 {
                plane.step(
                    city,
                    Controls {
                        brake: true,
                        aircraft_pitch: -1.,
                        ..Controls::default()
                    },
                    dt,
                );
                if plane.grounded {
                    break;
                }
            }
            assert!(plane.grounded);
            assert!((plane.position.z - start.z).abs() < 0.02);
            assert!(plane.aircraft_gear < 0.1);
        }
    }
}
