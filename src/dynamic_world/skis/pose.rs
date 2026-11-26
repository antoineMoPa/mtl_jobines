//! Terrain-supported ski frames drive the rig, never the walking animation.
use super::{
    super::{
        cycling_pose,
        pedestrian_catalog::{self, JOINT_LIMIT},
        pedestrian_gait::Ground,
        METERS_PER_UNIT,
    },
    movement::{basis, frame, Motion},
};
use glam::{Mat4, Quat, Vec3, Vec4};

pub(super) struct Pose {
    pub transform: Mat4,
    pub palette: [Mat4; JOINT_LIMIT],
    /// Game-space metres; boot and pole meshes share the solved contact frames.
    pub gear: [Mat4; 6],
    #[cfg(test)]
    pub binding_error: f32,
}
fn supported_ski(
    p: Vec3,
    yaw: f32,
    edge: f32,
    support: &impl Fn(Vec3) -> Option<Ground>,
    fallback: Ground,
) -> Mat4 {
    let g = support(p).unwrap_or(Ground {
        point: p,
        ..fallback
    });
    let (_, f, _) = basis(yaw, g);
    let q = frame(yaw, g) * Quat::from_rotation_x(edge);
    let mut center = g.point + Vec3::Z * (0.008 + edge.sin().abs() * 0.055);
    for d in [-0.85, 0., 0.85] {
        let sample = center + q * (Vec3::X * d);
        let expected = g.point + f * (d / f.truncate().length().max(0.01));
        if let Some(ground) = support(sample.truncate().extend(expected.z)) {
            center.z += (ground.point.z - sample.z).max(0.);
        }
    }
    Mat4::from_rotation_translation(q, center)
}
impl Pose {
    pub fn build(m: &mut Motion, support: &impl Fn(Vec3) -> Option<Ground>) -> Self {
        let def = &pedestrian_catalog::definitions()[pedestrian_catalog::PLAYER_WINTER_MODEL];
        let axes = Mat4::from_cols(Vec4::Y, Vec4::Z, Vec4::X, Vec4::W);
        let actor =
            Mat4::from_rotation_translation(frame(m.yaw, m.ground), m.position + Vec3::Z * 0.018)
                * axes;
        let inverse = actor.inverse();
        let (_, forward, right) = basis(m.yaw, m.ground);
        let mut locals = def.rest.clone();
        let tuck = m.tuck_blend;
        let brake = m.brake;
        let hockey = m.hockey_side != 0.;
        let poling = !hockey
            && !m.input.backward
            && !m.input.tuck
            && tuck <= 0.15
            && m.input.poles
            && m.speed() < m.poling_speed_limit()
            && brake < 0.1;
        if !poling {
            m.release_poles();
        }
        let stroke = if m.contact && m.phase < 0.42 {
            (std::f32::consts::PI * m.phase / 0.42).sin()
        } else {
            0.
        };
        let inside = (-m.turn_edge * 1.1).clamp(-0.22, 0.22);
        locals[def.node("Hips")].translation = Vec3::new(
            inside - m.hockey_side * 0.035 * m.hockey_blend
                + 0.018 * m.shuffle_blend * m.stride_phase.sin(),
            0.81 - inside.abs() * 0.18
                - 0.095 * stroke
                - 0.10 * tuck
                - 0.045 * brake
                - 0.08 * m.hockey_blend
                - 0.018 * m.shuffle_blend * m.stride_phase.sin().abs(),
            0.025 + 0.03 * stroke,
        );
        locals[def.node("Spine")].rotation *= Quat::from_euler(
            glam::EulerRot::XYZ,
            0.12 + 0.23 * stroke + 0.40 * tuck,
            0.,
            m.turn_edge * 1.35 + m.hockey_side * 0.18 * m.hockey_blend,
        );
        locals[def.node("Chest")].rotation *= Quat::from_euler(
            glam::EulerRot::XYZ,
            0.04 + 0.10 * tuck,
            0.,
            m.turn_edge * 0.4,
        );
        locals[def.node("Head")].rotation *= Quat::from_rotation_x(-0.09 - 0.20 * tuck);
        let mut overrides = vec![None; locals.len()];
        let mut gear = [Mat4::IDENTITY; 6];
        let mut targets = [Vec3::ZERO; 2];
        for (i, (side, label)) in [(1., "L"), (-1., "R")].into_iter().enumerate() {
            let yaw = m.yaw - side * (if hockey { 0. } else { brake }) * 0.16;
            let edge = if hockey {
                m.hockey_side * (0.10 + 0.48 * m.hockey_blend)
            } else {
                m.turn_edge
            };
            let p = m.position + right * (side * 0.15) + forward * m.stride_offsets[i];
            let ski = supported_ski(p, yaw, edge, support, m.ground);
            gear[i] = ski * Mat4::from_translation(Vec3::new(0., -side * 0.13, 0.));
            gear[i + 2] = gear[i];
            let target =
                inverse.transform_point3(ski.transform_point3(Vec3::new(-0.005, 0., 0.105)));
            targets[i] = target;
            let q = Quat::from_mat4(&(inverse * ski * axes));
            cycling_pose::limb(
                def,
                &locals,
                Mat4::IDENTITY,
                &mut overrides,
                [
                    &format!("Thigh_{label}"),
                    &format!("Shin_{label}"),
                    &format!("Foot_{label}"),
                ],
                target,
                Vec3::new(if hockey { 0. } else { -edge.sin() * 2.4 }, 0., 1.),
                q,
            );
        }
        let worlds = def.worlds(&locals, Mat4::IDENTITY, &overrides);
        let chest = worlds[def.node("Chest")];
        let (_, chest_q, _) = chest.to_scale_rotation_translation();
        let mut release = false;
        for (i, (side, label)) in [(1., "L"), (-1., "R")].into_iter().enumerate() {
            let (palm, hand_q) = if let Some(a) = m.anchors.filter(|_| poling && m.contact) {
                let (n, f, _) = basis(m.yaw, m.ground);
                let h = 1.065 - 0.11 * stroke;
                let reach = (1.13_f32.powi(2) - h * h).max(0.).sqrt();
                (
                    inverse.transform_point3(a[i] + n * h + f * reach),
                    Quat::IDENTITY,
                )
            } else if poling {
                (
                    Vec3::new(
                        side * 0.33,
                        1.08,
                        0.18 + 0.06 * (m.phase * std::f32::consts::TAU).sin(),
                    ),
                    Quat::IDENTITY,
                )
            } else {
                let local = if hockey {
                    Vec3::new(side * 0.32, -0.15, 0.22)
                } else {
                    Vec3::new(side * 0.28, -0.29, 0.15)
                        .lerp(Vec3::new(side * 0.18, 0.04, 0.28), tuck)
                };
                (chest.transform_point3(local), chest_q)
            };
            let offset = hand_q * Vec3::new(0., -0.05, 0.025);
            let wrist = palm - offset;
            let shoulder = worlds[def.node(&format!("UpperArm_{label}"))]
                .w_axis
                .truncate();
            let pole = if poling {
                Vec3::new(side * 0.5, -0.2, -0.45)
            } else {
                chest.transform_point3(Vec3::new(
                    side * (0.34 - 0.10 * tuck),
                    -0.34 + 0.10 * tuck,
                    -0.12 + 0.22 * tuck,
                )) - shoulder
            };
            cycling_pose::limb(
                def,
                &locals,
                Mat4::IDENTITY,
                &mut overrides,
                [
                    &format!("UpperArm_{label}"),
                    &format!("Forearm_{label}"),
                    &format!("Hand_{label}"),
                ],
                wrist,
                pole,
                hand_q,
            );
            let actual_worlds = def.worlds(&locals, Mat4::IDENTITY, &overrides);
            let actual = actor.transform_point3(
                actual_worlds[def.node(&format!("Hand_{label}"))]
                    .w_axis
                    .truncate()
                    + offset,
            );
            if m.contact && actual.distance(actor.transform_point3(palm)) > 0.02 {
                release = true;
            }
            let pole_frame = if let Some(a) = m.anchors.filter(|_| poling && m.contact && !release)
            {
                let d = actual - a[i];
                Mat4::from_rotation_translation(
                    Quat::from_rotation_arc(Vec3::Z, d / d.length().max(0.001)),
                    a[i],
                )
            } else {
                let height =
                    support(actual).map_or(actual.z - m.position.z, |g| actual.z - g.point.z);
                let cruise = ((height - 0.14) / 1.13).clamp(0.15, 0.95).acos();
                let tilt = if poling {
                    0.55 - 0.18 * (m.phase * std::f32::consts::TAU).sin()
                } else if hockey {
                    1.05
                } else {
                    cruise + (1.30 - cruise) * tuck
                };
                // +Y tilt makes the pole tip trail behind the forward-facing grip.
                let q = frame(m.yaw, m.ground) * Quat::from_rotation_y(tilt);
                Mat4::from_rotation_translation(q, actual - q * (Vec3::Z * 1.13))
            };
            gear[i + 4] = pole_frame * Mat4::from_translation(Vec3::new(0., -side * 0.30, 0.));
        }
        if release {
            m.release_poles();
        }
        let worlds = def.worlds(&locals, Mat4::IDENTITY, &overrides);
        let mut palette = [Mat4::IDENTITY; JOINT_LIMIT];
        for (i, &node) in def.joints.iter().enumerate() {
            palette[i] = worlds[node] * def.inverse_bind[i];
        }
        Self {
            transform: Mat4::from_scale(Vec3::splat(1. / METERS_PER_UNIT)) * actor,
            palette,
            gear,
            #[cfg(test)]
            binding_error: ["L", "R"]
                .into_iter()
                .enumerate()
                .map(|(i, s)| {
                    worlds[def.node(&format!("Foot_{s}"))]
                        .w_axis
                        .truncate()
                        .distance(targets[i])
                })
                .fold(0., f32::max),
        }
    }
}
