//! Translucent, depth-tested pickup cones over waiting service passengers.
use crate::dynamic_world::METERS_PER_UNIT;
use glam::{Mat4, Vec3};

const SIDES: usize = 16;
const CAPACITY: usize = 5;
const VERTICES_PER_MARKER: usize = SIDES * 3;
const ATTRIBUTES: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0=>Float32x4];

pub(in crate::engine) struct MissionMarkers {
    pipeline: wgpu::RenderPipeline,
    matrix: wgpu::Buffer,
    group: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    count: u32,
}

impl MissionMarkers {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, samples: u32) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mission marker camera"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let matrix = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mission marker camera"),
            size: std::mem::size_of::<[[f32; 4]; 4]>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mission marker camera"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: matrix.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mission markers"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mission markers"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mission_markers.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mission markers"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<[f32; 4]>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &ATTRIBUTES,
                })],
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
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        let vertices = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mission marker cones"),
            size: (CAPACITY * VERTICES_PER_MARKER * std::mem::size_of::<[f32; 4]>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline,
            matrix,
            group,
            vertices,
            count: 0,
        }
    }

    pub fn update(&mut self, queue: &wgpu::Queue, positions: impl Iterator<Item = Vec3>) {
        let mut vertices = Vec::<[f32; 4]>::with_capacity(CAPACITY * VERTICES_PER_MARKER);
        for position in positions.take(CAPACITY) {
            let center = position / METERS_PER_UNIT;
            // The narrow tip points toward the head; the broad rim floats above it.
            let tip = center + Vec3::Z * (2.02 / METERS_PER_UNIT);
            let rim_z = center.z + 2.78 / METERS_PER_UNIT;
            let radius = 0.48 / METERS_PER_UNIT;
            for side in 0..SIDES {
                let angle = side as f32 * std::f32::consts::TAU / SIDES as f32;
                let next = (side + 1) as f32 * std::f32::consts::TAU / SIDES as f32;
                let a = Vec3::new(
                    center.x + angle.cos() * radius,
                    center.y + angle.sin() * radius,
                    rim_z,
                );
                let b = Vec3::new(
                    center.x + next.cos() * radius,
                    center.y + next.sin() * radius,
                    rim_z,
                );
                vertices.extend([
                    [tip.x, tip.y, tip.z, 0.55],
                    [a.x, a.y, a.z, 0.22],
                    [b.x, b.y, b.z, 0.22],
                ]);
            }
        }
        self.count = vertices.len() as u32;
        if self.count > 0 {
            queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
        }
    }

    pub fn set_view(&self, queue: &wgpu::Queue, view: Mat4) {
        queue.write_buffer(
            &self.matrix,
            0,
            bytemuck::bytes_of(&view.to_cols_array_2d()),
        );
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}
