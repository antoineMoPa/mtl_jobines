//! Street-facing edges of raised sidewalk bands, captured before map batching.
use crate::engine::render::{GpuMesh, Renderer};
use glam::Vec3;
use opencity::runtime::{Mesh, SurfaceKind, Vertex};
use std::collections::HashMap;

struct Curb {
    edge: [Vec3; 2],
    street: [Vec3; 2],
}

#[derive(Default)]
struct Tile {
    curbs: Vec<Curb>,
    gpu: Option<GpuMesh>,
}
pub(in crate::engine) struct Banks {
    tiles: HashMap<(i32, i32), Tile>,
    nearby: Vec<(i32, i32)>,
    center: Option<(i32, i32)>,
}
impl Banks {
    pub(in crate::engine) fn new(mesh: &Mesh) -> Self {
        let mut tiles: HashMap<(i32, i32), Tile> = HashMap::new();
        let mut joins: HashMap<_, (Vec3, Vec3)> = HashMap::new();
        for t in mesh.vertices.chunks_exact(3) {
            let bits = t[0].surface[3] as u32;
            if t[0].surface[0] != SurfaceKind::Sidewalk as u32 as f32 || bits & 3 != 2 {
                continue;
            }
            let width = ((bits >> 2) & 4095) as f32 * 0.0001;
            for (shift, across) in [(14, 0.), (19, width)] {
                if (bits >> shift) & 31 != 3 {
                    continue;
                }
                for i in 0..3 {
                    let j = (i + 1) % 3;
                    if [i, j]
                        .iter()
                        .any(|&k| (t[k].surface[2] - across).abs() > 0.00006)
                    {
                        continue;
                    }
                    let [a, b, c] = [i, j, (j + 1) % 3].map(|k| Vec3::from(t[k].position));
                    let d = (b - a).truncate();
                    if d.length_squared() < 1e-12 {
                        continue;
                    }
                    let outward = d.perp() / d.length();
                    let sign = outward.dot((a - c).truncate()).signum();
                    let street = (outward * sign).extend(0.);
                    let (a, b) = if (b - a).cross(street).z > 0. {
                        (a, b)
                    } else {
                        (b, a)
                    };
                    let count = (d.length() / 0.30).ceil().max(1.) as usize;
                    for part in 0..count {
                        let edge = [part, part + 1].map(|n| a.lerp(b, n as f32 / count as f32));
                        for &p in &edge {
                            let join = joins.entry(point_key(p)).or_insert((p, Vec3::ZERO));
                            join.1 += street;
                        }
                        let center = (edge[0] + edge[1]) * 0.5;
                        tiles.entry(tile(center)).or_default().curbs.push(Curb {
                            edge,
                            street: [street; 2],
                        });
                    }
                }
            }
        }
        // Use one cross-section at a shared endpoint, including between different GPU tiles.
        for curb in tiles.values_mut().flat_map(|t| &mut t.curbs) {
            for (p, street) in curb.edge.iter_mut().zip(&mut curb.street) {
                let (center, sum) = joins[&point_key(*p)];
                *p = center;
                if sum.dot(*street) > 0.001 {
                    *street = sum / sum.length();
                }
            }
        }
        Self {
            tiles,
            nearby: Vec::new(),
            center: None,
        }
    }
    pub(in crate::engine) fn update(&mut self, r: &Renderer, eye: Vec3) {
        let center = tile(eye);
        if self.center == Some(center) {
            return;
        }
        self.center = Some(center);
        for key in self.nearby.drain(..) {
            if (key.0 - center.0).abs() > 3 || (key.1 - center.1).abs() > 3 {
                self.tiles.get_mut(&key).unwrap().gpu = None;
            }
        }
        for x in center.0 - 3..=center.0 + 3 {
            for y in center.1 - 3..=center.1 + 3 {
                let Some(t) = self.tiles.get_mut(&(x, y)) else {
                    continue;
                };
                t.gpu
                    .get_or_insert_with(|| r.upload(&mesh(&t.curbs, |p| r.snow.exposed(p))));
                self.nearby.push((x, y));
            }
        }
    }
    pub(in crate::engine) fn visible(&self) -> impl Iterator<Item = &GpuMesh> {
        self.nearby
            .iter()
            .filter_map(|key| self.tiles[key].gpu.as_ref())
    }
}
fn tile(p: Vec3) -> (i32, i32) {
    ((p.x / 0.5).floor() as i32, (p.y / 0.5).floor() as i32)
}

fn point_key(p: Vec3) -> [i32; 3] {
    (p * 100000.).round().as_ivec3().to_array()
}

const PROFILE_POINTS: usize = 9;
type Section = [Vertex; PROFILE_POINTS];

fn section(p: Vec3, street: Vec3, grade: f32) -> Section {
    let tangent = Vec3::new(street.y, -street.x, grade);
    let broad = p.x * 75. + p.y * 39.;
    let lumps = p.x * 219. - p.y * 190.;
    let height = 0.0043 + 0.0014 * broad.sin() + 0.0007 * lumps.cos();
    let rise = 0.0014 * broad.cos() * (75. * tangent.x + 39. * tangent.y)
        - 0.0007 * lumps.sin() * (219. * tangent.x - 190. * tangent.y);
    let width = 1. + 0.12 * lumps.cos();
    std::array::from_fn(|i| {
        let angle = std::f32::consts::PI * i as f32 / (PROFILE_POINTS - 1) as f32;
        let (sin, cos) = angle.sin_cos();
        let u = (1. - cos) * 0.5;
        let pos =
            p + street * ((-0.003 + 0.011 * u) * width) + Vec3::Z * (height * sin - 0.0016 * u);
        // Derivatives of the rounded profile give the crest and both shoulders continuous lighting.
        let across = street * (0.0055 * width * sin) + Vec3::Z * (height * cos - 0.0008 * sin);
        let normal = (tangent + Vec3::Z * (rise * sin)).cross(across);
        Vertex {
            position: pos.to_array(),
            normal: (normal / normal.length()).to_array(),
            color: [0.63, 0.70, 0.77],
            surface: [SurfaceKind::Prop as u32 as f32, 0., 0., 0.],
        }
    })
}

fn mesh(curbs: &[Curb], exposed: impl Fn(Vec3) -> bool) -> Mesh {
    let mut mesh = Mesh::default();
    let mut caps = HashMap::new();
    for curb in curbs {
        let [a, b] = curb.edge;
        let length = (b - a).truncate().length();
        let grade = (b.z - a.z) / length;
        let steps = (length / 0.012).ceil().max(1.) as usize;
        let mut prev = section(a, curb.street[0], grade);
        let mut was_exposed = false;
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let p = if i == steps { b } else { a.lerp(b, t) };
            let next = section(p, curb.street[0].lerp(curb.street[1], t), grade);
            let visible = exposed(a.lerp(b, (i as f32 - 0.5) / steps as f32));
            if visible != was_exposed {
                boundary(&mut caps, prev, visible);
            }
            if visible {
                for j in 0..PROFILE_POINTS {
                    let k = (j + 1) % PROFILE_POINTS;
                    let mut face = [prev[j], next[j], next[k], prev[j], next[k], prev[k]];
                    if k == 0 {
                        for v in &mut face {
                            v.normal = [0., 0., -1.];
                        }
                    }
                    mesh.vertices.extend(face);
                }
            }
            was_exposed = visible;
            prev = next;
        }
        if was_exposed {
            boundary(&mut caps, prev, false);
        }
    }
    for (ring, start) in caps.into_values() {
        for i in 1..PROFILE_POINTS - 1 {
            let ids = if start { [0, i, i + 1] } else { [0, i + 1, i] };
            let mut tri = ids.map(|j| ring[j]);
            let [a, b, c] = tri.map(|v| Vec3::from(v.position));
            let n = (b - a).cross(c - a);
            for v in &mut tri {
                v.normal = (n / n.length()).into();
            }
            mesh.vertices.extend(tri);
        }
    }
    mesh
}

// Matching ends inside a tile already share their rim. Cap only the exposed boundaries.
fn boundary(caps: &mut HashMap<[[i32; 3]; 2], (Section, bool)>, ring: Section, start: bool) {
    let key = [0, PROFILE_POINTS - 1].map(|i| point_key(ring[i].position.into()));
    if caps.remove(&key).is_none() {
        caps.insert(key, (ring, start));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shaded_crown_and_closed_edges_survive_a_shelter_gap() {
        let curb = |lo, hi| Curb {
            edge: [Vec3::X * lo, Vec3::X * hi],
            street: [Vec3::Y; 2],
        };
        let mesh = mesh(&[curb(0., 0.03), curb(0.03, 0.09)], |p| {
            !(0.045..0.065).contains(&p.x)
        });
        let mut edges = HashMap::<_, (usize, i32)>::new();
        for tri in mesh.vertices.chunks_exact(3) {
            for i in 0..3 {
                let a = point_key(tri[i].position.into());
                let b = point_key(tri[(i + 1) % 3].position.into());
                assert_ne!(a, b);
                let entry = edges.entry([a.min(b), a.max(b)]).or_default();
                entry.0 += 1;
                entry.1 += if a < b { 1 } else { -1 };
            }
        }
        assert!(!edges.is_empty());
        assert!(
            edges.values().all(|&edge| edge == (2, 0)),
            "each rim and bottom edge must have two oppositely wound faces"
        );
        let top = &mesh.vertices[..3];
        assert!(Vec3::from(top[0].normal).dot(top[2].normal.into()) < 0.99);
        assert!(top
            .iter()
            .all(|v| (Vec3::from(v.normal).length() - 1.).abs() < 1e-5));
    }
}
