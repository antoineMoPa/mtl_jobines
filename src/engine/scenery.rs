//! Immutable map geometry is built once, then drawn in spatial batches.
//! Driving only selects GPU buffers; it never rebuilds or rasterizes the map.
use super::render::{GpuMesh, Renderer};
use crate::hermes::Hermes;
use opencity::runtime::Mesh;
use std::collections::BTreeMap;
const TILE_SPAN: f32 = 2.;
struct Batch {
    bounds: [f32; 4],
    mesh: GpuMesh,
}
pub struct Scenery {
    world: Vec<Batch>,
    details: Vec<Batch>,
}
fn batches(renderer: &Renderer, mesh: Mesh) -> Vec<Batch> {
    let mut tiles = BTreeMap::<(i32, i32), Mesh>::new();
    for triangle in mesh.vertices.chunks_exact(3) {
        let center = [
            triangle.iter().map(|v| v.position[0]).sum::<f32>() / 3.,
            triangle.iter().map(|v| v.position[1]).sum::<f32>() / 3.,
        ];
        let key = (
            (center[0] / TILE_SPAN).floor() as i32,
            (center[1] / TILE_SPAN).floor() as i32,
        );
        tiles
            .entry(key)
            .or_default()
            .vertices
            .extend_from_slice(triangle);
    }
    drop(mesh);
    tiles
        .into_values()
        .map(|mesh| {
            let mut bounds = [
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ];
            for v in &mesh.vertices {
                for i in 0..2 {
                    bounds[i] = bounds[i].min(v.position[i]);
                    bounds[i + 2] = bounds[i + 2].max(v.position[i]);
                }
            }
            Batch {
                bounds,
                mesh: renderer.upload(&mesh),
            }
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
            .map(|b| &b.mesh)
            .collect()
    }
}
