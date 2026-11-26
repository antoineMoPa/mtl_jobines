//! GPU geometry is uploaded once. Chairs only change compact instance matrices.
use super::super::render::{GpuMesh, Renderer};
use crate::dynamic_world::DynamicWorld;
#[derive(Default)]
pub(super) struct Scenery {
    meshes: Vec<GpuMesh>,
    transforms: Vec<Vec<[[f32; 4]; 4]>>,
}
impl Scenery {
    pub fn update(&mut self, renderer: &Renderer, world: &DynamicWorld) {
        for transforms in &mut self.transforms {
            transforms.clear();
        }
        let instances = world.chairlift_instances();
        if !instances.is_empty() && self.meshes.is_empty() {
            let models = world.chairlift_model_meshes();
            self.meshes = models
                .iter()
                .map(|m| renderer.upload_equipment(&m.vertices))
                .collect();
            self.transforms.resize_with(models.len(), Vec::new);
        }
        for (model, transform) in instances {
            self.transforms[model].push(transform.to_cols_array_2d());
        }
        for (mesh, transforms) in self.meshes.iter_mut().zip(&self.transforms) {
            renderer.update_instances(mesh, transforms);
        }
    }
    pub fn visible(&self) -> impl Iterator<Item = &GpuMesh> {
        self.meshes
            .iter()
            .zip(&self.transforms)
            .filter(|(_, t)| !t.is_empty())
            .map(|(m, _)| m)
    }
}
