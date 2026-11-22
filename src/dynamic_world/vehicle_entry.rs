//! Player door, entry and exit choreography in the sprung vehicle frame. Distances are metres above the road.
use super::{
    crowd::Instance,
    cycling_pose,
    doors::{self, Door, Opening},
    drivers,
    pedestrian_catalog::{self, JOINT_LIMIT},
    pedestrian_gait::game_to_motion,
    Controls, Pedestrian, Vehicle, WalkControls, METERS_PER_UNIT,
};
use glam::{Mat4, Quat, Vec3, Vec4};

mod bike_steps;

type Pose = [Mat4; JOINT_LIMIT];
enum Mount {
    Cabin {
        door: Door,
        opening: Opening,
        seat: Vec3,
        initial: f32,
        seated: &'static Pose,
    },
    Bicycle {
        standing_frame: Mat4,
    },
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
    start: Pose,
    pose: Pose,
}
pub(super) struct DisplacedOccupant {
    vehicle: usize,
    pub person: Pedestrian,
    exit: Option<Box<Entry>>,
}

impl DisplacedOccupant {
    pub fn new(
        vehicle: usize,
        car: &mut Vehicle,
        person: Pedestrian,
        season: crate::season::Season,
    ) -> Option<Self> {
        let mut exit = Entry::new(car, &person, season, car.nearest_door(person.position))?;
        exit.begin_exit(car);
        Some(Self {
            vehicle,
            person,
            exit: Some(exit),
        })
    }

    pub fn step(
        &mut self,
        vehicles: &mut [Vehicle],
        season: crate::season::Season,
        dt: f32,
    ) -> Option<usize> {
        if let Some(exit) = &mut self.exit {
            let car = &mut vehicles[self.vehicle];
            exit.update_landing(car, &self.person, season);
            if exit.step(car, dt) {
                let model = exit.model;
                self.exit = None;
                return Some(model);
            }
        }
        None
    }

    pub fn instance<'a>(
        &'a self,
        vehicles: &[Vehicle],
        season: crate::season::Season,
    ) -> Instance<'a> {
        match &self.exit {
            Some(exit) => exit.instance(&vehicles[self.vehicle]),
            None => self.person.instance(season),
        }
    }
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
        door: Option<Door>,
    ) -> Option<Box<Self>> {
        let source = person.instance(season);
        let standing_frame = frame(v);
        let relative = standing_frame.inverse() * source.transform;
        let (mount, side) = if v.is_bicycle() {
            (
                Mount::Bicycle { standing_frame },
                if relative.w_axis.x >= 0. { 1. } else { -1. },
            )
        } else {
            let door = door?;
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
            elapsed: 0.,
            action: Action::Enter,
            start,
            pose,
        }))
    }

    pub fn duration(&self) -> f32 {
        match self.mount {
            Mount::Bicycle { .. } => 0.36,
            Mount::Cabin { .. } => 2.,
        }
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
        let progress = (self.elapsed / duration).clamp(0., 1.);
        let t = match self.action {
            Action::Enter => progress,
            Action::Exit => 1. - progress,
            Action::Handle { .. } => progress.min(0.45),
        };
        let (door, opening, seat, initial, seated) = match self.mount {
            Mount::Cabin {
                door,
                opening,
                seat,
                initial,
                seated,
            } => (door, opening, seat, initial, seated),
            Mount::Bicycle { standing_frame } => {
                let relative = frame(v).inverse() * standing_frame;
                let standing = self.start.map(|joint| relative * joint);
                self.pose = bike_steps::pose(v, self.model, &standing, self.side, t);
                return self.elapsed >= duration;
            }
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
        let end = seated.map(|m| offset * m);
        let end_joints = def.joint_worlds(&end);
        let mut locals = def.rest.clone();
        locals[def.node("Spine")].rotation *= Quat::from_rotation_x(0.45 * arc);
        let facing = (-0.45 * s).atan2(0.20) * (1. - phase(t, 0.38, 0.74));
        let root = Mat4::from_translation(game_to_motion(pelvis))
            * Mat4::from_rotation_y(facing)
            * Mat4::from_translation(-hip);
        let mut overrides = vec![None; locals.len()];
        for (sign, leg, arm) in [
            (
                1.,
                ["Thigh_L", "Shin_L", "Foot_L"],
                ["UpperArm_L", "Forearm_L", "Hand_L"],
            ),
            (
                -1.,
                ["Thigh_R", "Shin_R", "Foot_R"],
                ["UpperArm_R", "Forearm_R", "Hand_R"],
            ),
        ] {
            let first = sign == -s;
            let foot_start = Vec3::new(stance.x, stance.y + 0.13 * sign, 0.09);
            let step = if first {
                phase(t, 0.44, 0.77)
            } else {
                phase(t, 0.60, 0.91)
            };
            let seated = super::pedestrian_gait::motion_to_game(
                end_joints[def.joint(leg[2])].w_axis.truncate(),
            );
            let mut foot = foot_start.lerp(seated, step);
            // The ankle is above the sole. Lift only to sill clearance,
            // hold while the foot crosses, then place it on the floor.
            let peak = opening.sill + 0.09;
            let lift_off = phase(step, 0., 0.25);
            let plant = phase(step, 0.67, 1.);
            foot.z = foot_start.z
                + (peak - foot_start.z) * lift_off
                + (seated.z.min(peak) - peak) * plant;
            let hand = if sign == -s {
                handle
            } else {
                pelvis + Vec3::new(0.21, 0.24 * sign, 0.12)
            };
            for (names, target, pole) in [
                (leg, foot, Vec3::Z),
                (arm, hand, Vec3::new(sign, -0.5, -0.3)),
            ] {
                cycling_pose::limb(
                    def,
                    &locals,
                    root,
                    &mut overrides,
                    names,
                    game_to_motion(target),
                    pole,
                    Quat::IDENTITY,
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
            def.blend_worlds(&staged, &end_joints, phase(t, 0.65, 0.97))
        };
        if matches!(self.action, Action::Handle { .. }) {
            // Release the handle and return weight to the original planted stance.
            let release = phase(progress, 0.45, 0.66);
            self.pose = def.blend_worlds(&def.joint_worlds(&self.pose), &self.start, release);
            return progress >= 0.66;
        }
        self.elapsed >= duration
    }
    pub fn yields_to_input(&self, ride: Controls, walk: WalkControls) -> bool {
        if !matches!(self.mount, Mount::Bicycle { .. }) {
            return false;
        }
        match self.action {
            Action::Enter => {
                ride.throttle != 0. || ride.steering != 0. || ride.brake || ride.handbrake
            }
            Action::Exit => {
                walk.forward != 0.
                    || walk.turn != 0.
                    || walk.jump
                    || walk
                        .aim_move
                        .is_some_and(|movement| movement != glam::Vec2::ZERO)
            }
            Action::Handle { .. } => false,
        }
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
        if matches!(self.mount, Mount::Bicycle { .. }) {
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
        dynamic_world::{catalog, DynamicWorld},
        hermes::Hermes,
    };

    #[test]
    fn theft_animates_the_original_occupant_to_the_far_side() {
        let map = Hermes::test_map_from_source(
            "road cycle: sidewalk bikelane carlane middle carlane bikelane sidewalk; end
             cycle(-2,0,2,0); land(-3,-2,3,-2,3,2,-3,2); player_start(0,0);",
        );
        for bike in [false, true] {
            for right in [false, true] {
                let mut world = DynamicWorld::new(&map);
                let (id, model) = if bike {
                    (0..world.vehicles.len())
                        .find_map(|id| Some((id, world.cyclists.rider_model(id)?)))
                        .unwrap()
                } else {
                    let id = world.traffic.driven_slots().next().unwrap().0;
                    (id, drivers::traffic_model(id, &world.vehicles[id]))
                };
                let car = &world.vehicles[id];
                world.pedestrian = Pedestrian::beside_on_side(
                    map.city(),
                    car,
                    pedestrian_catalog::PLAYER_SUMMER_MODEL,
                    right,
                    |_| false,
                );
                assert!(world.toggle_vehicle(&map));
                assert_eq!(world.player_vehicle.0, id);
                let exit = world.displaced[0].exit.as_ref().unwrap();
                assert_eq!(exit.model, model);
                assert_eq!(exit.on_right(), !right);
                let landing = world.displaced[0]
                    .person
                    .instance(world.season)
                    .transform
                    .w_axis
                    .truncate();
                let seated = exit.pose;
                let duration = exit.duration();
                let input = Controls {
                    throttle: if bike { 1. } else { 0. },
                    ..Default::default()
                };
                for _ in 0..(duration * 30.) as usize {
                    world.step(&map, input, Default::default(), 1. / 60.);
                }
                let exit = world.displaced[0].exit.as_ref().unwrap();
                assert_ne!(exit.pose, seated);
                if let Mount::Cabin { door, .. } = exit.mount {
                    assert!(world.player().doors.fraction(door) > 0.9);
                } else {
                    assert!(world.entry.is_none());
                }
                for _ in 0..(duration * 30.) as usize + 3 {
                    world.step(&map, input, Default::default(), 1. / 60.);
                }
                assert!(world.displaced.is_empty());
                assert!(world.crowd.instances(world.player_position()).any(|actor| {
                    actor.model == model
                        && actor.transform.w_axis.truncate().distance(landing)
                            < 0.5 / METERS_PER_UNIT
                }));
            }
        }
    }

    #[test]
    fn directional_input_takes_over_a_bike_transition_in_one_tick() {
        let map = Hermes::test_map_from_source(
            "pad=concrete(-2,-2,2,-2,2,2,-2,2); elevation(pad,0.1); player_start(0,0,0);",
        );
        let mut world = DynamicWorld::new(&map);
        let bike = catalog::definitions()
            .iter()
            .position(|d| d.is_bicycle())
            .unwrap();
        assert!(world.switch_vehicle(bike));
        assert!(world.toggle_vehicle(&map));
        assert!(world.toggle_vehicle(&map));
        assert!(world.entry.is_some());
        let tick = 1. / 60.;
        world.step(
            &map,
            Controls {
                throttle: 1.,
                ..Default::default()
            },
            Default::default(),
            tick,
        );
        assert!(world.entry.is_none());
        assert_eq!(world.player().sound_controls.throttle, 1.);

        assert!(world.toggle_vehicle(&map));
        world.step(&map, Default::default(), Default::default(), tick);
        assert!(world.entry.is_some());
        let yaw = world.player_yaw();
        world.step(
            &map,
            Default::default(),
            WalkControls {
                turn: 1.,
                ..Default::default()
            },
            tick,
        );
        assert!(world.entry.is_none());
        assert_ne!(world.player_yaw(), yaw);
    }

    #[test]
    fn boarding_from_either_rear_door_uses_the_front_door_on_that_side() {
        let map = Hermes::test_map_from_source(
            "pad=concrete(-2,-2,2,-2,2,2,-2,2); elevation(pad,0.1); player_start(0,0,0);",
        );
        for (rear, front) in [
            (Door::RearLeft, Door::FrontLeft),
            (Door::RearRight, Door::FrontRight),
        ] {
            let mut world = DynamicWorld::new(&map);
            assert!(world.switch_vehicle(0));
            assert!(world.toggle_vehicle(&map));
            let point = world.player().access_point(rear).unwrap();
            let person = world.pedestrian.as_mut().unwrap();
            person.position = point.extend(person.position.z);
            assert!(world.toggle_vehicle(&map));
            assert!(
                matches!(world.entry.as_ref().unwrap().mount, Mount::Cabin { door, .. } if door == front)
            );
        }
    }

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
                let frames = (entry.duration() * 60.).ceil() as i32;
                assert!(!bike || frames < 24);
                for i in 1..=frames + 3 {
                    world.step(
                        &map,
                        Controls {
                            throttle: if bike && world.entry.is_some() {
                                0.
                            } else {
                                1.
                            },
                            ..Default::default()
                        },
                        Default::default(),
                        1. / 60.,
                    );
                    if bike && i == frames {
                        assert!(world.entry.is_none());
                    }
                    let actors: Vec<_> = world
                        .pedestrian_instances()
                        .filter(|a| a.model == pedestrian_catalog::player_model(world.season))
                        .collect();
                    assert_eq!(actors.len(), 1);
                    let actor = &actors[0];
                    let joints = def.joint_worlds(actor.pose.unwrap());
                    assert!(joints.iter().all(|m| m.is_finite()));
                    if bike && i * 2 < frames {
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
                        if bike && i * 2 >= frames {
                            let riding = cycling_pose::pose(def, world.player());
                            let pedal = def.joint_worlds(&riding)[foot].w_axis;
                            assert!(ankle.distance(pedal) < 0.006, "far foot left pedal at {i}");
                        }
                        if bike && last_foot.x * side > 0. && ankle.x * side <= 0. {
                            assert!(
                                (0.48..0.58).contains(&ankle.y)
                                    && (-0.025..0.045).contains(&ankle.z),
                                "step-through frame crossing {ankle:?}"
                            );
                            cleared = true;
                        }
                        if bike && i * 4 > frames && i * 4 < frames * 3 {
                            for leg in [
                                ["Thigh_L", "Shin_L", "Foot_L"],
                                ["Thigh_R", "Shin_R", "Foot_R"],
                            ] {
                                let [hip, knee, ankle] =
                                    leg.map(|name| joints[def.joint(name)].w_axis.truncate());
                                let bend = (hip - knee).angle_between(ankle - knee).to_degrees();
                                assert!(bend > 60., "knee {leg:?} folded to {bend} degrees at frame {i} hip {hip:?} knee {knee:?} ankle {ankle:?} side {side}");
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
                    if bike && frame * 2 > exit_frames {
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
                world.step(
                    &map,
                    Default::default(),
                    Default::default(),
                    if bike { 1. / 60. } else { 0.04 },
                );
                assert!(world.entry.is_none());
                assert!(!world.is_driving());
                assert_eq!(world.player().doors.fraction(door), 0.);
            }
        }
    }
}
