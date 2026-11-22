//! Two foot placements from the standing pose to the pedals, played backward on exit.
use super::{cycling_pose, pedestrian_catalog, phase, Pose, Vehicle};
use crate::dynamic_world::pedestrian_gait::solve_two_bone;
use glam::{Mat4, Quat, Vec3};

pub(super) fn pose(v: &Vehicle, model: usize, standing: &Pose, side: f32, t: f32) -> Pose {
    let def = &pedestrian_catalog::definitions()[model];
    let riding = cycling_pose::pose(def, v);
    if t >= 1. {
        return riding;
    }
    if t <= 0. {
        return def.skin_from_worlds(standing);
    }
    let pedals = def.joint_worlds(&riding);
    let mut pose = def.blend_worlds(standing, &pedals, phase(t, 0.14, 1.));
    let contacts = [("Foot_L", 1.), ("Foot_R", -1.)].map(|(name, sign)| {
        let ankle = def.joint(name);
        let (_, from_rotation, from) = standing[ankle].to_scale_rotation_translation();
        let (_, to_rotation, to) = pedals[ankle].to_scale_rotation_translation();
        let far = sign == -side;
        let step = if far {
            (2. * t).min(1.)
        } else {
            (2. * t - 1.).max(0.)
        };
        let position = if far {
            // Lift, cross the low frame by the crank, then land on the far pedal.
            let raised = Vec3::new(from.x, 0.50, 0.008);
            let across = Vec3::new(-0.19 * side, raised.y, raised.z);
            from.lerp(raised, phase(step, 0., 0.35))
                .lerp(across, phase(step, 0.35, 0.70))
                .lerp(to, phase(step, 0.70, 1.))
        } else {
            from.lerp(to, phase(step, 0., 1.))
        };
        (
            position,
            from_rotation.slerp(to_rotation, phase(step, 0., 1.)),
        )
    });
    plant(def, &mut pose, contacts);
    pose
}

// Apply contacts after the torso blend. Otherwise rotating the hips also moves the soles.
fn plant(def: &pedestrian_catalog::Definition, pose: &mut Pose, contacts: [(Vec3, Quat); 2]) {
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
