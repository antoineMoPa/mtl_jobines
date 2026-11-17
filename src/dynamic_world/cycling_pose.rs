//! Bicycle contact targets and two-bone inverse kinematics in rig motion space.
use super::{
    pedestrian_catalog::{Definition, JOINT_LIMIT},
    pedestrian_gait::game_to_motion,
    Vehicle,
};
use glam::{Mat4, Quat, Vec3};

pub(super) const CRANK: Vec3 = Vec3::new(-0.18, 0., 0.31);
pub(super) fn pedal_position(side: usize, angle: f32) -> Vec3 {
    let sign = if side == 0 { -1. } else { 1. };
    CRANK
        + Quat::from_rotation_y(angle) * Vec3::new(sign * 0.10, 0., -sign * 0.12)
        + Vec3::Y * (-sign * 0.175)
}
pub(super) fn grip_position(side: usize, steering: f32) -> Vec3 {
    let pivot = Vec3::new(0.35, 0., 0.90);
    let grip = Vec3::new(0.17, if side == 0 { 0.255 } else { -0.255 }, 1.12);
    pivot + Quat::from_rotation_z(steering) * (grip - pivot)
}
fn solve(a: Vec3, target: Vec3, pole: Vec3, upper: f32, lower: f32) -> (Vec3, Vec3) {
    let delta = target - a;
    let direction = delta / delta.length().max(1e-6);
    let distance = delta
        .length()
        .clamp((upper - lower).abs() + 0.0001, upper + lower - 0.0001);
    let along = (upper * upper - lower * lower + distance * distance) / (2. * distance);
    let height = (upper * upper - along * along).max(0.).sqrt();
    let bend = pole - direction * pole.dot(direction);
    (
        a + direction * along + bend / bend.length().max(1e-6) * height,
        a + direction * distance,
    )
}
fn limb(
    def: &Definition,
    locals: &[super::pedestrian_catalog::Transform],
    root: Mat4,
    overrides: &mut [Option<Mat4>],
    names: [&str; 3],
    target: Vec3,
    pole: Vec3,
    end_rotation: Quat,
) {
    let nodes = names.map(|n| def.node(n));
    let worlds = def.worlds(locals, root, overrides);
    let a = worlds[nodes[0]].transform_point3(Vec3::ZERO);
    let (b, c) = solve(
        a,
        target,
        pole,
        def.rest[nodes[1]].translation.length(),
        def.rest[nodes[2]].translation.length(),
    );
    for (bone, child, from, to) in [(nodes[0], nodes[1], a, b), (nodes[1], nodes[2], b, c)] {
        let rest = def.rest[child].translation;
        let delta = to - from;
        overrides[bone] = Some(Mat4::from_rotation_translation(
            Quat::from_rotation_arc(rest / rest.length(), delta / delta.length().max(1e-6)),
            from,
        ));
    }
    overrides[nodes[2]] = Some(Mat4::from_rotation_translation(end_rotation, c));
}
pub(super) fn pose(def: &Definition, bike: &Vehicle) -> [Mat4; JOINT_LIMIT] {
    let mut locals = def.rest.clone();
    let standing = bike.riding_stand.clamp(0., 1.);
    locals[def.node("Spine")].rotation *= Quat::from_rotation_x(0.85);
    let rest = def.rest_worlds();
    let hip = rest[def.node("Hips")].transform_point3(Vec3::ZERO);
    let mut pelvis = Vec3::new(
        bike.pedal_angle.sin() * 0.015 * standing,
        1. + 0.14 * standing,
        -0.355 + 0.20 * standing,
    );
    if standing > 0. {
        // Limit the rise by each leg's actual reach at the current pedal phase.
        // This keeps soles attached while the rider gets out of the saddle.
        for (side, label) in ["L", "R"].into_iter().enumerate() {
            let thigh = def.node(&format!("Thigh_{label}"));
            let shin = def.node(&format!("Shin_{label}"));
            let foot = def.node(&format!("Foot_{label}"));
            let ankle = rest[foot].w_axis.truncate();
            let target = game_to_motion(pedal_position(side, bike.pedal_angle)) + Vec3::Y * 0.014
                - Vec3::new(0., -ankle.y, 0.065);
            let offset = rest[thigh].w_axis.truncate() - hip;
            let delta = target - (pelvis + offset);
            let reach =
                def.rest[shin].translation.length() + def.rest[foot].translation.length() - 0.015;
            let horizontal = delta.x * delta.x + delta.z * delta.z;
            pelvis.y = pelvis
                .y
                .min(target.y + (reach * reach - horizontal).max(0.).sqrt() - offset.y);
        }
    }
    let root = Mat4::from_translation(pelvis - hip);
    let mut overrides = vec![None; locals.len()];
    for (side, label) in ["L", "R"].into_iter().enumerate() {
        let foot = def.node(&format!("Foot_{label}"));
        let (_, rotation, ankle) = rest[foot].to_scale_rotation_translation();
        // Contact under the ball of the foot; keep the sole level on each pedal.
        let sole_offset = Vec3::new(0., -ankle.y, 0.065);
        let target =
            game_to_motion(pedal_position(side, bike.pedal_angle)) + Vec3::Y * 0.014 - sole_offset;
        limb(
            def,
            &locals,
            root,
            &mut overrides,
            [
                &format!("Thigh_{label}"),
                &format!("Shin_{label}"),
                &format!("Foot_{label}"),
            ],
            target,
            Vec3::Z,
            rotation,
        );
        let wrist =
            game_to_motion(grip_position(side, bike.steering)) + Vec3::new(0., 0.025, -0.035);
        let hand = def.node(&format!("Hand_{label}"));
        let (_, rest_rotation, _) = rest[hand].to_scale_rotation_translation();
        limb(
            def,
            &locals,
            root,
            &mut overrides,
            [
                &format!("UpperArm_{label}"),
                &format!("Forearm_{label}"),
                &format!("Hand_{label}"),
            ],
            wrist,
            Vec3::new(if side == 0 { 1. } else { -1. }, -0.5, -0.3),
            Quat::from_rotation_y(bike.steering) * rest_rotation,
        );
    }
    let worlds = def.worlds(&locals, root, &overrides);
    let mut skin = [Mat4::IDENTITY; JOINT_LIMIT];
    for (i, &node) in def.joints.iter().enumerate() {
        skin[i] = worlds[node] * def.inverse_bind[i];
    }
    skin
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{pedestrian_catalog, DynamicWorld};
    use crate::hermes::Hermes;
    #[test]
    fn bicycle_ik_keeps_feet_on_pedals_and_hands_on_turning_grips() {
        let world = DynamicWorld::new(&Hermes::test_map());
        for (model, def) in pedestrian_catalog::definitions().iter().enumerate() {
            for standing in [0., 1.] {
                for steering in [-0.55, 0., 0.55] {
                    for frame in 0..24 {
                        let mut bike = Vehicle::spawn(Hermes::test_map().city());
                        bike.riding_stand = standing;
                        bike.pedal_angle = frame as f32 * std::f32::consts::TAU / 24.;
                        bike.steering = steering;
                        let palette = pose(def, &bike);
                        assert!(palette.iter().all(|p| p.is_finite()));
                        let joints = def.joint_worlds(&palette);
                        let rest = def.rest_worlds();
                        for (side, label) in ["L", "R"].into_iter().enumerate() {
                            let ankle = rest[def.node(&format!("Foot_{label}"))].w_axis.truncate();
                            let sole = joints[def.joint(&format!("Foot_{label}"))]
                                .w_axis
                                .truncate()
                                + Vec3::new(0., -ankle.y, 0.065);
                            let pedal = game_to_motion(pedal_position(side, bike.pedal_angle))
                                + Vec3::Y * 0.014;
                            assert!(
                                sole.distance(pedal) < 0.012,
                                "model {model} foot {side} frame {frame}: {}",
                                sole.distance(pedal)
                            );
                            let hand = joints[def.joint(&format!("Hand_{label}"))]
                                .w_axis
                                .truncate();
                            let target = game_to_motion(grip_position(side, steering))
                                + Vec3::new(0., 0.025, -0.035);
                            assert!(
                                hand.distance(target) < 0.012,
                                "model {model} hand {side} steering {steering}: {}",
                                hand.distance(target)
                            );
                        }
                    }
                }
            }
        }
        assert!(world.player().is_bicycle());
    }
}
