//! Deterministic spatial clustering, with shared seam positions and pinned semantic surfaces.
use glam::Vec3;
use opencity::runtime::{SurfaceKind, Vertex};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
pub(super) enum Policy {
    Vehicle,
    Scenery,
}
#[derive(Clone, Copy)]
pub(super) enum Purpose {
    Camera,
    Shadow,
}
pub(super) struct Reduced {
    pub vertices: Vec<Vertex>,
    pub error: f32,
}
pub(super) struct Indexed {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub opaque: u32,
    pub paint: u32,
}
fn protected(triangle: &[Vertex], policy: Policy) -> bool {
    triangle.iter().any(|v| {
        let kind = v.surface[0];
        // Authored art (including paint 42), lamps and cutouts retain exact geometry/UVs.
        if kind >= 40.
            || kind == SurfaceKind::Light as u32 as f32
            || kind == SurfaceKind::Fence as u32 as f32
            || kind == SurfaceKind::Window as u32 as f32
            || kind == SurfaceKind::Glazing as u32 as f32
        {
            return true;
        }
        match policy {
            Policy::Vehicle => v.surface[1] < 0.999 || v.surface[3] == 64.,
            Policy::Scenery => {
                v.surface[3] < 0.
                    || (kind == SurfaceKind::Building as u32 as f32 && v.surface[3] == 2.)
                    || kind == SurfaceKind::Asphalt as u32 as f32
                    || kind == SurfaceKind::Marking as u32 as f32
                    || kind == SurfaceKind::Sidewalk as u32 as f32
                    || kind == SurfaceKind::Grass as u32 as f32
                    || kind == SurfaceKind::Water as u32 as f32
                    || kind == SurfaceKind::Sand as u32 as f32
            }
        }
    })
}
pub(super) fn reducible(source: &[Vertex], policy: Policy) -> bool {
    source
        .chunks_exact(3)
        .any(|triangle| !protected(triangle, policy))
}
fn position_key(v: &Vertex) -> [u32; 3] {
    v.position.map(f32::to_bits)
}

#[derive(Hash, PartialEq, Eq)]
enum ClusterKey {
    Pinned([u32; 3]),
    Cell([i32; 3], u8),
}
struct Cluster {
    sum: [f64; 3],
    count: u32,
    boundary: u8,
}

pub(super) fn reduce(source: &[Vertex], cell: f32, policy: Policy) -> Reduced {
    assert!(cell.is_finite() && cell > 0.);
    assert_eq!(source.len() % 3, 0);
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for v in source {
        let p = Vec3::from_array(v.position);
        lo = lo.min(p);
        hi = hi.max(p);
    }
    let pinned: HashSet<_> = source
        .chunks_exact(3)
        .filter(|triangle| protected(triangle, policy))
        .flatten()
        .map(position_key)
        .collect();
    let mut map = HashMap::new();
    let mut clusters: Vec<Cluster> = Vec::new();
    let mut ids = Vec::with_capacity(source.len());
    for v in source {
        let p = v.position;
        let boundary = (0..3).fold(0, |bits, axis| {
            bits | if p[axis] == lo[axis] {
                1 << (axis * 2)
            } else {
                0
            } | if p[axis] == hi[axis] {
                2 << (axis * 2)
            } else {
                0
            }
        });
        let key = if pinned.contains(&position_key(v)) {
            ClusterKey::Pinned(position_key(v))
        } else {
            ClusterKey::Cell(p.map(|x| (x / cell).floor() as i32), boundary)
        };
        let next = clusters.len();
        let id = *map.entry(key).or_insert_with(|| {
            clusters.push(Cluster {
                sum: [0.; 3],
                count: 0,
                boundary,
            });
            next
        });
        let cluster = &mut clusters[id];
        for (sum, value) in cluster.sum.iter_mut().zip(p) {
            *sum += f64::from(value);
        }
        cluster.count += 1;
        ids.push(id);
    }
    let positions: Vec<[f32; 3]> = clusters
        .iter()
        .map(|cluster| {
            std::array::from_fn(|axis| {
                if cluster.boundary & (1 << (axis * 2)) != 0 {
                    lo[axis]
                } else if cluster.boundary & (2 << (axis * 2)) != 0 {
                    hi[axis]
                } else {
                    (cluster.sum[axis] / f64::from(cluster.count)) as f32
                }
            })
        })
        .collect();
    let mut error = 0_f32;
    let mut vertices = Vec::with_capacity(source.len());
    for (triangle, group) in source.chunks_exact(3).zip(ids.chunks_exact(3)) {
        let exact = protected(triangle, policy);
        let mut result: [Vertex; 3] = triangle.try_into().unwrap();
        for (vertex, &id) in result.iter_mut().zip(group) {
            let before = Vec3::from_array(vertex.position);
            if !exact {
                vertex.position = positions[id];
            }
            error = error.max(before.distance(Vec3::from_array(vertex.position)));
        }
        if !exact {
            if group[0] == group[1] || group[0] == group[2] || group[1] == group[2] {
                continue;
            }
            let p = result.map(|v| Vec3::from_array(v.position));
            let old: [Vec3; 3] = std::array::from_fn(|i| Vec3::from_array(triangle[i].position));
            if (p[1] - p[0])
                .cross(p[2] - p[0])
                .dot((old[1] - old[0]).cross(old[2] - old[0]))
                <= 0.
            {
                continue;
            }
        }
        vertices.extend_from_slice(&result);
    }
    // Avoid deforming an asset when clustering buys almost no triangle reduction.
    if vertices.len() * 100 > source.len() * 90 {
        Reduced {
            vertices: source.to_vec(),
            error: 0.,
        }
    } else {
        Reduced { vertices, error }
    }
}

impl Indexed {
    pub(super) fn new(source: &[Vertex], opaque: u32, paint: u32, purpose: Purpose) -> Self {
        let mut result = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            opaque: 0,
            paint: 0,
        };
        let mut vertices = HashMap::new();
        let mut triangles = HashSet::new();
        for (i, triangle) in source.chunks_exact(3).enumerate() {
            let ids: [u32; 3] = std::array::from_fn(|corner| {
                let v = triangle[corner];
                let mut values = v
                    .position
                    .iter()
                    .chain(&v.normal)
                    .chain(&v.color)
                    .chain(&v.surface);
                let mut key: [u32; 13] = std::array::from_fn(|_| values.next().unwrap().to_bits());
                if matches!(purpose, Purpose::Shadow) {
                    key[3..9].fill(0);
                }
                let next = result.vertices.len() as u32;
                *vertices.entry(key).or_insert_with(|| {
                    result.vertices.push(v);
                    next
                })
            });
            let cyclic = [ids, [ids[1], ids[2], ids[0]], [ids[2], ids[0], ids[1]]]
                .into_iter()
                .min()
                .unwrap();
            if matches!(purpose, Purpose::Shadow) && !triangles.insert(cyclic) {
                continue;
            }
            result.indices.extend_from_slice(&ids);
            if i * 3 < opaque as usize {
                result.opaque = result.indices.len() as u32;
            }
            if i * 3 < paint as usize {
                result.paint = result.indices.len() as u32;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests;
