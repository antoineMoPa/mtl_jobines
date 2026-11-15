//! Session-persistent cleared road cells. Heights keep stacked decks independent.
use super::{catalog, Vehicle, METERS_PER_UNIT};
use glam::{Vec2, Vec3};
use opencity::runtime::{City, SurfaceKind};
use std::collections::{HashMap, VecDeque};

pub(crate) const CELL_METERS: f32 = 0.5;
pub(crate) const EMPTY: f32 = -10000.;
pub(crate) type Cell = (i32, i32);
#[derive(Default)]
pub(crate) struct Tracks {
    pub cells: HashMap<Cell, [f32; 4]>,
    pub revision: u64,
    pub epoch: u64,
    pub changes: VecDeque<(u64, Cell)>,
    previous: HashMap<usize, Vec3>,
}
impl Tracks {
    pub fn reset(&mut self) {
        self.cells.clear();
        self.changes.clear();
        self.previous.clear();
        self.revision = 0;
        self.epoch = self.epoch.wrapping_add(1);
    }
    pub fn update(
        &mut self,
        city: &City,
        vehicles: &[Vehicle],
        player: usize,
    ) -> Vec<crate::kairos::SnowCell> {
        let mut player_cells = Vec::new();
        for (id, vehicle) in vehicles.iter().enumerate().filter(|(_, v)| {
            v.position.z > -100.
                && v.definition()
                    .is_some_and(|d| d.config.research_id == catalog::SNOWPLOW_ID)
        }) {
            let forward = Vec3::new(vehicle.yaw.cos(), vehicle.yaw.sin(), 0.);
            let ground = vehicle.position - Vec3::Z * vehicle.ride_height();
            let blade = ground + forward * (vehicle.length() * 0.5 - 0.15);
            let previous = self.previous.insert(id, blade).unwrap_or(blade);
            if vehicle.velocity.dot(forward) < 0.25 {
                continue;
            }
            // Teleports and recycled traffic slots must not plow across the map.
            let previous = if previous.distance(blade) < 15. {
                previous
            } else {
                blade
            };
            let cells = self.sweep(city, previous, blade, vehicle.width() * 0.5);
            if id == player {
                player_cells.extend(cells);
            }
        }
        player_cells
    }
    pub fn sweep(
        &mut self,
        city: &City,
        from: Vec3,
        to: Vec3,
        radius: f32,
    ) -> Vec<crate::kairos::SnowCell> {
        let mut cleared = Vec::new();
        let a = from.truncate();
        let b = to.truncate();
        let delta = b - a;
        let lo = ((a.min(b) - Vec2::splat(radius)) / CELL_METERS)
            .floor()
            .as_ivec2();
        let hi = ((a.max(b) + Vec2::splat(radius)) / CELL_METERS)
            .ceil()
            .as_ivec2();
        for y in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                let point = (Vec2::new(x as f32, y as f32) + Vec2::splat(0.5)) * CELL_METERS;
                let t =
                    ((point - a).dot(delta) / delta.length_squared().max(0.00001)).clamp(0., 1.);
                if point.distance_squared(a + delta * t) > radius * radius {
                    continue;
                }
                let expected = from.z + (to.z - from.z) * t;
                let Some(surface) = city.surface_at(
                    (point / METERS_PER_UNIT).to_array(),
                    (expected + 0.6) / METERS_PER_UNIT,
                ) else {
                    continue;
                };
                if !matches!(surface.kind, SurfaceKind::Asphalt | SurfaceKind::Marking)
                    || (surface.height * METERS_PER_UNIT - expected).abs() > 0.6
                {
                    continue;
                }
                if self.clear_height((x, y), surface.height) {
                    cleared.push(crate::kairos::SnowCell {
                        x,
                        y,
                        height_bits: surface.height.to_bits(),
                    });
                }
            }
        }
        cleared
    }
    pub fn clear_height(&mut self, key: Cell, height: f32) -> bool {
        let layers = self.cells.entry(key).or_insert([EMPTY; 4]);
        if layers.iter().any(|h| (*h - height).abs() < 0.0025) {
            return false;
        }
        // Four independent road elevations per XY cell; keep the common case compact.
        let slot = layers.iter().position(|h| *h == EMPTY).unwrap_or_else(|| {
            layers
                .iter()
                .enumerate()
                .max_by(|a, b| (*a.1 - height).abs().total_cmp(&(*b.1 - height).abs()))
                .unwrap()
                .0
        });
        layers[slot] = height;
        self.revision += 1;
        self.changes.push_back((self.revision, key));
        if self.changes.len() > 4096 {
            self.changes.pop_front();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sweep_crosses_negative_cells_preserves_stacked_decks_and_skips_grass() {
        let hermes = crate::hermes::Hermes::test_map_from_source(include_str!(
            "../../tests/fixtures/snow_season.road"
        ));
        let mut tracks = Tracks::default();
        tracks.sweep(
            hermes.city(),
            Vec3::new(-8., 0., 0.),
            Vec3::new(8., 0., 0.),
            1.8,
        );
        assert!(tracks.cells.keys().any(|&(x, _)| x < 0));
        assert!(tracks.cells.keys().any(|&(x, _)| x > 0));
        let count = tracks.cells.len();
        tracks.sweep(
            hermes.city(),
            Vec3::new(-8., 30., 0.),
            Vec3::new(8., 30., 0.),
            1.8,
        );
        assert_eq!(tracks.cells.len(), count, "plow erased off-road snow");
        tracks.clear_height((0, 0), 0.);
        tracks.clear_height((0, 0), 0.15);
        assert!(tracks.cells[&(0, 0)].contains(&0.));
        assert!(tracks.cells[&(0, 0)].contains(&0.15));
        let revision = tracks.revision;
        tracks.clear_height((0, 0), 0.15);
        assert_eq!(tracks.revision, revision);
        tracks.reset();
        assert!(tracks.cells.is_empty());
        assert_eq!(tracks.epoch, 1);
    }
}
