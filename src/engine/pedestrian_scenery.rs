//! Shared skinned meshes, per-actor poses, one texture array; shared render lifecycle.
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
    poses: Poses,
    texture: wgpu::TextureView,
    sampler: wgpu::Sampler,
}
enum Poses {
    Buffer(wgpu::Buffer),
    Texture(wgpu::Texture, wgpu::TextureView),
}
// A row holds one actor's bone palette. WebGL 2 has vertex texture reads,
// but no storage buffers. Keep the same matrix indexing on both backends.
const POSE_WIDTH: u32 = JOINT_LIMIT as u32 * 4;
pub(super) fn shader(device: &wgpu::Device) -> String {
    shader_for_storage(device.limits().max_storage_buffers_per_shader_stage > 0)
}
pub(super) fn shader_for_storage(storage: bool) -> String {
    let source = include_str!("pedestrian.wgsl");
    if storage {
        return source.to_owned();
    }
    let texture_source = format!("const POSE_JOINTS: u32 = {JOINT_LIMIT}u;\n");
    texture_source
        + &source
            .replace(
                "@group(0) @binding(2) var<storage,read> pedestrian_poses: array<mat4x4<f32>>;",
                "@group(0) @binding(2) var pedestrian_poses: texture_2d<f32>;\n\
         fn pedestrian_pose(index: u32) -> mat4x4<f32> {\n\
             let p = vec2<i32>(i32(index % POSE_JOINTS) * 4, i32(index / POSE_JOINTS));\n\
             return mat4x4<f32>(textureLoad(pedestrian_poses,p,0),\n\
                 textureLoad(pedestrian_poses,p+vec2<i32>(1,0),0),\n\
                 textureLoad(pedestrian_poses,p+vec2<i32>(2,0),0),\n\
                 textureLoad(pedestrian_poses,p+vec2<i32>(3,0),0));\n\
         }",
            )
            .replace(
                "pedestrian_poses[base+skin.joints.x]",
                "pedestrian_pose(base+skin.joints.x)",
            )
            .replace(
                "pedestrian_poses[base+skin.joints.y]",
                "pedestrian_pose(base+skin.joints.y)",
            )
            .replace(
                "pedestrian_poses[base+skin.joints.z]",
                "pedestrian_pose(base+skin.joints.z)",
            )
            .replace(
                "pedestrian_poses[base+skin.joints.w]",
                "pedestrian_pose(base+skin.joints.w)",
            )
}
impl Resources {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, montreal_actors: bool) -> Self {
        let models = if montreal_actors {
            pedestrian_catalog::definitions()
        } else {
            &[]
        };
        let (texture, sampler) = super::vehicle_textures::upload_pngs(
            device,
            queue,
            models.iter().map(|d| d.texture_png.as_slice()),
        );
        let poses = if device.limits().max_storage_buffers_per_shader_stage == 0 {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pedestrian bone palettes"),
                size: wgpu::Extent3d {
                    width: POSE_WIDTH,
                    height: POSE_CAPACITY.div_ceil(JOINT_LIMIT) as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            Poses::Texture(texture, view)
        } else {
            Poses::Buffer(
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("pedestrian bone palettes"),
                    contents: bytemuck::cast_slice(&vec![
                        Mat4::IDENTITY.to_cols_array_2d();
                        POSE_CAPACITY
                    ]),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                }),
            )
        };
        Self {
            poses,
            texture,
            sampler,
        }
    }
    pub fn layout(device: &wgpu::Device) -> [wgpu::BindGroupLayoutEntry; 3] {
        [
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: if device.limits().max_storage_buffers_per_shader_stage == 0 {
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }
                } else {
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    }
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
                resource: match &self.poses {
                    Poses::Buffer(buffer) => buffer.as_entire_binding(),
                    Poses::Texture(_, view) => wgpu::BindingResource::TextureView(view),
                },
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
    pub fn upload_poses(&self, queue: &wgpu::Queue, poses: &[[[f32; 4]; 4]]) {
        if poses.is_empty() {
            return;
        }
        assert!(poses.len() <= POSE_CAPACITY && poses.len() % JOINT_LIMIT == 0);
        let bytes = bytemuck::cast_slice(poses);
        match &self.poses {
            Poses::Buffer(buffer) => queue.write_buffer(buffer, 0, bytes),
            Poses::Texture(texture, _) => queue.write_texture(
                texture.as_image_copy(),
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(POSE_WIDTH * 16),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: POSE_WIDTH,
                    height: (poses.len() / JOINT_LIMIT) as u32,
                    depth_or_array_layers: 1,
                },
            ),
        }
    }
}
struct Model {
    mesh: GpuMesh,
    transforms: Vec<[[f32; 4]; 4]>,
    player_shadow: Option<usize>,
}
pub(super) struct PedestrianScenery {
    models: Vec<Model>,
    poses: Vec<[[f32; 4]; 4]>,
    frozen_poses: Vec<[Mat4; JOINT_LIMIT]>,
}
impl PedestrianScenery {
    pub fn new(renderer: &Renderer) -> Self {
        Self {
            models: pedestrian_catalog::definitions()
                .iter()
                .map(|d| Model {
                    mesh: renderer.upload_skinned(&d.vertices, &d.skin),
                    transforms: Vec::new(),
                    player_shadow: None,
                })
                .collect(),
            poses: Vec::with_capacity(POSE_CAPACITY),
            frozen_poses: Vec::new(),
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
        self.update_with_stage_and_player_pose(renderer, world, stage, Mat4::IDENTITY);
    }
    pub fn update_with_stage_and_player_pose(
        &mut self,
        renderer: &Renderer,
        world: &DynamicWorld,
        stage: Option<super::fabula::Stage<'_>>,
        player_delta: Mat4,
    ) {
        if stage.is_none() {
            self.frozen_poses.clear();
        }
        self.poses.clear();
        for model in &mut self.models {
            model.transforms.clear();
            model.player_shadow = None;
        }
        let player_model = pedestrian_catalog::player_model(world.season());
        for (index, actor) in world.pedestrian_instances().enumerate() {
            // The world actor list reserves the seasonal player model for the avatar.
            let player = actor.model == player_model;
            let staged = stage
                .as_ref()
                .filter(|_| index == 0 && !world.is_driving())
                .and_then(|s| s.player());
            let transform = if world.is_driving() && player {
                player_delta * actor.transform
            } else if let (Some(s), Some(c)) = (stage, staged) {
                s.anchor.render_delta(c.offset, c.yaw) * actor.transform
            } else {
                actor.transform
            };
            assert!(self.poses.len() + JOINT_LIMIT <= POSE_CAPACITY);
            let mut columns = transform.to_cols_array_2d();
            columns[0][3] = actor.model as f32;
            columns[1][3] = if index == 0 && stage.is_none() && world.punching() {
                -1.
            } else {
                f32::from(actor.zombie)
            };
            columns[2][3] = self.poses.len() as f32;
            columns[3][3] = actor.opacity;
            let model = &mut self.models[actor.model];
            if player {
                model.player_shadow = Some(model.transforms.len());
            }
            model.transforms.push(columns);
            let world_pose = if stage.is_some() && index < self.frozen_poses.len() {
                self.frozen_poses[index]
            } else {
                let pose = actor.pose.copied().unwrap_or_else(|| {
                    pedestrian_catalog::definitions()[actor.model].pose(
                        actor.idle_time,
                        actor.walk_time,
                        actor.movement,
                    )
                });
                if stage.is_some() {
                    self.frozen_poses.push(pose);
                }
                pose
            };
            let pose = staged
                .map(|c| c.pose.palette(actor.model, stage.unwrap().elapsed))
                .unwrap_or(world_pose);
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
        renderer
            .pedestrians
            .upload_poses(&renderer.queue, &self.poses);
        for model in &mut self.models {
            renderer.update_actor_instances(
                &mut model.mesh,
                &model.transforms,
                model.player_shadow,
            );
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod albedo_tests;
