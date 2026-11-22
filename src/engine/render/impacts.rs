//! Depth-tested surface marks and translucent dust, composited after the sky.
use super::Renderer;
use glam::Vec3;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(in crate::engine) struct ImpactVertex {
    position: [f32; 3],
    uv: [f32; 2],
    color: [f32; 3],
    params: [f32; 4],
}
impl ImpactVertex {
    pub(in crate::engine) fn new(
        position: Vec3,
        uv: [f32; 2],
        color: Vec3,
        params: [f32; 4],
    ) -> Self {
        Self {
            position: position.into(),
            uv,
            color: color.into(),
            params,
        }
    }
}
pub(super) struct ImpactPass {
    pipeline: wgpu::RenderPipeline,
    atlas: wgpu::BindGroup,
    buffer: wgpu::Buffer,
    count: u32,
}
impl Renderer {
    pub(in crate::engine) fn update_impacts(&mut self, vertices: &[ImpactVertex]) {
        if vertices.is_empty() {
            if let Some(p) = &mut self.impacts {
                p.count = 0;
            }
            return;
        }
        let bytes = bytemuck::cast_slice(vertices);
        if self.impacts.is_none() {
            let camera = self.pipeline.get_bind_group_layout(0);
            let atlas_layout =
                self.device
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("crash smoke atlas"),
                        entries: &[
                            wgpu::BindGroupLayoutEntry {
                                binding: 0,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Texture {
                                    sample_type: wgpu::TextureSampleType::Float {
                                        filterable: true,
                                    },
                                    view_dimension: wgpu::TextureViewDimension::D2,
                                    multisampled: false,
                                },
                                count: None,
                            },
                            wgpu::BindGroupLayoutEntry {
                                binding: 1,
                                visibility: wgpu::ShaderStages::FRAGMENT,
                                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                                count: None,
                            },
                        ],
                    });
            // Four prelit smoke lobes. R stores lighting and G stores opacity.
            let texture = self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("prelit crash clouds"),
                    size: wgpu::Extent3d {
                        width: 224,
                        height: 224,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rg8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                include_bytes!("../../../assets/effects/crash-cloud.rg8"),
            );
            let view = texture.create_view(&Default::default());
            let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            let atlas = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("crash clouds"),
                layout: &atlas_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            let layout = self
                .device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("impact camera"),
                    bind_group_layouts: &[Some(&camera), Some(&atlas_layout)],
                    immediate_size: 0,
                });
            let shader = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("impact materials"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("impacts.wgsl").into()),
                });
            let vertices = wgpu::VertexBufferLayout {
                array_stride: size_of::<ImpactVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Float32x3,3=>Float32x4],
            };
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("dust and impact marks"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        compilation_options: Default::default(),
                        buffers: &[Some(vertices)],
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: self.config.format.add_srgb_suffix(),
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(wgpu::CompareFunction::LessEqual),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: wgpu::MultisampleState {
                        count: self.samples,
                        ..Default::default()
                    },
                    multiview_mask: None,
                    cache: None,
                });
            self.impacts = Some(ImpactPass {
                pipeline,
                atlas,
                buffer: self.dynamic_buffer(bytes.len() as u64),
                count: 0,
            });
        }
        if self.impacts.as_ref().unwrap().buffer.size() < bytes.len() as u64 {
            self.impacts.as_mut().unwrap().buffer = self.dynamic_buffer(bytes.len() as u64);
        }
        let pass = self.impacts.as_mut().unwrap();
        self.queue.write_buffer(&pass.buffer, 0, bytes);
        pass.count = vertices.len() as u32;
    }
}
impl ImpactPass {
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, camera: &wgpu::BindGroup) {
        if self.count > 0 {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, camera, &[]);
            pass.set_bind_group(1, &self.atlas, &[]);
            pass.set_vertex_buffer(0, self.buffer.slice(..));
            pass.draw(0..self.count, 0..1);
        }
    }
}
