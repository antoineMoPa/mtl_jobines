//! World-anchored footholds and the approved continuous full-body walking gait.
//! Simulation is in metres; motion space uses the GLB's +Y up and +Z forward.
use super::pedestrian_catalog::{Definition, JOINT_LIMIT};
use glam::{Mat4, Quat, Vec2, Vec3};
use opencity::runtime::{City, SurfaceKind};
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
#[derive(Clone, Copy, Default)]
pub(super) struct JumpMotion {
    forward: f32,
    sideways: f32,
    turn: f32,
}
impl JumpMotion {
    /// Map accepted game-space travel into the rig's facing and side axes.
    pub fn from_travel(velocity: Vec2, yaw: f32, turn_rate: f32) -> Self {
        Self {
            forward: (velocity.dot(Vec2::new(yaw.cos(), yaw.sin())) / 5.2).clamp(-1., 1.),
            sideways: (velocity.dot(Vec2::new(-yaw.sin(), yaw.cos())) / 5.2).clamp(-1., 1.),
            turn: (turn_rate / 2.62).clamp(-1., 1.),
        }
    }
    fn follow(&mut self, target: Self, dt: f32) {
        let amount = 1. - (-18. * dt).exp();
        self.forward += (target.forward - self.forward) * amount;
        self.sideways += (target.sideways - self.sideways) * amount;
        self.turn += (target.turn - self.turn) * amount;
    }
    fn side_lean(self) -> f32 {
        (self.sideways + self.turn * (0.5 + 0.5 * self.forward.abs())).clamp(-1., 1.)
    }
}
pub(super) struct JumpPreparation {
    pub compression: f32,
    pub extension: f32,
    pub motion: JumpMotion,
}
#[derive(Clone, Copy)]
pub(crate) struct Ground {
    pub point: Vec3,
    pub gradient: [f32; 2],
}
pub(super) fn footing(city: &City, point: Vec3, sidewalk_only: bool) -> Option<Ground> {
    let xy = (point.truncate() / super::METERS_PER_UNIT).to_array();
    let surface = city.surface_at(xy, (point.z + 0.42) / super::METERS_PER_UNIT)?;
    let walkable = match surface.kind {
        SurfaceKind::Sidewalk => true,
        SurfaceKind::Asphalt | SurfaceKind::Grass | SurfaceKind::Sand | SurfaceKind::Marking => {
            !sidewalk_only
        }
        _ => false,
    };
    let height = surface.height * super::METERS_PER_UNIT;
    if !walkable || (height - point.z).abs() > 0.42 {
        return None;
    }
    let bottom = (height + 0.12) / super::METERS_PER_UNIT;
    let top = (height + 1.75) / super::METERS_PER_UNIT;
    if city.building_collision(xy, 0.3 / super::METERS_PER_UNIT, bottom, top)
        || city
            .road_barrier_contact(xy, 0.3 / super::METERS_PER_UNIT, bottom, top)
            .is_some()
    {
        return None;
    }
    Some(Ground {
        point: point.truncate().extend(height),
        gradient: surface.gradient,
    })
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
    spacing: f32,
    foot_heading: [f32; 2],
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
            spacing: 0.50,
            foot_heading: [0.; 2],
        }
    }
    fn grounded_feet(
        &self,
        position: Vec3,
        yaw: f32,
        sample: impl Fn(Vec3) -> Option<Ground>,
    ) -> [Foot; 2] {
        let root = game_to_motion(position);
        std::array::from_fn(|i| {
            let ankle = self.ankles[i];
            let mut p =
                root + Quat::from_rotation_y(yaw) * Vec3::new(ankle.x * self.spacing, 0., ankle.z);
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
        })
    }
}
/// World-space ankle trajectories and ground anchors, in metres.
#[derive(Clone, Copy)]
pub(crate) struct FootMotion {
    pub root: Vec3,
    pub yaw: f32,
    pub ankles: [Vec3; 2],
    pub anchors: [Vec3; 2],
    pub contact: [bool; 2],
    pub speed: f32,
    pub cadence: f32,
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
    step_length: f32,
    planned: bool,
    movement: f32,
    idle_time: f32,
    compression: f32,
    jump_extension: f32,
    jump_motion: JumpMotion,
    air_time: Option<f32>,
    pub palette: [Mat4; JOINT_LIMIT],
    pub reach_error: f32,
    running: f32,
    air_velocity: Option<f32>,
    landing: Option<([Mat4; JOINT_LIMIT], f32)>,
}
impl Gait {
    pub(crate) fn foot_motion(&self) -> FootMotion {
        let model = &super::pedestrian_catalog::definitions()
            [super::pedestrian_catalog::PLAYER_SUMMER_MODEL];
        let worlds = model.joint_worlds(&self.palette);
        let root = self.position;
        let rotation = Quat::from_rotation_y(self.yaw);
        FootMotion {
            root: motion_to_game(self.position),
            yaw: self.yaw,
            ankles: std::array::from_fn(|i| {
                motion_to_game(
                    root + rotation
                        * worlds[model.joint(if i == 0 { "Foot_L" } else { "Foot_R" })]
                            .w_axis
                            .truncate(),
                )
            }),
            anchors: self.feet.map(|f| motion_to_game(f.position)),
            contact: std::array::from_fn(|i| {
                self.air_time.is_none()
                    && if !self.planned {
                        i != self.swing || self.speed.abs() < 0.02
                    } else if i == self.swing {
                        self.running < 0.01 && (self.phase <= 0.08 || self.phase >= 0.88)
                    } else {
                        self.running < 0.01 || self.phase <= 0.68
                    }
            }),
            speed: self.speed.abs(),
            cadence: if self.speed.abs() > 0.02 {
                60. / self.duration
            } else {
                0.
            },
        }
    }
    pub fn new(
        def: &Definition,
        position: Vec3,
        yaw: f32,
        seed: u32,
        sample: impl Fn(Vec3) -> Option<Ground>,
    ) -> Self {
        let rig = Rig::new(def);
        let root = game_to_motion(position);
        let feet = rig.grounded_feet(position, yaw, sample);
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
            step_length: 0.50,
            planned: false,
            movement: 0.,
            idle_time: (seed % 19) as f32 * 0.17,
            compression: 0.,
            jump_extension: 0.,
            jump_motion: JumpMotion::default(),
            air_time: None,
            palette: [Mat4::IDENTITY; JOINT_LIMIT],
            reach_error: 0.,
            running: 0.,
            air_velocity: None,
            landing: None,
        };
        gait.update_pose(def);
        gait
    }
    pub fn new_player(
        def: &Definition,
        position: Vec3,
        yaw: f32,
        sample: impl Fn(Vec3) -> Option<Ground>,
    ) -> Self {
        let mut gait = Self::new(def, position, yaw, 0, &sample);
        gait.step_length = 0.78;
        gait.rig.spacing = 0.26;
        // The shared player pose drives both outfits. Their meshes have authored
        // toe-out despite forward-facing ankle bones; compensate that bind shape.
        gait.rig.foot_heading = [0.23, -0.25];
        gait.feet = gait.rig.grounded_feet(position, yaw, sample);
        gait.update_pose(def);
        gait
    }
    pub(super) fn set_running(&mut self, running: bool, dt: f32) {
        let target = if running { 1. } else { 0. };
        self.running += (target - self.running) * (1. - (-10. * dt).exp());
    }
    pub(super) fn set_air_velocity(&mut self, velocity: f32) {
        self.air_velocity = Some(velocity);
    }
    pub(super) fn land(
        &mut self,
        def: &Definition,
        position: Vec3,
        yaw: f32,
        sample: impl Fn(Vec3) -> Option<Ground>,
    ) {
        let previous = def.joint_worlds(&self.palette);
        let running = self.running;
        *self = Self::new_player(def, position, yaw, sample);
        self.running = running;
        self.landing = Some((previous, 0.));
        self.palette = def.skin_from_worlds(&previous);
    }
    fn air_tuck(&self) -> f32 {
        self.air_tuck_with_delay(0.)
    }
    fn air_tuck_with_delay(&self, delay: f32) -> f32 {
        self.air_time.map_or(0., |t| {
            let release = self
                .air_velocity
                .map_or_else(|| 1. - ease((t - 0.55) / 0.35), |v| ease((v + 4.5) / 3.0));
            ease((t - delay) / 0.22) * release
        })
    }
    pub(super) fn prepare_jump(
        &mut self,
        def: &Definition,
        position: Vec3,
        yaw: f32,
        preparation: JumpPreparation,
        dt: f32,
        sample: impl Fn(Vec3) -> Option<Ground>,
    ) {
        self.jump_motion.follow(preparation.motion, dt);
        if let Some((_, elapsed)) = &mut self.landing {
            *elapsed += dt;
        }
        self.feet = self.rig.grounded_feet(position, yaw, &sample);
        // A small split stance gives the moving push-off a trailing support leg.
        for (side, foot) in self.feet.iter_mut().enumerate() {
            let offset = Quat::from_rotation_y(yaw)
                * Vec3::Z
                * self.jump_motion.forward
                * 0.065
                * if side == 0 { 1. } else { -1. };
            let candidate = foot.position + offset;
            if let Some(ground) = sample(motion_to_game(
                candidate - Vec3::Y * self.rig.ankles[side].y,
            )) {
                foot.position = game_to_motion(ground.point) + Vec3::Y * self.rig.ankles[side].y;
                foot.start = foot.position;
                foot.target = foot.position;
                foot.tilt = ground.tilt();
            }
        }
        self.position = game_to_motion(position);
        self.yaw = yaw;
        self.compression = preparation.compression;
        self.jump_extension = preparation.extension;
        self.air_time = None;
        self.planned = false;
        self.phase = 0.;
        self.speed = 0.;
        self.acceleration = 0.;
        self.turn_rate = 0.;
        self.movement = 0.;
        self.idle_time += dt;
        self.update_pose(def);
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
    /// Carry the takeoff pose with the body while neither foot has ground contact.
    pub(super) fn airborne(
        &mut self,
        def: &Definition,
        position: Vec3,
        yaw: f32,
        motion: JumpMotion,
        dt: f32,
    ) {
        let to = game_to_motion(position);
        if let Some((_, elapsed)) = &mut self.landing {
            *elapsed += dt;
        }
        let turn = angle_delta(self.yaw, yaw);
        let rotation = Quat::from_rotation_y(turn);
        for foot in &mut self.feet {
            foot.position = to + rotation * (foot.position - self.position);
            foot.start = to + rotation * (foot.start - self.position);
            foot.target = to + rotation * (foot.target - self.position);
            foot.yaw += turn;
            foot.start_yaw += turn;
            foot.target_yaw += turn;
        }
        self.position = to;
        self.yaw = yaw;
        self.compression = 0.;
        self.jump_motion.follow(motion, dt);
        let elapsed = self.air_time.map_or(0., |t| t + dt);
        self.air_time = Some(elapsed);
        self.jump_extension = 1. - ease((elapsed - 0.15) / 0.65);
        self.idle_time += dt;
        self.update_pose(def);
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
        self.advance_with_subdivisions(def, position, yaw, desired_speed, aim, dt, sample, false);
    }
    pub fn advance_coarse(
        &mut self,
        def: &Definition,
        position: Vec3,
        yaw: f32,
        desired_speed: f32,
        aim: Vec3,
        dt: f32,
        sample: impl Fn(Vec3) -> Option<Ground>,
    ) {
        self.advance_with_subdivisions(def, position, yaw, desired_speed, aim, dt, sample, true);
    }
    fn advance_with_subdivisions(
        &mut self,
        def: &Definition,
        position: Vec3,
        yaw: f32,
        desired_speed: f32,
        aim: Vec3,
        dt: f32,
        sample: impl Fn(Vec3) -> Option<Ground>,
        coarse: bool,
    ) {
        if dt <= 0. {
            return;
        }
        let to = game_to_motion(position);
        let from = self.position;
        let old_yaw = self.yaw;
        let forward = Vec3::new(self.yaw.sin(), 0., self.yaw.cos());
        let direction = if (to - from).dot(forward) < 0. {
            -1.
        } else {
            1.
        };
        // Support-height changes at curbs must not inflate forward stride speed.
        let travel = to - from;
        let speed = Vec2::new(travel.x, travel.z).length() / dt * direction;
        let acceleration = ((speed - self.speed) / dt).clamp(-32., 32.);
        self.compression = 0.;
        self.jump_extension = 0.;
        self.jump_motion = JumpMotion::default();
        self.air_time = None;
        self.air_velocity = None;
        if let Some((_, elapsed)) = &mut self.landing {
            *elapsed += dt;
        }
        let turn_rate = angle_delta(self.yaw, yaw) / dt;
        let subdivisions = if coarse {
            1
        } else {
            (dt / (1. / 120.)).ceil().max(1.) as usize
        };
        for i in 0..subdivisions {
            let h = dt / subdivisions as f32;
            self.position = from.lerp(to, (i + 1) as f32 / subdivisions as f32);
            self.yaw = old_yaw + angle_delta(old_yaw, yaw) * (i + 1) as f32 / subdivisions as f32;
            self.speed = speed;
            self.acceleration = acceleration;
            self.turn_rate = turn_rate;
            self.idle_time += h;
            let amount = (speed.abs() / desired_speed.abs().max(0.35)).min(1.);
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
                    + Quat::from_rotation_y(self.yaw)
                        * Vec3::new(ankle.x * self.rig.spacing, 0., ankle.z);
                let d = neutral - f.position;
                Vec3::new(d.x, 0., d.z).length() > 0.09 || angle_delta(f.yaw, self.yaw).abs() > 0.18
            });
            if self.speed.abs() < 0.015 && !unsettled {
                return;
            }
            // Faster travel needs a faster cadence; turns need short pivot steps even at rest.
            // A long stride needs a support foot already ahead of the body. Limit
            // the first/recovery step to the travel that its current support allows.
            let travel_direction = Vec3::new(self.yaw.sin(), 0., self.yaw.cos())
                * if desired_speed < 0. { -1. } else { 1. };
            let support = self.feet[1 - self.swing].position - self.position;
            let ahead = support.dot(travel_direction);
            let lateral = Vec3::new(support.x, 0., support.z) - travel_direction * ahead;
            let available = ahead
                + ((0.53 * self.rig.scale).powi(2) - lateral.length_squared())
                    .max(0.)
                    .sqrt();
            let planning_speed = desired_speed.abs().max(self.speed.abs()).max(0.35);
            // The old 90 ms floor capped cadence and stranded fast players at
            // their support foot's reach limit. Keep the same travel allowance
            // as speed rises, rather than carrying planted feet with the body.
            let minimum_duration = 0.09 * (3.45 / planning_speed).min(1.);
            let support_time = available / planning_speed;
            // Shorten turning steps: the outside hip travels farther than the root.
            let turn_stride = 1. / (1. + self.turn_rate.abs() * 0.35);
            self.duration =
                ((self.step_length + self.running * 0.12) * self.rig.scale * turn_stride
                    / planning_speed)
                    .min(support_time.max(minimum_duration))
                    .min(0.30 / self.turn_rate.abs().max(0.01))
                    .clamp(minimum_duration, 0.75);
            let mut p = self.position;
            let mut yaw = self.yaw;
            let mut speed = self.speed;
            for _ in 0..12 {
                let h = self.duration / 12.;
                speed += (desired_speed - speed).clamp(
                    -self.acceleration.abs().max(2.) * h,
                    self.acceleration.abs().max(1.2) * h,
                );
                // Backward steps keep the body's facing while planning behind it.
                let delta = (aim - p) * if speed < 0. { -1. } else { 1. };
                if self.turn_rate.abs() > 0.1 {
                    yaw += self.turn_rate * h;
                } else if delta.x * delta.x + delta.z * delta.z > 0.0025 && speed.abs() > 0.015 {
                    yaw += angle_delta(yaw, delta.x.atan2(delta.z)).clamp(-2.2 * h, 2.2 * h);
                }
                p += Vec3::new(yaw.sin(), 0., yaw.cos()) * speed * h;
            }
            let ankle = self.rig.ankles[self.swing];
            let target = p + Quat::from_rotation_y(yaw)
                * Vec3::new(
                    ankle.x * self.rig.spacing,
                    0.,
                    ankle.z + speed * self.duration * 0.45,
                );
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
        let walking_flight = ((self.phase - 0.08) / 0.80).clamp(0., 1.);
        let flight = walking_flight + (self.phase - walking_flight) * self.running;
        let foot = &mut self.feet[self.swing];
        foot.position = foot.start.lerp(foot.target, ease(flight));
        foot.position.y +=
            (0.065 + self.running * 0.22) * self.rig.scale * (PI * flight).sin().max(0.).powf(1.4);
        foot.yaw = foot.start_yaw + angle_delta(foot.start_yaw, foot.target_yaw) * ease(flight);
        foot.tilt = foot.start_tilt.slerp(foot.target_tilt, ease(flight));
        // Late toe-off precedes the opposite landing, giving running a flight phase.
        // The raised foot becomes the next swing's actual start, so phase wrap stays continuous.
        if self.running > 0.001 {
            let support = &mut self.feet[1 - self.swing];
            support.position.y = support.target.y
                + self.running * self.rig.scale * 0.07 * ease((self.phase - 0.68) / 0.32);
        }
        // Let planted feet swivel before the body can wind the ankle around its old heading.
        for foot in &mut self.feet {
            foot.yaw = self.yaw + angle_delta(self.yaw, foot.yaw).clamp(-0.4, 0.4);
        }
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
        let run = self.running * movement;
        let forward = Vec3::new(self.yaw.sin(), 0., self.yaw.cos());

        // One continuous gait phase coordinates the whole body across both footfalls.
        let cycle = (self.steps as f32 + self.phase) * PI;
        let weight_shift = -cycle.sin() * 0.8 * movement;
        let pelvis_roll = weight_shift * 0.035;
        locals[self.rig.hips].translation.x += weight_shift * 0.040;
        locals[self.rig.hips].translation.y -= 0.020 + (2. * cycle).cos() * 0.012 * movement;
        locals[self.rig.hips].translation.y -= self.compression * 0.26 * self.rig.scale;
        locals[self.rig.hips].rotation *= Quat::from_rotation_x(run * 0.25);
        locals[self.rig.hips].translation.z += run * 0.065;
        locals[self.rig.hips].translation.y += run * 0.025 * (PI * self.phase).sin();
        let push = self.compression * 0.25 + self.jump_extension;
        let forward_lean = self.jump_motion.forward * push;
        let side_lean = self.jump_motion.side_lean() * push;
        locals[self.rig.hips].translation.z += forward_lean * 0.13 * self.rig.scale;
        locals[self.rig.hips].translation.x += side_lean * 0.065 * self.rig.scale;
        locals[self.rig.hips].rotation *=
            Quat::from_rotation_y(cycle.cos() * 0.075 * 0.8 * movement)
                * Quat::from_rotation_z(pelvis_roll);
        locals[self.rig.hips].rotation *= Quat::from_rotation_x(forward_lean * 0.10)
            * Quat::from_rotation_z(-side_lean * 0.08)
            * Quat::from_rotation_y(self.jump_motion.turn * self.jump_extension * 0.10);
        let lean = (self.turn_rate * self.speed * 0.035).clamp(-0.09, 0.09) * 0.8;
        locals[model.node("Spine")].rotation *= Quat::from_rotation_z(-lean - pelvis_roll * 0.65)
            * Quat::from_rotation_x(
                (self.acceleration.clamp(-10., 7.) * 0.025 + self.speed * 0.025) * 0.8
                    + self.compression * 0.14
                    + run * 0.18,
            );
        locals[model.node("Spine")].rotation *=
            Quat::from_rotation_x(forward_lean * 0.12) * Quat::from_rotation_z(-side_lean * 0.10);
        locals[model.node("Chest")].rotation *=
            Quat::from_rotation_y(-cycle.cos() * 0.14 * 0.8 * movement);
        locals[model.node("Chest")].rotation *=
            Quat::from_rotation_y(self.jump_motion.turn * self.jump_extension * 0.10);
        locals[model.node("Neck")].rotation *=
            Quat::from_rotation_z(lean * 0.5 - pelvis_roll * 0.2)
                * Quat::from_rotation_x(-run * 0.15);
        for (i, side) in ["L", "R"].iter().enumerate() {
            // Let the left arm follow the right, but release both before landing.
            let tuck = self.air_tuck_with_delay(if i == 0 { 0.06 } else { 0. });
            let sign = if i == 0 { 1. } else { -1. };
            let arm_phase = cycle - 0.16 + i as f32 * PI;
            let swing = -arm_phase.cos() * (35f32.to_radians() + self.running * 0.24) * movement;
            locals[model.node(&format!("UpperArm_{side}"))].rotation *= Quat::from_rotation_x(
                swing + self.compression * 0.35
                    - tuck * 1.25
                    - self.jump_extension
                        * (self.jump_motion.forward * 0.45 + sign * self.jump_motion.turn * 0.16),
            ) * Quat::from_rotation_z(
                sign * (0.16 + self.running * 0.06 + lean.abs() * 0.4) - side_lean * 0.12,
            );
            locals[model.node(&format!("Forearm_{side}"))].rotation *= Quat::from_rotation_x(
                (-0.18 + (arm_phase - 0.30).cos() * 0.13) * movement
                    - self.compression * 0.65
                    - run * 0.85
                    - tuck * 0.45,
            );
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
        // On a curb the root can rise before the trailing foot leaves the lower
        // surface. Preserve the same world-space reach allowance during that transfer.
        let support_drop = contact
            .iter()
            .enumerate()
            .map(|(i, (point, _))| self.position.y + self.rig.ankles[i].y - point.y)
            .fold(0f32, f32::max)
            .min(0.25 * self.rig.scale);
        locals[self.rig.hips].translation.y -= lower.max(0.).min(0.3 + run * 0.08 + support_drop);
        let mut reach_error: f32 = 0.;
        for (i, leg) in self.rig.legs.iter().enumerate() {
            let worlds = model.worlds(&locals, root, &overrides);
            let hip = worlds[leg[0]].transform_point3(Vec3::ZERO);
            let target = contact[i].0;
            let pole = forward;
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
                let facing = if bone == leg[1] {
                    self.feet[i].yaw
                } else {
                    self.yaw
                };
                let local_direction = Quat::from_rotation_y(-facing) * direction;
                let rotation = if local_direction.y < -0.05 {
                    Quat::from_rotation_y(facing)
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
        if let Some((previous, elapsed)) = self.landing {
            let amount = ease(elapsed / 0.16);
            let mut worlds = model.joint_worlds(&skin);
            for (world, start) in worlds.iter_mut().zip(previous) {
                let (scale, rotation, translation) = world.to_scale_rotation_translation();
                let (old_scale, old_rotation, old_translation) =
                    start.to_scale_rotation_translation();
                *world = Mat4::from_scale_rotation_translation(
                    old_scale.lerp(scale, amount),
                    old_rotation.slerp(rotation, amount),
                    old_translation.lerp(translation, amount),
                );
            }
            skin = model.skin_from_worlds(&worlds);
            if elapsed >= 0.16 {
                self.landing = None;
            }
        }
        self.palette = skin;
        self.reach_error = reach_error;
    }
    fn foot_contact(&self, _model: &Definition, side: usize) -> (Vec3, Quat) {
        let foot = &self.feet[side];
        let active = self.speed.abs() > 0.02 || self.planned;
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
        let pitch = pitch * (0.3 + self.running * 0.7);
        let ankle_height = self.rig.ankles[side].y;
        let follow_through = self
            .air_time
            .map_or(0., |t| ease(t / 0.12) * (1. - ease((t - 0.6) / 0.3)));
        let drive = self
            .jump_motion
            .forward
            .abs()
            .max(self.jump_motion.turn.abs() * 0.5);
        let pitch = pitch + 0.20 * drive * self.jump_extension * (1. - follow_through);
        let pivot = Vec3::new(0., -ankle_height, 0.14 * (1. - heel) - 0.065 * heel);
        let ground = foot.tilt * Quat::from_rotation_y(foot.yaw - self.rig.foot_heading[side]);
        let roll = Quat::from_rotation_x(pitch);
        let sign = if side == 0 { 1. } else { -1. };
        let flight = Quat::from_rotation_y(self.yaw)
            * Vec3::new(
                0.,
                (drive * 0.075
                    + sign * self.jump_motion.forward * 0.055
                    + sign * self.jump_motion.turn * 0.025)
                    .max(0.),
                sign * self.jump_motion.forward * 0.10,
            )
            * follow_through
            * self.rig.scale;
        (
            foot.position
                + ground * (pivot - roll * pivot)
                + flight
                + Quat::from_rotation_y(self.yaw)
                    * Vec3::new(
                        0.,
                        self.air_tuck() * 0.24 * self.rig.scale,
                        sign * self.air_tuck() * 0.07,
                    ),
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
    fn player_has_longer_slower_steps_at_the_same_travel_speed() {
        let def = &pedestrian_catalog::definitions()[pedestrian_catalog::PLAYER_SUMMER_MODEL];
        let sample = |p: Vec3| {
            Some(Ground {
                point: Vec3::new(p.x, p.y, 0.),
                gradient: [0., 0.],
            })
        };
        let measure = |mut gait: Gait| {
            let mut max_step: f32 = 0.;
            for frame in 1..=240 {
                let position = Vec3::new(frame as f32 * 5.2 / 120., 0., 0.);
                gait.advance(
                    def,
                    position,
                    0.,
                    5.2,
                    position + Vec3::X,
                    1. / 120.,
                    sample,
                );
                for foot in &gait.feet {
                    max_step = max_step.max(foot.start.distance(foot.target));
                }
                assert!(
                    gait.reach_error < 0.06,
                    "frame {frame}, step length {}, reach {}",
                    gait.step_length,
                    gait.reach_error
                );
            }
            (gait.steps, max_step)
        };
        let standard = measure(Gait::new(def, Vec3::ZERO, 0., 0, sample));
        let player = measure(Gait::new_player(def, Vec3::ZERO, 0., sample));
        assert!(
            player.0 < standard.0 * 9 / 10,
            "step counts: {standard:?} {player:?}"
        );
        assert!(
            player.1 > standard.1 * 1.1,
            "step lengths: {standard:?} {player:?}"
        );
    }
    #[test]
    fn directional_jump_poses_remain_reachable_on_all_six_rigs() {
        let sample = |p: Vec3| {
            Some(Ground {
                point: p.truncate().extend(p.x * 0.04),
                gradient: [0.04, 0.],
            })
        };
        for def in pedestrian_catalog::definitions() {
            for (speed, turn) in [(0., 2.62), (15.6, 2.62), (-15.6, -2.62)] {
                let mut gait = Gait::new(def, Vec3::ZERO, 0., 0, sample);
                for frame in 0..180 {
                    let yaw = frame as f32 * turn / 120.;
                    let motion =
                        JumpMotion::from_travel(Vec2::new(yaw.cos(), yaw.sin()) * speed, yaw, turn);
                    if frame < 26 {
                        let elapsed = (frame + 1) as f32 / 120.;
                        let extension = ease((elapsed - 0.14) / 0.08);
                        let compression = if elapsed < 0.14 {
                            ease(elapsed / 0.14)
                        } else {
                            1. - extension
                        };
                        gait.prepare_jump(
                            def,
                            Vec3::ZERO,
                            yaw,
                            JumpPreparation {
                                compression,
                                extension,
                                motion,
                            },
                            1. / 120.,
                            sample,
                        );
                    } else {
                        gait.airborne(def, Vec3::Z, yaw, motion, 1. / 120.);
                    }
                    assert!(
                        gait.reach_error < 0.06,
                        "directional jump strained a leg: {}",
                        gait.reach_error
                    );
                    assert!(gait.palette.iter().all(|m| m.is_finite()));
                }
            }
        }
    }
    #[test]
    fn turning_keeps_shoes_compact_on_every_rig() {
        let sample = |p: Vec3| {
            Some(Ground {
                point: p.truncate().extend(0.),
                gradient: [0., 0.],
            })
        };
        for (index, def) in pedestrian_catalog::definitions().iter().enumerate() {
            for speed in [0., 1.05] {
                for turn in [-2.62, 2.62] {
                    let mut position = Vec3::ZERO;
                    let mut gait = Gait::new(def, position, 0., 0, sample);
                    for frame in 1..=600 {
                        let yaw = frame as f32 * turn / 120.;
                        position += Vec3::new(yaw.cos(), yaw.sin(), 0.) * speed / 120.;
                        gait.advance(
                            def,
                            position,
                            yaw,
                            speed,
                            position + Vec3::new(yaw.cos(), yaw.sin(), 0.),
                            1. / 120.,
                            sample,
                        );
                        for foot in &gait.feet {
                            assert!(angle_delta(yaw, foot.yaw).abs() < 0.401);
                        }
                        assert!(
                            gait.reach_error < 0.06,
                            "rig {index}, speed {speed}: {}",
                            gait.reach_error
                        );
                        if frame % 10 != 0 {
                            continue;
                        }
                        // Check the rendered shoe triangles, including vertices blended at the ankle.
                        for (vertices, weights) in
                            def.vertices.chunks_exact(3).zip(def.skin.chunks_exact(3))
                        {
                            if vertices
                                .iter()
                                .any(|v| v.position[1] > gait.rig.ankles[0].y * 0.95)
                            {
                                continue;
                            }
                            let posed: [Vec3; 3] = std::array::from_fn(|i| {
                                weights[i].joints.iter().zip(weights[i].weights).fold(
                                    Vec3::ZERO,
                                    |p, (&joint, weight)| {
                                        p + gait.palette[joint as usize]
                                            .transform_point3(Vec3::from(vertices[i].position))
                                            * weight
                                    },
                                )
                            });
                            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                                let rest = Vec3::from(vertices[a].position)
                                    .distance(Vec3::from(vertices[b].position));
                                assert!(posed[a].distance(posed[b]) < rest * 1.25 + 0.003,
                                    "rig {index}, speed {speed}, frame {frame}: shoe edge {} became {}", rest, posed[a].distance(posed[b]));
                            }
                        }
                    }
                }
            }
        }
    }
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

#[cfg(test)]
#[path = "pedestrian_gait_motion_tests.rs"]
mod motion_tests;
