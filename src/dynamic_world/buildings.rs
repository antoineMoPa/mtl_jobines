//! Static broad phase: only nearby building footprints reach vehicle contact geometry.
use super::METERS_PER_UNIT;
use glam::Vec2;
use opencity::{runtime::City, LandscapeKind};
use std::collections::HashMap;
const CELL: f32 = 50.;
pub(super) struct Buildings {
    cells: HashMap<(i32, i32), Vec<usize>>,
}
impl Buildings {
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
