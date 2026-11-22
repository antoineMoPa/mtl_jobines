//! Bounds in study_01's source coordinates, shared by all placements.
use glam::{Affine3A, Quat, Vec2, Vec3};
use opencity::runtime::ModelCollider;
use rapier3d::{
    na::{Point3, Vector3},
    parry::{
        query::{Ray, RayCast},
        shape::{Ball, Cuboid},
    },
};

const GLOBE: Vec3 = Vec3::new(0., 0., 0.35);
const RADIUS: f32 = 1.5;
const GLOBE_FLOOR: f32 = -0.85;
// Includes the serving stand's roof overhang and counter.
const STAND_MIN: Vec3 = Vec3::new(0.045, -1.7, -0.855);
const STAND_MAX: Vec3 = Vec3::new(1.475, -0.935, -0.355);

pub(super) struct Collider {
    local: Affine3A,
    rotation: Quat,
    scale: f32,
}
impl Collider {
    pub(super) fn new(
        center: [f32; 2],
        angle: f32,
        scale: f32,
        height_scale: f32,
        bounds: [f32; 6],
        elevation: f32,
    ) -> Self {
        let rotation = Quat::from_rotation_z(angle);
        let source_origin = Vec3::new(
            (bounds[0] + bounds[3]) * 0.5,
            (bounds[1] + bounds[4]) * 0.5,
            bounds[2],
        );
        let placement = Affine3A::from_scale_rotation_translation(
            Vec3::new(scale, scale, height_scale),
            rotation,
            Vec2::from(center).extend(elevation),
        ) * Affine3A::from_translation(-source_origin);
        Self {
            local: placement.inverse(),
            rotation,
            scale,
        }
    }
}
impl ModelCollider for Collider {
    fn contact(
        &self,
        point: [f32; 2],
        radius: f32,
        bottom: f32,
        top: f32,
    ) -> Option<([f32; 2], f32)> {
        if bottom > top {
            return None;
        }
        let p = self
            .local
            .transform_point3(Vec2::from(point).extend(bottom));
        let top = self.local.transform_point3(Vec2::from(point).extend(top)).z;
        let radius = radius / self.scale;
        let mut best = None;
        // The globe mesh is cut flat at its foot.
        if top >= GLOBE_FLOOR && p.z <= GLOBE.z + RADIUS {
            let dz = GLOBE.z.clamp(p.z.max(GLOBE_FLOOR), top) - GLOBE.z;
            let distance = p.truncate().length();
            let depth = (RADIUS * RADIUS - dz * dz).max(0.).sqrt() + radius - distance;
            if depth >= 0. {
                best = Some((
                    if distance > 0. {
                        p.truncate() / distance
                    } else {
                        Vec2::X
                    },
                    depth,
                ));
            }
        }
        if p.z <= STAND_MAX.z && top >= STAND_MIN.z {
            let xy = p.truncate();
            let min = STAND_MIN.truncate();
            let max = STAND_MAX.truncate();
            let delta = xy - xy.clamp(min, max);
            let distance = delta.length();
            let (normal, depth) = if distance > 0. {
                (delta / distance, radius - distance)
            } else {
                [
                    (-Vec2::X, xy.x - min.x),
                    (Vec2::X, max.x - xy.x),
                    (-Vec2::Y, xy.y - min.y),
                    (Vec2::Y, max.y - xy.y),
                ]
                .into_iter()
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(n, d)| (n, radius + d))
                .unwrap()
            };
            if depth >= 0. && best.is_none_or(|(_, previous)| depth > previous) {
                best = Some((normal, depth));
            }
        }
        best.map(|(n, d)| {
            (
                (self.rotation * n.extend(0.)).truncate().to_array(),
                d * self.scale,
            )
        })
    }

    fn ray(&self, origin: [f32; 3], direction: [f32; 3], limit: f32) -> Option<(f32, [f32; 3])> {
        let p = self.local.transform_point3(origin.into());
        let dir = self.local.transform_vector3(direction.into());
        let ray = |center: Vec3| {
            Ray::new(
                Point3::from((p - center).to_array()),
                Vector3::from(dir.to_array()),
            )
        };
        let globe = Ball::new(RADIUS)
            .cast_local_ray_and_get_normal(&ray(GLOBE), limit, false)
            .filter(|hit| p.z + dir.z * hit.time_of_impact >= GLOBE_FLOOR);
        let half = (STAND_MAX - STAND_MIN) * 0.5;
        let stand = Cuboid::new(Vector3::from(half.to_array())).cast_local_ray_and_get_normal(
            &ray(STAND_MIN + half),
            limit,
            false,
        );
        let mut nearest = globe
            .into_iter()
            .chain(stand)
            .min_by(|a, b| a.time_of_impact.total_cmp(&b.time_of_impact))
            .map(|hit| {
                (
                    hit.time_of_impact,
                    Vec3::new(hit.normal.x, hit.normal.y, hit.normal.z),
                )
            });
        if dir.z != 0. {
            let t = (GLOBE_FLOOR - p.z) / dir.z;
            if t >= 0.
                && t <= limit
                && (p + dir * t - GLOBE).length_squared() <= RADIUS * RADIUS
                && nearest.is_none_or(|(previous, _)| t < previous)
            {
                nearest = Some((t, -Vec3::Z));
            }
        }
        nearest.map(|(t, n)| {
            let n = Vec3::from(self.local.matrix3.transpose() * glam::Vec3A::from(n));
            (t, (n / n.length()).to_array())
        })
    }
}
