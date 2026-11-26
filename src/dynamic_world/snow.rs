//! Session-persistent cleared road cells. Heights keep stacked decks independent.
use super::{catalog, Vehicle, METERS_PER_UNIT};
use glam::{Vec2, Vec3};
use opencity::runtime::{City, SurfaceKind};
use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, VecDeque},
};

pub const CELL_METERS: f32 = 0.5;
pub const EMPTY: f32 = -10000.;
pub type Cell = (i32, i32);
// Tracks update every 0.1 seconds in winter. Staggered recovery grows snowy patches
// across a cleared road over one to three minutes instead of restoring a whole strip at once.
const RECOVERY_START_TICKS: u64 = 600;
const RECOVERY_SPREAD_TICKS: u64 = 1200;
#[derive(Default)]
pub struct Tracks {
    pub cells: HashMap<Cell, [f32; 4]>,
    pub revision: u64,
    pub epoch: u64,
    pub changes: VecDeque<(u64, Cell)>,
    previous: HashMap<usize, Vec3>,
    tick: u64,
    recoveries: BinaryHeap<Reverse<(u64, i32, i32, u32)>>,
}
pub struct SnowWheel {
    pub key: (usize, usize),
    pub point: Vec3,
    pub outward: Vec3,
    pub width: f32,
    pub sprays: bool,
}
impl super::DynamicWorld {
    /// Sample each tire on its own path so yaw and lateral slip separate the two axles.
    pub fn snow_wheels(&self, eye: Vec3) -> impl Iterator<Item = SnowWheel> + '_ {
        use super::vehicle::tire::Wheel;
        self.vehicles
            .iter()
            .enumerate()
            .filter(move |(id, v)| {
                v.grounded
                    && v.position.distance_squared(eye) <= 100_f32.powi(2)
                    && v.velocity.truncate().length_squared() >= 0.16
                    && !v.is_aircraft()
                    && !v.is_boat()
                    && !self.cyclists.inactive(*id)
                    && self.traffic.opacity(*id) >= 0.5
            })
            .flat_map(|(id, v)| {
                let bike = v.is_bicycle();
                let pose = v.orientation();
                let reversing = v.velocity.dot(pose * Vec3::X) < 0.;
                Wheel::ALL.into_iter().filter_map(move |wheel| {
                    let left = matches!(wheel, Wheel::RearLeft | Wheel::FrontLeft);
                    if bike && left {
                        return None;
                    }
                    let front = matches!(wheel, Wheel::FrontLeft | Wheel::FrontRight);
                    let mut local = wheel.contact(v);
                    if bike {
                        local.y = 0.;
                    }
                    Some(SnowWheel {
                        key: (id, wheel as usize),
                        point: v.position + pose * local,
                        outward: if bike {
                            Vec3::ZERO
                        } else {
                            pose * Vec3::Y * local.y.signum()
                        },
                        width: if bike {
                            0.06
                        } else {
                            (v.width() * 0.13).clamp(0.17, 0.34)
                        },
                        sprays: front == reversing,
                    })
                })
            })
            .chain(self.ski_snow_contacts(eye))
    }
}
impl Tracks {
    pub fn reset(&mut self) {
        self.cells.clear();
        self.changes.clear();
        self.previous.clear();
        self.revision = 0;
        self.epoch = self.epoch.wrapping_add(1);
        self.tick = 0;
        self.recoveries.clear();
    }
    pub fn update(
        &mut self,
        city: &City,
        vehicles: &[Vehicle],
        player: usize,
    ) -> Vec<crate::kairos::SnowCell> {
        self.advance_reaccumulation();
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
        let spread = (key.0 as u32).wrapping_mul(0x9e37_79b9)
            ^ (key.1 as u32).wrapping_mul(0x85eb_ca6b)
            ^ height.to_bits().wrapping_mul(0xc2b2_ae35);
        let due = self
            .tick
            .saturating_add(RECOVERY_START_TICKS + u64::from(spread) % RECOVERY_SPREAD_TICKS);
        self.recoveries
            .push(Reverse((due, key.0, key.1, height.to_bits())));
        self.revision += 1;
        self.changes.push_back((self.revision, key));
        if self.changes.len() > 4096 {
            self.changes.pop_front();
        }
        true
    }

    fn advance_reaccumulation(&mut self) {
        self.tick = self.tick.saturating_add(1);
        while let Some(&Reverse((due, x, y, height_bits))) = self.recoveries.peek() {
            if due > self.tick {
                break;
            }
            self.recoveries.pop();
            let key = (x, y);
            let Some(layers) = self.cells.get_mut(&key) else {
                continue;
            };
            let Some(slot) = layers
                .iter()
                .position(|height| height.to_bits() == height_bits)
            else {
                continue;
            };
            layers[slot] = EMPTY;
            if layers.iter().all(|height| *height == EMPTY) {
                self.cells.remove(&key);
            }
            self.revision += 1;
            self.changes.push_back((self.revision, key));
            if self.changes.len() > 4096 {
                self.changes.pop_front();
            }
        }
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

    #[test]
    fn cleared_snow_returns_gradually_and_can_be_plowed_again() {
        let mut tracks = Tracks::default();
        for x in 0..64 {
            assert!(tracks.clear_height((x, 0), 0.));
        }
        let cleared_revision = tracks.revision;
        for _ in 0..RECOVERY_START_TICKS - 1 {
            tracks.advance_reaccumulation();
        }
        assert_eq!(tracks.cells.len(), 64);
        for _ in 0..RECOVERY_SPREAD_TICKS / 2 + 1 {
            tracks.advance_reaccumulation();
        }
        assert!(!tracks.cells.is_empty());
        assert!(tracks.cells.len() < 64);
        for _ in 0..RECOVERY_SPREAD_TICKS {
            tracks.advance_reaccumulation();
        }
        assert!(tracks.cells.is_empty());
        assert!(tracks.revision > cleared_revision);
        assert!(tracks.clear_height((0, 0), 0.));
        tracks.reset();
        assert!(tracks.cells.is_empty());
        assert!(tracks.recoveries.is_empty());
    }
}
