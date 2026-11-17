use super::{
    crowd::Instance,
    pedestrian_catalog,
    pedestrian_gait::{footing, Gait, JumpMotion, JumpPreparation},
    Vehicle, METERS_PER_UNIT,
};
use glam::{Vec2, Vec3};
use opencity::runtime::City;

// Keep the public player/camera position at body height; the rig root is at its feet.
pub(super) const HALF_HEIGHT: f32 = 0.9;
pub(super) const RADIUS: f32 = 0.4;
const MODEL: usize = pedestrian_catalog::PLAYER_SUMMER_MODEL;
const WALK_SPEED: f32 = 1.8;
// Arcade sprint: 2.5x the original run, with cadence following actual travel.
const RUN_SPEED: f32 = 3.45 * 2.5;
const ACCELERATION: f32 = 24.;
const BRAKING: f32 = 32.;
const GRAVITY: f32 = 9.81;
const JUMP_HEIGHT: f32 = 1.5;
const JUMP_CROUCH_TIME: f32 = 0.14;
const JUMP_EXTENSION_TIME: f32 = 0.08;
pub const TURN_SPEED: f32 = std::f32::consts::PI * 5. / 6.;

enum Jump {
    Grounded,
    Preparing { elapsed: f32 },
    Airborne,
}
impl Jump {
    fn is_airborne(&self) -> bool {
        matches!(self, Self::Airborne)
    }
    fn compression(&self) -> f32 {
        let Self::Preparing { elapsed } = *self else {
            return 0.;
        };
        let amount = if elapsed <= JUMP_CROUCH_TIME {
            elapsed / JUMP_CROUCH_TIME
        } else {
            1. - (elapsed - JUMP_CROUCH_TIME) / JUMP_EXTENSION_TIME
        }
        .clamp(0., 1.);
        amount * amount * (3. - 2. * amount)
    }
    fn extension(&self) -> f32 {
        let Self::Preparing { elapsed } = *self else {
            return 0.;
        };
        let amount = ((elapsed - JUMP_CROUCH_TIME) / JUMP_EXTENSION_TIME).clamp(0., 1.);
        amount * amount * (3. - 2. * amount)
    }
    fn advance(&mut self, dt: f32) -> bool {
        if let Self::Preparing { elapsed } = self {
            *elapsed += dt;
            if *elapsed >= JUMP_CROUCH_TIME + JUMP_EXTENSION_TIME {
                *self = Self::Airborne;
                return true;
            }
        }
        false
    }
}

#[derive(Clone, Copy, Default)]
pub struct WalkControls {
    pub forward: f32,
    pub turn: f32,
    pub run: bool,
    pub jump: bool,
}

pub struct Pedestrian {
    pub position: Vec3,
    pub yaw: f32,
    speed: f32,
    vertical_speed: f32,
    jump: Jump,
    jump_held: bool,
    gait: Gait,
}
pub(super) fn blocked_by_vehicle(position: Vec3, vehicle: &Vehicle) -> bool {
    let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
    let left = Vec2::new(-forward.y, forward.x);
    let relative = position - vehicle.position;
    relative.z.abs() < 1.5
        && relative.truncate().dot(forward).abs() < vehicle.length() * 0.5 + RADIUS
        && relative.truncate().dot(left).abs() < vehicle.width() * 0.5 + RADIUS
}
impl Pedestrian {
    pub(super) fn at(city: &City, start: super::SpawnPoint) -> Result<Self, String> {
        let support = city
            .surface_at(start.point, f32::INFINITY)
            .ok_or_else(|| "Selected walking start has no ground support".to_string())?;
        if support.kind == opencity::runtime::SurfaceKind::Water {
            return Err("Selected walking start is in water".into());
        }
        let position = Vec3::new(
            start.point[0] * METERS_PER_UNIT,
            start.point[1] * METERS_PER_UNIT,
            support.height * METERS_PER_UNIT + HALF_HEIGHT,
        );
        if Self::blocked(city, position) {
            return Err("Selected walking start overlaps a building or barrier".into());
        }
        Ok(Self::new(city, position, start.heading.unwrap_or(0.)))
    }
    pub(super) fn new(city: &City, position: Vec3, yaw: f32) -> Self {
        Self {
            position,
            yaw,
            speed: 0.,
            vertical_speed: 0.,
            jump: Jump::Grounded,
            jump_held: false,
            gait: Gait::new_player(
                &pedestrian_catalog::definitions()[MODEL],
                position - Vec3::Z * HALF_HEIGHT,
                yaw,
                |p| footing(city, p, false),
            ),
        }
    }
    #[cfg(test)]
    pub fn beside(city: &City, vehicle: &Vehicle) -> Option<Self> {
        Self::beside_if(city, vehicle, |_| false)
    }
    pub fn beside_if(
        city: &City,
        vehicle: &Vehicle,
        blocked: impl Fn(Vec3) -> bool,
    ) -> Option<Self> {
        let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
        let left = Vec2::new(-forward.y, forward.x);
        for offset in [
            left * (vehicle.width() * 0.5 + 0.9),
            -left * (vehicle.width() * 0.5 + 0.9),
            -forward * (vehicle.length() * 0.5 + 0.9),
            forward * (vehicle.length() * 0.5 + 0.9),
        ] {
            let point = vehicle.position.truncate() + offset;
            let Some(ground) = footing(
                city,
                point.extend(vehicle.position.z - vehicle.ride_height()),
                false,
            ) else {
                continue;
            };
            let position = ground.point + Vec3::Z * HALF_HEIGHT;
            if !Self::blocked(city, position) && !blocked(position) {
                return Some(Self::new(city, position, vehicle.yaw));
            }
        }
        None
    }
    fn blocked(city: &City, position: Vec3) -> bool {
        city.road_barrier_contact(
            (position.truncate() / METERS_PER_UNIT).to_array(),
            RADIUS / METERS_PER_UNIT,
            (position.z - HALF_HEIGHT + 0.05) / METERS_PER_UNIT,
            (position.z + HALF_HEIGHT) / METERS_PER_UNIT,
        )
        .is_some()
            || city.building_collision(
                (position.truncate() / METERS_PER_UNIT).to_array(),
                RADIUS / METERS_PER_UNIT,
                (position.z - HALF_HEIGHT + 0.05) / METERS_PER_UNIT,
                (position.z + HALF_HEIGHT) / METERS_PER_UNIT,
            )
    }
    pub(crate) fn foot_motion(&self) -> super::pedestrian_gait::FootMotion {
        self.gait.foot_motion()
    }
    pub fn step(
        &mut self,
        city: &City,
        input: WalkControls,
        dt: f32,
        blocked: impl Fn(Vec3) -> bool,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let steps = (dt / (1. / 120.)).ceil().max(1.) as usize;
        for _ in 0..steps {
            self.step_once(city, input, dt / steps as f32, &blocked);
        }
    }
    fn step_once(
        &mut self,
        city: &City,
        input: WalkControls,
        dt: f32,
        blocked: &impl Fn(Vec3) -> bool,
    ) {
        let model = &pedestrian_catalog::definitions()[MODEL];
        let target = input.forward.clamp(-1., 1.) * if input.run { RUN_SPEED } else { WALK_SPEED };
        if input.jump && !self.jump_held && matches!(self.jump, Jump::Grounded) {
            self.jump = Jump::Preparing { elapsed: 0. };
        }
        if self.jump.advance(dt) {
            self.vertical_speed = (2. * GRAVITY * JUMP_HEIGHT).sqrt();
        }
        self.jump_held = input.jump;
        self.speed += (target - self.speed).clamp(-BRAKING * dt, ACCELERATION * dt);
        self.gait
            .set_running(self.speed.abs() > WALK_SPEED * 1.05, dt);
        let turn = input.turn * TURN_SPEED * dt;
        let heading = self.yaw + turn * 0.5;
        self.yaw += turn;
        let movement = Vec2::new(heading.cos(), heading.sin()) * self.speed * dt;
        let before = self.position;
        for delta in [Vec2::new(movement.x, 0.), Vec2::new(0., movement.y)] {
            if delta.length_squared() < 1e-12 {
                continue;
            }
            if self.jump.is_airborne() {
                let position = self.position + delta.extend(0.);
                if !Self::blocked(city, position) && !blocked(position) {
                    self.position = position;
                }
                continue;
            }
            let foot = self.position - Vec3::Z * HALF_HEIGHT;
            let Some(ground) = footing(city, foot + delta.extend(0.), false) else {
                continue;
            };
            let position = ground.point + Vec3::Z * HALF_HEIGHT;
            if Self::blocked(city, position)
                || blocked(position)
                || !self.gait.permits(ground.point)
            {
                continue;
            }
            // A walkable step changes support height without consuming forward
            // travel. Interpolating toward the raised point can leave the body
            // on the old side of the curb, repeatedly climbing and dropping.
            self.position = position;
        }
        let mut landed = false;
        if self.jump.is_airborne() {
            let previous_foot = self.position.z - HALF_HEIGHT;
            let mut next = self.position;
            next.z += self.vertical_speed * dt - 0.5 * GRAVITY * dt * dt;
            self.vertical_speed -= GRAVITY * dt;
            let ground = city
                .surface_at(
                    (next.truncate() / METERS_PER_UNIT).to_array(),
                    (previous_foot + 0.01) / METERS_PER_UNIT,
                )
                .and_then(|surface| {
                    footing(
                        city,
                        next.truncate().extend(surface.height * METERS_PER_UNIT),
                        false,
                    )
                });
            if let Some(ground) =
                ground.filter(|g| self.vertical_speed <= 0. && next.z - HALF_HEIGHT <= g.point.z)
            {
                let landing = ground.point + Vec3::Z * HALF_HEIGHT;
                if !Self::blocked(city, landing) && !blocked(landing) {
                    next = landing;
                    self.vertical_speed = 0.;
                    self.jump = Jump::Grounded;
                    landed = true;
                }
            }
            if !Self::blocked(city, next) && !blocked(next) {
                self.position = next;
            } else if self.vertical_speed > 0. {
                self.vertical_speed = 0.;
            }
        }
        let foot = self.position - Vec3::Z * HALF_HEIGHT;
        let jump_motion = JumpMotion::from_travel(
            (self.position - before).truncate() / dt,
            self.yaw,
            turn / dt,
        );
        if landed {
            self.gait
                .land(model, foot, self.yaw, |p| footing(city, p, false));
        } else if self.jump.is_airborne() {
            self.gait.set_air_velocity(self.vertical_speed);
            self.gait.airborne(model, foot, self.yaw, jump_motion, dt);
        } else if matches!(self.jump, Jump::Preparing { .. }) {
            self.gait.prepare_jump(
                model,
                foot,
                self.yaw,
                JumpPreparation {
                    compression: self.jump.compression(),
                    extension: self.jump.extension(),
                    motion: jump_motion,
                },
                dt,
                |p| footing(city, p, false).filter(|g| !blocked(g.point + Vec3::Z * HALF_HEIGHT)),
            );
        } else {
            self.gait.advance(
                model,
                foot,
                self.yaw,
                target,
                foot + Vec3::new(self.yaw.cos(), self.yaw.sin(), 0.)
                    * if self.speed < 0. { -1. } else { 1. },
                dt,
                |p| footing(city, p, false).filter(|g| !blocked(g.point + Vec3::Z * HALF_HEIGHT)),
            );
        }
        // Animate only accepted travel. A wall or a prop cannot advance the body then roll it back.
        let accepted = self.position - before;
        // Stepping up or down changes support height, not forward speed.
        let distance = accepted.truncate().length();
        self.speed = distance / dt * if self.speed < 0. { -1. } else { 1. };
    }
    pub(super) fn instance(&self, season: crate::season::Season) -> Instance<'_> {
        Instance {
            model: pedestrian_catalog::player_model(season),
            transform: pedestrian_catalog::instance_transform(
                self.position - Vec3::Z * HALF_HEIGHT,
                self.yaw,
            ),
            opacity: 1.,
            idle_time: 0.,
            walk_time: 0.,
            movement: 0.,
            pose: Some(&self.gait.palette),
        }
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
    fn open_ground() -> City {
        City::from_program(
            &opencity::parse(
                "land(-10,-10,10,-10,10,10,-10,10); road street:carlane;end street(-2,0,-1,0);",
            )
            .unwrap(),
            None,
        )
        .unwrap()
    }
    fn origin(city: &City) -> Vec3 {
        Vec3::Z * (city.surface_at([0., 0.], 1.).unwrap().height * METERS_PER_UNIT + HALF_HEIGHT)
    }
    #[test]
    fn exit_uses_ground_and_walk_does_not_pass_through_car() {
        let city = city();
        let vehicle = Vehicle::spawn(&city);
        let player = Pedestrian::beside(&city, &vehicle).unwrap();
        let start = player.position;
        let mut player = Pedestrian::new(&city, start, vehicle.yaw - std::f32::consts::FRAC_PI_2);
        assert!(!Pedestrian::blocked(&city, player.position));
        for _ in 0..360 {
            player.step(
                &city,
                WalkControls {
                    forward: 1.,
                    ..Default::default()
                },
                1. / 120.,
                |p| blocked_by_vehicle(p, &vehicle),
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
        let blocked = City::from_program(
            &opencity::parse(
                "road street: sidewalk carlane sidewalk; end street(-25,3,-20,3); \
                    building(0.2,-22.6,2.9,-22.4,2.9,-22.4,3.1,-22.6,3.1);",
            )
            .unwrap(),
            None,
        )
        .unwrap();
        let vehicle = Vehicle::spawn(&blocked);
        assert!(Pedestrian::beside(&blocked, &vehicle).is_none());
    }
    #[test]
    fn season_changes_player_outfit_without_resetting_gait() {
        let city = open_ground();
        let player = Pedestrian::new(&city, origin(&city), 0.);
        let summer = player.instance(crate::season::Season::Summer);
        let winter = player.instance(crate::season::Season::Winter);
        assert_eq!(summer.model, pedestrian_catalog::PLAYER_SUMMER_MODEL);
        assert_eq!(winter.model, pedestrian_catalog::PLAYER_WINTER_MODEL);
        assert_eq!(summer.transform, winter.transform);
        assert!(std::ptr::eq(summer.pose.unwrap(), winter.pose.unwrap()));
    }
    #[test]
    fn turning_in_place_keeps_body_position_and_rotates_the_skinned_model() {
        let city = open_ground();
        for direction in [-1., 1.] {
            let mut player = Pedestrian::new(&city, origin(&city), 0.);
            let start = player.position;
            let initial = player.gait.palette;
            for _ in 0..120 {
                player.step(
                    &city,
                    WalkControls {
                        turn: direction,
                        ..Default::default()
                    },
                    1. / 120.,
                    |_| false,
                );
            }
            assert_eq!(player.position, start);
            assert!((player.yaw - direction * TURN_SPEED).abs() < 0.0001);
            let instance = player.instance(crate::season::Season::Summer);
            let forward = instance.transform.transform_vector3(Vec3::Z) * METERS_PER_UNIT;
            assert!(forward.abs_diff_eq(Vec3::new(player.yaw.cos(), player.yaw.sin(), 0.), 1e-5));
            assert!(initial
                .iter()
                .zip(player.gait.palette)
                .any(|(a, b)| !a.abs_diff_eq(b, 0.01)));
            assert!(player.gait.reach_error < 0.04);
        }
    }
    #[test]
    fn shared_gait_walks_runs_reverses_and_stops_with_grounded_feet() {
        let city = open_ground();
        let mut distances = Vec::new();
        for run in [false, true] {
            for forward in [-1., 1.] {
                let mut player = Pedestrian::new(&city, origin(&city), std::f32::consts::FRAC_PI_2);
                let mut max_reach: f32 = 0.;
                for _ in 0..480 {
                    player.step(
                        &city,
                        WalkControls {
                            forward,
                            run,
                            ..Default::default()
                        },
                        1. / 120.,
                        |_| false,
                    );
                    max_reach = max_reach.max(player.gait.reach_error);
                    assert!(player.gait.palette.iter().all(|m| m.is_finite()));
                }
                assert!(player.position.x.abs() < 0.001);
                assert!(
                    player.position.y * forward > 2.,
                    "player failed to travel: {:?}",
                    player.position
                );
                assert!(max_reach < 0.06, "unreachable feet: {max_reach}");
                distances.push(player.position.y.abs());
                for _ in 0..240 {
                    player.step(&city, WalkControls::default(), 1. / 120., |_| false);
                }
                let stopped = player.position;
                for _ in 0..120 {
                    player.step(&city, WalkControls::default(), 1. / 120., |_| false);
                }
                assert_eq!(player.position, stopped);
                assert!(player.gait.reach_error < 0.04);
            }
        }
        assert!(distances[2] > distances[0] * 1.2);
        assert!(distances[3] > distances[1] * 1.2);
    }
    #[test]
    fn player_contacts_stay_world_anchored_at_full_walk_and_run_speed() {
        let city = open_ground();
        for run in [false, true] {
            for forward in [-1., 1.] {
                let mut player = Pedestrian::new(&city, origin(&city), 0.);
                let mut previous = player.foot_motion();
                let mut contacts = 0;
                let mut max_speed: f32 = 0.;
                let mut flight_frames = 0;
                for _ in 0..480 {
                    player.step(
                        &city,
                        WalkControls {
                            forward,
                            run,
                            ..Default::default()
                        },
                        1. / 120.,
                        |_| false,
                    );
                    let motion = player.foot_motion();
                    max_speed = max_speed.max(motion.speed);
                    if !motion.contact[0] && !motion.contact[1] {
                        flight_frames += 1;
                    }
                    for side in 0..2 {
                        assert!(motion.ankles[side].is_finite());
                        if previous.contact[side] && motion.contact[side] {
                            contacts += 1;
                            assert!(
                                previous.anchors[side].distance(motion.anchors[side]) < 0.0001,
                                "planted foot slid at run={run}, forward={forward}"
                            );
                        }
                    }
                    previous = motion;
                }
                assert!(contacts > 100);
                if run {
                    assert!(flight_frames > 20, "fast run lost its flight phase");
                    assert!(
                        previous.cadence > 450.,
                        "run cadence did not follow travel speed"
                    );
                }
                let target = if run { RUN_SPEED } else { WALK_SPEED };
                assert!(
                    max_speed > target * 0.95,
                    "gait blocked requested travel: {max_speed}"
                );
            }
        }
    }
    #[test]
    fn external_blockers_prevent_body_and_foothold_travel() {
        let city = open_ground();
        let mut player = Pedestrian::new(&city, origin(&city), 0.);
        for _ in 0..480 {
            player.step(
                &city,
                WalkControls {
                    forward: 1.,
                    ..Default::default()
                },
                1. / 120.,
                |p| p.x > 0.8,
            );
        }
        assert!(player.position.x > 0.3 && player.position.x <= 0.8);
        assert!(player.gait.reach_error < 0.04);
    }
    #[test]
    fn forward_turning_follows_a_curve_independently_of_step_frequency() {
        let city = open_ground();
        let simulate = |steps| {
            let mut player = Pedestrian::new(&city, origin(&city), 0.);
            for _ in 0..steps {
                let before = player.position;
                let heading = player.yaw + TURN_SPEED * 0.5 / steps as f32;
                player.step(
                    &city,
                    WalkControls {
                        forward: 1.,
                        turn: 1.,
                        ..Default::default()
                    },
                    1. / steps as f32,
                    |_| false,
                );
                assert!(
                    (player.position - before).dot(Vec3::new(heading.cos(), heading.sin(), 0.))
                        >= 0.
                );
            }
            player
        };
        let a = simulate(60);
        let b = simulate(120);
        assert!((a.yaw - b.yaw).abs() < 0.0001);
        assert!(a.position.distance(b.position) < 0.02);
        assert!(b.position.y > 0. && b.position.truncate().length() > 0.3);
    }
    #[test]
    fn walk_and_sprint_reach_their_requested_speeds() {
        let city = open_ground();
        for (run, expected) in [(false, WALK_SPEED), (true, RUN_SPEED)] {
            for direction in [-1., 1.] {
                let mut player = Pedestrian::new(&city, origin(&city), 0.);
                for _ in 0..360 {
                    player.step(
                        &city,
                        WalkControls {
                            forward: direction,
                            run,
                            ..Default::default()
                        },
                        1. / 120.,
                        |_| false,
                    );
                }
                let start = player.position;
                for _ in 0..120 {
                    player.step(
                        &city,
                        WalkControls {
                            forward: direction,
                            run,
                            ..Default::default()
                        },
                        1. / 120.,
                        |_| false,
                    );
                }
                let distance = (player.position.x - start.x) * direction;
                assert!(
                    (distance - expected).abs() < 0.15,
                    "run={run}, direction={direction}: {distance} m/s"
                );
            }
        }
    }
    #[test]
    fn jump_is_one_and_a_half_metres_and_requires_a_new_press() {
        let city = open_ground();
        for hz in [30, 60, 120] {
            let mut player = Pedestrian::new(&city, origin(&city), 0.);
            let ground = player.position.z;
            let mut peak = ground;
            for _ in 0..hz * 3 {
                player.step(
                    &city,
                    WalkControls {
                        jump: true,
                        ..Default::default()
                    },
                    1. / hz as f32,
                    |_| false,
                );
                peak = peak.max(player.position.z);
                assert!(player.gait.reach_error < 0.04);
            }
            assert!((peak - ground - JUMP_HEIGHT).abs() < 0.01);
            assert_eq!(player.position.z, ground);
            assert!(!player.jump.is_airborne());
            player.step(&city, WalkControls::default(), 1. / 120., |_| false);
            player.step(
                &city,
                WalkControls {
                    jump: true,
                    ..Default::default()
                },
                1. / 120.,
                |_| false,
            );
            assert!(matches!(player.jump, Jump::Preparing { .. }));
            assert_eq!(player.position.z, ground);
        }
    }
    #[test]
    fn jump_preparation_lowers_the_hips_bends_knees_and_extends_before_takeoff() {
        let city = open_ground();
        let model = &pedestrian_catalog::definitions()[MODEL];
        let bend = |worlds: &[glam::Mat4; pedestrian_catalog::JOINT_LIMIT]| {
            let hip = worlds[model.joint("Thigh_L")].w_axis.truncate();
            let knee = worlds[model.joint("Shin_L")].w_axis.truncate();
            let ankle = worlds[model.joint("Foot_L")].w_axis.truncate();
            (hip - knee).dot(ankle - knee) / (hip.distance(knee) * ankle.distance(knee))
        };
        for hz in [30, 60, 120] {
            let mut player = Pedestrian::new(&city, origin(&city), 0.);
            let standing = model.joint_worlds(&player.gait.palette);
            let standing_hips = standing[model.joint("Hips")].w_axis.y;
            let mut lowest = standing_hips;
            let mut last_grounded_hips = standing_hips;
            let mut knee_bend = bend(&standing);
            let mut takeoff = None;
            for frame in 0..hz {
                player.step(
                    &city,
                    WalkControls {
                        jump: true,
                        ..Default::default()
                    },
                    1. / hz as f32,
                    |_| false,
                );
                let worlds = model.joint_worlds(&player.gait.palette);
                if player.jump.is_airborne() {
                    takeoff = Some((frame + 1) as f32 / hz as f32);
                    break;
                }
                assert_eq!(player.position, origin(&city));
                last_grounded_hips = worlds[model.joint("Hips")].w_axis.y;
                lowest = lowest.min(last_grounded_hips);
                knee_bend = knee_bend.max(bend(&worlds));
                for side in ["L", "R"] {
                    let foot = model.joint(&format!("Foot_{side}"));
                    assert!(
                        worlds[foot].w_axis.abs_diff_eq(standing[foot].w_axis, 1e-5),
                        "preparing jump lifted or slid a planted foot"
                    );
                }
            }
            assert!(standing_hips - lowest > 0.20, "crouch was not visible");
            assert!(knee_bend > bend(&standing) + 0.3, "knees did not bend");
            assert!(
                last_grounded_hips > lowest + 0.15,
                "missing extension before takeoff"
            );
            let delay = takeoff.expect("jump preparation never launched");
            assert!((delay - JUMP_CROUCH_TIME - JUMP_EXTENSION_TIME).abs() <= 1. / hz as f32);
        }
    }
    #[test]
    fn moving_and_turning_jumps_direct_the_push_off_using_accepted_travel() {
        let city = open_ground();
        let model = &pedestrian_catalog::definitions()[MODEL];
        let pose = |forward, turn, blocked: bool, hz| {
            let mut player = Pedestrian::new(&city, origin(&city), 0.);
            for _ in 0..hz {
                player.step(
                    &city,
                    WalkControls {
                        forward,
                        run: true,
                        ..Default::default()
                    },
                    1. / hz as f32,
                    |_| blocked,
                );
            }
            for _ in 0..hz / 5 {
                player.step(
                    &city,
                    WalkControls {
                        forward,
                        run: true,
                        turn,
                        jump: true,
                    },
                    1. / hz as f32,
                    |_| blocked,
                );
                assert!(player.gait.reach_error < 0.06);
            }
            assert!(matches!(player.jump, Jump::Preparing { .. }));
            let worlds = model.joint_worlds(&player.gait.palette);
            let feet = (worlds[model.joint("Foot_L")].w_axis.truncate()
                + worlds[model.joint("Foot_R")].w_axis.truncate())
                * 0.5;
            worlds[model.joint("Hips")].w_axis.truncate() - feet
        };
        for hz in [30, 60, 120] {
            let standing = pose(0., 0., false, hz);
            assert!(pose(1., 0., false, hz).z > standing.z + 0.06);
            assert!(pose(-1., 0., false, hz).z < standing.z - 0.06);
            assert!(pose(0., 1., false, hz).x > standing.x + 0.02);
            assert!(pose(0., -1., false, hz).x < standing.x - 0.02);
            assert!(pose(1., 1., false, hz).x > standing.x + 0.035);
            assert!(pose(1., -1., false, hz).x < standing.x - 0.035);
            assert!(
                pose(1., 0., true, hz).abs_diff_eq(standing, 0.01),
                "blocked input leaned the jump into an obstacle"
            );
            assert!(pose(1., 1., false, hz).distance(pose(1., 1., false, 120)) < 0.02);
        }
    }
    #[test]
    fn directional_takeoff_is_continuous_across_yaw_wrap_and_preserves_jump_height() {
        let city = open_ground();
        let model = &pedestrian_catalog::definitions()[MODEL];
        for (forward, turn) in [(1., 1.), (-1., -1.), (0., 1.)] {
            let mut player = Pedestrian::new(&city, origin(&city), std::f32::consts::PI - 0.05);
            let ground = player.position.z;
            let mut peak = ground;
            let mut previous = model.joint_worlds(&player.gait.palette);
            for frame in 0..240 {
                let was_preparing = matches!(player.jump, Jump::Preparing { .. });
                player.step(
                    &city,
                    WalkControls {
                        forward,
                        turn,
                        run: true,
                        jump: true,
                    },
                    1. / 120.,
                    |_| false,
                );
                let worlds = model.joint_worlds(&player.gait.palette);
                if was_preparing && player.jump.is_airborne() {
                    for joint in ["Hips", "Foot_L", "Foot_R", "Chest"] {
                        let id = model.joint(joint);
                        assert!(
                            worlds[id].w_axis.distance(previous[id].w_axis) < 0.035,
                            "takeoff snapped the {joint} pose"
                        );
                    }
                }
                assert!(
                    player.gait.reach_error < 0.06,
                    "unreachable directional pose at {frame}"
                );
                assert!(player.gait.palette.iter().all(|m| m.is_finite()));
                if !matches!(player.jump, Jump::Grounded) {
                    assert!(
                        worlds[model.joint("Foot_L")].w_axis.x
                            > worlds[model.joint("Foot_R")].w_axis.x,
                        "feet crossed during jump at {frame} (forward={forward}, turn={turn})"
                    );
                }
                peak = peak.max(player.position.z);
                previous = worlds;
            }
            assert!((peak - ground - JUMP_HEIGHT).abs() < 0.01);
            assert!(!player.jump.is_airborne());
        }
    }
    #[test]
    fn boosted_sprint_can_turn_stop_and_hit_blockers_without_stretching_legs() {
        let city = open_ground();
        for hz in [30, 60, 120] {
            let mut player = Pedestrian::new(&city, origin(&city), 0.);
            for frame in 0..hz * 4 {
                player.step(
                    &city,
                    WalkControls {
                        forward: 1.,
                        run: true,
                        turn: if frame >= hz * 2 { 1. } else { 0. },
                        ..Default::default()
                    },
                    1. / hz as f32,
                    |_| false,
                );
                assert!(
                    player.gait.reach_error < 0.06,
                    "sprint strained the rig at frame={frame}, hz={hz}: {}",
                    player.gait.reach_error
                );
            }
            for _ in 0..hz {
                player.step(&city, WalkControls::default(), 1. / hz as f32, |_| false);
                assert!(player.gait.reach_error < 0.06);
            }
            let stopped = player.position;
            player.step(&city, WalkControls::default(), 1. / hz as f32, |_| false);
            assert_eq!(player.position, stopped);
            player.yaw = 0.;
            let wall = player.position.x + 2.;
            for _ in 0..hz {
                player.step(
                    &city,
                    WalkControls {
                        forward: 1.,
                        run: true,
                        ..Default::default()
                    },
                    1. / hz as f32,
                    |p| p.x > wall,
                );
                assert!(player.position.x <= wall);
            }
            assert!(
                player.position.x > wall - 0.3,
                "stopped {} before wall {wall}",
                player.position.x
            );
        }
    }
    #[test]
    fn moving_jump_keeps_its_shape_lands_and_respects_blockers() {
        let city = open_ground();
        let mut player = Pedestrian::new(&city, origin(&city), 0.);
        let ground = player.position.z;
        let mut peak = ground;
        for frame in 0..240 {
            player.step(
                &city,
                WalkControls {
                    forward: 1.,
                    run: true,
                    jump: frame == 0 || frame == 50,
                    turn: if frame < 60 { 0.5 } else { 0. },
                },
                1. / 120.,
                |p| p.x > 2.,
            );
            peak = peak.max(player.position.z);
            assert!(player.position.x <= 2.);
            assert!(player.gait.palette.iter().all(|m| m.is_finite()));
            assert!(player.gait.reach_error < 0.06);
        }
        assert!(
            (peak - ground - JUMP_HEIGHT).abs() < 0.01,
            "midair press caused another jump"
        );
        assert_eq!(player.position.z, ground);
        assert!(!player.jump.is_airborne());
        assert!(player.position.truncate().length() > 1.);
    }
}

#[cfg(test)]
#[path = "player_curb_tests.rs"]
mod curb_tests;
