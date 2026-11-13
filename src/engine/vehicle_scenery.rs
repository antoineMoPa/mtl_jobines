//! Immutable catalog geometry and small per-part instance buffers shared by both render passes.
use super::render::{GpuMesh, Renderer};
use crate::dynamic_world::{catalog, DynamicWorld};
#[cfg(test)]
mod tests;
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
                        .zip(definition.part_shadow_vertices())
                        .map(|(vertices, shadow)| Part {
                            mesh: renderer.upload_vehicle_lod(vertices, shadow),
                            transforms: Vec::new(),
                        })
                        .collect()
                })
                .collect(),
        }
    }
    pub fn update(&mut self, renderer: &Renderer, world: &DynamicWorld) {
        renderer.local_lights.set(world.local_lights());
        for part in self.models.iter_mut().flatten() {
            part.transforms.clear();
        }
        for (model, transforms, opacity, headlights, player, paint) in world.vehicle_instances() {
            for (part, transform) in self.models[model].iter_mut().zip(transforms) {
                let mut columns = transform.to_cols_array_2d();
                // Affine W is spare: vehicle shaders use XYZ and carry W as coverage.
                columns[3][3] = opacity;
                columns[0][3] = if headlights { 1. } else { 0. };
                // XYZ alone participates in geometry; Y.w pins player render detail.
                columns[1][3] = if player { 1. } else { 0. };
                columns[2][3] = f32::from(paint);
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
