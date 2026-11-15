//! Six immutable meshes, per-actor poses, one texture array; shared render lifecycle.
use super::render::{GpuMesh, Renderer};
use crate::dynamic_world::{
    pedestrian_catalog::{self, JOINT_LIMIT},
    DynamicWorld,
};
use glam::Mat4;
use wgpu::util::DeviceExt;

const POSE_CAPACITY: usize = (crate::dynamic_world::PEDESTRIAN_CAPACITY
    + super::fabula::staging::EXTRA_ACTORS)
    * JOINT_LIMIT;
pub(super) struct Resources {
    pub poses: wgpu::Buffer,
    texture: wgpu::TextureView,
    sampler: wgpu::Sampler,
}
impl Resources {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let (texture, sampler) = super::vehicle_textures::upload_pngs(
            device,
            queue,
            pedestrian_catalog::definitions()
                .iter()
                .map(|d| d.texture_png.as_slice()),
        );
        let poses = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("pedestrian bone palettes"),
            contents: bytemuck::cast_slice(&vec![Mat4::IDENTITY.to_cols_array_2d(); POSE_CAPACITY]),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            poses,
            texture,
            sampler,
        }
    }
    pub fn layout() -> [wgpu::BindGroupLayoutEntry; 3] {
        [
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ]
    }
    pub fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 3] {
        [
            wgpu::BindGroupEntry {
                binding: 2,
                resource: self.poses.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&self.texture),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
        ]
    }
}
struct Model {
    mesh: GpuMesh,
    transforms: Vec<[[f32; 4]; 4]>,
}
pub(super) struct PedestrianScenery {
    models: Vec<Model>,
    poses: Vec<[[f32; 4]; 4]>,
}
impl PedestrianScenery {
    pub fn new(renderer: &Renderer) -> Self {
        Self {
            models: pedestrian_catalog::definitions()
                .iter()
                .map(|d| Model {
                    mesh: renderer.upload_skinned(&d.vertices, &d.skin),
                    transforms: Vec::new(),
                })
                .collect(),
            poses: Vec::with_capacity(POSE_CAPACITY),
        }
    }
    pub fn update(&mut self, renderer: &Renderer, world: &DynamicWorld) {
        self.update_with_stage(renderer, world, None);
    }
    pub fn update_with_stage(
        &mut self,
        renderer: &Renderer,
        world: &DynamicWorld,
        stage: Option<super::fabula::Stage<'_>>,
    ) {
        self.poses.clear();
        for model in &mut self.models {
            model.transforms.clear();
        }
        for (index, actor) in world.pedestrian_instances().enumerate() {
            let staged = stage
                .as_ref()
                .filter(|_| index == 0 && !world.is_driving())
                .and_then(|s| s.player());
            let transform = if let (Some(s), Some(c)) = (stage, staged) {
                s.anchor.render_delta(c.offset, c.yaw) * actor.transform
            } else {
                actor.transform
            };
            assert!(self.poses.len() + JOINT_LIMIT <= POSE_CAPACITY);
            let mut columns = transform.to_cols_array_2d();
            columns[0][3] = actor.model as f32;
            columns[2][3] = self.poses.len() as f32;
            columns[3][3] = actor.opacity;
            self.models[actor.model].transforms.push(columns);
            let pose = staged
                .map(|c| c.pose.palette(actor.model, stage.unwrap().elapsed))
                .unwrap_or_else(|| {
                    actor.pose.copied().unwrap_or_else(|| {
                        pedestrian_catalog::definitions()[actor.model].pose(
                            actor.idle_time,
                            actor.walk_time,
                            actor.movement,
                        )
                    })
                });
            self.poses.extend(pose.map(|m| m.to_cols_array_2d()));
        }
        if let Some(stage) = stage {
            for (model, transform, pose) in stage.extra_characters() {
                let mut columns = transform.to_cols_array_2d();
                columns[0][3] = model as f32;
                columns[2][3] = self.poses.len() as f32;
                self.models[model].transforms.push(columns);
                self.poses.extend(
                    pose.palette(model, stage.elapsed)
                        .map(|m| m.to_cols_array_2d()),
                );
            }
        }
        if !self.poses.is_empty() {
            renderer.queue.write_buffer(
                &renderer.pedestrians.poses,
                0,
                bytemuck::cast_slice(&self.poses),
            );
        }
        for model in &mut self.models {
            renderer.update_instances(&mut model.mesh, &model.transforms);
        }
    }
    pub fn visible(&self) -> impl Iterator<Item = &GpuMesh> {
        self.models
            .iter()
            .filter(|m| !m.transforms.is_empty())
            .map(|m| &m.mesh)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
