//! Run the same grounded steps forward to the bicycle or backward to the checked exit position.
use super::{frame, pedestrian_catalog, Pedestrian, Pose, Vehicle};
use crate::dynamic_world::{
    pedestrian_gait::{footing, solve_two_bone},
    player::HALF_HEIGHT,
};
use glam::{Mat4, Quat, Vec2, Vec3};
use opencity::runtime::City;

pub(super) struct BikeSteps {
    basis: Mat4,
    joints: Vec<Pose>,
}
impl BikeSteps {
    const DT: f32 = 1. / 30.;

    pub fn plan(
        v: &Vehicle,
        person: &Pedestrian,
        side: f32,
        model: usize,
        start: Pose,
        city: &City,
    ) -> Self {
        let basis = frame(v);
        let def = &pedestrian_catalog::definitions()[model];
        let mut gait = person.gait.clone();
        let mut root = person.position - Vec3::Z * HALF_HEIGHT;
        let mut yaw = person.yaw;
        let forward = Vec2::from_angle(v.yaw);
        let target = v.position.truncate() - forward * 0.31 + forward.perp() * (0.33 * side);
        let heading = v.yaw - 0.62 * side;
        // A scripted relocation may have moved the pedestrian since its last gait tick.
        let source = gait.foot_motion();
        gait.carry(source.root, root, yaw - source.yaw);
        let mut joints = vec![start];
        for _ in 0..120 {
            let delta = target - root.truncate();
            let travel = (delta * 5.).clamp_length_max(1.65) * Self::DT;
            let turn = (heading - yaw).sin().atan2((heading - yaw).cos());
            yaw += turn.clamp(-3.2 * Self::DT, 3.2 * Self::DT);
            let next = root + travel.extend(0.);
            if gait.permits(next) {
                root = footing(city, next, false).map_or(next, |ground| ground.point);
            }
            let velocity = travel / Self::DT;
            // Bound the next footfall to the remaining approach distance.
            gait.set_strafe_velocity(Some(
                velocity.clamp_length_max(delta.length() / (0.75 * 1.45)),
            ));
            gait.set_running(false, Self::DT);
            gait.advance_coarse(
                def,
                root,
                yaw,
                velocity.length(),
                target.extend(root.z),
                Self::DT,
                |p| footing(city, p, false),
            );
            let relative = basis.inverse() * pedestrian_catalog::instance_transform(root, yaw);
            joints.push(
                def.joint_worlds(&gait.palette)
                    .map(|joint| relative * joint),
            );
            if delta.length_squared() < 0.0001 && turn.abs() < 0.01 && gait.both_feet_settled() {
                break;
            }
        }
        Self { basis, joints }
    }
    pub fn duration(&self) -> f32 {
        (self.joints.len() - 1) as f32 * Self::DT
    }
    pub fn planted_pose(&self, v: &Vehicle) -> Pose {
        let relative = frame(v).inverse() * self.basis;
        self.joints.last().unwrap().map(|joint| relative * joint)
    }
    pub fn pose(&self, v: &Vehicle, model: usize, time: f32) -> Pose {
        let sample = (time / Self::DT).clamp(0., (self.joints.len() - 1) as f32);
        let i = sample as usize;
        let a = &self.joints[i];
        let b = &self.joints[(i + 1).min(self.joints.len() - 1)];
        let def = &pedestrian_catalog::definitions()[model];
        let relative = frame(v).inverse() * self.basis;
        def.blend_worlds(a, b, sample.fract())
            .map(|joint| relative * joint)
    }
}

// Apply contacts after the torso blend. Otherwise rotating the hips also moves the soles.
pub(super) fn plant(
    def: &pedestrian_catalog::Definition,
    pose: &mut Pose,
    contacts: [(Vec3, Quat); 2],
) {
    let mut joints = def.joint_worlds(pose);
    let legs = [
        ["Thigh_L", "Shin_L", "Foot_L", "Toe_L"],
        ["Thigh_R", "Shin_R", "Foot_R", "Toe_R"],
    ]
    .map(|names| names.map(|name| def.joint(name)));
    let mut drop: f32 = 0.;
    for ([thigh, shin, foot, _], (ankle, _)) in legs.into_iter().zip(contacts) {
        let hip = joints[thigh].w_axis.truncate();
        let knee = joints[shin].w_axis.truncate();
        let reach = hip.distance(knee) + knee.distance(joints[foot].w_axis.truncate()) - 0.001;
        let d = ankle - hip;
        drop = drop.max(hip.y - ankle.y - (reach * reach - d.x * d.x - d.z * d.z).max(0.).sqrt());
    }
    for joint in &mut joints {
        joint.w_axis.y -= drop;
    }
    for ([thigh, shin, foot, toe], (target, rot)) in legs.into_iter().zip(contacts) {
        let hip = joints[thigh].w_axis.truncate();
        let knee = joints[shin].w_axis.truncate();
        let ankle = joints[foot].w_axis.truncate();
        let toe_local = joints[foot].inverse() * joints[toe];
        let (bend, tip) = solve_two_bone(
            hip,
            target,
            knee - hip,
            hip.distance(knee),
            knee.distance(ankle),
        );
        for (i, from, to, before) in [
            (thigh, hip, bend, knee - hip),
            (shin, bend, tip, ankle - knee),
        ] {
            let (scale, q, _) = joints[i].to_scale_rotation_translation();
            let dir = to - from;
            joints[i] = Mat4::from_scale_rotation_translation(
                scale,
                Quat::from_rotation_arc(
                    before / before.length().max(1e-6),
                    dir / dir.length().max(1e-6),
                ) * q,
                from,
            );
        }
        joints[foot] = Mat4::from_rotation_translation(rot, tip);
        joints[toe] = joints[foot] * toe_local;
    }
    *pose = def.skin_from_worlds(&joints);
}
