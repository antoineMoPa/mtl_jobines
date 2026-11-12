use glam::{Mat4, Vec2, Vec3, Vec4};
use opencity::{runtime::Mesh, LandscapeFeature};

#[derive(Clone, Copy)]
pub(super) struct View {
    pub matrix: Mat4,
    pub viewport: egui::Rect,
}
impl View {
    pub fn project(self, p: Vec3) -> Option<egui::Pos2> {
        let q = self.matrix * p.extend(1.);
        if q.w <= 0. {
            return None;
        }
        let q = q.truncate() / q.w;
        (q.z >= 0. && q.z <= 1.).then(|| {
            egui::pos2(
                self.viewport.center().x + q.x * self.viewport.width() / 2.,
                self.viewport.center().y - q.y * self.viewport.height() / 2.,
            )
        })
    }
    pub fn ray(self, p: egui::Pos2) -> (Vec3, Vec3) {
        let x = (p.x - self.viewport.left()) / self.viewport.width() * 2. - 1.;
        let y = 1. - (p.y - self.viewport.top()) / self.viewport.height() * 2.;
        let inverse = self.matrix.inverse();
        let a = inverse * Vec4::new(x, y, 0., 1.);
        let b = inverse * Vec4::new(x, y, 1., 1.);
        let a = a.truncate() / a.w;
        (a, b.truncate() / b.w - a)
    }
    pub fn ground(self, p: egui::Pos2, elevation: f32) -> Option<Vec2> {
        let (a, d) = self.ray(p);
        if d.z.abs() < 1e-7 {
            return None;
        }
        let t = (elevation - a.z) / d.z;
        (t >= 0. && t.is_finite()).then(|| (a + d * t).truncate())
    }
}
pub(super) fn center(feature: &LandscapeFeature) -> Vec2 {
    feature
        .points
        .iter()
        .map(|p| Vec2::from_array(*p))
        .sum::<Vec2>()
        / feature.points.len() as f32
}
pub(super) fn rectangle(a: Vec2, b: Vec2) -> Vec<[f32; 2]> {
    let low = a.min(b);
    let high = a.max(b).max(low + Vec2::splat(0.06));
    vec![
        [low.x, low.y],
        [high.x, low.y],
        [high.x, high.y],
        [low.x, high.y],
    ]
}
pub(super) fn translated(feature: &LandscapeFeature, delta: Vec2) -> LandscapeFeature {
    let mut result = feature.clone();
    for p in &mut result.points {
        *p = (Vec2::from_array(*p) + delta).to_array();
    }
    result
}
pub(super) fn rotated(feature: &LandscapeFeature, degrees: f32) -> LandscapeFeature {
    let mut result = feature.clone();
    let c = center(feature);
    let edge = Vec2::from_array(feature.points[1]) - Vec2::from_array(feature.points[0]);
    let delta = degrees.to_radians() - edge.y.atan2(edge.x);
    let (s, cos) = delta.sin_cos();
    for p in &mut result.points {
        let v = Vec2::from_array(*p) - c;
        *p = (c + Vec2::new(cos * v.x - s * v.y, s * v.x + cos * v.y)).to_array();
    }
    result
}
/// Separating axes work directly on authored coordinates; axes need no unit conversion.
pub(super) fn overlap(a: &[[f32; 2]], b: &[[f32; 2]]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    for polygon in [a, b] {
        for (p, q) in polygon.iter().zip(polygon.iter().cycle().skip(1)) {
            let axis = Vec2::new(p[1] - q[1], q[0] - p[0]);
            if axis.length_squared() < 1e-14 {
                continue;
            }
            let interval = |points: &[[f32; 2]]| {
                points
                    .iter()
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                        let d = Vec2::from_array(*p).dot(axis);
                        (lo.min(d), hi.max(d))
                    })
            };
            let (al, ah) = interval(a);
            let (bl, bh) = interval(b);
            if ah <= bl + 1e-8 || bh <= al + 1e-8 {
                return false;
            }
        }
    }
    true
}
pub(super) fn triangle_hit(origin: Vec3, direction: Vec3, triangle: [Vec3; 3]) -> Option<f32> {
    let e1 = triangle[1] - triangle[0];
    let e2 = triangle[2] - triangle[0];
    let p = direction.cross(e2);
    let determinant = e1.dot(p);
    if determinant.abs() < 1e-12 {
        return None;
    }
    let t = origin - triangle[0];
    let u = t.dot(p) / determinant;
    if !(0. ..=1.).contains(&u) {
        return None;
    }
    let q = t.cross(e1);
    let v = direction.dot(q) / determinant;
    if v < 0. || u + v > 1. {
        return None;
    }
    let distance = e2.dot(q) / determinant;
    (0. ..=1.).contains(&distance).then_some(distance)
}
pub(super) fn mesh_hit(view: View, pos: egui::Pos2, mesh: &Mesh) -> Option<f32> {
    let (a, d) = view.ray(pos);
    mesh.vertices
        .chunks_exact(3)
        .filter_map(|tri| {
            triangle_hit(
                a,
                d,
                std::array::from_fn(|i| Vec3::from_array(tri[i].position)),
            )
        })
        .min_by(f32::total_cmp)
}
pub(super) fn prism(feature: &LandscapeFeature) -> Mesh {
    use opencity::runtime::SurfaceKind;
    let mut mesh = Mesh::default();
    let z = feature.elevation;
    let h = feature.height.unwrap_or(0.);
    let vertex = |p: [f32; 2], height| [p[0], p[1], height];
    for i in 0..feature.points.len() {
        let a = feature.points[i];
        let b = feature.points[(i + 1) % feature.points.len()];
        for tri in [
            [vertex(a, z), vertex(b, z), vertex(b, z + h)],
            [vertex(a, z), vertex(b, z + h), vertex(a, z + h)],
        ] {
            mesh.triangle(tri, [0.6; 3], SurfaceKind::Building, [0.; 3]);
        }
        if i >= 2 {
            mesh.triangle(
                [
                    vertex(feature.points[0], z + h),
                    vertex(feature.points[i - 1], z + h),
                    vertex(a, z + h),
                ],
                [0.6; 3],
                SurfaceKind::Roof,
                [0.; 3],
            );
        }
    }
    mesh
}
