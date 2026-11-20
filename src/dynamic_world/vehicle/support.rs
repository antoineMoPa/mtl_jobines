//! Suspension loads and hard contacts at tires, chassis sides and roof.
use super::*;

impl Vehicle {
    pub(super) fn apply_wheel_support(
        &mut self,
        wheels: &[(Vec3, Option<SurfaceSample>); 4],
        tires: &[Option<TireContact>; 4],
        suspension: super::super::suspension::Suspension,
        dt: f32,
    ) {
        let mut impulse = Vec3::ZERO;
        let mut torque = Vec3::ZERO;
        for ((wheel, sample), tire) in wheels.iter().zip(tires) {
            let (Some(surface), Some(_)) = (sample, tire) else {
                continue;
            };
            let arm = *wheel - self.position;
            let gradient = Vec2::from_array(surface.gradient);
            let velocity = self.velocity_at(arm);
            let compression_speed = velocity.z - gradient.dot(velocity.truncate());
            let compression = surface.height * METERS_PER_UNIT - wheel.z;
            // Each tire carries a quarter of the static load. Missing wheels
            // supply no load or damping, so unsupported ends fall under gravity.
            let load = (9.81 + compression * suspension.heave_spring
                - compression_speed * suspension.heave_damping)
                .max(0.)
                / self.tire_count()
                * dt;
            let force = Vec3::Z * load;
            impulse += force;
            torque += arm.cross(force);
        }
        self.velocity += impulse - Vec3::Z * (9.81 * dt);
        self.angular_momentum += torque;
    }

    fn ground_points(&self) -> ([Vec3; 12], usize) {
        let q = self.orientation();
        let mut points = [Vec3::ZERO; 12];
        let mut count = 0;
        if (q * Vec3::Z).z > 0.3 {
            for wheel in Wheel::ALL {
                points[count] = self.position + q * wheel.contact(self);
                count += 1;
            }
        }
        if count > 0 {
            let axis = q * Vec3::X;
            let side = q * Vec3::Y;
            let up = q * Vec3::Z;
            let wheel_bottom = points[..count]
                .iter()
                .map(|point| point.z - self.position.z)
                .fold(f32::INFINITY, f32::min);
            let body_bottom = -axis.z.abs() * self.length() * 0.5
                - side.z.abs() * self.width() * 0.5
                + up.z * (-self.ride_height() + 0.3);
            if body_bottom >= wheel_bottom {
                return (points, count);
            }
        }
        for x in [-self.length() * 0.5, self.length() * 0.5] {
            for y in [-self.width() * 0.5, self.width() * 0.5] {
                for z in [
                    -self.ride_height() + 0.3,
                    self.height() - self.ride_height(),
                ] {
                    points[count] = self.position + q * Vec3::new(x, y, z);
                    count += 1;
                }
            }
        }
        (points, count)
    }

    pub(super) fn resolve_ground_contacts(
        &mut self,
        city: &City,
        previous_position: Vec3,
        previous_orientation: Quat,
    ) {
        let mut lift = 0_f32;
        let mut impact = 0_f32;
        let mut contacts = [(Vec3::ZERO, Vec3::ZERO, 0.); 12];
        let mut count = 0;
        let (points, point_count) = self.ground_points();
        for &point in &points[..point_count] {
            let local = self.orientation().conjugate() * (point - self.position);
            let previous = previous_position + previous_orientation * local;
            let Some(surface) = city.surface_at(
                (point.truncate() / METERS_PER_UNIT).to_array(),
                (point.z.max(previous.z) + WHEEL_STEP_HEIGHT) / METERS_PER_UNIT,
            ) else {
                continue;
            };
            let depth = surface.height * METERS_PER_UNIT - point.z;
            if depth < -0.001 {
                continue;
            }
            lift = lift.max(depth);
            let gradient = Vec2::from_array(surface.gradient);
            let normal =
                Vec3::new(-gradient.x, -gradient.y, 1.) / (1. + gradient.length_squared()).sqrt();
            contacts[count] = (point - self.position, normal, surface.kind.friction());
            count += 1;
        }
        self.position.z += lift;
        // Sequential contact impulses include rotational point velocity. This
        // catches a rotating corner even when the centre is moving upward.
        for _ in 0..8 {
            for &(arm, normal, friction) in &contacts[..count] {
                let closing = self.velocity_at(arm).dot(normal);
                if closing >= 0. {
                    continue;
                }
                impact = impact.max(-closing);
                let impulse = -closing / self.impulse_response(arm, normal);
                self.apply_impulse(arm, normal * impulse);
                // Tires have their own grip model. Sliding bodywork dissipates
                // tangential energy without giving an overturned car traction.
                if !self.grounded {
                    let velocity = self.velocity_at(arm);
                    let tangent = velocity - normal * velocity.dot(normal);
                    let speed = tangent.length();
                    if speed > 0.0001 {
                        let direction = tangent / speed;
                        let drag =
                            (speed / self.impulse_response(arm, direction)).min(friction * impulse);
                        self.apply_impulse(arm, -direction * drag);
                    }
                }
            }
        }
        if impact > 2. {
            self.collisions += 1;
            self.sound_impact(impact, crate::audio::Material::Hard, false);
        }
    }
}
