//! Bounded world-keyed flakes, drawn after fog in the existing composite pass.
use super::super::snow::Snow;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

const CANDIDATES: u32 = 24 * 24 * 8 * 3;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Settings {
    matrix: [[f32; 4]; 4],
    eye_time: [f32; 4],
    viewport: [f32; 4],
    weather: [f32; 4],
}
pub(super) struct Snowfall {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    depth_layout: wgpu::BindGroupLayout,
    depth_group: wgpu::BindGroup,
    indices: wgpu::Buffer,
}
impl Snowfall {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        depth: &wgpu::TextureView,
        snow: &Snow,
    ) -> Self {
        let buffer_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world snow field"),
            entries: &[
                buffer_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                buffer_entry(2, wgpu::ShaderStages::VERTEX),
            ],
        });
        let depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world snow occlusion"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: true,
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world snow settings"),
            size: std::mem::size_of::<Settings>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (exposure, cache) = snow.exposure_resources();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world snow field"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(exposure),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: cache.as_entire_binding(),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world snow billboards"),
            source: wgpu::ShaderSource::Wgsl(include_str!("snowfall.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world snow"),
            bind_group_layouts: &[Some(&layout), Some(&depth_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("world snow"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shared snow quad corners"),
            contents: bytemuck::cast_slice(&[0_u16, 1, 2, 0, 2, 3]),
            usage: wgpu::BufferUsages::INDEX,
        });
        let depth_group = Self::depth_group(device, &depth_layout, depth);
        Self {
            pipeline,
            uniform,
            group,
            depth_layout,
            depth_group,
            indices,
        }
    }
    fn depth_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        depth: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world snow depth"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(depth),
            }],
        })
    }
    pub fn resize(&mut self, device: &wgpu::Device, depth: &wgpu::TextureView) {
        self.depth_group = Self::depth_group(device, &self.depth_layout, depth);
    }
    pub fn prepare(
        &self,
        queue: &wgpu::Queue,
        matrix: Mat4,
        eye: Vec3,
        time: f32,
        width: u32,
        height: u32,
        daylight: f32,
    ) {
        let data = Settings {
            matrix: matrix.to_cols_array_2d(),
            eye_time: [eye.x, eye.y, eye.z, time],
            viewport: [
                width as f32,
                height as f32,
                height as f32 / (2. * (29_f32.to_radians()).tan()),
                (height as f32 / 530.).clamp(1., 2.),
            ],
            weather: [daylight, 0.78, 0., 0.],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
    }
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_bind_group(1, &self.depth_group, &[]);
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        pass.draw_indexed(0..6, 0, 0..CANDIDATES);
    }
}

#[cfg(test)]
mod tests;
