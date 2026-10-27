use glam::{Quat, Vec2, Vec3};
use opencity::{
    runtime::{City, Mesh, SurfaceKind, SurfaceSample},
    RoadDirection, RoadElementKind,
};
pub const METERS_PER_UNIT: f32 = 100.;
const RIDE_HEIGHT: f32 = 0.65;
const WHEELBASE: f32 = 2.6;
const HALF_TRACK: f32 = 0.94;
const WHEEL_STEP_HEIGHT: f32 = 0.4;
// Arcade tires allow tighter city corners while preserving the surface's grip
// ratio and the shared acceleration/braking budget (an elliptical tire envelope).
const LATERAL_GRIP_SCALE: f32 = 1.65;
const STEERING_SPEED_TAPER: f32 = 0.012;
// Arcade service brakes shorten stops without changing powered tire grip.
const SERVICE_BRAKE_GRIP_SCALE: f32 = 1.8;
const BUILDING_REBOUND: f32 = 0.45;
// Rectangle inertia divided by mass, for the 4.2 x 1.8 metre chassis.
const CHASSIS_YAW_INERTIA: f32 = (4.2 * 4.2 + 1.8 * 1.8) / 12.;
// All tire impulses are divided by chassis mass; inertia uses the same units.
#[derive(Clone, Copy)]
struct TireContact {
    arm: Vec2,
    forward: Vec2,
    grip: f32,
    rolling: f32,
    rear: bool,
}

#[derive(Default, Clone, Copy)]
pub struct Controls {
    pub throttle: f32,
    pub brake: bool,
    pub steering: f32,
    pub handbrake: bool,
}
pub struct Vehicle {
    pub position: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub steering: f32,
    pub wheel_angle: f32,
    pub surface: SurfaceKind,
    pub grounded: bool,
    pub collisions: u64,
    /// Chassis angular velocity in radians per second, driven by tire/contact torque.
    pub yaw_rate: f32,
}
impl Vehicle {
    pub fn spawn(city: &City) -> Self {
        let (road, band) = city
            .roads
            .iter()
            .filter_map(|r| {
                r.definition
                    .elements
                    .iter()
                    .position(|e| e.kind == RoadElementKind::CarLane)
                    .map(|b| (r, b))
            })
            .min_by(|(a, _), (b, _)| {
                let a = a.segment.point_at(0.5);
                let b = b.segment.point_at(0.5);
                ((a.x + 24.).powi(2) + (a.y - 3.).powi(2))
                    .total_cmp(&((b.x + 24.).powi(2) + (b.y - 3.).powi(2)))
            })
            .expect("map needs at least one car lane");
        let p =
            opencity::map::element_position(&road.segment, &road.definition, band, 0.5).unwrap();
        let t = road.segment.tangent_at(0.5);
        let reverse = road.definition.lane_direction(band) == Some(RoadDirection::Reverse);
        let yaw = t.y.atan2(t.x) + if reverse { std::f32::consts::PI } else { 0. };
        let height = city
            .surface_at([p.x, p.y], road.segment.height_at(0.5) + 0.008)
            .expect("spawn lane support")
            .height;
        Self {
            position: Vec3::new(
                p.x * METERS_PER_UNIT,
                p.y * METERS_PER_UNIT,
                height * METERS_PER_UNIT + RIDE_HEIGHT,
            ),
            velocity: Vec3::ZERO,
            yaw,
            pitch: 0.,
            roll: 0.,
            steering: 0.,
            wheel_angle: 0.,
            surface: SurfaceKind::Asphalt,
            grounded: true,
            collisions: 0,
            yaw_rate: 0.,
        }
    }
    pub fn orientation(&self) -> Quat {
        Quat::from_rotation_z(self.yaw)
            * Quat::from_rotation_y(-self.pitch)
            * Quat::from_rotation_x(self.roll)
    }
    pub fn step(&mut self, city: &City, input: Controls, dt: f32) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        // Keep the suspension and tire impulses stable when a caller uses larger frames.
        let steps = (dt / (1. / 120.)).ceil().max(1.) as usize;
        for _ in 0..steps {
            self.step_substep(city, input, dt / steps as f32);
        }
    }

    fn step_substep(&mut self, city: &City, input: Controls, dt: f32) {
        let f = Vec2::new(self.yaw.cos(), self.yaw.sin());
        let left = Vec2::new(-f.y, f.x);
        let speed = self.velocity.truncate().dot(f);
        let target_steering =
            input.steering.clamp(-1., 1.) * 0.52 / (1. + speed.abs() * STEERING_SPEED_TAPER);
        self.steering += (target_steering - self.steering) * (1. - (-8. * dt).exp());
        let mut gradient = Vec2::ZERO;
        let mut support = 0.;
        let mut contacts = 0;
        let mut tires = [None; 4];
        let mut worst = SurfaceKind::Asphalt;
        for (index, (wheel, sample)) in self.wheel_contacts(city).into_iter().enumerate() {
            if let Some(s) = sample {
                let h = s.height * METERS_PER_UNIT;
                if wheel.z - h < 0.6 {
                    support += h;
                    gradient += Vec2::from_array(s.gradient);
                    let rear = index < 2;
                    let angle = self.yaw + if rear { 0. } else { self.steering };
                    let gravity =
                        9.81 / (1. + Vec2::from_array(s.gradient).length_squared()).sqrt();
                    tires[index] = Some(TireContact {
                        arm: wheel.truncate() - self.position.truncate(),
                        forward: Vec2::new(angle.cos(), angle.sin()),
                        // Each supported tire carries one quarter of the static load.
                        grip: s.kind.friction() * gravity * 0.25,
                        rolling: s.kind.rolling_resistance() * gravity * 0.25,
                        rear,
                    });
                    contacts += 1;
                    if s.kind.friction() < worst.friction() {
                        worst = s.kind;
                    }
                }
            }
        }
        self.grounded = contacts > 0;
        self.surface = worst;
        let mut v = self.velocity.truncate();
        let speed = v.dot(f);
        if contacts > 0 {
            let count = contacts as f32;
            support /= count;
            gradient /= count;
            self.apply_tire_forces(&tires, input, &mut v, dt);
            // Gravity projected onto the support plane, in world horizontal axes.
            v -= gradient * (9.81 / (1. + gradient.length_squared())) * dt;
            let target_pitch = gradient.dot(f).atan();
            let target_roll = gradient.dot(left).atan();
            self.pitch += (target_pitch - self.pitch) * (dt * 9.).min(1.);
            self.roll += (target_roll - self.roll) * (dt * 9.).min(1.);
            let target = support + RIDE_HEIGHT;
            self.velocity.z += ((target - self.position.z) * 90. - self.velocity.z * 14.) * dt;
        } else {
            self.velocity.z -= 9.81 * dt;
        }
        // Quadratic air drag acts on the whole velocity, including while airborne.
        v /= 1. + 0.004 * v.length() * dt;
        self.velocity.x = v.x;
        self.velocity.y = v.y;
        self.move_with_building_contacts(city, dt);
        // Resolve at the new footprint, including chassis tilt and impact yaw.
        // Suspension smoothing must never leave a tire below a curb or verge.
        let mut lift = 0.0_f32;
        for (wheel, sample) in self.wheel_contacts(city) {
            if let Some(surface) = sample {
                lift = lift.max(surface.height * METERS_PER_UNIT - wheel.z);
            }
        }
        if lift > 0. {
            self.position.z += lift;
            self.velocity.z = self.velocity.z.max(0.);
        }
        self.wheel_angle += speed / 0.32 * dt;
    }

    fn apply_tire_forces(
        &mut self,
        tires: &[Option<TireContact>; 4],
        input: Controls,
        velocity: &mut Vec2,
        dt: f32,
    ) {
        let forward = Vec2::new(self.yaw.cos(), self.yaw.sin());
        let speed = velocity.dot(forward);
        let throttle = input.throttle.clamp(-1., 1.);
        let motor = if input.brake || input.handbrake {
            0.
        } else if throttle >= 0. {
            6. * throttle * (1. - (speed.max(0.) / 36.).powi(2)).max(0.)
        } else {
            3. * throttle * (1. - ((-speed).max(0.) / 10.).powi(2)).max(0.)
        };
        let mut longitudinal_usage = [0.; 4];
        let mut impulse = Vec2::ZERO;
        let mut torque = 0.;
        for (index, tire) in tires.iter().enumerate() {
            let Some(tire) = tire else { continue };
            let point_velocity = *velocity + Vec2::new(-tire.arm.y, tire.arm.x) * self.yaw_rate;
            let longitudinal_speed = point_velocity.dot(tire.forward);
            // Four simultaneous tire impulses must not over-correct contact speed.
            // This bound includes the rotational response at the contact point.
            let response = 4. * (1. + tire.arm.length_squared() / CHASSIS_YAW_INERTIA);
            let stop_impulse = longitudinal_speed.abs() / response;
            let brake_scale = if input.brake {
                SERVICE_BRAKE_GRIP_SCALE
            } else {
                1.
            };
            let braking = input.brake || (input.handbrake && tire.rear);
            let drive = if braking {
                -longitudinal_speed.signum()
                    * (tire.grip * brake_scale * 0.9 * dt).min(stop_impulse)
            } else {
                // Leave lateral authority on slippery surfaces under power.
                (motor * 0.25).clamp(-tire.grip * 0.8, tire.grip * 0.8) * dt
            };
            longitudinal_usage[index] = if tire.grip > 0. {
                (drive / (tire.grip * brake_scale * dt)).clamp(-1., 1.)
            } else {
                0.
            };
            let rolling = -longitudinal_speed.signum()
                * (tire.rolling * dt).min((stop_impulse - drive.abs()).max(0.));
            let wheel_impulse = tire.forward * (drive + rolling);
            impulse += wheel_impulse;
            torque += tire.arm.perp_dot(wheel_impulse);
        }
        *velocity += impulse;
        self.yaw_rate += torque / CHASSIS_YAW_INERTIA;

        impulse = Vec2::ZERO;
        torque = 0.;
        for (index, tire) in tires.iter().enumerate() {
            let Some(tire) = tire else { continue };
            let lateral = Vec2::new(-tire.forward.y, tire.forward.x);
            let point_velocity = *velocity + Vec2::new(-tire.arm.y, tire.arm.x) * self.yaw_rate;
            let slip = point_velocity.dot(lateral);
            let response = 4. * (1. + tire.arm.length_squared() / CHASSIS_YAW_INERTIA);
            // Linear slip-angle response at speed; viscous slip at parking speed.
            // The impulse bound prevents oscillation and passive energy gain.
            let stiffness = 30. / (1. + point_velocity.dot(tire.forward).abs() * 0.08);
            let requested = -slip * (stiffness * dt).min(1. / response);
            let limit = tire.grip
                * LATERAL_GRIP_SCALE
                * (1. - longitudinal_usage[index].powi(2)).max(0.).sqrt()
                * if input.handbrake && tire.rear {
                    0.2
                } else {
                    1.
                }
                * dt;
            let wheel_impulse = lateral * requested.clamp(-limit, limit);
            impulse += wheel_impulse;
            torque += tire.arm.perp_dot(wheel_impulse);
        }
        *velocity += impulse;
        self.yaw_rate += torque / CHASSIS_YAW_INERTIA;
    }

    fn wheel_contacts(&self, city: &City) -> [(Vec3, Option<SurfaceSample>); 4] {
        let orientation = self.orientation();
        [
            [-WHEELBASE * 0.5, -HALF_TRACK],
            [-WHEELBASE * 0.5, HALF_TRACK],
            [WHEELBASE * 0.5, -HALF_TRACK],
            [WHEELBASE * 0.5, HALF_TRACK],
        ]
        .map(|[x, y]| {
            let wheel = self.position + orientation * Vec3::new(x, y, -RIDE_HEIGHT);
            let surface = city.surface_at(
                (wheel.truncate() / METERS_PER_UNIT).to_array(),
                (wheel.z + WHEEL_STEP_HEIGHT) / METERS_PER_UNIT,
            );
            (wheel, surface)
        })
    }
    fn move_with_building_contacts(&mut self, city: &City, dt: f32) {
        // Sweep in increments shorter than the body radius to avoid tunneling.
        let steps = ((self.velocity.length() + self.yaw_rate.abs() * 1.2) * dt / 0.4)
            .ceil()
            .max(1.) as usize;
        let step_dt = dt / steps as f32;
        let mut collided = false;
        for _ in 0..steps {
            self.position += self.velocity * step_dt;
            // Angular momentum persists until a tire or contact supplies torque.
            // Rotating the chassis does not rotate its world-space velocity.
            self.yaw += self.yaw_rate * step_dt;
            for _ in 0..8 {
                // Rotation changes the capsule footprint: always query the
                // current heading, including while separating a resting car.
                let forward = Vec2::new(self.yaw.cos(), self.yaw.sin());
                let Some((normal, depth, arm)) = self.building_contact(city, forward) else {
                    break;
                };
                collided = true;
                let separation = normal * (depth + 0.001);
                self.position.x += separation.x;
                self.position.y += separation.y;
                let inward_speed = self.velocity.truncate().dot(normal);
                if inward_speed < 0. {
                    let incoming = self.velocity.truncate();
                    // Preserve the reflected ray's angle, while losing only
                    // energy associated with the normal impact. Grazing scrapes
                    // retain almost all speed; head-on hits retain 45%.
                    let normal_energy_fraction = (inward_speed * inward_speed
                        / incoming.length_squared().max(f32::MIN_POSITIVE))
                    .clamp(0., 1.);
                    let retention = (1.
                        - (1. - BUILDING_REBOUND * BUILDING_REBOUND) * normal_energy_fraction)
                        .sqrt();
                    let outgoing = (incoming - 2. * inward_speed * normal) * retention;
                    self.velocity.x = outgoing.x;
                    self.velocity.y = outgoing.y;
                    // The nose/tail contact's normal impulse supplies yaw torque.
                    // Symmetric head-on hits have no lever across the normal;
                    // reverse impacts automatically use the rear contact arm.
                    let normal_impulse = normal * (-(1. + retention) * inward_speed);
                    let yaw_impulse = arm.perp_dot(normal_impulse) / CHASSIS_YAW_INERTIA;
                    let lost_energy =
                        (incoming.length_squared() - outgoing.length_squared()).max(0.);
                    let spin_limit = (self.yaw_rate * self.yaw_rate
                        + lost_energy / CHASSIS_YAW_INERTIA)
                        .sqrt()
                        .min(2.8);
                    self.yaw_rate = (self.yaw_rate + yaw_impulse).clamp(-spin_limit, spin_limit);
                }
            }
        }
        if collided {
            self.collisions += 1;
        }
    }

    fn building_contact(&self, city: &City, forward: Vec2) -> Option<(Vec2, f32, Vec2)> {
        let mut deepest = None;
        for building in &city.landscape {
            if building.kind != opencity::LandscapeKind::Building
                || building.elevation * METERS_PER_UNIT >= self.position.z + 0.45
                || (building.elevation + building.height.unwrap_or(0.)) * METERS_PER_UNIT
                    <= self.position.z - 0.35
            {
                continue;
            }
            for offset in [-1.2, 0., 1.2] {
                let center = self.position.truncate() + forward * offset;
                let mut inside = false;
                let mut nearest = Vec2::ZERO;
                let mut edge_normal = Vec2::X;
                let mut distance_squared = f32::INFINITY;
                for (a, b) in building
                    .points
                    .iter()
                    .zip(building.points.iter().cycle().skip(1))
                {
                    let a = Vec2::from_array(*a) * METERS_PER_UNIT;
                    let b = Vec2::from_array(*b) * METERS_PER_UNIT;
                    let edge = b - a;
                    if (a.y > center.y) != (b.y > center.y)
                        && center.x < edge.x * (center.y - a.y) / edge.y + a.x
                    {
                        inside = !inside;
                    }
                    let length_squared = edge.length_squared();
                    if length_squared == 0. {
                        continue;
                    }
                    let t = ((center - a).dot(edge) / length_squared).clamp(0., 1.);
                    let point = a + edge * t;
                    let d = center.distance_squared(point);
                    if d < distance_squared {
                        distance_squared = d;
                        nearest = point;
                        edge_normal = Vec2::new(-edge.y, edge.x) / length_squared.sqrt();
                    }
                }
                let distance = distance_squared.sqrt();
                let depth = if inside {
                    0.92 + distance
                } else {
                    0.92 - distance
                };
                if depth <= 0. || !depth.is_finite() {
                    continue;
                }
                let normal = if distance > 0.00001 {
                    if inside {
                        (nearest - center) / distance
                    } else {
                        (center - nearest) / distance
                    }
                } else {
                    // Determine which side of an exact edge contact is outside.
                    let probe = (center + edge_normal * 0.01) / METERS_PER_UNIT;
                    if city.building_collision(
                        probe.to_array(),
                        0.,
                        (self.position.z - 0.35) / METERS_PER_UNIT,
                        (self.position.z + 0.45) / METERS_PER_UNIT,
                    ) {
                        -edge_normal
                    } else {
                        edge_normal
                    }
                };
                if deepest.is_none_or(|(_, previous, _)| depth > previous) {
                    deepest = Some((normal, depth, forward * offset));
                }
            }
        }
        deepest
    }

    pub fn mesh(&self) -> Mesh {
        let mut m = Mesh::default();
        m.cuboid(
            [-2.1, -0.9, -0.35],
            [2.1, 0.9, 0.30],
            [0.68, 0.13, 0.035],
            SurfaceKind::Prop,
        );
        m.cuboid(
            [-0.8, -0.77, 0.30],
            [0.85, 0.77, 1.0],
            [0.06, 0.13, 0.17],
            SurfaceKind::Prop,
        );
        m.cuboid(
            [2.09, -0.80, -0.08],
            [2.12, 0.80, 0.08],
            [0.85, 0.8, 0.55],
            SurfaceKind::Marking,
        );
        for x in [-WHEELBASE * 0.5, WHEELBASE * 0.5] {
            for y in [-HALF_TRACK, HALF_TRACK] {
                let start = m.vertices.len();
                m.cylinder(
                    [0., 0., 0.],
                    0.32,
                    0.23,
                    [0.025, 0.028, 0.03],
                    SurfaceKind::Prop,
                );
                let spin = Quat::from_rotation_z(self.wheel_angle);
                let axle = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
                let steer = Quat::from_rotation_z(if x > 0. { self.steering } else { 0. });
                let q = steer * axle * spin;
                for v in &mut m.vertices[start..] {
                    let p = q * Vec3::from_array(v.position) + Vec3::new(x, y + 0.115, -0.33);
                    v.position = p.to_array();
                    v.normal = (q * Vec3::from_array(v.normal)).to_array();
                }
            }
        }
        let q = self.orientation();
        for v in &mut m.vertices {
            v.position =
                ((q * Vec3::from_array(v.position) + self.position) / METERS_PER_UNIT).to_array();
            v.normal = (q * Vec3::from_array(v.normal)).to_array();
        }
        m
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn map() -> City {
        City::from_program(
            &opencity::parse("road street: sidewalk carlane sidewalk; end street(-25,3,-20,3);")
                .unwrap(),
            None,
        )
        .unwrap()
    }
    fn planar_energy(vehicle: &Vehicle) -> f32 {
        vehicle.velocity.truncate().length_squared()
            + CHASSIS_YAW_INERTIA * vehicle.yaw_rate.powi(2)
    }

    #[test]
    fn turns_build_yaw_and_bend_travel_within_surface_grip() {
        let city = map();
        for speed in [-30.0_f32, -20., -10., 10., 20., 30.] {
            for steering in [-1.0_f32, -0.1, 0.1, 1.] {
                let mut vehicle = Vehicle::spawn(&city);
                vehicle.yaw = 0.;
                vehicle.velocity = Vec3::X * speed;
                vehicle.steering = steering * 0.52 / (1. + speed.abs() * STEERING_SPEED_TAPER);
                let start = vehicle.position;
                for _ in 0..30 {
                    let energy = planar_energy(&vehicle);
                    vehicle.step(
                        &city,
                        Controls {
                            steering,
                            ..Default::default()
                        },
                        1. / 120.,
                    );
                    assert!(vehicle.grounded);
                    assert!(vehicle.position.is_finite() && vehicle.velocity.is_finite());
                    assert!(planar_energy(&vehicle) <= energy + 1e-4);
                }
                let travel = vehicle.velocity.truncate() * speed.signum();
                let travel_heading = travel.y.atan2(travel.x);
                let chord = vehicle.position.truncate().distance(start.truncate());
                let radius = chord / (2. * (travel_heading.abs() * 0.5).sin());
                assert_eq!(vehicle.yaw.signum(), (speed * steering).signum());
                if speed > 0. {
                    assert_eq!(travel_heading.signum(), steering.signum());
                }
                // In reverse the front contact initially deflects the tail's
                // travel outward while its torque builds the opposite body yaw.
                // Curvature follows actual travel, including transient sideslip.
                let grip_radius = speed.powi(2) / (0.95 * 9.81 * LATERAL_GRIP_SCALE);
                let geometric_radius = WHEELBASE / vehicle.steering.tan().abs();
                assert!(
                    radius > grip_radius * 0.75,
                    "speed={speed}, radius={radius}"
                );
                if speed > 0. {
                    assert!(radius < geometric_radius.max(grip_radius) * 8.);
                }
                assert!(vehicle.yaw.abs() > 0.001 * steering.abs());
            }
        }
    }

    #[test]
    fn steering_accelerates_yaw_without_snapping_momentum_to_the_body() {
        let city = map();
        for speed in [-5.0_f32, 5., 20.] {
            for direction in [-1.0, 1.0] {
                let mut vehicle = Vehicle::spawn(&city);
                vehicle.yaw = 0.;
                vehicle.velocity = Vec3::X * speed;
                vehicle.steering = direction * 0.4;
                let energy = planar_energy(&vehicle);
                vehicle.step(
                    &city,
                    Controls {
                        steering: direction,
                        ..Default::default()
                    },
                    1. / 120.,
                );
                assert_eq!(vehicle.yaw_rate.signum(), (speed * direction).signum());
                let geometric_rate = speed / WHEELBASE * vehicle.steering.tan();
                assert!(vehicle.yaw_rate.abs() < geometric_rate.abs());
                let travel = vehicle.velocity.truncate() * speed.signum();
                let heading = travel.y.atan2(travel.x);
                assert!((heading - vehicle.yaw).abs() > 1e-5);
                assert!(planar_energy(&vehicle) <= energy);
                // Releasing steering does not discard angular momentum.
                let rate = vehicle.yaw_rate;
                vehicle.step(&city, Controls::default(), 1. / 120.);
                assert_eq!(vehicle.yaw_rate.signum(), rate.signum());
            }
        }
    }
    #[test]
    fn braking_respects_surface_grip_and_never_reverses() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3); ground=land(-26,2,-19,2,-19,5,-26,5); elevation(ground,0.02);").unwrap(),
            None,
        ).unwrap();
        let mut asphalt = Vehicle::spawn(&city);
        let mut grass = Vehicle::spawn(&city);
        grass.position.y += 20.;
        asphalt.velocity = Vec3::X * 20.;
        grass.velocity = Vec3::X * 20.;
        let input = Controls {
            brake: true,
            throttle: 1.,
            ..Default::default()
        };
        let dt = 1. / 120.;
        asphalt.step(&city, input, dt);
        grass.step(&city, input, dt);
        assert!(asphalt.velocity.x < grass.velocity.x);
        // Exclude rolling/air resistance when checking the tire braking budget.
        let deceleration = (20. - asphalt.velocity.x) / dt;
        assert!(
            deceleration - 0.015 * 9.81 - 0.004 * 20_f32.powi(2)
                < 0.95 * 9.81 * SERVICE_BRAKE_GRIP_SCALE
        );
        for _ in 0..600 {
            asphalt.step(&city, input, dt);
            grass.step(&city, input, dt);
            assert!(asphalt.velocity.x >= 0.);
            assert!(grass.velocity.x >= 0.);
        }
        assert!(asphalt.velocity.length() < 0.01);
        assert!(grass.velocity.length() < 0.01);
    }

    #[test]
    fn service_brake_stops_promptly_in_forward_and_reverse() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3); ground=land(-26,2,-19,2,-19,5,-26,5); elevation(ground,0.02);").unwrap(),
            None,
        ).unwrap();
        for speed in [-20., 20.] {
            for on_grass in [false, true] {
                let mut vehicle = Vehicle::spawn(&city);
                if on_grass {
                    vehicle.position.y += 20.;
                }
                vehicle.velocity = Vec3::X * speed;
                let start = vehicle.position;
                let mut elapsed = 0.;
                while vehicle.velocity.truncate().length() > 0.01 && elapsed < 4. {
                    vehicle.step(
                        &city,
                        Controls {
                            brake: true,
                            throttle: speed.signum(),
                            ..Default::default()
                        },
                        1. / 120.,
                    );
                    elapsed += 1. / 120.;
                    assert!(vehicle.velocity.x * speed >= 0., "braking reversed travel");
                }
                let distance = vehicle.position.truncate().distance(start.truncate());
                eprintln!(
                    "service stop speed={speed}, grass={on_grass}: {elapsed:.2}s, {distance:.2}m"
                );
                assert!(elapsed < if on_grass { 2.8 } else { 1.4 });
                assert!(distance < if on_grass { 28. } else { 14. });
            }
        }
    }

    fn flat_tires() -> [Option<TireContact>; 4] {
        [
            Vec2::new(-WHEELBASE * 0.5, -HALF_TRACK),
            Vec2::new(-WHEELBASE * 0.5, HALF_TRACK),
            Vec2::new(WHEELBASE * 0.5, -HALF_TRACK),
            Vec2::new(WHEELBASE * 0.5, HALF_TRACK),
        ]
        .map(|arm| {
            Some(TireContact {
                arm,
                forward: Vec2::X,
                grip: 0.95 * 9.81 * 0.25,
                rolling: 0.,
                rear: arm.x < 0.,
            })
        })
    }

    #[test]
    fn acceleration_and_slide_recovery_share_each_tires_force_budget() {
        let city = map();
        let mut coasting = Vehicle::spawn(&city);
        let mut powered = Vehicle::spawn(&city);
        let mut coast_velocity = Vec2::new(10., 3.);
        let mut power_velocity = coast_velocity;
        coasting.apply_tire_forces(
            &flat_tires(),
            Controls::default(),
            &mut coast_velocity,
            1. / 120.,
        );
        powered.apply_tire_forces(
            &flat_tires(),
            Controls {
                throttle: 1.,
                ..Default::default()
            },
            &mut power_velocity,
            1. / 120.,
        );
        assert!(coast_velocity.y < power_velocity.y);
        assert!(power_velocity.y < 3. && power_velocity.y > 0.);
        assert!(power_velocity.x > coast_velocity.x);

        // One isolated contact exposes the actual force and its moment arm.
        for brake in [false, true] {
            let mut vehicle = Vehicle::spawn(&city);
            let tire = flat_tires()[0].unwrap();
            let mut velocity = Vec2::new(10., 3.);
            let initial = velocity;
            let dt = 1. / 120.;
            vehicle.apply_tire_forces(
                &[Some(tire), None, None, None],
                Controls {
                    throttle: 1.,
                    brake,
                    ..Default::default()
                },
                &mut velocity,
                dt,
            );
            let impulse = velocity - initial;
            let longitudinal_axis =
                tire.grip * if brake { SERVICE_BRAKE_GRIP_SCALE } else { 1. } * dt;
            let lateral_axis = tire.grip * LATERAL_GRIP_SCALE * dt;
            let usage =
                (impulse.x / longitudinal_axis).powi(2) + (impulse.y / lateral_axis).powi(2);
            assert!(usage <= 1.0001, "ellipse usage={usage}");
            assert!(
                (vehicle.yaw_rate * CHASSIS_YAW_INERTIA - tire.arm.perp_dot(impulse)).abs() < 1e-5
            );
        }
    }

    #[test]
    fn tires_damp_spin_but_steering_cannot_rotate_a_stationary_vehicle() {
        let city = map();
        let mut vehicle = Vehicle::spawn(&city);
        for _ in 0..120 {
            vehicle.step(
                &city,
                Controls {
                    steering: 1.,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        assert_eq!(vehicle.yaw_rate, 0.);
        assert_eq!(vehicle.velocity.truncate(), Vec2::ZERO);
        vehicle.yaw_rate = 1.;
        let mut velocity = Vec2::ZERO;
        for _ in 0..120 {
            vehicle.apply_tire_forces(&flat_tires(), Controls::default(), &mut velocity, 1. / 120.);
        }
        assert!(vehicle.yaw_rate.abs() < 0.001);
        assert!(velocity.length() < 0.001);
    }

    #[test]
    fn split_grip_braking_turns_toward_the_grippier_side() {
        let city = map();
        for grippy_side in [-1_f32, 1.] {
            let mut vehicle = Vehicle::spawn(&city);
            let mut tires = flat_tires();
            for tire in tires.iter_mut().flatten() {
                if tire.arm.y * grippy_side < 0. {
                    tire.grip *= 0.2;
                }
            }
            let mut velocity = Vec2::X * 20.;
            vehicle.apply_tire_forces(
                &tires,
                Controls {
                    brake: true,
                    ..Default::default()
                },
                &mut velocity,
                1. / 120.,
            );
            assert!(vehicle.yaw_rate * grippy_side > 0.);
            assert!(velocity.x < 20.);
        }
    }

    #[test]
    fn rear_handbrake_reduces_rear_lateral_authority_and_increases_rotation() {
        let city = map();
        let mut coasting = Vehicle::spawn(&city);
        let mut handbraking = Vehicle::spawn(&city);
        let mut coast_velocity = Vec2::new(10., 4.);
        let mut brake_velocity = coast_velocity;
        coasting.apply_tire_forces(
            &flat_tires(),
            Controls::default(),
            &mut coast_velocity,
            1. / 120.,
        );
        handbraking.apply_tire_forces(
            &flat_tires(),
            Controls {
                handbrake: true,
                ..Default::default()
            },
            &mut brake_velocity,
            1. / 120.,
        );
        assert!(handbraking.yaw_rate < coasting.yaw_rate - 0.01);
        assert!(brake_velocity.y > coast_velocity.y);
        assert!(brake_velocity.x < coast_velocity.x);
        assert!(
            brake_velocity.length_squared() + CHASSIS_YAW_INERTIA * handbraking.yaw_rate.powi(2)
                < 116.
        );
    }

    #[test]
    fn passive_tires_dissipate_translation_and_spin_for_varied_slip_and_support() {
        let city = map();
        for mask in 0..16 {
            for speed in [-30., -0.01, 0., 0.01, 30.] {
                for slip in [-5., 0., 5.] {
                    for spin in [-2_f32, 0., 2.] {
                        for (brake, handbrake) in [(false, false), (true, false), (false, true)] {
                            let mut vehicle = Vehicle::spawn(&city);
                            vehicle.yaw_rate = spin;
                            let mut tires = flat_tires();
                            for (i, tire) in tires.iter_mut().enumerate() {
                                if mask & (1 << i) == 0 {
                                    *tire = None;
                                } else if i >= 2 {
                                    tire.as_mut().unwrap().forward =
                                        Vec2::new(0.4_f32.cos(), 0.4_f32.sin());
                                }
                            }
                            let mut velocity = Vec2::new(speed, slip);
                            let initial =
                                velocity.length_squared() + CHASSIS_YAW_INERTIA * spin.powi(2);
                            vehicle.apply_tire_forces(
                                &tires,
                                Controls {
                                    brake,
                                    handbrake,
                                    ..Default::default()
                                },
                                &mut velocity,
                                1. / 120.,
                            );
                            let energy = velocity.length_squared()
                                + CHASSIS_YAW_INERTIA * vehicle.yaw_rate.powi(2);
                            assert!(energy <= initial + 0.0001, "mask={mask}, speed={speed}, slip={slip}, spin={spin}, brake={brake}, handbrake={handbrake}: {initial} -> {energy}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn airborne_rotation_preserves_angular_momentum_without_tire_authority() {
        let city = map();
        let mut vehicle = Vehicle::spawn(&city);
        vehicle.position.z += 20.;
        vehicle.velocity = Vec3::X * 10.;
        vehicle.yaw_rate = 0.8;
        let yaw = vehicle.yaw;
        for _ in 0..60 {
            vehicle.step(
                &city,
                Controls {
                    steering: 1.,
                    throttle: 1.,
                    brake: true,
                    handbrake: true,
                },
                1. / 120.,
            );
            assert!(!vehicle.grounded);
            assert_eq!(vehicle.yaw_rate, 0.8);
            assert_eq!(vehicle.velocity.y, 0.);
        }
        assert!((vehicle.yaw - yaw - 0.4).abs() < 1e-5);
    }

    #[test]
    fn only_supported_front_wheels_supply_steering_force() {
        let city = City::from_program(
            &opencity::parse("road street: verge verge verge carlane; end street(-25,3,-20,3);")
                .unwrap(),
            None,
        )
        .unwrap();
        let road = &city.roads[0];
        let grass =
            opencity::map::element_position(&road.segment, &road.definition, 1, 0.5).unwrap();
        for direction in [-1., 1.] {
            for on_grass in [false, true] {
                let mut vehicle = Vehicle::spawn(&city);
                if on_grass {
                    vehicle.position.x = grass.x * METERS_PER_UNIT;
                    vehicle.position.y = grass.y * METERS_PER_UNIT;
                } else {
                    // Front wheels over the road's open end: half the normal load.
                    vehicle.position.x = -2000.;
                }
                vehicle.yaw = 0.;
                vehicle.velocity = Vec3::X * 5.;
                vehicle.steering = direction * 0.5;
                vehicle.step(
                    &city,
                    Controls {
                        throttle: 1.,
                        steering: direction,
                        ..Default::default()
                    },
                    1. / 120.,
                );
                assert!(vehicle.grounded);
                assert_eq!(
                    vehicle.surface,
                    if on_grass {
                        SurfaceKind::Grass
                    } else {
                        SurfaceKind::Asphalt
                    }
                );
                assert!(
                    vehicle.velocity.x > 5.,
                    "turning must still allow acceleration"
                );
                if on_grass {
                    assert!(vehicle.yaw_rate * direction > 0., "grass lost steering");
                } else {
                    // With both steered wheels unsupported, steering cannot turn
                    // the chassis or bend momentum through the rear tires.
                    assert!(vehicle.yaw_rate.abs() < 1e-6);
                    assert!(vehicle.velocity.y.abs() < 1e-6);
                }
            }
        }
    }

    #[test]
    fn crossing_curbs_keeps_each_tire_above_the_surface_in_both_directions() {
        let city = City::from_program(
            &opencity::parse("road street: verge verge verge sidewalk carlane sidewalk verge verge verge; end street(-25,3,-20,3);").unwrap(),
            None,
        ).unwrap();
        let road = &city.roads[0];
        for band in [1, 7] {
            for speed in [5., 20.] {
                let start =
                    opencity::map::element_position(&road.segment, &road.definition, band, 0.5)
                        .unwrap();
                let end =
                    opencity::map::element_position(&road.segment, &road.definition, 8 - band, 0.5)
                        .unwrap();
                let direction = (end.y - start.y).signum();
                let mut vehicle = Vehicle::spawn(&city);
                let height = city
                    .surface_at([start.x, start.y], f32::INFINITY)
                    .unwrap()
                    .height;
                vehicle.position = Vec3::new(
                    start.x * METERS_PER_UNIT,
                    start.y * METERS_PER_UNIT,
                    height * METERS_PER_UNIT + RIDE_HEIGHT,
                );
                vehicle.yaw = direction * std::f32::consts::FRAC_PI_2;
                vehicle.velocity = Vec3::Y * direction * speed;
                let mut saw_curb = false;
                let mut reached_other_side = false;
                for _ in 0..240 {
                    vehicle.step(
                        &city,
                        Controls {
                            throttle: 1.,
                            ..Default::default()
                        },
                        1. / 30.,
                    );
                    for (wheel, sample) in vehicle.wheel_contacts(&city) {
                        let surface = sample.unwrap_or_else(|| panic!("wheel left road profile: band={band}, speed={speed}, position={:?}, wheel={wheel:?}", vehicle.position));
                        saw_curb |= surface.kind == SurfaceKind::Sidewalk;
                        assert!(
                            wheel.z >= surface.height * METERS_PER_UNIT - 1e-4,
                            "tire below curb: {wheel:?}, surface={surface:?}"
                        );
                    }
                    if (vehicle.position.y - end.y * METERS_PER_UNIT) * direction >= 0. {
                        reached_other_side = true;
                        break;
                    }
                }
                assert!(saw_curb && reached_other_side);
            }
        }
    }

    #[test]
    fn default_map_roadside_reentry_does_not_pass_under_the_pavement() {
        let city = City::load(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/opencity/examples/default_map.road"
        )))
        .unwrap();
        let spawn = Vehicle::spawn(&city);
        let road = city
            .roads
            .iter()
            .min_by(|a, b| {
                let distance = |road: &opencity::runtime::Road| {
                    let p = road.segment.point_at(0.5);
                    Vec2::new(p.x, p.y)
                        .distance_squared(spawn.position.truncate() / METERS_PER_UNIT)
                };
                distance(a).total_cmp(&distance(b))
            })
            .unwrap();
        let center = road.segment.point_at(0.5);
        for band in [0, road.definition.elements.len() - 1] {
            let edge = opencity::map::element_position(&road.segment, &road.definition, band, 0.5)
                .unwrap();
            let angle = (edge.y - center.y).atan2(edge.x - center.x);
            let outward = Vec2::new(angle.cos(), angle.sin());
            let start = Vec2::new(edge.x, edge.y) * METERS_PER_UNIT + outward * 4.;
            let ceiling = road.segment.height_at(0.5) + 0.01;
            let ground = city
                .surface_at((start / METERS_PER_UNIT).to_array(), ceiling)
                .unwrap();
            let mut vehicle = Vehicle::spawn(&city);
            vehicle.position = start.extend(ground.height * METERS_PER_UNIT + RIDE_HEIGHT);
            vehicle.yaw = angle + std::f32::consts::PI;
            vehicle.velocity = (-outward * 5.).extend(0.);
            let mut reached_lane = false;
            for _ in 0..240 {
                vehicle.step(
                    &city,
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    1. / 30.,
                );
                for (wheel, _) in vehicle.wheel_contacts(&city) {
                    let surface = city
                        .surface_at((wheel.truncate() / METERS_PER_UNIT).to_array(), ceiling)
                        .unwrap();
                    assert!(
                        wheel.z >= surface.height * METERS_PER_UNIT - 0.001,
                        "default map band={band}: wheel={wheel:?}, surface={surface:?}"
                    );
                }
                if (vehicle.position.truncate() - Vec2::new(center.x, center.y) * METERS_PER_UNIT)
                    .dot(outward)
                    < 0.5
                {
                    reached_lane = true;
                    break;
                }
            }
            assert!(reached_lane);
        }
    }

    #[test]
    fn larger_frame_matches_fixed_substeps_and_airborne_has_no_tire_authority() {
        let city = map();
        let mut framed = Vehicle::spawn(&city);
        let mut fixed = Vehicle::spawn(&city);
        let input = Controls {
            throttle: 1.,
            steering: 0.3,
            ..Default::default()
        };
        for _ in 0..30 {
            framed.step(&city, input, 1. / 30.);
            for _ in 0..4 {
                fixed.step(&city, input, 1. / 120.);
            }
        }
        assert!(framed.position.distance(fixed.position) < 0.001);
        assert!(framed.velocity.distance(fixed.velocity) < 0.001);
        let mut airborne = Vehicle::spawn(&city);
        airborne.position.z += 10.;
        airborne.velocity = Vec3::X * 20.;
        let yaw = airborne.yaw;
        airborne.step(&city, input, 1. / 30.);
        assert!(!airborne.grounded);
        assert_eq!(airborne.yaw, yaw);
        assert!(airborne.velocity.x < 20.);
        assert!(airborne.velocity.z < 0.);
    }

    #[test]
    fn wheel_clearance_stays_on_the_lower_deck_under_a_bridge() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3); upper=street(-25,3,-20,3); elevation(upper,0.05);").unwrap(),
            None,
        ).unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        let floor = city
            .surface_at(
                (vehicle.position.truncate() / METERS_PER_UNIT).to_array(),
                0.04,
            )
            .unwrap()
            .height
            * METERS_PER_UNIT;
        vehicle.position.z = floor + RIDE_HEIGHT;
        for _ in 0..120 {
            vehicle.step(&city, Controls::default(), 1. / 120.);
            assert!(vehicle.grounded);
            assert!((vehicle.position.z - floor - RIDE_HEIGHT).abs() < 0.001);
        }
    }

    fn building_map() -> City {
        City::from_program(
            &opencity::parse(
                "road street: carlane; end street(-25,3,-20,3); \
                 building(0.2, -22.4,2.8, -22.35,2.8, -22.35,3.2, -22.4,3.2);",
            )
            .unwrap(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn collision_normal_follows_rotated_facade_in_either_polygon_winding() {
        let anchor = Vec2::new(-2240., 300.);
        let angle = std::f32::consts::FRAC_PI_4;
        let forward = Vec2::new(angle.cos(), angle.sin());
        let left = Vec2::new(-forward.y, forward.x);
        for reverse in [false, true] {
            let mut city = building_map();
            for building in &mut city.landscape {
                if building.kind != opencity::LandscapeKind::Building {
                    continue;
                }
                for point in &mut building.points {
                    let local = Vec2::from_array(*point) * METERS_PER_UNIT - anchor;
                    *point = ((anchor + forward * local.x + left * local.y) / METERS_PER_UNIT)
                        .to_array();
                }
                if reverse {
                    building.points.reverse();
                }
            }
            let mut vehicle = Vehicle::spawn(&city);
            vehicle.yaw = angle + (5_f32 / 30.).atan();
            let initial_yaw = vehicle.yaw;
            let start = anchor - forward * 3.;
            vehicle.position.x = start.x;
            vehicle.position.y = start.y;
            let incoming = forward * 30. + left * 5.;
            vehicle.velocity = incoming.extend(0.);
            vehicle.move_with_building_contacts(&city, 0.1);
            assert_eq!(vehicle.collisions, 1);
            let retention = ((30_f32.powi(2) * BUILDING_REBOUND.powi(2) + 25.) / 925.).sqrt();
            assert!((vehicle.velocity.truncate().dot(forward) + 30. * retention).abs() < 0.02);
            assert!((vehicle.velocity.truncate().dot(left) - 5. * retention).abs() < 0.02);
            assert!(vehicle.yaw > initial_yaw);
            let chassis_forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
            assert!(vehicle.building_contact(&city, chassis_forward).is_none());
        }
    }

    #[test]
    fn building_impact_rebounds_without_adding_energy_or_stopping_slide() {
        let city = building_map();
        let mut vehicle = Vehicle::spawn(&city);
        vehicle.yaw = 0.;
        vehicle.position.x = -2243.;
        vehicle.position.y = 300.;
        vehicle.velocity = Vec3::new(30., 5., 0.5);
        let initial_energy = vehicle.velocity.length_squared();
        let initial_z = vehicle.position.z;
        vehicle.move_with_building_contacts(&city, 0.1);
        assert_eq!(vehicle.collisions, 1);
        let retention = ((30_f32.powi(2) * BUILDING_REBOUND.powi(2) + 25.) / 925.).sqrt();
        assert!((vehicle.velocity.x + 30. * retention).abs() < 0.001);
        assert!((vehicle.velocity.y - 5. * retention).abs() < 0.001);
        assert!(
            (vehicle.velocity.truncate().length_squared()
                - (30_f32.powi(2) * BUILDING_REBOUND.powi(2) + 25.))
                .abs()
                < 0.001
        );
        // Incoming and reflected rays make equal angles with the wall normal.
        assert!((vehicle.velocity.y / -vehicle.velocity.x - 5. / 30.).abs() < 1e-5);
        assert!(vehicle.velocity.length_squared() < initial_energy);
        assert!(vehicle.position.x < -2242.12);
        assert!((vehicle.position.z - initial_z - 0.05).abs() < 0.001);
        assert!(vehicle.building_contact(&city, Vec2::X).is_none());
    }

    #[test]
    fn overlapping_vehicle_separates_and_can_reverse_away() {
        let city = building_map();
        for starting_x in [-2241.8, -2241.2, -2239.] {
            let mut vehicle = Vehicle::spawn(&city);
            vehicle.yaw = 0.;
            vehicle.position.x = starting_x;
            vehicle.position.y = 300.;
            vehicle.move_with_building_contacts(&city, 0.);
            assert!(vehicle.building_contact(&city, Vec2::X).is_none());
            assert_eq!(vehicle.velocity, Vec3::ZERO);
            let separated_x = vehicle.position.x;
            let collisions = vehicle.collisions;
            for _ in 0..240 {
                vehicle.step(
                    &city,
                    Controls {
                        throttle: -1.,
                        ..Default::default()
                    },
                    1. / 120.,
                );
            }
            assert!(vehicle.position.x < separated_x - 2.);
            assert!(vehicle.velocity.x < -1.);
            assert_eq!(vehicle.collisions, collisions);
        }
    }

    #[test]
    fn fast_vehicle_cannot_tunnel_through_building() {
        let city = building_map();
        let mut vehicle = Vehicle::spawn(&city);
        vehicle.yaw = 0.;
        vehicle.position.x = -2250.;
        vehicle.position.y = 300.;
        vehicle.velocity = Vec3::X * 200.;
        vehicle.move_with_building_contacts(&city, 0.2);
        assert_eq!(vehicle.collisions, 1);
        assert!(vehicle.position.x < -2242.12);
        assert!((vehicle.velocity.x + 200. * BUILDING_REBOUND).abs() < 0.001);
        assert!(vehicle.velocity.length() <= 200.);
    }

    #[test]
    fn oblique_impacts_turn_chassis_smoothly_in_forward_and_reverse() {
        let city = building_map();
        for direction in [-1_f32, 1.] {
            for angle in [-0.6_f32, -0.15, 0., 0.15, 0.6] {
                let mut vehicle = Vehicle::spawn(&city);
                // Reverse travel has the same incoming ray but the tail meets
                // the wall. Both must turn consistently with the contact torque.
                vehicle.yaw = angle
                    + if direction < 0. {
                        std::f32::consts::PI
                    } else {
                        0.
                    };
                let initial_yaw = vehicle.yaw;
                vehicle.position.x = -2243.;
                vehicle.position.y = 300.;
                vehicle.velocity = Vec3::new(angle.cos() * 30., angle.sin() * 30., 0.);
                let initial_energy = vehicle.velocity.length_squared();
                vehicle.move_with_building_contacts(&city, 0.1);
                assert_eq!(vehicle.collisions, 1);
                assert!(vehicle.velocity.x < 0.);
                assert!((vehicle.velocity.y / -vehicle.velocity.x - angle.tan()).abs() < 0.001);
                let yaw_delta = vehicle.yaw - initial_yaw;
                if angle == 0. {
                    assert!(yaw_delta.abs() < 1e-5);
                    assert!(vehicle.yaw_rate.abs() < 1e-4);
                } else {
                    assert!(
                        yaw_delta * angle > 0.,
                        "angle={angle}, reverse={direction}, yaw={yaw_delta}"
                    );
                    assert!(vehicle.yaw_rate.abs() > 0.1);
                }
                let outgoing = vehicle.velocity;
                let outgoing_rate = vehicle.yaw_rate;
                let rebound_yaw = vehicle.yaw;
                for _ in 0..120 {
                    let previous_yaw = vehicle.yaw;
                    vehicle.move_with_building_contacts(&city, 1. / 120.);
                    assert!((vehicle.yaw - previous_yaw).abs() <= 2.8 / 120. + 1e-5);
                    assert!(vehicle.position.is_finite() && vehicle.velocity.is_finite());
                    let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
                    assert!(vehicle.building_contact(&city, forward).is_none());
                    assert!(vehicle.velocity.length_squared() <= initial_energy);
                }
                assert!(vehicle.velocity.distance(outgoing) < 0.001);
                assert_eq!(vehicle.collisions, 1);
                // Free motion conserves spin; tire forces recover it in step().
                assert_eq!(vehicle.yaw_rate, outgoing_rate);
                assert!((vehicle.yaw - rebound_yaw - outgoing_rate).abs() < 0.0001);
            }
        }
    }

    #[test]
    fn grazing_impacts_reseparate_rotating_body_without_energy_gain() {
        let city = building_map();
        for speed in [0.1_f32, 3., 30., 200.] {
            for angle in [-1.45_f32, 1.45] {
                let mut vehicle = Vehicle::spawn(&city);
                vehicle.yaw = angle;
                // Begin just inside contact to exercise even almost-resting hits.
                vehicle.position.x = -2240. - 0.92 - 1.2 * angle.cos() + 0.01;
                vehicle.position.y = 300.;
                vehicle.velocity = Vec3::new(angle.cos() * speed, angle.sin() * speed, 0.5);
                let initial_planar_energy = vehicle.velocity.truncate().length_squared();
                let initial_tangent_speed = vehicle.velocity.y.abs();
                for frame in 0..120 {
                    vehicle.move_with_building_contacts(&city, 1. / 120.);
                    if frame == 0 {
                        assert!(vehicle.velocity.x < 0.);
                        assert!(vehicle.velocity.y.abs() > initial_tangent_speed * 0.99);
                        let expected_energy = initial_planar_energy
                            * (angle.sin().powi(2)
                                + BUILDING_REBOUND.powi(2) * angle.cos().powi(2));
                        assert!(
                            (vehicle.velocity.truncate().length_squared() - expected_energy).abs()
                                < initial_planar_energy * 0.0001 + 0.00001
                        );
                        assert!(
                            (vehicle.velocity.y / -vehicle.velocity.x - angle.tan()).abs() < 0.01
                        );
                    }
                    let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
                    assert!(vehicle.building_contact(&city, forward).is_none());
                    assert!(vehicle.position.is_finite() && vehicle.yaw.is_finite());
                    let energy = vehicle.velocity.truncate().length_squared()
                        + CHASSIS_YAW_INERTIA * vehicle.yaw_rate.powi(2);
                    assert!(energy <= initial_planar_energy + 0.001);
                    assert_eq!(vehicle.velocity.z, 0.5);
                }
            }
        }
    }

    #[test]
    fn tire_recovery_after_oblique_rebound_keeps_vehicle_clear_of_wall() {
        let city = building_map();
        for reverse in [false, true] {
            for angle in [-0.4_f32, 0.4] {
                let mut vehicle = Vehicle::spawn(&city);
                vehicle.yaw = angle + if reverse { std::f32::consts::PI } else { 0. };
                vehicle.position.x = -2243.;
                vehicle.position.y = 300.;
                vehicle.velocity = Vec3::new(angle.cos() * 20., angle.sin() * 20., 0.);
                vehicle.step(&city, Controls::default(), 0.1);
                assert_eq!(vehicle.collisions, 1);
                assert!(vehicle.velocity.x < 0.);
                let rebound_x = vehicle.position.x;
                let rebound_energy = vehicle.velocity.truncate().length_squared()
                    + CHASSIS_YAW_INERTIA * vehicle.yaw_rate.powi(2);
                for _ in 0..240 {
                    vehicle.step(&city, Controls::default(), 1. / 120.);
                    assert!(vehicle.velocity.x <= 0.);
                    assert!(vehicle.position.is_finite() && vehicle.velocity.is_finite());
                    let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
                    assert!(vehicle.building_contact(&city, forward).is_none());
                    let energy = vehicle.velocity.truncate().length_squared()
                        + CHASSIS_YAW_INERTIA * vehicle.yaw_rate.powi(2);
                    assert!(energy <= rebound_energy + 0.001);
                }
                assert!(vehicle.position.x < rebound_x - 2.);
                assert_eq!(vehicle.collisions, 1);
                assert!(vehicle.yaw_rate.abs() < 0.001);
            }
        }
    }

    #[test]
    fn throttle_brake_and_fixed_step_remain_finite() {
        let city = map();
        let mut v = Vehicle::spawn(&city);
        for _ in 0..600 {
            v.step(
                &city,
                Controls {
                    throttle: 1.,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        assert!(v.velocity.length() > 10.);
        for _ in 0..360 {
            v.step(
                &city,
                Controls {
                    brake: true,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        assert!(v.velocity.length() < 0.1);
        assert!(v.position.is_finite());
    }
    #[test]
    fn grass_reduces_acceleration_and_increases_coasting_loss() {
        let city=City::from_program(&opencity::parse("road street: carlane; end street(-25,3,-20,3); ground=land(-26,2,-19,2,-19,5,-26,5); elevation(ground,0.02);").unwrap(),None).unwrap();
        let mut asphalt = Vehicle::spawn(&city);
        let mut grass = Vehicle::spawn(&city);
        grass.position.y += 20.;
        for _ in 0..360 {
            let input = Controls {
                throttle: 1.,
                ..Default::default()
            };
            asphalt.step(&city, input, 1. / 120.);
            grass.step(&city, input, 1. / 120.);
        }
        assert_eq!(grass.surface, SurfaceKind::Grass);
        assert!(asphalt.velocity.length() > grass.velocity.length() * 1.2);
        asphalt.velocity = Vec3::X * 10.;
        grass.velocity = Vec3::X * 10.;
        for _ in 0..120 {
            asphalt.step(&city, Controls::default(), 1. / 120.);
            grass.step(&city, Controls::default(), 1. / 120.);
        }
        assert!(asphalt.velocity.x > grass.velocity.x);
    }
    #[test]
    fn uphill_gravity_rolls_unpowered_vehicle_downhill() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end s=street(-25,3,-20,3); grade(s,0,0.5);")
                .unwrap(),
            None,
        )
        .unwrap();
        let mut v = Vehicle::spawn(&city);
        for _ in 0..240 {
            v.step(&city, Controls::default(), 1. / 120.);
        }
        assert!(v.velocity.x < -0.1);
    }
}
