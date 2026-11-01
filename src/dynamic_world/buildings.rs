//! Static broad phase: only nearby building footprints reach vehicle contact geometry.
use super::METERS_PER_UNIT;
use glam::{Vec2, Vec3};
use opencity::{runtime::City, LandscapeKind};
use std::collections::HashMap;
const CELL: f32 = 50.;
pub(super) struct Buildings {
    cells: HashMap<(i32, i32), Vec<usize>>,
}
impl Buildings {
    /// Sweep a padded camera along the follow arm, pulling in immediately at
    /// the first obstruction. Sampling is finer than the clearance radius so
    /// thin walls cannot be skipped. The caller eases the return to full range.
    pub fn camera_eye(&self, city: &City, anchor: Vec3, desired: Vec3) -> Vec3 {
        const RADIUS: f32 = 0.6;
        let anchor = anchor * METERS_PER_UNIT;
        let desired = desired * METERS_PER_UNIT;
        let delta = desired - anchor;
        let mut candidates = Vec::new();
        self.nearby(
            (anchor + desired).truncate() * 0.5,
            delta.truncate().length() * 0.5 + RADIUS,
            &mut candidates,
        );
        if candidates.is_empty() {
            return desired / METERS_PER_UNIT;
        }
        let blocked = |p: Vec3| {
            candidates.iter().any(|&id| {
                let building = &city.landscape[id];
                if building.elevation * METERS_PER_UNIT >= p.z + RADIUS
                    || (building.elevation + building.height.unwrap_or(0.)) * METERS_PER_UNIT
                        <= p.z - RADIUS
                {
                    return false;
                }
                let point = p.truncate();
                let mut inside = false;
                let mut near_edge = false;
                for (a, b) in building
                    .points
                    .iter()
                    .zip(building.points.iter().cycle().skip(1))
                {
                    let a = Vec2::from_array(*a) * METERS_PER_UNIT;
                    let b = Vec2::from_array(*b) * METERS_PER_UNIT;
                    let edge = b - a;
                    if (a.y > point.y) != (b.y > point.y)
                        && point.x < edge.x * (point.y - a.y) / edge.y + a.x
                    {
                        inside = !inside;
                    }
                    let along =
                        ((point - a).dot(edge) / edge.length_squared().max(1e-10)).clamp(0., 1.);
                    near_edge |= point.distance_squared(a + edge * along) <= RADIUS * RADIUS;
                }
                inside || near_edge
            })
        };
        let steps = (delta.length() / 0.2).ceil().max(1.) as usize;
        let mut safe = 0.;
        for step in 1..=steps {
            let t = step as f32 / steps as f32;
            if blocked(anchor + delta * t) {
                let mut hit = t;
                for _ in 0..8 {
                    let middle = (safe + hit) * 0.5;
                    if blocked(anchor + delta * middle) {
                        hit = middle;
                    } else {
                        safe = middle;
                    }
                }
                return (anchor + delta * safe) / METERS_PER_UNIT;
            }
            safe = t;
        }
        desired / METERS_PER_UNIT
    }
    pub fn new(city: &City) -> Self {
        let mut cells = HashMap::<_, Vec<_>>::new();
        for (id, feature) in city.landscape.iter().enumerate() {
            if feature.kind != LandscapeKind::Building {
                continue;
            }
            let (min, max) = feature.points.iter().fold(
                (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
                |(min, max), point| {
                    let p = Vec2::from_array(*point) * METERS_PER_UNIT;
                    (min.min(p), max.max(p))
                },
            );
            for x in (min.x / CELL).floor() as i32..=(max.x / CELL).floor() as i32 {
                for y in (min.y / CELL).floor() as i32..=(max.y / CELL).floor() as i32 {
                    cells.entry((x, y)).or_default().push(id);
                }
            }
        }
        Self { cells }
    }
    pub fn nearby(&self, center: Vec2, reach: f32, output: &mut Vec<usize>) {
        output.clear();
        for x in
            ((center.x - reach) / CELL).floor() as i32..=((center.x + reach) / CELL).floor() as i32
        {
            for y in ((center.y - reach) / CELL).floor() as i32
                ..=((center.y + reach) / CELL).floor() as i32
            {
                if let Some(ids) = self.cells.get(&(x, y)) {
                    output.extend_from_slice(ids);
                }
            }
        }
        output.sort_unstable();
        output.dedup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_stops_before_thin_wall_even_with_clear_endpoint_and_handles_roofs() {
        let city = City::from_program(
            &opencity::parse("wall=building(0.1,-0.05,-0.1,-0.049,-0.1,-0.049,0.1,-0.05,0.1);")
                .unwrap(),
            None,
        )
        .unwrap();
        let buildings = Buildings::new(&city);
        let anchor = Vec3::new(0., 0., 0.02);
        let desired = Vec3::new(-0.15, 0., 0.04);
        let eye = buildings.camera_eye(&city, anchor, desired);
        assert!(eye.x > -0.0431 && eye.x < -0.04, "{eye:?}");
        assert_eq!(
            buildings.camera_eye(&city, anchor + Vec3::Z, desired + Vec3::Z),
            desired + Vec3::Z
        );
        assert_eq!(
            buildings.camera_eye(&city, anchor, anchor + Vec3::X),
            anchor + Vec3::X
        );
        for angle in -90..=90 {
            let theta = (angle as f32).to_radians();
            let target = anchor + Vec3::new(-theta.cos() * 0.15, theta.sin() * 0.15, 0.02);
            let eye = buildings.camera_eye(&city, anchor, target);
            assert!(!city.building_collision(
                eye.truncate().to_array(),
                0.0059,
                eye.z - 0.0059,
                eye.z + 0.0059
            ));
        }
    }
}
