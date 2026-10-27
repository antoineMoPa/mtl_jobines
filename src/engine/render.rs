use super::hud::{Hud, Overlay};
use crate::hermes::MinimapView;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use opencity::runtime::{Mesh, Vertex};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::{path::Path, sync::mpsc};
use wgpu::util::DeviceExt;
use winit::window::Window;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Camera {
    view_projection: [[f32; 4]; 4],
    light_view_projection: [[f32; 4]; 4],
    eye_time: [f32; 4],
    sun_exposure: [f32; 4],
}
pub struct GpuMesh {
    buffer: wgpu::Buffer,
    count: u32,
}
pub struct Renderer {
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    pub surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    shadow_group: wgpu::BindGroup,
    depth: wgpu::TextureView,
    color: wgpu::TextureView,
    shadow: wgpu::TextureView,
    pub adapter_name: String,
    hud: Hud,
    pub overlay: Overlay,
}
const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32x4];
fn surface_formats(formats: &[wgpu::TextureFormat]) -> (wgpu::TextureFormat, wgpu::TextureFormat) {
    let surface = formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(formats[0]);
    (surface, surface.add_srgb_suffix())
}
fn layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}
fn depth(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    label: &str,
    sample: bool,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: if sample { 1 } else { 4 },
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | if sample {
                    wgpu::TextureUsages::TEXTURE_BINDING
                } else {
                    wgpu::TextureUsages::empty()
                },
            view_formats: &[],
        })
        .create_view(&Default::default())
}
fn color_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("4x MSAA color"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 4,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
impl Renderer {
    pub async fn new(window: Option<Arc<Window>>, width: u32, height: u32) -> Result<Self, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = window
            .map(|w| instance.create_surface(w).map_err(|e| e.to_string()))
            .transpose()?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface.as_ref(),
                force_fallback_adapter: false,
            })
            .await
            .ok_or("no wgpu adapter")?;
        let adapter_name = adapter.get_info().name;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("mtl_jobines"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .map_err(|e| e.to_string())?;
        let supported_formats = surface
            .as_ref()
            .map(|s| s.get_capabilities(&adapter).formats)
            .unwrap_or_else(|| vec![wgpu::TextureFormat::Rgba8UnormSrgb]);
        // WebGPU canvases expose unorm formats; use an sRGB view for linear lighting.
        let (surface_format, format) = surface_formats(&supported_formats);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: if format == surface_format {
                vec![]
            } else {
                vec![format]
            },
            desired_maximum_frame_latency: 2,
        };
        if let Some(s) = &surface {
            s.configure(&device, &config);
        }
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<Camera>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let shadow = depth(&device, 2048, 2048, "sun shadow", true);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow PCF"),
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sun shadow"),
            layout: &shadow_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&shadow),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("OpenCity procedural game shader"),
            source: wgpu::ShaderSource::Wgsl(opencity::runtime::GAME_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&camera_layout, &shadow_layout],
            push_constant_ranges: &[],
        });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[&camera_layout],
                push_constant_ranges: &[],
            });
        let targets = [Some(wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let make_pipeline = |shadow: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if shadow { "sun depth" } else { "world" }),
                layout: Some(if shadow {
                    &shadow_pipeline_layout
                } else {
                    &pipeline_layout
                }),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(if shadow { "vs_shadow" } else { "vs" }),
                    compilation_options: Default::default(),
                    buffers: &[layout()],
                },
                fragment: if shadow {
                    None
                } else {
                    Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &targets,
                    })
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: Default::default(),
                    bias: if shadow {
                        wgpu::DepthBiasState {
                            constant: 2,
                            slope_scale: 2.,
                            clamp: 0.,
                        }
                    } else {
                        Default::default()
                    },
                }),
                multisample: wgpu::MultisampleState {
                    count: if shadow { 1 } else { 4 },
                    ..Default::default()
                },
                multiview: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline(false);
        let shadow_pipeline = make_pipeline(true);
        let depth = depth(&device, width.max(1), height.max(1), "depth", false);
        let color = color_target(&device, width, height, format);
        let hud = Hud::new(&device, format);
        Ok(Self {
            device,
            queue,
            surface,
            config,
            pipeline,
            shadow_pipeline,
            uniform,
            camera_group,
            shadow_group,
            depth,
            color,
            shadow,
            adapter_name,
            hud,
            overlay: Overlay::Driving,
        })
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        }
        self.depth = depth(&self.device, width, height, "depth", false);
        self.color = color_target(
            &self.device,
            width,
            height,
            self.config.format.add_srgb_suffix(),
        );
    }
    pub fn update_minimap(&mut self, pixels: &[u8]) {
        self.hud.update(&self.queue, pixels);
    }
    pub fn upload(&self, mesh: &Mesh) -> GpuMesh {
        // Vertex has repr(C), f32-only fields, no padding or uninitialized bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                mesh.vertices.as_ptr().cast::<u8>(),
                std::mem::size_of_val(mesh.vertices.as_slice()),
            )
        };
        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("world vertices"),
                contents: if bytes.is_empty() { &[0; 4] } else { bytes },
                usage: wgpu::BufferUsages::VERTEX,
            });
        GpuMesh {
            buffer,
            count: mesh.vertices.len() as u32,
        }
    }
    pub fn render(
        &self,
        target: &wgpu::TextureView,
        meshes: &[&GpuMesh],
        eye: Vec3,
        look: Vec3,
        time: f32,
        navigation: MinimapView,
    ) {
        let projection = Mat4::perspective_rh(
            58_f32.to_radians(),
            self.config.width as f32 / self.config.height as f32,
            0.003,
            10.0,
        );
        let sun = Vec3::new(-0.55, -0.4, 0.85);
        let sun = sun / sun.length();
        let focus = Vec3::new(look.x, look.y, 0.15);
        let light = Mat4::orthographic_rh(-1.6, 1.6, -1.6, 1.6, 0.01, 10.)
            * Mat4::look_at_rh(focus + sun * 4., focus, Vec3::Z);
        let data = Camera {
            view_projection: (projection * Mat4::look_at_rh(eye, look, Vec3::Z)).to_cols_array_2d(),
            light_view_projection: light.to_cols_array_2d(),
            eye_time: [eye.x, eye.y, eye.z, time],
            sun_exposure: [sun.x, sun.y, sun.z, 1.35],
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("world frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sun depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.shadow_pipeline);
            pass.set_bind_group(0, &self.camera_group, &[]);
            for mesh in meshes {
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw(0..mesh.count, 0..1);
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color,
                    resolve_target: Some(target),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.24,
                            g: 0.50,
                            b: 0.82,
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_group, &[]);
            pass.set_bind_group(1, &self.shadow_group, &[]);
            for mesh in meshes {
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw(0..mesh.count, 0..1);
            }
        }
        self.hud.render(
            &self.queue,
            &mut encoder,
            target,
            self.config.width,
            self.config.height,
            navigation,
            self.overlay,
        );
        self.queue.submit(Some(encoder.finish()));
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn snapshot(&self, texture: &wgpu::Texture, path: &Path) -> Result<(), String> {
        let row = (self.config.width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot"),
            size: row as u64 * self.config.height as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let (tx, rx) = mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let bytes = buffer.slice(..).get_mapped_range();
        let mut ppm = format!(
            "P6
{} {}
255
",
            self.config.width, self.config.height
        )
        .into_bytes();
        for row in bytes.chunks(row as usize) {
            for pixel in row[..self.config.width as usize * 4].chunks(4) {
                ppm.extend_from_slice(&pixel[..3]);
            }
        }
        std::fs::write(path, ppm).map_err(|e| e.to_string())
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn browser_canvas_uses_supported_unorm_format_with_srgb_view() {
        use wgpu::TextureFormat::*;
        assert_eq!(
            super::surface_formats(&[Bgra8Unorm, Rgba8Unorm]),
            (Bgra8Unorm, Bgra8UnormSrgb)
        );
        assert_eq!(
            super::surface_formats(&[Rgba8Unorm]),
            (Rgba8Unorm, Rgba8UnormSrgb)
        );
        assert_eq!(
            super::surface_formats(&[Bgra8Unorm, Bgra8UnormSrgb]),
            (Bgra8UnormSrgb, Bgra8UnormSrgb)
        );
    }
    #[test]
    fn shader_validates() {
        let module = naga::front::wgsl::parse_str(opencity::runtime::GAME_SHADER).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
