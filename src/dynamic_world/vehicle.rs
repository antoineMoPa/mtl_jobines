use glam::{Quat, Vec2, Vec3};
#[cfg(test)]
use opencity::runtime::Mesh;
use opencity::{
    runtime::{City, SurfaceKind, SurfaceSample},
    RoadDirection, RoadElementKind,
};
pub const METERS_PER_UNIT: f32 = 100.;
const RIDE_HEIGHT: f32 = 0.65;
const WHEELBASE: f32 = 2.6;
const HALF_TRACK: f32 = 0.94;
const WHEEL_STEP_HEIGHT: f32 = 0.4;
// Arcade tires recover from slides quickly while preserving each surface's grip
// ratio and the shared acceleration/braking budget (an elliptical tire envelope).
const LATERAL_GRIP_SCALE: f32 = 1.85;
// Retain the parking-speed steering angle while tempering inputs at road speeds.
const STEERING_SPEED_TAPER: f32 = 0.025;
// Arcade service brakes shorten stops without changing powered tire grip.
const SERVICE_BRAKE_GRIP_SCALE: f32 = 1.8;
// Forward arcade performance; tire grip still limits launches and cornering.
const FORWARD_MOTOR_ACCELERATION: f32 = 10.;
const FORWARD_MOTOR_CUTOFF: f32 = 72.;
const AIR_DRAG: f32 = 0.0012;
const BUILDING_REBOUND: f32 = 0.30;
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
    pub(crate) sound_controls: Controls,
    pub(crate) sound_running: bool,
    pub(super) sound_impacts: [crate::audio::Impact; 3],
    pub(super) sound_impact_count: usize,
    pub(super) sound_cooldown: f32,
    pub(super) definition: Option<usize>,
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
    /// Sprung-body pitch and roll relative to the wheel support plane.
    pub(super) body_lean: Vec2,
    pub(super) body_lean_rate: Vec2,
}
impl Vehicle {
    pub(super) fn sound_impact(
        &mut self,
        speed: f32,
        material: crate::audio::Material,
        broken: bool,
    ) {
        if speed < 0.8 || (!broken && self.sound_cooldown > 0.) {
            return;
        }
        self.sound_cooldown = 0.12;
        let slot = self.sound_impact_count.min(2);
        self.sound_impacts[slot] = crate::audio::Impact {
            position: self.position,
            speed,
            material,
            broken,
            ..Default::default()
        };
        self.sound_impact_count = (self.sound_impact_count + 1).min(3);
    }
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
            sound_controls: Controls::default(),
            sound_running: false,
            sound_impacts: [crate::audio::Impact::default(); 3],
            sound_impact_count: 0,
            sound_cooldown: 0.,
            definition: None,
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
            body_lean: Vec2::ZERO,
            body_lean_rate: Vec2::ZERO,
        }
    }
    pub(super) fn definition(&self) -> Option<&'static super::catalog::Definition> {
        self.definition.map(|id| &super::catalog::definitions()[id])
    }
    #[cfg(test)]
    pub fn name(&self) -> &str {
        self.definition()
            .map_or("Prototype", |d| d.config.name.as_str())
    }
    pub fn length(&self) -> f32 {
        self.definition()
            .map_or(4.2, |d| d.config.dimensions.length)
    }
    pub fn width(&self) -> f32 {
        self.definition().map_or(1.8, |d| d.config.dimensions.width)
    }
    pub fn height(&self) -> f32 {
        self.definition()
            .map_or(1.65, |d| d.config.dimensions.height)
    }
    pub fn mass(&self) -> f32 {
        self.definition().map_or(1250., |d| d.config.physics.mass)
    }
    pub(super) fn ride_height(&self) -> f32 {
        self.definition()
            .map_or(RIDE_HEIGHT, |d| d.config.physics.height_offset)
    }
    fn wheelbase(&self) -> f32 {
        self.definition()
            .map_or(WHEELBASE, |d| d.config.physics.wheelbase)
    }
    fn half_track(&self) -> f32 {
        self.definition()
            .map_or(HALF_TRACK, |d| d.config.physics.track * 0.5)
    }
    fn inertia(&self) -> f32 {
        self.definition().map_or(CHASSIS_YAW_INERTIA, |_| {
            (self.length().powi(2) + self.width().powi(2)) / 12.
        })
    }
    pub(super) fn entry_distance(&self, point: Vec3) -> f32 {
        let delta = point - self.position;
        let f = Vec2::new(self.yaw.cos(), self.yaw.sin());
        let l = Vec2::new(-f.y, f.x);
        let x = (delta.truncate().dot(f).abs() - self.length() * 0.5).max(0.);
        let y = (delta.truncate().dot(l).abs() - self.width() * 0.5).max(0.);
        Vec3::new(x, y, delta.z).length()
    }
    pub fn orientation(&self) -> Quat {
        Quat::from_rotation_z(self.yaw)
            * Quat::from_rotation_y(-self.pitch)
            * Quat::from_rotation_x(self.roll)
    }
    pub fn step(&mut self, city: &City, input: Controls, dt: f32) {
        self.step_with_buildings(city, input, dt, None);
    }
    pub(super) fn step_with_buildings(
        &mut self,
        city: &City,
        input: Controls,
        dt: f32,
        buildings: Option<&[usize]>,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        // Keep the suspension and tire impulses stable when a caller uses larger frames.
        let steps = (dt / (1. / 120.)).ceil().max(1.) as usize;
        for _ in 0..steps {
            self.step_substep(city, input, dt / steps as f32, buildings);
        }
    }

    fn step_substep(&mut self, city: &City, input: Controls, dt: f32, buildings: Option<&[usize]>) {
        let suspension = super::suspension::profile(self);
        let previous_velocity = self.velocity.truncate();
        let f = Vec2::new(self.yaw.cos(), self.yaw.sin());
        let left = Vec2::new(-f.y, f.x);
        let speed = self.velocity.truncate().dot(f);
        let target_steering = input.steering.clamp(-1., 1.)
            * self
                .definition()
                .map_or(0.52, |d| d.config.physics.max_steer_angle)
            / (1. + speed.abs() * STEERING_SPEED_TAPER);
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
                        grip: s.kind.friction()
                            * self
                                .definition()
                                .map_or(1., |d| d.config.physics.friction / 0.9)
                            * gravity
                            * 0.25,
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
            let target = support + self.ride_height();
            self.velocity.z += ((target - self.position.z) * suspension.heave_spring
                - self.velocity.z * suspension.heave_damping)
                * dt;
        } else {
            self.velocity.z -= 9.81 * dt;
        }
        // Quadratic air drag acts on the whole velocity, including while airborne.
        v /= 1. + AIR_DRAG * v.length() * dt;
        self.velocity.x = v.x;
        self.velocity.y = v.y;
        self.move_with_building_candidates(city, dt, buildings);
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
        // Actual acceleration includes braking, tire slip and impacts. Heading
        // changes alone must not lean the body when no contact supplies force.
        let acceleration = (self.velocity.truncate() - previous_velocity) / dt;
        let target = if self.grounded {
            Vec2::new(
                (acceleration.dot(f) * suspension.lean_response.x)
                    .clamp(-suspension.lean_limit.x, suspension.lean_limit.x),
                (acceleration.dot(left) * suspension.lean_response.y)
                    .clamp(-suspension.lean_limit.y, suspension.lean_limit.y),
            )
        } else {
            Vec2::ZERO
        };
        // Catalog bodies settle quickly; long bodies use smaller pitch and roll angles.
        self.body_lean_rate += ((target - self.body_lean) * suspension.lean_spring
            - self.body_lean_rate * suspension.lean_damping)
            * dt;
        self.body_lean += self.body_lean_rate * dt;
        self.wheel_angle += speed
            / self
                .definition()
                .map_or(0.32, |d| d.config.physics.wheel_radius)
            * dt;
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
            self.definition().map_or(FORWARD_MOTOR_ACCELERATION, |d| {
                d.config.physics.acceleration
            }) * throttle
                * (1.
                    - (speed.max(0.)
                        / self
                            .definition()
                            .map_or(FORWARD_MOTOR_CUTOFF, |d| d.config.physics.max_speed))
                    .powi(2))
                .max(0.)
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
            let response = 4. * (1. + tire.arm.length_squared() / self.inertia());
            let stop_impulse = longitudinal_speed.abs() / response;
            let brake_scale = if input.brake || input.handbrake {
                SERVICE_BRAKE_GRIP_SCALE
            } else {
                1.
            };
            // Space combines service braking with reduced rear lateral grip.
            let braking = input.brake || input.handbrake;
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
        self.yaw_rate += torque / self.inertia();

        impulse = Vec2::ZERO;
        torque = 0.;
        for (index, tire) in tires.iter().enumerate() {
            let Some(tire) = tire else { continue };
            let lateral = Vec2::new(-tire.forward.y, tire.forward.x);
            let point_velocity = *velocity + Vec2::new(-tire.arm.y, tire.arm.x) * self.yaw_rate;
            let slip = point_velocity.dot(lateral);
            let response = 4. * (1. + tire.arm.length_squared() / self.inertia());
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
        self.yaw_rate += torque / self.inertia();
    }

    fn wheel_contacts(&self, city: &City) -> [(Vec3, Option<SurfaceSample>); 4] {
        let orientation = self.orientation();
        [
            [-self.wheelbase() * 0.5, -self.half_track()],
            [-self.wheelbase() * 0.5, self.half_track()],
            [self.wheelbase() * 0.5, -self.half_track()],
            [self.wheelbase() * 0.5, self.half_track()],
        ]
        .map(|[x, y]| {
            let wheel = self.position + orientation * Vec3::new(x, y, -self.ride_height());
            let surface = city.surface_at(
                (wheel.truncate() / METERS_PER_UNIT).to_array(),
                (wheel.z + WHEEL_STEP_HEIGHT) / METERS_PER_UNIT,
            );
            (wheel, surface)
        })
    }
    #[cfg(test)]
    fn move_with_building_contacts(&mut self, city: &City, dt: f32) {
        self.move_with_building_candidates(city, dt, None);
    }
    #[cfg(test)]
    fn building_contact(&self, city: &City, forward: Vec2) -> Option<(Vec2, f32, Vec2)> {
        self.building_contact_candidates(city, forward, None)
    }
    fn move_with_building_candidates(&mut self, city: &City, dt: f32, buildings: Option<&[usize]>) {
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
                let Some((normal, depth, arm)) =
                    self.building_contact_candidates(city, forward, buildings)
                else {
                    break;
                };
                collided = true;
                let separation = normal * (depth + 0.001);
                self.position.x += separation.x;
                self.position.y += separation.y;
                let inward_speed = self.velocity.truncate().dot(normal);
                self.sound_impact(-inward_speed, crate::audio::Material::Hard, false);
                if inward_speed < 0. {
                    let incoming = self.velocity.truncate();
                    // Preserve the reflected ray's angle, while losing only
                    // energy associated with the normal impact. Grazing scrapes
                    // retain almost all speed; head-on hits retain 30%.
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
                    let yaw_impulse = arm.perp_dot(normal_impulse) / self.inertia();
                    let lost_energy =
                        (incoming.length_squared() - outgoing.length_squared()).max(0.);
                    let spin_limit = (self.yaw_rate * self.yaw_rate + lost_energy / self.inertia())
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

    fn building_contact_candidates(
        &self,
        city: &City,
        forward: Vec2,
        buildings: Option<&[usize]>,
    ) -> Option<(Vec2, f32, Vec2)> {
        let mut deepest = None;
        for offset in [-self.length() * 0.28, 0., self.length() * 0.28] {
            let center = self.position.truncate() + forward * offset;
            if let Some((normal, depth)) = city.road_barrier_contact(
                (center / METERS_PER_UNIT).to_array(),
                (self.width() * 0.5 + 0.02) / METERS_PER_UNIT,
                (self.position.z - 0.35) / METERS_PER_UNIT,
                (self.position.z + 0.45) / METERS_PER_UNIT,
            ) {
                let depth = depth * METERS_PER_UNIT;
                if deepest.is_none_or(|(_, previous, _)| depth > previous) {
                    deepest = Some((Vec2::from_array(normal), depth, forward * offset));
                }
            }
        }
        for index in 0..buildings.map_or(city.landscape.len(), |ids| ids.len()) {
            let building = &city.landscape[buildings.map_or(index, |ids| ids[index])];
            if building.kind != opencity::LandscapeKind::Building
                || building.elevation * METERS_PER_UNIT >= self.position.z + 0.45
                || (building.elevation + building.height.unwrap_or(0.)) * METERS_PER_UNIT
                    <= self.position.z - 0.35
            {
                continue;
            }
            for offset in [-self.length() * 0.28, 0., self.length() * 0.28] {
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
                    self.width() * 0.5 + 0.02 + distance
                } else {
                    self.width() * 0.5 + 0.02 - distance
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

    #[cfg(test)]
    pub fn append_mesh(&self, output: &mut Mesh) {
        if let Some(definition) = self.definition() {
            definition.append_mesh(self, output);
        } else {
            output.vertices.extend(self.mesh().vertices);
        }
    }
    #[cfg(test)]
    pub fn mesh(&self) -> Mesh {
        if let Some(definition) = self.definition() {
            return definition.mesh(self);
        }
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
        // The sprung chassis moves independently of the unsprung wheels, so
        // acceleration lean cannot lift tires off the road or alter tire grip.
        let lean =
            Quat::from_rotation_y(-self.body_lean.x) * Quat::from_rotation_x(self.body_lean.y);
        for v in &mut m.vertices {
            v.position = (lean * Vec3::from_array(v.position)).to_array();
            v.normal = (lean * Vec3::from_array(v.normal)).to_array();
        }
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
mod performance_tests;
#[cfg(test)]
mod tests;
