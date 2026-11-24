//! GPU images for static GLB materials, including imports made after renderer creation.
use crate::hermes::external_models::textures::{snapshot, CAPACITY, SIZE};
use std::cell::Cell;
pub(super) struct ModelTextures {
    colour: wgpu::Texture,
    detail: wgpu::Texture,
    pub views: [wgpu::TextureView; 2],
    pub sampler: wgpu::Sampler,
    uploaded: Cell<usize>,
    pub layout: wgpu::BindGroupLayout,
    pub group: wgpu::BindGroup,
}
pub(super) fn layout() -> Vec<wgpu::BindGroupLayoutEntry> {
    let mut entries: Vec<_> = (14..16)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        })
        .collect();
    entries.push(wgpu::BindGroupLayoutEntry {
        binding: 16,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    });
    entries
}
impl ModelTextures {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = |format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("static GLB material array"),
                size: wgpu::Extent3d {
                    width: SIZE,
                    height: SIZE,
                    depth_or_array_layers: CAPACITY,
                },
                mip_level_count: 10,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let colour = texture(wgpu::TextureFormat::Rgba8UnormSrgb);
        let detail = texture(wgpu::TextureFormat::Rgba8Unorm);
        let views = [&colour, &detail].map(|t| {
            t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("static model textures"),
            entries: &layout(),
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("static model textures"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 14,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 15,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 16,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Self {
            colour,
            detail,
            views,
            sampler,
            uploaded: Cell::new(0),
            layout,
            group,
        }
    }
    pub fn sync(&self, queue: &wgpu::Queue) {
        let layers = snapshot();
        for (layer, data) in layers.iter().enumerate().skip(self.uploaded.get()) {
            for (texture, source, srgb) in [
                (&self.colour, &data.colour, true),
                (&self.detail, &data.normal, false),
            ] {
                let mut pixels = source.to_vec();
                let mut size = SIZE;
                for level in 0..10 {
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture,
                            mip_level: level,
                            origin: wgpu::Origin3d {
                                x: 0,
                                y: 0,
                                z: layer as u32,
                            },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &pixels,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(size * 4),
                            rows_per_image: Some(size),
                        },
                        wgpu::Extent3d {
                            width: size,
                            height: size,
                            depth_or_array_layers: 1,
                        },
                    );
                    if size == 1 {
                        break;
                    }
                    let next_size = size / 2;
                    let mut next = vec![0; (next_size * next_size * 4) as usize];
                    for y in 0..next_size {
                        for x in 0..next_size {
                            for c in 0..4 {
                                let mut sum = 0.;
                                for dy in 0..2 {
                                    for dx in 0..2 {
                                        let v = pixels
                                            [(((y * 2 + dy) * size + x * 2 + dx) * 4 + c) as usize]
                                            as f32
                                            / 255.;
                                        sum += if srgb && c < 3 {
                                            if v <= 0.04045 {
                                                v / 12.92
                                            } else {
                                                ((v + 0.055) / 1.055).powf(2.4)
                                            }
                                        } else {
                                            v
                                        };
                                    }
                                }
                                let v = sum / 4.;
                                let v = if srgb && c < 3 {
                                    if v <= 0.0031308 {
                                        v * 12.92
                                    } else {
                                        1.055 * v.powf(1. / 2.4) - 0.055
                                    }
                                } else {
                                    v
                                };
                                next[((y * next_size + x) * 4 + c) as usize] =
                                    (v * 255.).round() as u8;
                            }
                        }
                    }
                    pixels = next;
                    size = next_size;
                }
            }
        }
        self.uploaded.set(layers.len());
    }
}
pub(super) fn pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    camera: &wgpu::BindGroupLayout,
    shadow: &wgpu::BindGroupLayout,
    environment: &wgpu::BindGroupLayout,
    models: &wgpu::BindGroupLayout,
    samples: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("static textured models"),
        source: wgpu::ShaderSource::Wgsl(super::commerce::shader_for_device(device).into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("static models within default texture limits"),
        bind_group_layouts: &[Some(camera), Some(shadow), Some(environment), Some(models)],
        immediate_size: 0,
    });
    let vertices = wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32x4];
    let instances = wgpu::vertex_attr_array![4=>Float32x4,5=>Float32x4,6=>Float32x4,7=>Float32x4];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("static GLB instances"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_model"),
            compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: 52,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &vertices,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instances,
                }),
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_model"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    })
}
