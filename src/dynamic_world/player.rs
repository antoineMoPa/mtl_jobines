use super::{Vehicle, METERS_PER_UNIT};
use glam::{Quat, Vec2, Vec3};
use opencity::runtime::{City, Mesh, SurfaceKind};

const HALF_HEIGHT: f32 = 0.55;
const RADIUS: f32 = 0.4;
const WALK_SPEED: f32 = 5.5;
const RUN_SPEED: f32 = 10.;
const TURN_SPEED: f32 = std::f32::consts::PI * 5. / 6.;

#[derive(Clone, Copy, Default)]
pub struct WalkControls {
    pub forward: f32,
    pub turn: f32,
    pub run: bool,
}

pub struct Pedestrian {
    pub position: Vec3,
    pub yaw: f32,
}
impl Pedestrian {
    pub fn beside(city: &City, vehicle: &Vehicle) -> Option<Self> {
        let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
        let left = Vec2::new(-forward.y, forward.x);
        for offset in [left * 1.8, -left * 1.8, -forward * 3., forward * 3.] {
            let point = vehicle.position.truncate() + offset;
            let surface = city.surface_at(
                (point / METERS_PER_UNIT).to_array(),
                (vehicle.position.z + 0.7) / METERS_PER_UNIT,
            );
            let Some(surface) = surface else { continue };
            let support = surface.height * METERS_PER_UNIT;
            if (support - (vehicle.position.z - 0.65)).abs() > 0.6 {
                continue;
            }
            let position = point.extend(support + HALF_HEIGHT);
            if !Self::blocked(city, position) {
                return Some(Self {
                    position,
                    yaw: vehicle.yaw,
                });
            }
        }
        None
    }
    fn blocked(city: &City, position: Vec3) -> bool {
        city.building_collision(
            (position.truncate() / METERS_PER_UNIT).to_array(),
            RADIUS / METERS_PER_UNIT,
            (position.z - HALF_HEIGHT + 0.05) / METERS_PER_UNIT,
            (position.z + HALF_HEIGHT) / METERS_PER_UNIT,
        )
    }
    pub fn step(&mut self, city: &City, vehicle: &Vehicle, input: WalkControls, dt: f32) {
        let turn = input.turn * TURN_SPEED * dt;
        // Move along the middle of this step's turn for consistent curved paths.
        let heading = self.yaw + turn * 0.5;
        self.yaw += turn;
        let forward = Vec2::new(heading.cos(), heading.sin());
        let movement =
            forward * input.forward * if input.run { RUN_SPEED } else { WALK_SPEED } * dt;
        for delta in [Vec2::new(movement.x, 0.), Vec2::new(0., movement.y)] {
            let point = self.position.truncate() + delta;
            let Some(surface) = city.surface_at(
                (point / METERS_PER_UNIT).to_array(),
                (self.position.z - HALF_HEIGHT + 0.4) / METERS_PER_UNIT,
            ) else {
                continue;
            };
            let position = point.extend(surface.height * METERS_PER_UNIT + HALF_HEIGHT);
            // Reject cliffs and walls; permit curbs and gradual slopes.
            if (position.z - self.position.z).abs() > 0.45 || Self::blocked(city, position) {
                continue;
            }
            let car_forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
            let car_left = Vec2::new(-car_forward.y, car_forward.x);
            let relative = point - vehicle.position.truncate();
            if (position.z - vehicle.position.z).abs() < 1.5
                && relative.dot(car_forward).abs() < 2.2 + RADIUS
                && relative.dot(car_left).abs() < 1.0 + RADIUS
            {
                continue;
            }
            self.position = position;
        }
    }
    pub fn mesh(&self) -> Mesh {
        let mut mesh = Mesh::default();
        let half = Vec3::new(RADIUS, RADIUS, HALF_HEIGHT);
        mesh.cuboid(
            (-half).to_array(),
            half.to_array(),
            [0.95, 0.38, 0.12],
            SurfaceKind::Prop,
        );
        let rotation = Quat::from_rotation_z(self.yaw);
        for vertex in &mut mesh.vertices {
            vertex.position = ((self.position + rotation * Vec3::from_array(vertex.position))
                / METERS_PER_UNIT)
                .to_array();
            vertex.normal = (rotation * Vec3::from_array(vertex.normal)).to_array();
        }
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn city() -> City {
        City::from_program(
            &opencity::parse("road street: sidewalk carlane sidewalk; end street(-25,3,-20,3);")
                .unwrap(),
            None,
        )
        .unwrap()
    }
    #[test]
    fn exit_uses_ground_and_walk_does_not_pass_through_car() {
        let city = city();
        let vehicle = Vehicle::spawn(&city);
        let mut player = Pedestrian::beside(&city, &vehicle).unwrap();
        let start = player.position;
        player.yaw = vehicle.yaw - std::f32::consts::FRAC_PI_2;
        assert!(!Pedestrian::blocked(&city, player.position));
        for _ in 0..120 {
            player.step(
                &city,
                &vehicle,
                WalkControls {
                    forward: 1.,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        assert!(
            (player.position - start).length() < 0.8,
            "walked through parked vehicle"
        );
    }
    #[test]
    fn exits_reject_lower_decks_and_surrounding_walls() {
        let city = city();
        let mut vehicle = Vehicle::spawn(&city);
        vehicle.position.z += 4.;
        assert!(Pedestrian::beside(&city, &vehicle).is_none());
        let blocked = City::from_program(&opencity::parse("road street: sidewalk carlane sidewalk; end street(-25,3,-20,3); building(0.2,-22.6,2.9,-22.4,2.9,-22.4,3.1,-22.6,3.1);").unwrap(), None).unwrap();
        let vehicle = Vehicle::spawn(&blocked);
        assert!(Pedestrian::beside(&blocked, &vehicle).is_none());
    }
    #[test]
    fn cube_faces_follow_walking_heading_in_both_turn_directions() {
        let city = city();
        let vehicle = Vehicle::spawn(&city);
        for direction in [-1., 1.] {
            let mut player = Pedestrian::beside(&city, &vehicle).unwrap();
            player.yaw = 0.;
            let initial = player.mesh();
            player.step(
                &city,
                &vehicle,
                WalkControls {
                    turn: direction,
                    ..Default::default()
                },
                0.2,
            );
            let turned = player.mesh();
            let forward = Vec3::new(player.yaw.cos(), player.yaw.sin(), 0.);
            let left = Vec3::new(-forward.y, forward.x, 0.);
            for (before, after) in initial.vertices.iter().zip(&turned.vertices) {
                let local = Vec3::from_array(before.position) * METERS_PER_UNIT - player.position;
                let world = Vec3::from_array(after.position) * METERS_PER_UNIT - player.position;
                assert!((world.dot(forward) - local.x).abs() < 0.001);
                assert!((world.dot(left) - local.y).abs() < 0.001);
                let normal = Vec3::from_array(after.normal);
                assert!((normal.dot(forward) - before.normal[0]).abs() < 1e-5);
                assert!((normal.dot(left) - before.normal[1]).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn turning_in_place_changes_heading_without_sideways_movement() {
        let city = city();
        let vehicle = Vehicle::spawn(&city);
        for direction in [-1., 1.] {
            let mut player = Pedestrian::beside(&city, &vehicle).unwrap();
            let origin = player.position;
            let heading = player.yaw;
            for _ in 0..120 {
                player.step(
                    &city,
                    &vehicle,
                    WalkControls {
                        turn: direction,
                        ..Default::default()
                    },
                    1. / 120.,
                );
            }
            assert_eq!(player.position, origin);
            assert!((player.yaw - heading - direction * TURN_SPEED).abs() < 0.0001);
        }
    }
    #[test]
    fn walking_and_running_use_the_players_heading_at_the_faster_speeds() {
        let city = City::from_program(
            &opencity::parse(
                "land(-10,-10,10,-10,10,10,-10,10); road street:carlane;end street(-2,0,-1,0);",
            )
            .unwrap(),
            None,
        )
        .unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        vehicle.position.x = -100.;
        let origin = Vec3::Z
            * (city.surface_at([0., 0.], 1.).unwrap().height * METERS_PER_UNIT + HALF_HEIGHT);
        for (run, speed) in [(false, 5.5), (true, 10.)] {
            for forward in [-1., 1.] {
                let mut player = Pedestrian {
                    position: origin,
                    yaw: std::f32::consts::FRAC_PI_2,
                };
                for _ in 0..120 {
                    player.step(
                        &city,
                        &vehicle,
                        WalkControls {
                            forward,
                            run,
                            ..Default::default()
                        },
                        1. / 120.,
                    );
                }
                assert!(player.position.x.abs() < 0.0001);
                assert!((player.position.y - forward * speed).abs() < 0.001);
            }
        }
    }
    #[test]
    fn forward_turning_follows_a_curve_independently_of_step_frequency() {
        let city = City::from_program(
            &opencity::parse(
                "land(-10,-10,10,-10,10,10,-10,10); road street:carlane;end street(-2,0,-1,0);",
            )
            .unwrap(),
            None,
        )
        .unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        vehicle.position.x = -100.;
        let origin = Vec3::Z
            * (city.surface_at([0., 0.], 1.).unwrap().height * METERS_PER_UNIT + HALF_HEIGHT);
        let simulate = |steps| {
            let mut player = Pedestrian {
                position: origin,
                yaw: 0.,
            };
            for _ in 0..steps {
                player.step(
                    &city,
                    &vehicle,
                    WalkControls {
                        forward: 1.,
                        turn: 1.,
                        ..Default::default()
                    },
                    1. / steps as f32,
                );
            }
            player
        };
        let a = simulate(60);
        let b = simulate(120);
        assert!((a.yaw - b.yaw).abs() < 0.0001);
        assert!(a.position.distance(b.position) < 0.001);
        assert!(b.position.x > 0. && b.position.y > 0.);
        let radius = WALK_SPEED / TURN_SPEED;
        assert!((b.position.x - radius * TURN_SPEED.sin()).abs() < 0.001);
        assert!((b.position.y - radius * (1. - TURN_SPEED.cos())).abs() < 0.001);
    }
}
