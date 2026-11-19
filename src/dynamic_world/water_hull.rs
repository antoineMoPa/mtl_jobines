//! Intersect the watertight envelope of the authored hull panels with the ocean plane.
use glam::{Mat4, Vec2, Vec3};
use rapier3d::prelude::{ConvexPolyhedron, Point};

pub(super) fn hull_panel(name: &str) -> bool {
    if name == "Deep navy hull • hard chine" {
        return true;
    }
    let side = name
        .strip_prefix("Port ")
        .or_else(|| name.strip_prefix("Starboard "));
    side.is_some_and(|panel| {
        [
            "garboard ",
            "bottom ",
            "topsides ",
            "upper strake ",
            "sheer strake ",
        ]
        .iter()
        .any(|prefix| panel.starts_with(prefix))
    })
}

pub struct WaterHull(ConvexPolyhedron);

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Waterline {
    bounds: [f32; 4],
    // Two CCW vertices per vec4 for WebGL uniform alignment.
    pairs: [[f32; 4]; 16],
}

impl WaterHull {
    pub(super) fn new(mut points: Vec<Point<f32>>) -> Result<Self, String> {
        points.sort_unstable_by(|a, b| {
            a.x.total_cmp(&b.x)
                .then(a.y.total_cmp(&b.y))
                .then(a.z.total_cmp(&b.z))
        });
        points.dedup();
        if points.len() < 4 {
            return Err("Missing boat hull panels".into());
        }
        ConvexPolyhedron::from_convex_hull(&points)
            .map(Self)
            .ok_or_else(|| "Missing boat hull geometry".into())
    }

    pub fn waterline(&self, transform: Mat4, level: f32) -> Option<Waterline> {
        // Work near the origin in metres to retain precision at the shoreline.
        let center = transform.w_axis.truncate();
        let surface = (level - center.z) * super::METERS_PER_UNIT;
        let corners: Vec<_> = self
            .0
            .points()
            .iter()
            .map(|p| transform.transform_vector3(Vec3::new(p.x, p.y, p.z)) * super::METERS_PER_UNIT)
            .collect();
        let mut ring = Vec::new();
        for edge in self.0.edges() {
            let a = corners[edge.vertices.x as usize];
            let b = corners[edge.vertices.y as usize];
            if (a.z < surface) != (b.z < surface) {
                let p = a.lerp(b, (surface - a.z) / (b.z - a.z));
                ring.push(p.truncate());
            }
        }
        if ring.len() < 3 {
            return None;
        }
        // Every crossing is on the convex boundary. Sort around an interior point.
        let middle = ring.iter().copied().sum::<Vec2>() / ring.len() as f32;
        let angle = |p: &Vec2| (p.y - middle.y).atan2(p.x - middle.x);
        ring.sort_unstable_by(|a, b| angle(a).total_cmp(&angle(b)));
        ring.dedup();
        if ring.len() < 3 {
            return None;
        }
        // Preserve the bow and chines when a detailed mesh exceeds the uniform budget.
        while ring.len() > 32 {
            let area = |i: usize| {
                let before = ring[(i + ring.len() - 1) % ring.len()];
                let after = ring[(i + 1) % ring.len()];
                (ring[i] - before).perp_dot(after - before).abs()
            };
            let i = (0..ring.len())
                .min_by(|&i, &j| area(i).total_cmp(&area(j)))
                .unwrap();
            ring.remove(i);
        }
        let mut line = Waterline::default();
        let mut lo = Vec2::splat(f32::INFINITY);
        let mut hi = Vec2::splat(f32::NEG_INFINITY);
        for i in 0..32 {
            let p = ring[i.min(ring.len() - 1)];
            let world = center.truncate() + p / super::METERS_PER_UNIT;
            lo = lo.min(world);
            hi = hi.max(world);
            let k = 2 * (i % 2);
            line.pairs[i / 2][k..k + 2].copy_from_slice(&world.to_array());
        }
        line.bounds = [lo.x, lo.y, hi.x, hi.y];
        Some(line)
    }
}
