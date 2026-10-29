//! Immutable map geometry is built once, then drawn in spatial batches.
//! Driving only selects GPU buffers; it never rebuilds or rasterizes the map.
use super::render::{GpuMesh, Renderer};
use crate::hermes::Hermes;
use opencity::runtime::Mesh;
use std::collections::BTreeMap;
// Keep the existing 200 m radius-selection groups, then cull their 50 m
// GPU batches independently. Splitting must not shorten detail draw distance.
const TILE_SPAN: f32 = 2.;
const DRAW_SPAN: f32 = 0.5;
struct Batch {
    bounds: [f32; 4],
    meshes: Vec<GpuMesh>,
}
pub struct Scenery {
    world: Vec<Batch>,
    details: Vec<Batch>,
}
fn partition(mesh: Mesh) -> Vec<([f32; 4], Vec<Mesh>)> {
    let mut tiles = BTreeMap::<(i32, i32), BTreeMap<(i32, i32), Mesh>>::new();
    for triangle in mesh.vertices.chunks_exact(3) {
        let center = [
            triangle.iter().map(|v| v.position[0]).sum::<f32>() / 3.,
            triangle.iter().map(|v| v.position[1]).sum::<f32>() / 3.,
        ];
        let key = |span: f32| {
            (
                (center[0] / span).floor() as i32,
                (center[1] / span).floor() as i32,
            )
        };
        tiles
            .entry(key(TILE_SPAN))
            .or_default()
            .entry(key(DRAW_SPAN))
            .or_default()
            .vertices
            .extend_from_slice(triangle);
    }
    drop(mesh);
    tiles
        .into_values()
        .map(|meshes| {
            let mut bounds = [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ];
            for mesh in meshes.values() {
                for v in &mesh.vertices {
                    for i in 0..2 {
                        bounds[i] = bounds[i].min(v.position[i]);
                        bounds[i + 2] = bounds[i + 2].max(v.position[i]);
                    }
                }
            }
            (bounds, meshes.into_values().collect())
        })
        .collect()
}
fn batches(renderer: &Renderer, mesh: Mesh) -> Vec<Batch> {
    partition(mesh)
        .into_iter()
        .map(|(bounds, meshes)| Batch {
            bounds,
            meshes: meshes.iter().map(|mesh| renderer.upload(mesh)).collect(),
        })
        .collect()
}
impl Scenery {
    pub fn new(renderer: &Renderer, hermes: &mut Hermes) -> Self {
        let (world, details) = hermes.take_geometry();
        Self {
            world: batches(renderer, world),
            details: batches(renderer, details),
        }
    }
    pub fn visible(&self, center: [f32; 2]) -> Vec<&GpuMesh> {
        let intersects = |b: &Batch, radius: f32| {
            b.bounds[0] <= center[0] + radius
                && b.bounds[2] >= center[0] - radius
                && b.bounds[1] <= center[1] + radius
                && b.bounds[3] >= center[1] - radius
        };
        self.world
            .iter()
            .filter(|b| intersects(b, 8.))
            .chain(self.details.iter().filter(|b| intersects(b, 1.5)))
            .flat_map(|b| &b.meshes)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn small_draw_batches_preserve_original_radius_groups_and_triangles() {
        use opencity::runtime::{Mesh, Vertex};
        let mut mesh = Mesh::default();
        for x in [-0.1, 0.1, 1.7] {
            for position in [[x, 0.1, 0.], [x + 0.01, 0.1, 0.], [x, 0.11, 0.]] {
                mesh.vertices.push(Vertex {
                    position,
                    normal: [0., 0., 1.],
                    color: [1.; 3],
                    surface: [0.; 4],
                });
            }
        }
        let groups = super::partition(mesh);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].1.len(), 1);
        assert_eq!(groups[1].1.len(), 2);
        // Both positive-X draws still share the original wide selection bounds.
        assert_eq!(groups[1].0, [0.1, 0.1, 1.71, 0.11]);
        assert_eq!(
            groups
                .iter()
                .flat_map(|(_, meshes)| meshes)
                .map(|mesh| mesh.vertices.len())
                .sum::<usize>(),
            9
        );
        assert!(groups
            .iter()
            .flat_map(|(_, meshes)| meshes)
            .all(|mesh| mesh.vertices.len() == 3));
    }
}
