//! Conservative world-space bounds for static meshes and individual vehicle parts.
use glam::{Mat4, Vec3, Vec4};
use opencity::runtime::Vertex;
use std::ops::Range;

#[derive(Clone, Copy)]
pub(super) struct Bounds {
    pub(super) min: Vec3,
    pub(super) max: Vec3,
}
impl Bounds {
    pub(super) fn of_vertices(vertices: &[Vertex]) -> Self {
        let mut bounds = Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        };
        for vertex in vertices {
            let position = Vec3::from_array(vertex.position);
            bounds.min = bounds.min.min(position);
            bounds.max = bounds.max.max(position);
        }
        bounds
    }
    pub(super) fn transformed(self, matrix: Mat4) -> Self {
        let center = (self.min + self.max) * 0.5;
        let half = (self.max - self.min) * 0.5;
        // Translation W carries vehicle opacity; only XYZ participates in geometry.
        let x = matrix.x_axis.truncate();
        let y = matrix.y_axis.truncate();
        let z = matrix.z_axis.truncate();
        let center = x * center.x + y * center.y + z * center.z + matrix.w_axis.truncate();
        let half = x.abs() * half.x + y.abs() * half.y + z.abs() * half.z;
        Self {
            min: center - half,
            max: center + half,
        }
    }
}
pub(super) struct Frustum([Vec4; 6]);
impl Frustum {
    pub(super) fn includes(&self, bounds: Option<Bounds>) -> bool {
        bounds.is_none_or(|bounds| self.intersects(bounds))
    }
    pub(super) fn from_matrix(matrix: Mat4) -> Self {
        let rows = matrix.transpose();
        // WebGPU clips x/y to [-w,w], but depth to [0,w]. Plane lengths
        // do not matter for the signed half-space test.
        Self([
            rows.w_axis + rows.x_axis,
            rows.w_axis - rows.x_axis,
            rows.w_axis + rows.y_axis,
            rows.w_axis - rows.y_axis,
            rows.z_axis,
            rows.w_axis - rows.z_axis,
        ])
    }
    pub(super) fn intersects(&self, bounds: Bounds) -> bool {
        self.0.iter().all(|plane| {
            // Retain touching boxes, eye crossings and near-plane intersections.
            let corner = Vec3::new(
                if plane.x >= 0. {
                    bounds.max.x
                } else {
                    bounds.min.x
                },
                if plane.y >= 0. {
                    bounds.max.y
                } else {
                    bounds.min.y
                },
                if plane.z >= 0. {
                    bounds.max.z
                } else {
                    bounds.min.z
                },
            );
            plane.truncate().dot(corner) + plane.w >= -0.00001
        })
    }
    /// Keep instance order and merge adjacent survivors without copying matrices
    /// or allocating a second upload. Camera and shadow use independent frusta.
    pub(super) fn instance_ranges<'a>(
        &'a self,
        bounds: &'a [Bounds],
    ) -> impl Iterator<Item = Range<u32>> + 'a {
        let mut index = 0;
        std::iter::from_fn(move || {
            while index < bounds.len() && !self.intersects(bounds[index]) {
                index += 1;
            }
            let start = index;
            while index < bounds.len() && self.intersects(bounds[index]) {
                index += 1;
            }
            (start < index).then_some(start as u32..index as u32)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    #[test]
    fn part_bounds_enclose_rotated_scaled_corners_independent_of_opacity() {
        let local = Bounds {
            min: Vec3::new(-2., -0.8, -0.4),
            max: Vec3::new(3., 0.9, 1.7),
        };
        for angle in [0., 0.4, 1.6, 3.2, 5.7] {
            let mut matrix = Mat4::from_scale_rotation_translation(
                Vec3::new(0.01, -0.02, 0.03),
                Quat::from_euler(glam::EulerRot::ZYX, angle, angle * 0.2, angle * 0.3),
                Vec3::new(-0.7, 1.2, 0.4),
            );
            let expected = local.transformed(matrix);
            for opacity in [0., 0.3, 1.] {
                matrix.w_axis.w = opacity;
                let actual = local.transformed(matrix);
                assert_eq!(actual.min, expected.min);
                assert_eq!(actual.max, expected.max);
                let mut min = Vec3::splat(f32::INFINITY);
                let mut max = Vec3::splat(f32::NEG_INFINITY);
                for i in 0..8 {
                    let corner = Vec3::new(
                        if i & 1 == 0 { local.min.x } else { local.max.x },
                        if i & 2 == 0 { local.min.y } else { local.max.y },
                        if i & 4 == 0 { local.min.z } else { local.max.z },
                    );
                    let world = (matrix * corner.extend(1.)).truncate();
                    min = min.min(world);
                    max = max.max(world);
                }
                assert!(actual.min.abs_diff_eq(min, 0.000001));
                assert!(actual.max.abs_diff_eq(max, 0.000001));
            }
        }
    }

    #[test]
    fn instance_ranges_keep_order_merge_neighbors_and_cull_each_pass_independently() {
        let camera = Frustum::from_matrix(Mat4::IDENTITY);
        let shadow = Frustum::from_matrix(Mat4::from_translation(-Vec3::X * 2.));
        let bounds: Vec<_> = [3., 0., 0.5, 2., 0., 3.]
            .into_iter()
            .map(|x| Bounds {
                min: Vec3::new(x, 0., 0.1),
                max: Vec3::new(x + 0.1, 0.1, 0.2),
            })
            .collect();
        assert_eq!(
            camera.instance_ranges(&bounds).collect::<Vec<_>>(),
            [1..3, 4..5]
        );
        // Instance 3 is outside the camera but must cast into the shadow map.
        assert_eq!(
            shadow.instance_ranges(&bounds).collect::<Vec<_>>(),
            [0..1, 3..4, 5..6]
        );
        assert_eq!(camera.instance_ranges(&[]).count(), 0);
        assert_eq!(camera.instance_ranges(&bounds[..1]).count(), 0);
        assert_eq!(
            camera.instance_ranges(&bounds[1..3]).collect::<Vec<_>>(),
            [0..2]
        );
    }
}
