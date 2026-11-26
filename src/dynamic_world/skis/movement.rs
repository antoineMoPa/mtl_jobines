//! Fixed-step ski movement in game coordinates (+X forward, +Z up).
use super::super::{pedestrian_gait::Ground, WalkControls};
use glam::{Mat3, Vec3};
use std::f32::consts::{FRAC_PI_2, TAU};

#[derive(Clone, Copy, Default)]
pub(super) struct Input {
    pub shuffle: bool,
    pub backward: bool,
    pub poles: bool,
    pub steer: f32,
    pub brake: f32,
    pub tuck: bool,
}
impl From<WalkControls> for Input {
    fn from(w: WalkControls) -> Self {
        Self {
            shuffle: w.forward > 0.,
            backward: w.forward < 0.,
            poles: w.poles,
            steer: -w.turn.clamp(-1., 1.),
            brake: if w.jump { 1. } else { 0. },
            tuck: w.run,
        }
    }
}
#[derive(Clone)]
pub(super) struct Motion {
    pub position: Vec3,
    pub yaw: f32,
    pub velocity: Vec3,
    pub phase: f32,
    pub contact: bool,
    pub anchors: Option<[Vec3; 2]>,
    attempted: bool,
    pub push: f32,
    pub shuffle_force: f32,
    pub shuffle_blend: f32,
    pub tuck_blend: f32,
    pub turn_edge: f32,
    pub hockey_side: f32,
    pub hockey_blend: f32,
    pub hockey_travel_yaw: f32,
    hockey_target_yaw: f32,
    pub stride_phase: f32,
    pub stride_offsets: [f32; 2],
    pub input: Input,
    pub brake: f32,
    pub ground: Ground,
    pub travel: f32,
}
pub(super) fn basis(yaw: f32, ground: Ground) -> (Vec3, Vec3, Vec3) {
    let [gx, gy] = ground.gradient;
    let raw_n = Vec3::new(-gx, -gy, 1.);
    let n = raw_n / raw_n.length();
    let raw_f = Vec3::new(yaw.cos(), yaw.sin(), gx * yaw.cos() + gy * yaw.sin());
    let f = raw_f / raw_f.length();
    (n, f, n.cross(f))
}
pub(super) fn frame(yaw: f32, ground: Ground) -> glam::Quat {
    let (n, f, r) = basis(yaw, ground);
    glam::Quat::from_mat3(&Mat3::from_cols(f, r, n))
}
impl Motion {
    pub fn new(position: Vec3, yaw: f32, ground: Ground) -> Self {
        Self {
            position,
            yaw,
            ground,
            velocity: Vec3::ZERO,
            phase: 0.6,
            contact: false,
            anchors: None,
            attempted: false,
            push: 0.,
            shuffle_force: 0.,
            shuffle_blend: 0.,
            tuck_blend: 0.,
            turn_edge: 0.,
            hockey_side: 0.,
            hockey_blend: 0.,
            hockey_travel_yaw: 0.,
            hockey_target_yaw: 0.,
            stride_phase: 0.,
            stride_offsets: [0.; 2],
            input: Input::default(),
            brake: 0.,
            travel: 0.,
        }
    }
    pub fn speed(&self) -> f32 {
        self.velocity.length()
    }
    // Fully apply level-ground controls below a 5% grade and fade out by 15%.
    fn level_ground_factor(&self) -> f32 {
        let [gx, gy] = self.ground.gradient;
        ((0.15 - gx.hypot(gy)) / 0.10).clamp(0., 1.)
    }
    pub fn poling_speed_limit(&self) -> f32 {
        3.5 * (1. + self.level_ground_factor())
    }
    pub fn release_poles(&mut self) {
        self.contact = false;
        self.anchors = None;
        self.push = 0.;
    }
    pub fn stop(&mut self) {
        self.velocity = Vec3::ZERO;
        self.release_poles();
        self.shuffle_force = 0.;
    }
    pub fn step(
        &mut self,
        dt: f32,
        support: &impl Fn(Vec3) -> Option<Ground>,
        blocked: &impl Fn(Vec3) -> bool,
    ) {
        let Some(ground) = support(self.position) else {
            self.stop();
            return;
        };
        self.ground = ground;
        let mut i = self.input;
        let (_, facing, _) = basis(self.yaw, ground);
        // Back brakes forward travel, then becomes a slow backward shuffle.
        let reverse = i.backward
            && i.brake < 0.1
            && self.velocity.dot(facing) <= 0.05
            && (self.hockey_side == 0. || self.speed() < 0.15);
        if i.backward && !reverse {
            i.brake = 1.;
        }
        self.brake = i.brake;
        let speed = self.speed();
        let level_ground = self.level_ground_factor();
        let ground_speed_scale = 1. + level_ground;
        self.tuck_blend +=
            ((if i.tuck { 1. } else { 0. }) - self.tuck_blend) * (1. - (-8. * dt).exp());
        if i.brake > 0.2 && i.steer.abs() > 0.15 && speed > 1. && self.hockey_side == 0. {
            self.hockey_side = i.steer.signum();
            self.hockey_travel_yaw = self.velocity.y.atan2(self.velocity.x);
            self.hockey_target_yaw = self.hockey_travel_yaw - self.hockey_side * FRAC_PI_2;
        }
        if i.brake < 0.1 {
            self.hockey_side = 0.;
        }
        let edge_target = if self.hockey_side != 0. {
            0.
        } else {
            i.steer * (0.12 + 0.22 * 0.25) * speed / (speed + 2.)
        };
        self.turn_edge += (edge_target - self.turn_edge) * (1. - (-9. * dt).exp());
        self.hockey_blend += ((if self.hockey_side != 0. { 1. } else { 0. }) - self.hockey_blend)
            .clamp(-5. * dt, 6. * dt);
        if self.hockey_side != 0. {
            let delta = self.hockey_target_yaw - self.yaw;
            self.yaw += delta.sin().atan2(delta.cos()).clamp(-3.8 * dt, 3.8 * dt);
        } else {
            let downhill_turn_rate = 1.25 * (speed / (speed + 2.5)) * (0.3 + 0.7 * 0.25);
            let level_turn_rate = 2.2 / (1. + 0.025 * speed);
            let turn_rate =
                downhill_turn_rate + level_ground * (level_turn_rate - downhill_turn_rate);
            self.yaw -= i.steer * turn_rate * dt;
        }
        let (n, f, r) = basis(self.yaw, ground);
        let tangent = self.velocity - n * self.velocity.dot(n);
        let mut vf = tangent.dot(f);
        let mut vl = tangent.dot(r);
        if self.hockey_side == 0. {
            vf = if vf < 0. { -1. } else { 1. } * vf.hypot(vl);
            vl = 0.;
        }
        let shuffle = i.brake < 0.1 && (i.shuffle || reverse);
        self.shuffle_blend +=
            ((if shuffle { 1. } else { 0. }) - self.shuffle_blend).clamp(-4. * dt, 4. * dt);
        if self.shuffle_blend > 0. {
            self.stride_phase +=
                TAU * (0.9 + 0.25 * (vf.abs() / (4. * ground_speed_scale)).min(1.)) * dt;
        }
        let glide = self.shuffle_blend
            * ((7. * ground_speed_scale - vf.max(0.)) / (3. * ground_speed_scale)).clamp(0., 1.);
        let offset = 0.18 * self.stride_phase.sin() * glide;
        self.stride_offsets = [offset, -offset];
        let shuffle_direction = if reverse { -1. } else { 1. };
        let shuffle_speed_limit = if reverse {
            2.
        } else {
            4.2 * ground_speed_scale
        };
        self.shuffle_force = if shuffle {
            self.shuffle_blend
                * (2.8 * ground_speed_scale)
                * (0.35 + 0.65 * self.stride_phase.sin().abs())
                * (1. - (vf * shuffle_direction).max(0.) / shuffle_speed_limit).max(0.)
                * shuffle_direction
        } else {
            0.
        };
        let poling = !reverse
            && i.brake < 0.1
            && !i.tuck
            && self.tuck_blend < 0.15
            && speed < self.poling_speed_limit()
            && i.poles;
        let old = self.phase;
        self.phase = if poling || self.contact || self.phase < 0.42 {
            (self.phase + dt / 1.6) % 1.
        } else {
            0.6
        };
        if self.phase < old {
            self.attempted = false;
        }
        let plant = self.phase < 0.42 && poling;
        if plant && !self.attempted {
            self.attempted = true;
            let candidates = [1., -1.].map(|side| self.position + f * 0.03 + r * (side * 0.4));
            self.anchors = match candidates.map(|p| support(p).map(|g| g.point + Vec3::Z * 0.003)) {
                [Some(left), Some(right)] => Some([left, right]),
                _ => None,
            };
        }
        self.contact = plant
            && self.anchors.is_some_and(|a| {
                a.iter()
                    .all(|p| p.truncate().distance(self.position.truncate()) < 1.08)
            });
        if !self.contact {
            self.anchors = None;
        }
        let stroke = if self.contact && (0.08..0.38).contains(&self.phase) {
            (std::f32::consts::PI * (self.phase - 0.08) / 0.3).sin()
        } else {
            0.
        };
        self.push = 320. * 0.85 / 80.
            * ground_speed_scale
            * stroke
            * (1. - vf.max(0.) / (3. * ground_speed_scale)).max(0.);
        let gravity = Vec3::NEG_Z * 9.81 + n * (9.81 * n.z);
        let acceleration = gravity + f * (self.push + self.shuffle_force);
        let normal = 9.81 * n.z;
        let static_grip = normal
            * ground_speed_scale
            * (0.018 + 0.4 * 0.25 + 0.6 * i.brake + 0.65 * self.hockey_blend);
        let damp = |v: f32, amount: f32| v.signum() * (v.abs() - amount).max(0.);
        if speed < 0.015 && acceleration.length() <= static_grip {
            vf = 0.;
            vl = 0.;
        } else {
            vf += acceleration.dot(f) * dt;
            vl += acceleration.dot(r) * dt;
            let drag = (0.007 - 0.003 * self.tuck_blend) * speed * speed / ground_speed_scale;
            vf = damp(
                vf,
                (normal * ground_speed_scale * (0.018 + 0.6 * i.brake)
                    + drag * vf.abs() / speed.max(0.01))
                    * dt,
            );
            vl = damp(
                vl,
                (normal
                    * ground_speed_scale
                    * (0.018 + 0.7 * 0.25 + 0.6 * i.brake + 0.45 * self.hockey_blend)
                    + drag * vl.abs() / speed.max(0.01))
                    * dt,
            );
        }
        if self.hockey_side == 0. {
            vl = 0.;
        }
        self.velocity = f * vf + r * vl;
        let next = self.position + self.velocity * dt;
        if let Some(g) = support(next)
            .filter(|g| !blocked(g.point + Vec3::Z * super::super::player::HALF_HEIGHT))
        {
            self.travel += self.position.distance(g.point);
            self.position = g.point;
            self.ground = g;
        } else {
            self.stop();
        }
    }
}
