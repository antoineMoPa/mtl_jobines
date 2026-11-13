use super::*;
use crate::engine::render::{GpuMesh, Renderer};
type Cell = (i32, i32);
const RADIUS: f32 = 1.5;
const TILE_RADIUS: i32 = 5;
const HALO: i32 = TILE_RADIUS + 1;

#[derive(Clone)]
struct Candidate {
    mesh: Mesh,
    center: Vec3,
    priority: (u8, u32),
    facing: Facing,
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Facing {
    East,
    West,
    North,
    South,
}
impl Facing {
    fn from_normal(n: [f32; 3]) -> Self {
        if n[0].abs() > n[1].abs() {
            if n[0] > 0. {
                Self::East
            } else {
                Self::West
            }
        } else if n[1] > 0. {
            Self::North
        } else {
            Self::South
        }
    }
}
pub(super) struct Planner {
    receivers: Vec<(PanelKey, Vec<[Vertex; 3]>)>,
    index: BTreeMap<Cell, Vec<usize>>,
    cache: BTreeMap<Cell, Vec<Candidate>>,
}
impl Planner {
    pub(super) fn new(world: &Mesh) -> Self {
        let receivers: Vec<_> = panels(world).into_iter().collect();
        let mut index = BTreeMap::<Cell, Vec<usize>>::new();
        for (id, (key, _)) in receivers.iter().enumerate() {
            let lo = (
                (key.0[0] as f32 / 1_000_000. / SPACING).floor() as i32,
                (key.0[1] as f32 / 1_000_000. / SPACING).floor() as i32,
            );
            let hi = (
                (key.0[3] as f32 / 1_000_000. / SPACING).floor() as i32,
                (key.0[4] as f32 / 1_000_000. / SPACING).floor() as i32,
            );
            for x in lo.0..=hi.0 {
                for y in lo.1..=hi.1 {
                    index.entry((x, y)).or_default().push(id);
                }
            }
        }
        eprintln!(
            "Graffiti indexed {} concrete receivers; maximum {MAX_VISIBLE} active nearby marks",
            receivers.len()
        );
        Self {
            receivers,
            index,
            cache: BTreeMap::new(),
        }
    }
    fn candidate(&self, cell: Cell) -> Vec<Candidate> {
        let mut best = BTreeMap::<Facing, Candidate>::new();
        for &id in self.index.get(&cell).into_iter().flatten() {
            let (key, triangles) = &self.receivers[id];
            let seed = seed(*key)
                ^ (cell.0 as u32).wrapping_mul(0x9e3779b9)
                ^ (cell.1 as u32).wrapping_mul(0x85ebca6b);
            let large = key.0[9] == 0 && key.0[5] - key.0[2] > 25_000;
            if sample(seed, 1) > if large { 0.8 } else { 0.3 } {
                continue;
            }
            let Some(mesh) = paint_panel_in_cell(triangles, seed, Some(cell)) else {
                continue;
            };
            let priority = (u8::from(!large), seed);
            let facing = Facing::from_normal(mesh.vertices[0].normal);
            if best
                .get(&facing)
                .is_some_and(|old| old.priority <= priority)
            {
                continue;
            }
            let center = mesh
                .vertices
                .iter()
                .fold(Vec3::ZERO, |p, v| p + Vec3::from_array(v.position))
                / 6.;
            best.insert(
                facing,
                Candidate {
                    mesh,
                    center,
                    priority,
                    facing,
                },
            );
        }
        best.into_values().collect()
    }
    pub(super) fn mesh(&mut self, center: [f32; 2]) -> Mesh {
        let cell = (
            (center[0] / SPACING).floor() as i32,
            (center[1] / SPACING).floor() as i32,
        );
        self.cache
            .retain(|&(x, y), _| (x - cell.0).abs() <= HALO && (y - cell.1).abs() <= HALO);
        for x in cell.0 - HALO..=cell.0 + HALO {
            for y in cell.1 - HALO..=cell.1 + HALO {
                if !self.cache.contains_key(&(x, y)) {
                    let candidate = self.candidate((x, y));
                    self.cache.insert((x, y), candidate);
                }
            }
        }
        let center = glam::Vec2::from_array(center);
        let mut active = Vec::new();
        for x in cell.0 - TILE_RADIUS..=cell.0 + TILE_RADIUS {
            for y in cell.1 - TILE_RADIUS..=cell.1 + TILE_RADIUS {
                for candidate in self.cache.get(&(x, y)).into_iter().flatten() {
                    let distance = candidate.center.truncate().distance_squared(center);
                    if distance > RADIUS * RADIUS {
                        continue;
                    }
                    // A fixed one-tile halo makes suppression independent of travel/cache order.
                    let suppressed = (-1..=1).any(|dx| {
                        (-1..=1).any(|dy| {
                            if dx == 0 && dy == 0 {
                                return false;
                            }
                            self.cache.get(&(x + dx, y + dy)).is_some_and(|others| {
                                others.iter().any(|other| {
                                    other.facing == candidate.facing
                                        && (other.priority, (x + dx, y + dy))
                                            < (candidate.priority, (x, y))
                                        && other
                                            .center
                                            .truncate()
                                            .distance_squared(candidate.center.truncate())
                                            < SPACING * SPACING
                                        && (other.center.z - candidate.center.z).abs() < 0.05
                                })
                            })
                        })
                    });
                    if !suppressed {
                        active.push((distance, (x, y, candidate.facing), candidate));
                    }
                }
            }
        }
        active.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut mesh = Mesh::default();
        for (_, _, candidate) in active.into_iter().take(MAX_VISIBLE) {
            mesh.vertices.extend_from_slice(&candidate.mesh.vertices);
        }
        mesh
    }
    #[cfg(test)]
    pub(super) fn cached_tiles(&self) -> usize {
        self.cache.len()
    }
}
pub(in crate::engine) struct Nearby {
    planner: Planner,
    pub gpu: GpuMesh,
    last_cell: Option<Cell>,
    active: usize,
}
impl Nearby {
    pub fn new(renderer: &Renderer, world: &Mesh) -> Self {
        Self {
            planner: Planner::new(world),
            gpu: renderer.upload_dynamic(&Mesh::default()),
            last_cell: None,
            active: 0,
        }
    }
    pub fn update(&mut self, renderer: &Renderer, center: [f32; 2]) {
        let cell = (
            (center[0] / 0.05).floor() as i32,
            (center[1] / 0.05).floor() as i32,
        );
        if self.last_cell == Some(cell) {
            return;
        }
        let mesh = self
            .planner
            .mesh([cell.0 as f32 * 0.05 + 0.025, cell.1 as f32 * 0.05 + 0.025]);
        self.active = mesh.vertices.len() / 6;
        debug_assert!(self.active <= MAX_VISIBLE);
        renderer.update_dynamic(&mut self.gpu, &mesh);
        self.last_cell = Some(cell);
    }
    pub fn visible(&self) -> impl Iterator<Item = &GpuMesh> {
        (self.active > 0).then_some(&self.gpu).into_iter()
    }
}
