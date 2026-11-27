//! Immutable catalog geometry and small per-part instance buffers shared by both render passes.
use super::render::{GpuMesh, Renderer};
use crate::dynamic_world::{catalog, DynamicWorld};
use glam::Mat4;
mod chairlift;
#[cfg(test)]
mod tests;
struct Part {
    mesh: GpuMesh,
    transforms: Vec<[[f32; 4]; 4]>,
    player_shadow: Option<usize>,
}
pub(super) struct VehicleScenery {
    models: Vec<Vec<Part>>,
    cutscene_cached: bool,
    chairlift: chairlift::Scenery,
    pedestrians: super::pedestrian_scenery::PedestrianScenery,
}
impl VehicleScenery {
    pub fn new(renderer: &Renderer) -> Self {
        Self {
            cutscene_cached: false,
            chairlift: Default::default(),
            pedestrians: super::pedestrian_scenery::PedestrianScenery::new(renderer),
            models: (0..catalog::definitions().len())
                .map(|_| Vec::new())
                .collect(),
        }
    }
    fn ensure_model(&mut self, renderer: &Renderer, model: usize) {
        if self.models[model].is_empty() {
            let definition = &catalog::definitions()[model];
            self.models[model] = definition
                .part_vertices()
                .zip(definition.part_shadow_vertices())
                .map(|(vertices, shadow)| Part {
                    mesh: renderer.upload_vehicle_lod(vertices, shadow),
                    transforms: Vec::new(),
                    player_shadow: None,
                })
                .collect();
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
        self.chairlift.update(renderer, world);
        let pedestrians = mtl_world::telemetry::span("scene.pedestrians");
        self.pedestrians.update_with_stage_and_player_pose(
            renderer,
            world,
            stage,
            player_pose_delta,
        );
        drop(pedestrians);
        let _span = mtl_world::telemetry::span("scene.vehicle_instances");
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
            part.player_shadow = None;
        }
        let mut hulls = [crate::dynamic_world::Waterline::default(); 8];
        let mut hull_slots = hulls.iter_mut();
        for ((model, transforms, opacity, headlights, player, paint), patrol, vehicle) in
            world.vehicle_render_instances()
        {
            self.ensure_model(renderer, model);
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
            for (i, (part, transform)) in self.models[model].iter_mut().zip(transforms).enumerate()
            {
                if catalog::definitions()[model].cart_part(i) {
                    continue;
                }
                let transform = if player {
                    player_pose_delta * transform
                } else {
                    transform
                };
                let Some((transform, dents)) = vehicle.damaged_instance(i, transform) else {
                    continue;
                };
                let mut columns = transform.to_cols_array_2d();
                // Affine W is spare: vehicle shaders use XYZ and carry W as coverage.
                columns[3][3] = opacity;
                columns[0][3] = if headlights { 1. } else { 0. };
                // XYZ alone participates in geometry; Y.w pins player render detail.
                columns[1][3] = if patrol {
                    2.
                } else if player {
                    1.
                } else {
                    0.
                };
                columns[2][3] = ((dents << 2) | u32::from(paint)) as f32;
                if player && world.is_driving() {
                    part.player_shadow = Some(part.transforms.len());
                }
                part.transforms.push(columns);
            }
        }
        for (model, part, transform, player) in world
            .baggage_cart_instances()
            .chain(world.baggage_luggage_instances())
        {
            let pose = if player {
                player_pose_delta * transform
            } else {
                transform
            };
            let mut columns = pose.to_cols_array_2d();
            columns[1][3] = if player { 1. } else { 0. };
            self.ensure_model(renderer, model);
            self.models[model][part].transforms.push(columns);
        }
        renderer.local_lights.hulls.set(hulls);
        for part in self.models.iter_mut().flatten() {
            renderer.update_actor_instances(&mut part.mesh, &part.transforms, part.player_shadow);
        }
    }
    pub fn visible(&self) -> impl Iterator<Item = &GpuMesh> {
        self.models
            .iter()
            .flatten()
            .filter(|p| !p.transforms.is_empty())
            .map(|p| &p.mesh)
            .chain(self.pedestrians.visible())
            .chain(self.chairlift.visible())
    }
}
