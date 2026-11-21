//! Player door, entry and exit choreography in the sprung vehicle frame. Distances are metres above the road.
use super::{
    crowd::Instance,
    cycling_pose,
    doors::{self, Door, Opening},
    drivers,
    pedestrian_catalog::{self, JOINT_LIMIT},
    pedestrian_gait::game_to_motion,
    Pedestrian, Vehicle, METERS_PER_UNIT,
};
use glam::{Mat4, Quat, Vec3, Vec4};

mod bike_steps;
use bike_steps::BikeSteps;

type Pose = [Mat4; JOINT_LIMIT];
enum Mount {
    Cabin {
        door: Door,
        opening: Opening,
        seat: Vec3,
        initial: f32,
        seated: &'static Pose,
    },
    Bicycle,
}
#[derive(Clone, Copy)]
enum Action {
    Enter,
    Exit,
    Handle { vehicle: usize, open: bool },
}
pub(super) struct Entry {
    mount: Mount,
    side: f32,
    model: usize,
    elapsed: f32,
    action: Action,
    standing_position: Vec3,
    steps: Option<BikeSteps>,
    start: Pose,
    pose: Pose,
}
fn phase(t: f32, from: f32, to: f32) -> f32 {
    let u = ((t - from) / (to - from)).clamp(0., 1.);
    u * u * (3. - 2. * u)
}
fn frame(v: &Vehicle) -> Mat4 {
    let rotation = if v.is_bicycle() {
        v.orientation()
    } else {
        v.orientation() * v.cabin_lean()
    };
    Mat4::from_scale_rotation_translation(
        Vec3::splat(1. / METERS_PER_UNIT),
        rotation,
        v.position / METERS_PER_UNIT,
    ) * Mat4::from_translation(-Vec3::Z * v.ride_height())
        * Mat4::from_cols(Vec4::Y, Vec4::Z, Vec4::X, Vec4::W)
}
impl Entry {
    pub fn new(
        v: &Vehicle,
        person: &Pedestrian,
        season: crate::season::Season,
    ) -> Option<Box<Self>> {
        let source = person.instance(season);
        let relative = frame(v).inverse() * source.transform;
        let (mount, side) = if v.is_bicycle() {
            (
                Mount::Bicycle,
                if relative.w_axis.x >= 0. { 1. } else { -1. },
            )
        } else {
            let door = v.nearest_door(person.position)?;
            let id = &v.definition()?.config.research_id;
            (
                Mount::Cabin {
                    door,
                    opening: doors::opening(id, door)?,
                    seat: drivers::seat(id)?,
                    initial: v.doors.fraction(door),
                    seated: drivers::seat_pose(id, source.model),
                },
                door.side(),
            )
        };
        let pose = source.pose?.map(|m| relative * m);
        let start = pedestrian_catalog::definitions()[source.model].joint_worlds(&pose);
        Some(Box::new(Self {
            mount,
            side,
            model: source.model,
            standing_position: person.position,
            steps: None,
            elapsed: 0.,
            action: Action::Enter,
            start,
            pose,
        }))
    }

    pub fn boarding(
        v: &Vehicle,
        person: &Pedestrian,
        season: crate::season::Season,
        city: &opencity::runtime::City,
    ) -> Option<Box<Self>> {
        let mut entry = Self::new(v, person, season)?;
        if v.is_bicycle() {
            entry.steps = Some(BikeSteps::plan(
                v,
                person,
                entry.side,
                entry.model,
                entry.start,
                city,
            ));
        }
        Some(entry)
    }
    pub fn duration(&self) -> f32 {
        2. + self.steps.as_ref().map_or(0., BikeSteps::duration)
    }

    pub fn instance(&self, v: &Vehicle) -> Instance<'_> {
        Instance {
            zombie: false,
            model: self.model,
            transform: frame(v),
            pose: Some(&self.pose),
            opacity: 1.,
            idle_time: 0.,
            walk_time: 0.,
            movement: 0.,
        }
    }

    pub fn step(&mut self, v: &mut Vehicle, dt: f32) -> bool {
        self.elapsed += dt;
        let duration = self.duration();
        let walk = duration - 2.;
        if let Some(steps) = &self.steps {
            let at = if matches!(self.action, Action::Exit) {
                duration - self.elapsed
            } else {
                self.elapsed
            };
            if at <= walk {
                self.pose = steps.pose(v, self.model, at);
                return self.elapsed >= duration;
            }
            self.start = steps.planted_pose(v);
        }
        let mount_elapsed = if matches!(self.action, Action::Enter) {
            self.elapsed - walk
        } else {
            self.elapsed
        };
        let progress = (mount_elapsed / 2.).clamp(0., 1.);
        let t = match self.action {
            Action::Enter => progress,
            Action::Exit => 1. - progress,
            Action::Handle { .. } => progress.min(0.45),
        };
        let s = self.side;
        let transfer = if matches!(self.action, Action::Handle { .. }) {
            0.
        } else {
            phase(t, 0.45, 0.77)
        };
        let arc = (std::f32::consts::PI * phase(t, 0.39, 0.86)).sin();
        let def = &pedestrian_catalog::definitions()[self.model];
        let rest = def.rest_worlds();
        let hip = rest[def.node("Hips")].w_axis.truncate();
        let (stance, pelvis, handle, end) = match self.mount {
            Mount::Cabin {
                door,
                opening,
                seat,
                initial,
                seated,
            } => {
                let open = phase(t, 0.23, 0.45);
                let close = phase(t, 0.79, 0.99);
                let fraction = match self.action {
                    Action::Exit => open * (1. - close + close * initial),
                    Action::Enter => (initial + (1. - initial) * open) * (1. - close),
                    Action::Handle { open: opening, .. } => {
                        initial + (f32::from(u8::from(opening)) - initial) * open
                    }
                };
                v.doors.set_fraction(door, fraction);
                let x = opening.rear + (opening.front - opening.rear) * 0.16;
                let handle = opening
                    .transform(door, fraction, -1.)
                    .transform_point3(Vec3::new(
                        x,
                        opening.hinge_y * s,
                        opening.sill + (opening.top - opening.sill) * 0.55,
                    ));
                // Stay behind the sweeping leaf until it clears the sill.
                let stance = Vec3::new(x - 0.20, (opening.hinge_y + 0.45) * s, hip.y);
                let sill = Vec3::new(x, seat.y.abs() * s, seat.z);
                let mut pelvis = stance.lerp(sill, transfer).lerp(seat, phase(t, 0.71, 0.96));
                if !matches!(self.action, Action::Handle { .. }) {
                    // Sit back as the door clears, before lifting the inside foot.
                    pelvis.z = stance.z + (seat.z - stance.z) * phase(t, 0.36, 0.67);
                }
                let offset = Mat4::from_translation(game_to_motion(seat));
                (stance, pelvis, handle, seated.map(|m| offset * m))
            }
            Mount::Bicycle => {
                // Stand in the opening ahead of the saddle until the far foot is across.
                let stance = Vec3::new(-0.31, 0.33 * s, hip.y);
                let across = phase(t, 0.36, 0.76);
                let straddle = Vec3::new(-0.17, 0., 1.045);
                let settle = phase(t, 0.72, 0.96);
                let mut pelvis = stance
                    .lerp(straddle, across)
                    .lerp(Vec3::new(-0.355, 0., 1.), settle);
                pelvis.z = hip.y
                    + 0.045 * phase(t, 0.20, 0.40)
                    + (straddle.z - hip.y - 0.045) * phase(t, 0.58, 0.76)
                    - (straddle.z - 1.) * settle;
                (stance, pelvis, Vec3::ZERO, cycling_pose::pose(def, v))
            }
        };
        let end_joints = def.joint_worlds(&end);
        let bike = matches!(self.mount, Mount::Bicycle);
        let mut locals = def.rest.clone();
        locals[def.node("Spine")].rotation *= Quat::from_rotation_x(if bike {
            0.28 + 0.40 * phase(t, 0.65, 0.94)
        } else {
            0.45 * arc
        });
        // Aim the hips and shoulders at the latch before pulling. Keep the IK targets
        // in vehicle space so the hand follows the leaf as the body turns into the seat.
        let toward_handle = match self.mount {
            Mount::Cabin { .. } => (-0.45 * s).atan2(0.20),
            Mount::Bicycle => 0.,
        };
        let facing = if bike {
            -0.62 * s * (1. - phase(t, 0.30, 0.76))
        } else {
            toward_handle * (1. - phase(t, 0.38, 0.74))
        };
        let root = Mat4::from_translation(game_to_motion(pelvis))
            * Mat4::from_rotation_y(facing)
            * Mat4::from_translation(-hip);
        let mut overrides = vec![None; locals.len()];
        let mut contacts = [(Vec3::ZERO, Quat::IDENTITY); 2];
        for (i, sign, leg, arm) in [
            (
                0,
                1.,
                ["Thigh_L", "Shin_L", "Foot_L"],
                ["UpperArm_L", "Forearm_L", "Hand_L"],
            ),
            (
                1,
                -1.,
                ["Thigh_R", "Shin_R", "Foot_R"],
                ["UpperArm_R", "Forearm_R", "Hand_R"],
            ),
        ] {
            let first = sign == -s;
            let ankle = def.joint(leg[2]);
            let planted = self.start[ankle];
            let foot_start = if bike {
                super::pedestrian_gait::motion_to_game(planted.w_axis.truncate())
            } else {
                Vec3::new(stance.x, stance.y + 0.13 * sign, 0.09)
            };
            let step = if first {
                phase(t, 0.44, 0.77)
            } else {
                phase(t, 0.60, 0.91)
            };
            let foot = match self.mount {
                Mount::Bicycle => {
                    let pedal = super::pedestrian_gait::motion_to_game(
                        end_joints[def.joint(leg[2])].w_axis.truncate(),
                    );
                    if first {
                        // Clear the low tube near the crank with a level foot. Cross
                        // at this height before lowering onto the far pedal.
                        let lifted = Vec3::new(0.008, foot_start.y, 0.50);
                        let crossed = Vec3::new(lifted.x, -0.19 * s, lifted.z);
                        foot_start
                            .lerp(lifted, phase(t, 0.19, 0.43))
                            .lerp(crossed, phase(t, 0.40, 0.66))
                            .lerp(pedal, phase(t, 0.64, 0.83))
                    } else {
                        foot_start.lerp(pedal, phase(t, 0.62, 0.89))
                    }
                }
                Mount::Cabin { opening, .. } => {
                    let seated = super::pedestrian_gait::motion_to_game(
                        end_joints[def.joint(leg[2])].w_axis.truncate(),
                    );
                    let mut ankle = foot_start.lerp(seated, step);
                    // The ankle is above the sole. Lift only to sill clearance,
                    // hold while the foot crosses, then place it on the floor.
                    let peak = opening.sill + 0.09;
                    let lift_off = phase(step, 0., 0.25);
                    let plant = phase(step, 0.67, 1.);
                    ankle.z = foot_start.z
                        + (peak - foot_start.z) * lift_off
                        + (seated.z.min(peak) - peak) * plant;
                    ankle
                }
            };
            let hand = if bike {
                cycling_pose::grip_position(i, v.steering)
            } else if sign == -s {
                handle
            } else {
                pelvis + Vec3::new(0.21, 0.24 * sign, 0.12)
            };
            let foot_yaw = if bike && first {
                -s * 0.85 * phase(t, 0.18, 0.39) * (1. - phase(t, 0.64, 0.83))
            } else {
                0.
            };
            if bike {
                let lift = if first {
                    phase(t, 0.19, 0.40)
                } else {
                    phase(t, 0.62, 0.83)
                };
                let pedal = if first {
                    phase(t, 0.64, 0.83)
                } else {
                    phase(t, 0.62, 0.89)
                };
                let q = planted
                    .to_scale_rotation_translation()
                    .1
                    .slerp(Quat::from_rotation_y(foot_yaw), lift)
                    .slerp(end_joints[ankle].to_scale_rotation_translation().1, pedal);
                contacts[i] = (game_to_motion(foot), q);
            }
            for (names, target, pole, rotation) in [
                (
                    leg,
                    foot,
                    if bike {
                        Quat::from_rotation_y(facing) * Vec3::Z
                    } else {
                        Vec3::Z
                    },
                    Quat::from_rotation_y(foot_yaw),
                ),
                (arm, hand, Vec3::new(sign, -0.5, -0.3), Quat::IDENTITY),
            ] {
                cycling_pose::limb(
                    def,
                    &locals,
                    root,
                    &mut overrides,
                    names,
                    game_to_motion(target),
                    pole,
                    rotation,
                );
            }
        }
        let worlds = def.worlds(&locals, root, &overrides);
        let mut staged = [Mat4::IDENTITY; JOINT_LIMIT];
        for (i, &node) in def.joints.iter().enumerate() {
            staged[i] = worlds[node];
        }
        self.pose = if t <= 0.23 {
            let hips = def.joint("Hips");
            let pivot = self.start[hips].w_axis.truncate();
            let heading = self.start[hips].z_axis;
            let from = heading.x.atan2(heading.z);
            let delta = (facing - from).sin().atan2((facing - from).cos());
            let turn = Mat4::from_translation(pivot)
                * Mat4::from_rotation_y(delta * phase(t, 0., 0.18))
                * Mat4::from_translation(-pivot);
            let turned = self.start.map(|joint| turn * joint);
            def.blend_worlds(&turned, &staged, phase(t, 0.06, 0.23))
        } else {
            def.blend_worlds(
                &staged,
                &end_joints,
                phase(t, if bike { 0.76 } else { 0.65 }, 0.97),
            )
        };
        if bike && t > 0. && t < 0.97 {
            bike_steps::plant(def, &mut self.pose, contacts);
        }
        if matches!(self.action, Action::Handle { .. }) {
            // Release the handle and return weight to the original planted stance.
            let release = phase(progress, 0.45, 0.66);
            self.pose = def.blend_worlds(&def.joint_worlds(&self.pose), &self.start, release);
            return progress >= 0.66;
        }
        self.elapsed >= duration
    }
    pub fn begin_exit(&mut self, vehicle: &mut Vehicle) {
        self.action = Action::Exit;
        self.step(vehicle, 0.);
    }
    pub fn operate_handle(&mut self, vehicle: usize, open: bool) {
        self.action = Action::Handle { vehicle, open };
    }
    pub fn vehicle(&self, controlled: usize) -> usize {
        match self.action {
            Action::Handle { vehicle, .. } => vehicle,
            _ => controlled,
        }
    }
    pub fn on_right(&self) -> bool {
        self.side < 0.
    }
    pub fn lost_landing(&self, person: &Pedestrian) -> bool {
        person.fallen.is_some()
            || person.position.distance_squared(self.standing_position) > 0.25 * 0.25
    }

    pub fn update_landing(
        &mut self,
        car: &Vehicle,
        person: &Pedestrian,
        season: crate::season::Season,
    ) {
        if self.steps.is_some() {
            return;
        }
        let actor = person.instance(season);
        let transform = frame(car).inverse() * actor.transform;
        self.start = pedestrian_catalog::definitions()[self.model]
            .joint_worlds(&actor.pose.unwrap().map(|joint| transform * joint));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        dynamic_world::{catalog, Controls, DynamicWorld},
        hermes::Hermes,
    };

    #[test]
    fn entry_and_exit_on_either_side_preserve_clearance_and_handoff() {
        let map = Hermes::test_map_from_source(
            "pad=concrete(-2,-2,2,-2,2,2,-2,2); elevation(pad,0.1); player_start(0,0,0);",
        );
        for bike in [false, true] {
            for (side, door) in [(1., Door::FrontLeft), (-1., Door::FrontRight)] {
                let mut world = DynamicWorld::new(&map);
                let model = catalog::definitions()
                    .iter()
                    .position(|d| {
                        if bike {
                            d.is_bicycle()
                        } else {
                            d.config.research_id == "vehicle-v2-09"
                        }
                    })
                    .unwrap();
                assert!(world.switch_vehicle(model));
                assert!(world.toggle_vehicle(&map));
                let car = world.player();
                let left = glam::Vec2::from_angle(car.yaw).perp();
                let pos = car
                    .access_point(door)
                    .unwrap_or(car.position.truncate() + left * side * 0.72);
                let p = world.pedestrian.as_mut().unwrap();
                p.position = pos.extend(p.position.z);
                assert!(world.toggle_vehicle(&map));
                let entry = world.entry.as_ref().unwrap();
                assert_eq!(entry.side, side);
                let def = &pedestrian_catalog::definitions()[entry.model];
                let foot = def.joint(if side > 0. { "Foot_R" } else { "Foot_L" });
                let mut last_foot = entry.start[foot].w_axis;
                let support = def.joint(if side > 0. { "Foot_L" } else { "Foot_R" });
                let mut planted_sole = None;
                let mut cleared = false;
                let mut previous = None;
                let walk_frames = ((entry.duration() - 2.) * 60.).round() as i32;
                for frame in 1..=walk_frames + 123 {
                    let i = frame - walk_frames;
                    world.step(
                        &map,
                        Controls {
                            throttle: 1.,
                            ..Default::default()
                        },
                        Default::default(),
                        1. / 60.,
                    );
                    let actors: Vec<_> = world
                        .pedestrian_instances()
                        .filter(|a| a.model == pedestrian_catalog::player_model(world.season))
                        .collect();
                    assert_eq!(actors.len(), 1);
                    let actor = &actors[0];
                    let joints = def.joint_worlds(actor.pose.unwrap());
                    assert!(joints.iter().all(|m| m.is_finite()));
                    if bike && (1..=73).contains(&i) {
                        let sole = actor.transform.transform_point3(
                            joints[support].transform_point3(Vec3::new(0., -0.09, 0.03)),
                        ) * METERS_PER_UNIT;
                        let anchor = *planted_sole.get_or_insert(sole);
                        assert!(
                            sole.distance(anchor) < 0.006,
                            "support shoe slid at frame {i}: {}",
                            sole.distance(anchor)
                        );
                    }
                    if let Some(entry) = &world.entry {
                        assert_eq!(world.player().sound_controls.throttle, 0.);
                        let ankle = def.joint_worlds(&entry.pose)[foot].w_axis;
                        if bike && last_foot.x * side > 0. && ankle.x * side <= 0. {
                            assert!(
                                (0.48..0.58).contains(&ankle.y)
                                    && (-0.025..0.045).contains(&ankle.z),
                                "step-through frame crossing {ankle:?}"
                            );
                            cleared = true;
                        }
                        if bike && (28..90).contains(&i) {
                            for leg in [
                                ["Thigh_L", "Shin_L", "Foot_L"],
                                ["Thigh_R", "Shin_R", "Foot_R"],
                            ] {
                                let [hip, knee, ankle] =
                                    leg.map(|name| joints[def.joint(name)].w_axis.truncate());
                                let bend = (hip - knee).angle_between(ankle - knee).to_degrees();
                                assert!(bend > 60., "knee {leg:?} folded to {bend} degrees at frame {i} hip {hip:?} knee {knee:?} ankle {ankle:?} side {side} step {walk_frames}");
                            }
                        }
                        if !bike && (55..90).contains(&i) {
                            assert!(world.player().doors.fraction(door) > 0.99);
                        }
                        if !bike {
                            if let Mount::Cabin { opening, .. } = entry.mount {
                                for leg in ["Foot_L", "Foot_R"] {
                                    let height = joints[def.joint(leg)].w_axis.y;
                                    if (52..78).contains(&i) {
                                        assert!(
                                            height < opening.sill + 0.14,
                                            "{leg} above compact sill at {i}: {height}"
                                        );
                                    }
                                }
                            }
                            if i == 27 {
                                let forward = joints[def.joint("Hips")].z_axis;
                                assert!(
                                    forward.x * side < -0.72,
                                    "hips did not face latch: {forward:?}"
                                );
                            }
                        }
                        last_foot = ankle;
                    }
                    let hip = actor
                        .transform
                        .transform_point3(joints[def.joint("Hips")].w_axis.truncate())
                        * METERS_PER_UNIT;
                    if let Some(last) = previous {
                        assert!(hip.distance(last) < 0.22, "hip jumped at {i}");
                    }
                    previous = Some(hip);
                }
                assert!(world.entry.is_none());
                assert!(cleared || !bike);
                assert_eq!(world.player().doors.fraction(door), 0.);
                assert_eq!(world.player().sound_controls.throttle, 1.);
                assert!(world.toggle_vehicle(&map));
                assert_eq!(world.entry.as_ref().unwrap().side, side);
                let landing = world.pedestrian.as_ref().unwrap().position;
                let exit_frames = (world.entry.as_ref().unwrap().duration() * 60.).round() as usize;
                let mut exit_sole = None;
                for frame in 1..exit_frames {
                    world.step(&map, Default::default(), Default::default(), 1. / 60.);
                    assert!(world.entry.is_some());
                    assert_eq!(world.pedestrian.as_ref().unwrap().position, landing);
                    if bike && (47..120).contains(&frame) {
                        let entry = world.entry.as_ref().unwrap();
                        let actor = entry.instance(world.player());
                        let joints = def.joint_worlds(actor.pose.unwrap());
                        let sole = actor.transform.transform_point3(
                            joints[support].transform_point3(Vec3::new(0., -0.09, 0.03)),
                        ) * METERS_PER_UNIT;
                        let anchor = *exit_sole.get_or_insert(sole);
                        assert!(
                            sole.distance(anchor) < 0.006,
                            "support shoe slid during dismount at frame {frame}"
                        );
                    }
                    assert_eq!(
                        world
                            .pedestrian_instances()
                            .filter(|a| a.model == pedestrian_catalog::player_model(world.season))
                            .count(),
                        1
                    );
                }
                world.step(&map, Default::default(), Default::default(), 0.04);
                assert!(world.entry.is_none());
                assert!(!world.is_driving());
                assert_eq!(world.player().doors.fraction(door), 0.);
            }
        }
    }
}
