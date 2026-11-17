//! Reuse local sign collision shapes; each placement carries only its pose.
use super::*;
type ShapeKey = (bool, [u32; 3]);
pub(super) struct SignBody {
    pub shape: SharedShape,
    pub rotation: Rotation<f32>,
    pub mass: f32,
}
pub(super) fn prepare(
    part: &Part,
    shapes: &mut HashMap<ShapeKey, SharedShape>,
) -> Option<(SignBody, Vec3, Vec3)> {
    let profile = part.sign_profile?;
    let shape = shapes
        .entry((profile.pole, profile.half.to_array().map(f32::to_bits)))
        .or_insert_with(|| {
            if profile.pole {
                SharedShape::capsule_z((profile.half.z - profile.half.x).max(0.), profile.half.x)
            } else {
                SharedShape::cuboid(profile.half.x, profile.half.y, profile.half.z)
            }
        })
        .clone();
    let center = part
        .transform
        .transform_point3(profile.center / METERS_PER_UNIT)
        * METERS_PER_UNIT;
    let q = Quat::from_mat4(&part.transform);
    let rotation = Rotation::new_unchecked(rapier3d::na::Quaternion::new(q.w, q.x, q.y, q.z));
    let half = (q * Vec3::X).abs() * profile.half.x
        + (q * Vec3::Y).abs() * profile.half.y
        + (q * Vec3::Z).abs() * profile.half.z;
    Some((
        SignBody {
            shape,
            rotation,
            mass: profile.mass,
        },
        center,
        half,
    ))
}
