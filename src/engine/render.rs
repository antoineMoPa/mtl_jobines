use super::commerce::Commerce;
use super::gpu_profile::GpuProfile;
use super::hud::{Hud, MenuItem, Overlay};
use super::lighting::{Lighting, SkySettings};
use super::sky::Sky;
use crate::hermes::MinimapView;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3, Vec4};
use opencity::runtime::{Mesh, SurfaceKind, Vertex};
use std::borrow::Cow;
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
    daylight_sky: [f32; 4],
    inverse_view_projection: [[f32; 4]; 4],
    sun_color: [f32; 4],
    star_params: [f32; 4],
    star_color: [f32; 4],
}
pub struct GpuMesh {
    buffer: wgpu::Buffer,
    instances: Option<(wgpu::Buffer, u32)>,
    count: u32,
    opaque_count: u32,
    bounds: Option<Bounds>,
}
#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}
impl Bounds {
    fn of_vertices(vertices: &[Vertex]) -> Self {
        let mut bounds = Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
        };
        for vertex in vertices {
            let position = Vec3::from_array(vertex.position);
            bounds.min = bounds.min.min(position);
            bounds.max = bounds.max.max(position);
        }
        bounds
    }
}
struct Frustum([Vec4; 6]);
impl Frustum {
    fn includes(&self, bounds: Option<Bounds>) -> bool {
        bounds.is_none_or(|bounds| self.intersects(bounds))
    }
    fn from_matrix(matrix: Mat4) -> Self {
        let rows = matrix.transpose();
        // WebGPU clips x/y to [-w,w], but depth to [0,w]. Plane lengths
        // do not matter for the signed half-space test.
        Self([
            rows.w_axis + rows.x_axis,
            rows.w_axis - rows.x_axis,
            rows.w_axis + rows.y_axis,
            rows.w_axis - rows.y_axis,
            rows.z_axis,
            rows.w_axis - rows.z_axis,
        ])
    }
    fn intersects(&self, bounds: Bounds) -> bool {
        self.0.iter().all(|plane| {
            // The corner furthest into this plane's positive half-space.
            // Retain touching/intersecting boxes, including ones enclosing
            // the eye or crossing the near plane.
            let corner = Vec3::new(
                if plane.x >= 0. {
                    bounds.max.x
                } else {
                    bounds.min.x
                },
                if plane.y >= 0. {
                    bounds.max.y
                } else {
                    bounds.min.y
                },
                if plane.z >= 0. {
                    bounds.max.z
                } else {
                    bounds.min.z
                },
            );
            plane.truncate().dot(corner) + plane.w >= -0.00001
        })
    }
}
#[derive(Debug)]
#[cfg(not(target_arch = "wasm32"))]
pub struct VisibilityStats {
    pub candidates: usize,
    pub candidate_vertices: u64,
    pub camera_draws: usize,
    pub camera_vertices: u64,
    pub shadow_draws: usize,
    pub shadow_vertices: u64,
}
pub struct Renderer {
    instance: wgpu::Instance,
    pub device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    opaque_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    opaque_shadow_pipeline: wgpu::RenderPipeline,
    vehicle_pipeline: wgpu::RenderPipeline,
    vehicle_shadow_pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    shadow_group: wgpu::BindGroup,
    depth: wgpu::TextureView,
    color: wgpu::TextureView,
    shadow: wgpu::TextureView,
    pub adapter_name: String,
    hud: Hud,
    pub overlay: Overlay,
    pub menu_selection: MenuItem,
    pub daylight: Option<f32>,
    pub sky_settings: SkySettings,
    sky: Sky,
    commerce: Commerce,
    pub fps: Option<f32>,
    pub vehicle_name: Option<String>,
    pub gpu_profile: Option<GpuProfile>,
}
const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32x4];
fn material_vertices(source: &[Vertex]) -> (Cow<'_, [Vertex]>, u32) {
    let masked = |triangle: &[Vertex]| {
        triangle.iter().any(|v| {
            v.surface[0] == SurfaceKind::Fence as u32 as f32
                || v.surface[0] >= crate::hermes::commerce_mesh::SIGN
        })
    };
    if !source.chunks_exact(3).any(masked) {
        return (Cow::Borrowed(source), source.len() as u32);
    }
    let mut vertices = Vec::with_capacity(source.len());
    for triangle in source.chunks_exact(3).filter(|triangle| !masked(triangle)) {
        vertices.extend_from_slice(triangle);
    }
    let opaque_count = vertices.len() as u32;
    for triangle in source.chunks_exact(3).filter(|triangle| masked(triangle)) {
        vertices.extend_from_slice(triangle);
    }
    (Cow::Owned(vertices), opaque_count)
}
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
        let descriptor = match &window {
            Some(window) => {
                wgpu::InstanceDescriptor::new_with_display_handle(Box::new(window.clone()))
            }
            None => wgpu::InstanceDescriptor::new_without_display_handle(),
        };
        let instance = wgpu::Instance::new(descriptor);
        let surface = window
            .map(|w| instance.create_surface(w).map_err(|e| e.to_string()))
            .transpose()?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface.as_ref(),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;
        let adapter_name = adapter.get_info().name;
        #[cfg(not(target_arch = "wasm32"))]
        let profile = std::env::args().any(|arg| arg == "--profile" || arg == "--profile-120");
        #[cfg(target_arch = "wasm32")]
        let profile = false;
        let timestamp_features = if profile {
            adapter.features() & wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mtl_jobines"),
                required_features: timestamp_features,
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                ..Default::default()
            })
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
            color_space: wgpu::SurfaceColorSpace::Srgb,
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
        let sky = Sky::new(&device, &camera_layout, format);
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
        let commerce = Commerce::new(&device, &queue);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("OpenCity procedural game shader"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    super::commerce::shader(),
                    include_str!("vehicle.wgsl")
                )
                .into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[
                Some(&camera_layout),
                Some(&shadow_layout),
                Some(&sky.layout),
                Some(&commerce.layout),
            ],
            immediate_size: 0,
        });
        let shadow_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&camera_layout)],
                immediate_size: 0,
            });
        let targets = [Some(wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let instance_attributes = wgpu::vertex_attr_array![4=>Float32x4,5=>Float32x4,
            6=>Float32x4,7=>Float32x4];
        let make_pipeline = |shadow: bool, masked: bool, instanced: bool| {
            let buffers = [
                Some(layout()),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attributes,
                }),
            ];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if shadow { "sun depth" } else { "world" }),
                layout: Some(if shadow {
                    &shadow_pipeline_layout
                } else {
                    &pipeline_layout
                }),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(match (shadow, instanced) {
                        (true, true) => "vs_vehicle_shadow",
                        (false, true) => "vs_vehicle",
                        (true, false) => "vs_shadow",
                        (false, false) => "vs",
                    }),
                    compilation_options: Default::default(),
                    buffers: if instanced { &buffers } else { &buffers[..1] },
                },
                fragment: (!(shadow && !masked)).then_some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if shadow {
                        if instanced {
                            "fs_vehicle_shadow"
                        } else {
                            "fs_shadow_game"
                        }
                    } else if instanced {
                        "fs_vehicle"
                    } else if masked {
                        "fs_game"
                    } else {
                        "fs_opaque_game"
                    }),
                    compilation_options: Default::default(),
                    targets: if shadow { &[] } else { &targets },
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
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
                    alpha_to_coverage_enabled: !shadow && masked,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline(false, true, false);
        let opaque_pipeline = make_pipeline(false, false, false);
        let shadow_pipeline = make_pipeline(true, true, false);
        let opaque_shadow_pipeline = make_pipeline(true, false, false);
        let vehicle_pipeline = make_pipeline(false, true, true);
        let vehicle_shadow_pipeline = make_pipeline(true, true, true);
        let depth = depth(&device, width.max(1), height.max(1), "depth", false);
        let color = color_target(&device, width, height, format);
        let hud = Hud::new(&device, format);
        #[cfg(not(target_arch = "wasm32"))]
        let gpu_profile = timestamp_features
            .contains(wgpu::Features::TIMESTAMP_QUERY)
            .then(|| GpuProfile::new(&device));
        #[cfg(target_arch = "wasm32")]
        let gpu_profile = None;
        Ok(Self {
            instance,
            device,
            queue,
            surface,
            config,
            pipeline,
            opaque_pipeline,
            shadow_pipeline,
            opaque_shadow_pipeline,
            vehicle_pipeline,
            vehicle_shadow_pipeline,
            uniform,
            camera_group,
            shadow_group,
            depth,
            color,
            shadow,
            adapter_name,
            hud,
            overlay: Overlay::Driving,
            menu_selection: MenuItem::Resume,
            daylight: None,
            sky_settings: SkySettings::default(),
            sky,
            commerce,
            fps: None,
            vehicle_name: None,
            gpu_profile,
        })
    }
    pub fn recreate_surface(&mut self, window: Arc<Window>) -> Result<(), String> {
        self.surface = Some(
            self.instance
                .create_surface(window)
                .map_err(|e| e.to_string())?,
        );
        self.resize(self.config.width, self.config.height);
        Ok(())
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
    #[cfg(not(target_arch = "wasm32"))]
    pub fn sky_update_count(&self) -> u64 {
        self.sky.update_count()
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn validate_sky_reflections(&self) -> Result<(), String> {
        let layouts = [
            self.opaque_pipeline.get_bind_group_layout(0),
            self.opaque_pipeline.get_bind_group_layout(1),
            self.opaque_pipeline.get_bind_group_layout(2),
        ];
        super::sky_probe::validate(
            &self.device,
            &self.queue,
            [&layouts[0], &layouts[1], &layouts[2]],
            [&self.camera_group, &self.shadow_group, &self.sky.group],
        )
    }
    pub fn update_route(&mut self, points: &[[f32; 2]]) {
        self.hud.update_route(&self.device, points);
    }
    pub fn update_minimap(&mut self, pixels: &[u8]) {
        self.hud.update(&self.queue, pixels);
    }
    pub fn upload(&self, mesh: &Mesh) -> GpuMesh {
        self.upload_vertices(&mesh.vertices)
    }
    pub fn upload_vertices(&self, vertices: &[Vertex]) -> GpuMesh {
        self.upload_with_bounds(vertices, Some(Bounds::of_vertices(vertices)))
    }
    // Transient meshes are already assembled around the player. Avoid scanning
    // every vertex again each frame; conservatively draw them in both passes.
    pub fn upload_dynamic(&self, mesh: &Mesh) -> GpuMesh {
        let mut gpu = GpuMesh {
            instances: None,
            buffer: self.dynamic_buffer(std::mem::size_of_val(mesh.vertices.as_slice()) as u64),
            count: 0,
            opaque_count: 0,
            bounds: None,
        };
        self.update_dynamic(&mut gpu, mesh);
        gpu
    }
    pub fn upload_instanced(&self, vertices: &[Vertex]) -> GpuMesh {
        let mut mesh = self.upload_vertices(vertices);
        mesh.bounds = None;
        mesh.instances = Some((self.dynamic_buffer(64), 0));
        mesh
    }
    pub fn update_instances(&self, mesh: &mut GpuMesh, transforms: &[[[f32; 4]; 4]]) {
        let (buffer, count) = mesh.instances.as_mut().expect("instanced geometry");
        let bytes = bytemuck::cast_slice(transforms);
        if bytes.len() as u64 > buffer.size() {
            *buffer = self.dynamic_buffer(bytes.len() as u64);
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(buffer, 0, bytes);
        }
        *count = transforms.len() as u32;
    }
    fn dynamic_buffer(&self, required: u64) -> wgpu::Buffer {
        self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("reusable dynamic vertices"),
            // Leave spare capacity so nearby prop count changes do not require
            // reallocations. The draw count always uses the current mesh size.
            size: required.max(4).next_power_of_two(),
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }
    pub fn update_dynamic(&self, gpu: &mut GpuMesh, mesh: &Mesh) {
        let (vertices, opaque_count) = material_vertices(&mesh.vertices);
        let bytes = unsafe {
            std::slice::from_raw_parts(
                vertices.as_ptr().cast::<u8>(),
                std::mem::size_of_val(vertices.as_ref()),
            )
        };
        if bytes.len() as u64 > gpu.buffer.size()
            || !gpu.buffer.usage().contains(wgpu::BufferUsages::COPY_DST)
        {
            gpu.buffer = self.dynamic_buffer(bytes.len() as u64);
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&gpu.buffer, 0, bytes);
        }
        gpu.count = mesh.vertices.len() as u32;
        gpu.opaque_count = opaque_count;
        gpu.bounds = None;
    }
    fn upload_with_bounds(&self, source: &[Vertex], bounds: Option<Bounds>) -> GpuMesh {
        let (vertices, opaque_count) = material_vertices(source);
        // Vertex has repr(C), f32-only fields, no padding or uninitialized bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                vertices.as_ptr().cast::<u8>(),
                std::mem::size_of_val(vertices.as_ref()),
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
            instances: None,
            count: source.len() as u32,
            opaque_count,
            bounds,
        }
    }
    fn view_matrices(&self, eye: Vec3, look: Vec3, sun: Vec3) -> (Mat4, Mat4) {
        let projection = Mat4::perspective_rh(
            58_f32.to_radians(),
            self.config.width as f32 / self.config.height as f32,
            0.003,
            10.0,
        );
        let focus = Vec3::new(look.x, look.y, 0.15);
        let light = Mat4::orthographic_rh(-1.6, 1.6, -1.6, 1.6, 0.01, 10.)
            * Mat4::look_at_rh(
                focus + sun * 4.,
                focus,
                if sun.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z },
            );
        (projection * Mat4::look_at_rh(eye, look, Vec3::Z), light)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn visibility_stats(
        &self,
        meshes: &[&GpuMesh],
        eye: Vec3,
        look: Vec3,
        time: f32,
    ) -> VisibilityStats {
        let (view, light) = self.view_matrices(
            eye,
            look,
            Lighting::with_settings(time, self.daylight, self.sky_settings).sun,
        );
        let camera = Frustum::from_matrix(view);
        let shadow = Frustum::from_matrix(light);
        let mut stats = VisibilityStats {
            candidates: meshes.len(),
            candidate_vertices: 0,
            camera_draws: 0,
            camera_vertices: 0,
            shadow_draws: 0,
            shadow_vertices: 0,
        };
        for mesh in meshes {
            let vertices = u64::from(mesh.count)
                * u64::from(mesh.instances.as_ref().map_or(1, |(_, count)| *count));
            stats.candidate_vertices += vertices;
            if camera.includes(mesh.bounds) {
                stats.camera_draws += 1;
                stats.camera_vertices += vertices;
            }
            if shadow.includes(mesh.bounds) {
                stats.shadow_draws += 1;
                stats.shadow_vertices += vertices;
            }
        }
        stats
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
        let lighting = Lighting::with_settings(time, self.daylight, self.sky_settings);
        let sun = lighting.sun;
        let (view, light) = self.view_matrices(eye, look, sun);
        let camera_frustum = Frustum::from_matrix(view);
        let shadow_frustum = Frustum::from_matrix(light);
        let data = Camera {
            view_projection: view.to_cols_array_2d(),
            light_view_projection: light.to_cols_array_2d(),
            eye_time: [eye.x, eye.y, eye.z, time],
            sun_exposure: [sun.x, sun.y, sun.z, 1.35],
            daylight_sky: [lighting.daylight, 0., 0., 0.],
            inverse_view_projection: view.inverse().to_cols_array_2d(),
            sun_color: [
                lighting.sun_color[0],
                lighting.sun_color[1],
                lighting.sun_color[2],
                self.sky_settings.sun_intensity,
            ],
            star_params: [
                self.sky_settings.star_density,
                self.sky_settings.star_size,
                self.sky_settings.star_brightness,
                58_f32.to_radians() / self.config.height as f32,
            ],
            star_color: [
                self.sky_settings.star_color[0],
                self.sky_settings.star_color[1],
                self.sky_settings.star_color[2],
                lighting.star_visibility,
            ],
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("world frame"),
            });
        self.sky.update(
            &self.queue,
            &mut encoder,
            time,
            self.daylight,
            self.sky_settings,
        );
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
                timestamp_writes: self.gpu_profile.as_ref().map(|profile| profile.pass(0)),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.opaque_shadow_pipeline);
            pass.set_bind_group(0, &self.camera_group, &[]);
            for mesh in meshes {
                if mesh.instances.is_some()
                    || mesh.opaque_count == 0
                    || !shadow_frustum.includes(mesh.bounds)
                {
                    continue;
                }
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw(0..mesh.opaque_count, 0..1);
            }
            pass.set_pipeline(&self.shadow_pipeline);
            for mesh in meshes {
                if mesh.opaque_count == mesh.count || !shadow_frustum.includes(mesh.bounds) {
                    continue;
                }
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw(mesh.opaque_count..mesh.count, 0..1);
            }
            pass.set_pipeline(&self.vehicle_shadow_pipeline);
            for mesh in meshes {
                if let Some((instances, count)) = &mesh.instances {
                    if *count == 0 {
                        continue;
                    }
                    pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                    pass.set_vertex_buffer(1, instances.slice(..));
                    pass.draw(0..mesh.count, 0..*count);
                }
            }
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color,
                    depth_slice: None,
                    resolve_target: Some(target),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Discard,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: self.gpu_profile.as_ref().map(|profile| profile.pass(1)),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.opaque_pipeline);
            pass.set_bind_group(0, &self.camera_group, &[]);
            pass.set_bind_group(1, &self.shadow_group, &[]);
            pass.set_bind_group(2, &self.sky.group, &[]);
            pass.set_bind_group(3, &self.commerce.group, &[]);
            for mesh in meshes {
                if mesh.instances.is_some()
                    || mesh.opaque_count == 0
                    || !camera_frustum.includes(mesh.bounds)
                {
                    continue;
                }
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw(0..mesh.opaque_count, 0..1);
            }
            pass.set_pipeline(&self.pipeline);
            for mesh in meshes {
                if mesh.opaque_count == mesh.count || !camera_frustum.includes(mesh.bounds) {
                    continue;
                }
                pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                pass.draw(mesh.opaque_count..mesh.count, 0..1);
            }
            pass.set_pipeline(&self.vehicle_pipeline);
            for mesh in meshes {
                if let Some((instances, count)) = &mesh.instances {
                    if *count == 0 {
                        continue;
                    }
                    pass.set_vertex_buffer(0, mesh.buffer.slice(..));
                    pass.set_vertex_buffer(1, instances.slice(..));
                    pass.draw(0..mesh.count, 0..*count);
                }
            }
            self.sky.draw(&mut pass, &self.camera_group);
        }
        self.hud.render(
            &self.queue,
            &mut encoder,
            target,
            self.config.width,
            self.config.height,
            navigation,
            self.overlay,
            self.menu_selection,
            self.fps,
            self.vehicle_name.as_deref(),
            self.gpu_profile.as_ref().map(|profile| profile.pass(2)),
        );
        if let Some(profile) = &self.gpu_profile {
            profile.resolve(&mut encoder);
        }
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
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
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
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let bytes = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| e.to_string())?;
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
    fn opaque_and_cutout_ranges_preserve_every_triangle_and_material() {
        use super::*;
        let mut mesh = Mesh::default();
        for (id, kind) in [
            SurfaceKind::Asphalt,
            SurfaceKind::Fence,
            SurfaceKind::Building,
        ]
        .into_iter()
        .enumerate()
        {
            for corner in 0..3 {
                mesh.vertices.push(Vertex {
                    position: [id as f32, corner as f32, 0.],
                    normal: [0., 0., 1.],
                    color: [1.; 3],
                    surface: [kind as u32 as f32, 0., 0., 0.],
                });
            }
        }
        let (vertices, opaque) = material_vertices(&mesh.vertices);
        assert_eq!(opaque, 6);
        assert_eq!(vertices.len(), 9);
        assert_eq!(
            vertices
                .iter()
                .map(|v| v.position[0] as usize)
                .collect::<Vec<_>>(),
            vec![0, 0, 0, 2, 2, 2, 1, 1, 1]
        );
        assert!(vertices[6..]
            .iter()
            .all(|v| v.surface[0] == SurfaceKind::Fence as u32 as f32));
        mesh.vertices.truncate(3);
        let (vertices, opaque) = material_vertices(&mesh.vertices);
        assert!(matches!(vertices, Cow::Borrowed(_)));
        assert_eq!(opaque, 3);
    }
    use super::{Bounds, Frustum};
    use glam::{Mat4, Vec3};

    fn bounds(min: [f32; 3], max: [f32; 3]) -> Bounds {
        Bounds {
            min: Vec3::from_array(min),
            max: Vec3::from_array(max),
        }
    }

    #[test]
    fn frustum_uses_webgpu_depth_and_keeps_intersecting_bounds() {
        let frustum = Frustum::from_matrix(Mat4::IDENTITY);
        assert!(frustum.includes(None));
        assert!(frustum.intersects(bounds([-0.5, -0.5, 0.1], [0.5, 0.5, 0.9])));
        assert!(frustum.intersects(bounds([-2., -2., -1.], [2., 2., 2.])));
        assert!(frustum.intersects(bounds([1., 0., 0.], [2., 1., 1.])));
        assert!(frustum.intersects(bounds([-0.1, -0.1, -0.1], [0.1, 0.1, 0.1])));
        for outside in [
            bounds([-2., 0., 0.2], [-1.1, 0.1, 0.3]),
            bounds([1.1, 0., 0.2], [2., 0.1, 0.3]),
            bounds([0., -2., 0.2], [0.1, -1.1, 0.3]),
            bounds([0., 1.1, 0.2], [0.1, 2., 0.3]),
            bounds([0., 0., -0.2], [0.1, 0.1, -0.1]),
            bounds([0., 0., 1.1], [0.1, 0.1, 2.]),
        ] {
            assert!(!frustum.intersects(outside));
        }
    }

    #[test]
    fn perspective_culling_keeps_eye_crossings_and_offscreen_shadow_casters() {
        let eye = Vec3::new(0., 0., 1.);
        let view = Mat4::perspective_rh(58_f32.to_radians(), 1.6, 0.003, 10.)
            * Mat4::look_at_rh(eye, Vec3::new(0., 1., 0.), Vec3::Z);
        let camera = Frustum::from_matrix(view);
        assert!(camera.intersects(bounds([-0.1, -0.1, 0.9], [0.1, 0.1, 1.1])));
        let caster = bounds([-0.1, -1.1, 0.], [0.1, -0.9, 0.3]);
        assert!(!camera.intersects(caster));
        let light = Mat4::orthographic_rh(-1.6, 1.6, -1.6, 1.6, 0.01, 10.)
            * Mat4::look_at_rh(Vec3::new(2., -2., 4.), Vec3::ZERO, Vec3::Z);
        assert!(Frustum::from_matrix(light).intersects(caster));
    }

    #[test]
    fn transformed_frustum_agrees_with_homogeneous_corner_half_spaces() {
        let matrix = Mat4::perspective_rh(58_f32.to_radians(), 1.6, 0.003, 10.)
            * Mat4::look_at_rh(Vec3::new(3., -2., 0.4), Vec3::new(3.3, -1., 0.), Vec3::Z);
        let frustum = Frustum::from_matrix(matrix);
        for x in -8..8 {
            for y in -8..8 {
                let min = Vec3::new(x as f32, y as f32, -0.1);
                let bounds = Bounds {
                    min,
                    max: min + Vec3::new(0.8, 0.8, 0.5),
                };
                let corners: Vec<_> = (0..8)
                    .map(|i| {
                        matrix
                            * Vec3::new(
                                if i & 1 == 0 {
                                    bounds.min.x
                                } else {
                                    bounds.max.x
                                },
                                if i & 2 == 0 {
                                    bounds.min.y
                                } else {
                                    bounds.max.y
                                },
                                if i & 4 == 0 {
                                    bounds.min.z
                                } else {
                                    bounds.max.z
                                },
                            )
                            .extend(1.)
                    })
                    .collect();
                let intersects = (0..6).all(|plane| {
                    corners.iter().any(|p| {
                        let distance =
                            [p.w + p.x, p.w - p.x, p.w + p.y, p.w - p.y, p.z, p.w - p.z][plane];
                        distance >= -0.00001
                    })
                });
                assert_eq!(frustum.intersects(bounds), intersects);
            }
        }
    }

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
        let source = format!(
            "{}\n{}",
            super::super::commerce::shader(),
            include_str!("vehicle.wgsl")
        );
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
