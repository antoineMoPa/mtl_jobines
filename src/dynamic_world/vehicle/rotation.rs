//! Rigid chassis rotation. Momentum stays in world axes during unsupported flight.
use super::*;

impl Vehicle {
    fn principal_inertia(&self) -> Vec3 {
        let offset = self.height() * 0.5 - self.ride_height();
        Vec3::new(
            (self.width().powi(2) + self.height().powi(2)) / 12. + offset.powi(2),
            (self.length().powi(2) + self.height().powi(2)) / 12. + offset.powi(2),
            self.inertia(),
        )
    }

    pub(in crate::dynamic_world) fn inverse_inertia(&self, torque: Vec3) -> Vec3 {
        if self.pitch == 0. && self.roll == 0. && torque.truncate() == Vec2::ZERO {
            return Vec3::Z * (torque.z / self.inertia());
        }
        let q = self.orientation();
        q * ((q.conjugate() * torque) / self.principal_inertia())
    }

    /// World angular velocity in radians per second, from the current inertia tensor.
    pub fn angular_velocity(&self) -> Vec3 {
        self.inverse_inertia(self.angular_momentum)
    }

    pub(in crate::dynamic_world) fn velocity_at(&self, arm: Vec3) -> Vec3 {
        self.velocity + self.angular_velocity().cross(arm)
    }

    /// Change in contact speed for a unit impulse divided by chassis mass.
    pub(in crate::dynamic_world) fn impulse_response(&self, arm: Vec3, direction: Vec3) -> f32 {
        1. + self
            .inverse_inertia(arm.cross(direction))
            .cross(arm)
            .dot(direction)
    }

    pub(in crate::dynamic_world) fn apply_impulse(&mut self, arm: Vec3, impulse: Vec3) {
        self.velocity += impulse;
        self.angular_momentum += arm.cross(impulse);
    }

    pub(super) fn advance_orientation(&mut self, dt: f32) {
        if self.angular_momentum == Vec3::ZERO || dt == 0. {
            return;
        }
        // Preserve unwrapped headings and exact planar steering integration.
        if self.pitch == 0. && self.roll == 0. && self.angular_momentum.truncate() == Vec2::ZERO {
            self.yaw += self.angular_momentum.z / self.inertia() * dt;
            return;
        }
        let q = self.orientation();
        let omega = self.angular_velocity();
        let midpoint = Quat::from_scaled_axis(omega * (dt * 0.5)) * q;
        let middle_omega =
            midpoint * ((midpoint.conjugate() * self.angular_momentum) / self.principal_inertia());
        let next = Quat::from_scaled_axis(middle_omega * dt) * q;
        let forward = next * Vec3::X;
        let mut yaw = self.yaw;
        // Heading is undefined at vertical. Keep the previous heading there,
        // and choose the nearest Euler branch elsewhere to avoid camera flips.
        if forward.truncate().length_squared() > 0.000001 {
            yaw = forward.y.atan2(forward.x);
            let delta = yaw - self.yaw;
            if delta.sin().atan2(delta.cos()).abs() > std::f32::consts::FRAC_PI_2 {
                yaw += std::f32::consts::PI;
            }
        }
        let heading = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        let pitch = forward.z.atan2(forward.dot(heading));
        let twist = Quat::from_rotation_y(pitch) * Quat::from_rotation_z(-yaw) * next;
        let roll = 2. * twist.x.atan2(twist.w);
        let delta = yaw - self.yaw;
        self.yaw += delta.sin().atan2(delta.cos());
        let delta = pitch - self.pitch;
        self.pitch += delta.sin().atan2(delta.cos());
        let delta = roll - self.roll;
        self.roll += delta.sin().atan2(delta.cos());
    }
}
