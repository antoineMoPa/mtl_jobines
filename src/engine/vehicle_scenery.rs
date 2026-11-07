//! Immutable catalog geometry and small per-part instance buffers shared by both render passes.
use super::render::{GpuMesh, Renderer};
use crate::dynamic_world::{catalog, DynamicWorld};
struct Part {
    mesh: GpuMesh,
    transforms: Vec<[[f32; 4]; 4]>,
}
pub(super) struct VehicleScenery {
    models: Vec<Vec<Part>>,
}
impl VehicleScenery {
    pub fn new(renderer: &Renderer) -> Self {
        Self {
            models: catalog::definitions()
                .iter()
                .map(|definition| {
                    definition
                        .part_vertices()
                        .map(|vertices| Part {
                            mesh: renderer.upload_instanced(vertices),
                            transforms: Vec::new(),
                        })
                        .collect()
                })
                .collect(),
        }
    }
    pub fn update(&mut self, renderer: &Renderer, world: &DynamicWorld) {
        for part in self.models.iter_mut().flatten() {
            part.transforms.clear();
        }
        for (model, transforms, opacity) in world.vehicle_instances() {
            for (part, transform) in self.models[model].iter_mut().zip(transforms) {
                let mut columns = transform.to_cols_array_2d();
                // Affine W is spare: vehicle shaders use XYZ and carry W as coverage.
                columns[3][3] = opacity;
                part.transforms.push(columns);
            }
        }
        for part in self.models.iter_mut().flatten() {
            renderer.update_instances(&mut part.mesh, &part.transforms);
        }
    }
    pub fn visible(&self) -> impl Iterator<Item = &GpuMesh> {
        self.models
            .iter()
            .flatten()
            .filter(|p| !p.transforms.is_empty())
            .map(|p| &p.mesh)
    }
}
