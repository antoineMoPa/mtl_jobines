//! World-anchored footholds and the approved continuous full-body walking gait.
//! Simulation is in metres; motion space uses the GLB's +Y up and +Z forward.
use super::pedestrian_catalog::{Definition, JOINT_LIMIT};
use glam::{Mat4, Quat, Vec3};
use std::f32::consts::PI;

pub(super) fn game_to_motion(p: Vec3) -> Vec3 {
    Vec3::new(p.y, p.z, p.x)
}
pub(super) fn motion_to_game(p: Vec3) -> Vec3 {
    Vec3::new(p.z, p.x, p.y)
}
fn angle_delta(a: f32, b: f32) -> f32 {
    (b - a + PI).rem_euclid(2. * PI) - PI
}
fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
#[derive(Clone, Copy)]
pub(crate) struct Ground {
    pub point: Vec3,
    pub gradient: [f32; 2],
}
impl Ground {
    fn tilt(self) -> Quat {
        let normal = Vec3::new(-self.gradient[1], 1., -self.gradient[0]);
        Quat::from_rotation_arc(Vec3::Y, normal / normal.length())
    }
}
#[derive(Clone, Copy)]
struct Foot {
    position: Vec3,
    start: Vec3,
    target: Vec3,
    yaw: f32,
    start_yaw: f32,
    target_yaw: f32,
    tilt: Quat,
    start_tilt: Quat,
    target_tilt: Quat,
}
struct Rig {
    legs: [[usize; 4]; 2],
    hips: usize,
    ankles: [Vec3; 2],
    scale: f32,
}
impl Rig {
    fn new(def: &Definition) -> Self {
        let worlds = def.rest_worlds();
        let legs = ["L", "R"].map(|side| {
            ["Thigh", "Shin", "Foot", "Toe"].map(|bone| def.node(&format!("{bone}_{side}")))
        });
        let ankles = legs.map(|leg| worlds[leg[2]].transform_point3(Vec3::ZERO));
        let hips = def.node("Hips");
        let scale = worlds[hips].w_axis.y / 0.8858;
        Self {
            legs,
            hips,
            ankles,
            scale,
        }
    }
}
pub(crate) struct Gait {
    rig: Rig,
    feet: [Foot; 2],
    position: Vec3,
    yaw: f32,
    speed: f32,
    acceleration: f32,
    turn_rate: f32,
    phase: f32,
    steps: usize,
    swing: usize,
    duration: f32,
    planned: bool,
    movement: f32,
    idle_time: f32,
    pub palette: [Mat4; JOINT_LIMIT],
    pub reach_error: f32,
}
impl Gait {
    pub fn new(
        def: &Definition,
        position: Vec3,
        yaw: f32,
        seed: u32,
        sample: impl Fn(Vec3) -> Option<Ground>,
    ) -> Self {
        let rig = Rig::new(def);
        let root = game_to_motion(position);
        let feet = std::array::from_fn(|i| {
            let ankle = rig.ankles[i];
            let mut p = root + Quat::from_rotation_y(yaw) * Vec3::new(ankle.x * 0.50, 0., ankle.z);
            let ground = sample(motion_to_game(p));
            let tilt = ground.map_or(Quat::IDENTITY, Ground::tilt);
            p.y = ground.map_or(root.y, |s| s.point.z) + ankle.y;
            Foot {
                position: p,
                start: p,
                target: p,
                yaw,
                start_yaw: yaw,
                target_yaw: yaw,
                tilt,
                start_tilt: tilt,
                target_tilt: tilt,
            }
        });
        let mut gait = Self {
            rig,
            feet,
            position: root,
            yaw,
            speed: 0.,
            acceleration: 0.,
            turn_rate: 0.,
            phase: 0.,
            steps: (seed % 2) as usize,
            swing: (seed % 2) as usize,
            duration: 0.5,
            planned: false,
            movement: 0.,
            idle_time: (seed % 19) as f32 * 0.17,
            palette: [Mat4::IDENTITY; JOINT_LIMIT],
            reach_error: 0.,
        };
        gait.update_pose(def);
        gait
    }
    /// Body travel is bounded by available support; a blocked foothold cannot leave a leg behind.
    pub fn permits(&self, position: Vec3) -> bool {
        let p = game_to_motion(position);
        self.feet.iter().enumerate().all(|(i, f)| {
            if self.planned && i == self.swing {
                return true;
            }
            let delta = p - f.position;
            Vec3::new(delta.x, 0., delta.z).length() < 0.61 * self.rig.scale
        })
    }
    pub fn advance(
        &mut self,
        def: &Definition,
        position: Vec3,
        yaw: f32,
        desired_speed: f32,
        aim: Vec3,
        dt: f32,
        sample: impl Fn(Vec3) -> Option<Ground>,
    ) {
        if dt <= 0. {
            return;
        }
        let to = game_to_motion(position);
        let from = self.position;
        let old_yaw = self.yaw;
        let speed = to.distance(from) / dt;
        let acceleration = ((speed - self.speed) / dt).clamp(-2., 1.2);
        let turn_rate = angle_delta(self.yaw, yaw) / dt;
        let subdivisions = (dt / (1. / 120.)).ceil().max(1.) as usize;
        for i in 0..subdivisions {
            let h = dt / subdivisions as f32;
            self.position = from.lerp(to, (i + 1) as f32 / subdivisions as f32);
            self.yaw = old_yaw + angle_delta(old_yaw, yaw) * (i + 1) as f32 / subdivisions as f32;
            self.speed = speed;
            self.acceleration = acceleration;
            self.turn_rate = turn_rate;
            self.idle_time += h;
            let amount = (speed / desired_speed.max(0.35)).min(1.);
            self.movement += (amount - self.movement).clamp(-h * 3., h * 3.);
            self.tick(def, desired_speed, game_to_motion(aim), h, &sample);
        }
        self.update_pose(def);
    }
    fn tick(
        &mut self,
        _def: &Definition,
        desired_speed: f32,
        aim: Vec3,
        dt: f32,
        sample: &impl Fn(Vec3) -> Option<Ground>,
    ) {
        if !self.planned {
            let unsettled = self.feet.iter().enumerate().any(|(i, f)| {
                let ankle = self.rig.ankles[i];
                let neutral = self.position
                    + Quat::from_rotation_y(self.yaw) * Vec3::new(ankle.x * 0.50, 0., ankle.z);
                let d = neutral - f.position;
                Vec3::new(d.x, 0., d.z).length() > 0.09 || angle_delta(f.yaw, self.yaw).abs() > 0.18
            });
            if self.speed < 0.015 && !unsettled {
                return;
            }
            self.duration = (0.55 * self.rig.scale / desired_speed.max(0.35)).clamp(0.34, 0.75);
            let mut p = self.position;
            let mut yaw = self.yaw;
            let mut speed = self.speed;
            for _ in 0..12 {
                let h = self.duration / 12.;
                speed += (desired_speed - speed).clamp(-2. * h, 1.2 * h);
                let delta = aim - p;
                if delta.x * delta.x + delta.z * delta.z > 0.0025 && speed > 0.015 {
                    yaw += angle_delta(yaw, delta.x.atan2(delta.z)).clamp(-2.2 * h, 2.2 * h);
                }
                p += Vec3::new(yaw.sin(), 0., yaw.cos()) * speed * h;
            }
            let ankle = self.rig.ankles[self.swing];
            let target = p + Quat::from_rotation_y(yaw)
                * Vec3::new(ankle.x * 0.50, 0., ankle.z + speed * self.duration * 0.45);
            let foot = &mut self.feet[self.swing];
            let landing = [1., 0.8, 0.6, 0.4, 0.2, 0.]
                .into_iter()
                .find_map(|fraction| {
                    let candidate = foot.position.lerp(target, fraction);
                    sample(motion_to_game(candidate - Vec3::Y * ankle.y))
                        .map(|g| (g, game_to_motion(g.point) + Vec3::Y * ankle.y))
                });
            let Some((ground, target)) = landing else {
                return;
            };
            foot.start = foot.position;
            foot.target = target;
            foot.start_yaw = foot.yaw;
            foot.target_yaw = yaw;
            foot.start_tilt = foot.tilt;
            foot.target_tilt = ground.tilt();
            self.planned = true;
        }
        self.phase = (self.phase + dt / self.duration).min(1.);
        let flight = ((self.phase - 0.08) / 0.80).clamp(0., 1.);
        let foot = &mut self.feet[self.swing];
        foot.position = foot.start.lerp(foot.target, ease(flight));
        foot.position.y += 0.11 * self.rig.scale * (PI * flight).sin().max(0.).powf(1.4);
        foot.yaw = foot.start_yaw + angle_delta(foot.start_yaw, foot.target_yaw) * ease(flight);
        foot.tilt = foot.start_tilt.slerp(foot.target_tilt, ease(flight));
        if self.phase >= 1. {
            self.steps += 1;
            self.swing = 1 - self.swing;
            self.phase = 0.;
            self.planned = false;
        }
    }
    fn update_pose(&mut self, model: &Definition) {
        let mut locals = model.idle_locals(self.idle_time);
        let movement = self.movement;
        let forward = Vec3::new(self.yaw.sin(), 0., self.yaw.cos());
        let right = Vec3::new(self.yaw.cos(), 0., -self.yaw.sin());
        // One continuous gait phase coordinates the whole body across both footfalls.
        let cycle = (self.steps as f32 + self.phase) * PI;
        let weight_shift = -cycle.sin() * 0.8 * movement;
        let pelvis_roll = weight_shift * 0.035;
        locals[self.rig.hips].translation.x += weight_shift * 0.040;
        locals[self.rig.hips].translation.y -= 0.020 + (2. * cycle).cos() * 0.012 * movement;
        locals[self.rig.hips].rotation *=
            Quat::from_rotation_y(cycle.cos() * 0.075 * 0.8 * movement)
                * Quat::from_rotation_z(pelvis_roll);
        let lean = (self.turn_rate * self.speed * 0.035).clamp(-0.09, 0.09) * 0.8;
        locals[model.node("Spine")].rotation *= Quat::from_rotation_z(-lean - pelvis_roll * 0.65)
            * Quat::from_rotation_x((self.acceleration * 0.025 + self.speed * 0.025) * 0.8);
        locals[model.node("Chest")].rotation *=
            Quat::from_rotation_y(-cycle.cos() * 0.14 * 0.8 * movement);
        locals[model.node("Neck")].rotation *=
            Quat::from_rotation_z(lean * 0.5 - pelvis_roll * 0.2);
        for (i, side) in ["L", "R"].iter().enumerate() {
            let sign = if i == 0 { 1. } else { -1. };
            let arm_phase = cycle - 0.16 + i as f32 * PI;
            let swing = -arm_phase.cos() * 35f32.to_radians() * movement;
            locals[model.node(&format!("UpperArm_{side}"))].rotation *=
                Quat::from_rotation_x(swing)
                    * Quat::from_rotation_z(sign * (0.045 + lean.abs() * 0.5) * 0.8);
            locals[model.node(&format!("Forearm_{side}"))].rotation *=
                Quat::from_rotation_x((-0.18 + (arm_phase - 0.30).cos() * 0.13) * movement);
            locals[model.node(&format!("Hand_{side}"))].rotation *=
                Quat::from_rotation_x(-(arm_phase - 0.50).cos() * 0.10 * movement * 0.8);
        }
        let contact = [self.foot_contact(model, 0), self.foot_contact(model, 1)];
        let root = Mat4::from_rotation_translation(Quat::from_rotation_y(self.yaw), self.position);
        let mut overrides = vec![None; locals.len()];
        let initial = model.worlds(&locals, root, &overrides);
        // Lower the pelvis to keep both ankle constraints reachable, including slopes.
        let mut lower: f32 = 0.;
        for (i, leg) in self.rig.legs.iter().enumerate() {
            let hip = initial[leg[0]].transform_point3(Vec3::ZERO);
            let length = (model.rest[leg[1]].translation.length()
                + model.rest[leg[2]].translation.length())
                * 0.995;
            let delta = contact[i].0 - hip;
            let allowed_y = (length * length - delta.x * delta.x - delta.z * delta.z)
                .max(0.01)
                .sqrt();
            let constraint = hip.y - contact[i].0.y - allowed_y;
            let difference = lower - constraint;
            lower = 0.5 * (lower + constraint + (difference * difference + 0.0001).sqrt());
        }
        locals[self.rig.hips].translation.y -= lower.max(0.).min(0.3);
        let mut reach_error: f32 = 0.;
        for (i, leg) in self.rig.legs.iter().enumerate() {
            let worlds = model.worlds(&locals, root, &overrides);
            let hip = worlds[leg[0]].transform_point3(Vec3::ZERO);
            let target = contact[i].0;
            let pole = forward + right * if i == 0 { -0.04 } else { 0.04 };
            let a = model.rest[leg[1]].translation.length();
            let b = model.rest[leg[2]].translation.length();
            let (knee, ankle) = solve_leg(hip, target, pole, a, b);
            reach_error = reach_error.max(ankle.distance(target));
            for (bone, child, from, to) in
                [(leg[0], leg[1], hip, knee), (leg[1], leg[2], knee, ankle)]
            {
                let rest_direction =
                    model.rest[child].translation / model.rest[child].translation.length();
                let direction = (to - from) / to.distance(from).max(0.00001);
                let rotation = Quat::from_rotation_arc(rest_direction, direction);
                // The knee bend selects the leg plane; retain facing-axis twist.
                let local_direction = Quat::from_rotation_y(-self.yaw) * direction;
                let rotation = if local_direction.y < -0.05 {
                    Quat::from_rotation_y(self.yaw)
                        * Quat::from_rotation_arc(rest_direction, local_direction)
                } else {
                    rotation
                };
                overrides[bone] = Some(Mat4::from_rotation_translation(rotation, from));
            }
            overrides[leg[2]] = Some(Mat4::from_rotation_translation(
                contact[i].1 * model.rest[leg[2]].rotation,
                ankle,
            ));
        }
        let worlds = model.worlds(&locals, root, &overrides);
        let mut skin = [Mat4::IDENTITY; 32];
        let inverse_root = root.inverse();
        for (i, &node) in model.joints.iter().enumerate() {
            skin[i] = inverse_root * worlds[node] * model.inverse_bind[i];
        }
        self.palette = skin;
        self.reach_error = reach_error;
    }
    fn foot_contact(&self, _model: &Definition, side: usize) -> (Vec3, Quat) {
        let foot = &self.feet[side];
        let active = self.speed > 0.02 || self.planned;
        let (pitch, heel) = if !active {
            (0., 0.)
        } else if side == self.swing {
            if self.phase < 0.08 {
                (0.18 + 0.06 * ease(self.phase / 0.08), 0.)
            } else if self.phase < 0.88 {
                let flight = (self.phase - 0.08) / 0.80;
                (
                    0.24 * (1. - ease(flight)) - 0.12 * ease(flight),
                    ease(flight),
                )
            } else {
                (-0.12 * (1. - ease((self.phase - 0.88) / 0.12)), 1.)
            }
        } else {
            (0.18 * ease((self.phase - 0.48) / 0.52), 0.)
        };
        let ankle_height = self.rig.ankles[side].y;
        let pivot = Vec3::new(0., -ankle_height, 0.14 * (1. - heel) - 0.065 * heel);
        let ground = foot.tilt * Quat::from_rotation_y(foot.yaw);
        let roll = Quat::from_rotation_x(pitch);
        (
            foot.position + ground * (pivot - roll * pivot),
            ground * roll,
        )
    }
}
fn solve_leg(hip: Vec3, target: Vec3, pole: Vec3, a: f32, b: f32) -> (Vec3, Vec3) {
    let delta = target - hip;
    let distance = delta.length().max(0.00001);
    let direction = delta / distance;
    let reach = distance.clamp((a - b).abs() + 0.001, a + b - 0.001);
    let along = (a * a - b * b + reach * reach) / (2. * reach);
    let radius = (a * a - along * along).max(0.).sqrt();
    let perpendicular = pole - direction * pole.dot(direction);
    let bend = perpendicular / perpendicular.length().max(0.00001);
    (
        hip + direction * along + bend * radius,
        hip + direction * reach,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::pedestrian_catalog;
    #[test]
    fn approved_gait_keeps_stance_targets_and_has_continuous_balance_on_all_six_rigs() {
        for (index, def) in pedestrian_catalog::definitions().iter().enumerate() {
            let sample = |p: Vec3| {
                Some(Ground {
                    point: Vec3::new(p.x, p.y, p.x * 0.04),
                    gradient: [0.04, 0.],
                })
            };
            let mut position = Vec3::ZERO;
            let mut gait = Gait::new(def, position, 0., index as u32, sample);
            let mut wrist_range = (f32::INFINITY, f32::NEG_INFINITY);
            let mut sway_range = (f32::INFINITY, f32::NEG_INFINITY);
            let mut max_reach: f32 = 0.;
            let mut speed: f32 = 0.;
            for frame in 0..1200 {
                let desired = if (480..720).contains(&frame) {
                    0.
                } else {
                    1.05
                };
                speed += (desired - speed).clamp(-2. / 120., 1.2 / 120.);
                position.x += speed / 120.;
                position.z = position.x * 0.04;
                let stance = gait.feet[1 - gait.swing].position;
                let swing = gait.swing;
                gait.advance(
                    def,
                    position,
                    0.,
                    desired,
                    position + Vec3::X,
                    1. / 120.,
                    sample,
                );
                assert_eq!(gait.feet[1 - swing].position, stance, "stance target slid");
                assert!(gait.palette.iter().all(|m| m.is_finite()));
                max_reach = max_reach.max(gait.reach_error);
                let worlds = def.joint_worlds(&gait.palette);
                let wrist = worlds[def.joint("Hand_L")].w_axis.z;
                let sway = worlds[def.joint("Hips")].w_axis.x;
                if (240..480).contains(&frame) {
                    wrist_range.0 = wrist_range.0.min(wrist);
                    wrist_range.1 = wrist_range.1.max(wrist);
                    sway_range.0 = sway_range.0.min(sway);
                    sway_range.1 = sway_range.1.max(sway);
                }
            }
            assert!(
                max_reach < 0.03,
                "rig {index}: unreachable foot {max_reach}"
            );
            assert!(
                wrist_range.1 - wrist_range.0 > 0.4,
                "rig {index}: insufficient arm motion"
            );
            assert!(
                sway_range.1 - sway_range.0 > 0.05,
                "rig {index}: missing weight transfer"
            );
            assert!(
                (gait.feet[0].target.x - gait.feet[1].target.x).abs() < 0.12,
                "rig {index}: feet too wide"
            );
        }
    }
    #[test]
    fn axis_mapping_and_ground_tilt_preserve_heading_height_and_deck() {
        let p = Vec3::new(42., -19., 7.);
        assert_eq!(motion_to_game(game_to_motion(p)), p);
        let tilt = Ground {
            point: p,
            gradient: [0.1, -0.2],
        }
        .tilt();
        let normal = tilt * Vec3::Y;
        assert!(normal.y > 0.95 && normal.x > 0. && normal.z < 0.);
    }
}
