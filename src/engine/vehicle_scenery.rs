//! Immutable catalog geometry and small per-part instance buffers shared by both render passes.
use super::render::{GpuMesh, Renderer};
use crate::dynamic_world::{catalog, DynamicWorld};
use glam::Mat4;
#[cfg(test)]
mod tests;
struct Part {
    mesh: GpuMesh,
    transforms: Vec<[[f32; 4]; 4]>,
}
pub(super) struct VehicleScenery {
    models: Vec<Vec<Part>>,
    cutscene_cached: bool,
    pedestrians: super::pedestrian_scenery::PedestrianScenery,
}
impl VehicleScenery {
    pub fn new(renderer: &Renderer) -> Self {
        Self {
            cutscene_cached: false,
            pedestrians: super::pedestrian_scenery::PedestrianScenery::new(renderer),
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
        self.update_with_player_pose(renderer, world, Mat4::IDENTITY);
    }
    pub fn update_with_player_pose(
        &mut self,
        renderer: &Renderer,
        world: &DynamicWorld,
        player_pose_delta: Mat4,
    ) {
        self.update_with_stage(renderer, world, player_pose_delta, None);
    }
    pub fn update_with_stage(
        &mut self,
        renderer: &Renderer,
        world: &DynamicWorld,
        player_pose_delta: Mat4,
        stage: Option<super::fabula::Stage<'_>>,
    ) {
        self.pedestrians.update_with_stage_and_player_pose(
            renderer,
            world,
            stage,
            player_pose_delta,
        );
        if stage.is_some() && self.cutscene_cached {
            return;
        }
        self.cutscene_cached = stage.is_some();
        if world.season() == crate::season::Season::Winter {
            let center = world.player_position() / crate::dynamic_world::METERS_PER_UNIT;
            renderer
                .snow
                .update(&renderer.queue, [center.x, center.y], &world.snow);
        }
        renderer.local_lights.set(world.local_lights());
        renderer.local_lights.wakes.set(world.water_wakes());
        for part in self.models.iter_mut().flatten() {
            part.transforms.clear();
        }
        let mut hulls = [crate::dynamic_world::Waterline::default(); 8];
        let mut hull_slots = hulls.iter_mut();
        for (model, transforms, opacity, headlights, player, paint) in world.vehicle_instances() {
            if let (Some(hull), Some(level)) =
                (&catalog::definitions()[model].water_hull, world.water_level)
            {
                let pose = if player {
                    player_pose_delta * transforms[0]
                } else {
                    transforms[0]
                };
                if let Some(line) = hull.waterline(pose, level) {
                    if let Some(slot) = hull_slots.next() {
                        *slot = line;
                    }
                }
            }
            for (part, transform) in self.models[model].iter_mut().zip(transforms) {
                let transform = if player {
                    player_pose_delta * transform
                } else {
                    transform
                };
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
        renderer.local_lights.hulls.set(hulls);
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
            .chain(self.pedestrians.visible())
    }
}
