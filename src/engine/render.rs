use super::commerce::Commerce;
use super::gpu_profile::GpuProfile;
use super::gpu_trace::{GpuTrace, Pass, Shader};
use super::hud::{Hud, MenuItem, Overlay};
use super::lighting::{Lighting, SkySettings};
use super::sky::Sky;
use crate::hermes::MinimapView;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
mod culling;
mod projected_grass;
pub(super) use projected_grass::SupportCache;
mod impacts;
mod layer_model;
mod lod;
mod mission_markers;
mod player_shadow;
pub(super) use impacts::ImpactVertex;
pub(super) use player_shadow::world_shader;
mod snowfall;
mod ssao;
mod wheel_snow;
use culling::{Bounds, Frustum};
pub(super) use lod::RenderLod;
const SHADOW_RESOLUTION: u32 = 2048;
const SHADOW_SPAN: f32 = 3.2;
use opencity::runtime::{Mesh, SurfaceKind, Vertex};
use std::borrow::Cow;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::Arc;
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
    player_light_view_projection: [[f32; 4]; 4],
    shadow_atlas: [f32; 4],
}
pub struct GpuMesh {
    buffer: wgpu::Buffer,
    skin: Option<wgpu::Buffer>,
    instances: Option<Instances>,
    pub(super) static_model: bool,
    count: u32,
    opaque_count: u32,
    paint_start: u32,
    bounds: Option<Bounds>,
    lod: Option<Box<lod::Variants>>,
}
impl GpuMesh {
    pub(super) fn visible_underground(&self, exits: &[glam::Vec2]) -> bool {
        let visible = |bounds: Bounds| bounds.visible_underground(exits);
        if let Some(instances) = &self.instances {
            instances.bounds.iter().copied().any(visible)
        } else {
            self.bounds.is_none_or(visible)
        }
    }
}
struct Instances {
    buffer: wgpu::Buffer,
    bounds: Vec<Bounds>,
    scales: Vec<f32>,
    force_full: Vec<bool>,
    player_shadow: Option<usize>,
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
    pub camera_vertex_buffers: u64,
    pub shadow_vertex_buffers: u64,
}
pub struct Renderer {
    pub(crate) season: crate::season::Season,
    pub(super) snow: super::snow::Snow,
    instance: wgpu::Instance,
    pub device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub surface: Option<wgpu::Surface<'static>>,
    pub config: wgpu::SurfaceConfiguration,
    pub(super) reserved_bottom: u32,
    pub(super) flight_visibility: f32,
    pub(super) construction_span: Option<f32>,
    pub(super) underground_exits: Option<Vec<glam::Vec2>>,
    pub(super) ambient_occlusion: bool,
    pub(super) montreal_grade: bool,
    pub(super) retro_lens: f32,
    pub(super) zombie_mode: bool,
    pub(super) winter_weather: bool,
    pub(super) render_lod: RenderLod,
    pub(super) contact_debris: bool,
    contact_texture: super::contact_texture::ContactTexture,
    ssao: ssao::Ssao,
    layer_model: Option<layer_model::LayerModel>,
    snowfall: Option<snowfall::Snowfall>,
    pub(super) mission_markers: mission_markers::MissionMarkers,
    pipeline: wgpu::RenderPipeline,
    ribbons: projected_grass::Ribbons,
    ribbon_pipeline: wgpu::RenderPipeline,
    impacts: Option<impacts::ImpactPass>,
    wheel_snow: Option<wheel_snow::WheelSnow>,
    opaque_pipeline: wgpu::RenderPipeline,
    graffiti_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    opaque_shadow_pipeline: wgpu::RenderPipeline,
    vehicle_pipeline: wgpu::RenderPipeline,
    model_pipeline: wgpu::RenderPipeline,
    vehicle_shadow_pipeline: wgpu::RenderPipeline,
    pedestrian_pipeline: wgpu::RenderPipeline,
    pedestrian_shadow_pipeline: wgpu::RenderPipeline,
    pub(super) pedestrians: super::pedestrian_scenery::Resources,
    uniform: wgpu::Buffer,
    pub(super) local_lights: super::local_lights::LocalLights,
    camera_group: wgpu::BindGroup,
    shadow_group: wgpu::BindGroup,
    depth: wgpu::TextureView,
    color: Option<wgpu::TextureView>,
    samples: u32,
    shadow: wgpu::TextureView,
    world_shadow_resolution: u32,
    player_shadow_uniform: wgpu::Buffer,
    player_shadow_group: wgpu::BindGroup,
    pub adapter_name: String,
    hud: Hud,
    pub overlay: Overlay,
    pub cinematic: bool,
    pub full_map: super::hud::FullMapView,
    pub menu_selection: MenuItem,
    pub daylight: Option<f32>,
    pub sky_settings: SkySettings,
    pub(super) sky: Sky,
    commerce: Commerce,
    pub fps: Option<f32>,
    pub gpu_profile: Option<GpuProfile>,
    gpu_trace: Option<GpuTrace>,
}
const ATTRIBUTES: [wgpu::VertexAttribute; 4] =
    wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3,3=>Float32x4];
fn material_vertices(source: &[Vertex]) -> (Cow<'_, [Vertex]>, u32, u32) {
    let marking = |v: &Vertex| v.surface[0] == SurfaceKind::Marking as u32 as f32;
    let paint = |triangle: &[Vertex]| {
        triangle
            .iter()
            .any(|v| v.surface[0] == super::graffiti::MATERIAL || marking(v))
    };
    let masked = |triangle: &[Vertex]| {
        triangle.iter().any(|v| {
            marking(v)
                || v.surface[0] == SurfaceKind::Water as u32 as f32
                || v.surface[0] == SurfaceKind::Fence as u32 as f32
                || (15.0..=23.0).contains(&v.surface[0])
                || (24.0..=29.0).contains(&v.surface[0])
                || v.surface[0] >= crate::hermes::commerce_mesh::SIGN
        })
    };
    if !source.chunks_exact(3).any(masked) {
        return (
            Cow::Borrowed(source),
            source.len() as u32,
            source.len() as u32,
        );
    }
    let mut vertices = Vec::with_capacity(source.len());
    for triangle in source.chunks_exact(3).filter(|triangle| !masked(triangle)) {
        vertices.extend_from_slice(triangle);
    }
    let opaque_count = vertices.len() as u32;
    for triangle in source.chunks_exact(3).filter(|t| masked(t) && !paint(t)) {
        vertices.extend_from_slice(triangle);
    }
    let paint_start = vertices.len() as u32;
    for triangle in source.chunks_exact(3).filter(|t| paint(t)) {
        vertices.extend_from_slice(triangle);
    }
    (Cow::Owned(vertices), opaque_count, paint_start)
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
    samples: u32,
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
            sample_count: samples,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
fn color_target(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    samples: u32,
) -> Option<wgpu::TextureView> {
    if samples == 1 {
        return None;
    }
    Some(
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("MSAA color"),
                size: wgpu::Extent3d {
                    width: width.max(1),
                    height: height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default()),
    )
}
fn world_pipelines(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    camera_layout: &wgpu::BindGroupLayout,
    shadow_layout: &wgpu::BindGroupLayout,
    environment_layout: &wgpu::BindGroupLayout,
    commerce_layout: &wgpu::BindGroupLayout,
    samples: u32,
) -> [wgpu::RenderPipeline; 10] {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("OpenCity procedural game shader"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{}",
                super::commerce::shader_for_device(device),
                format!(
                    "{}\n{}",
                    include_str!("vehicle.wgsl"),
                    super::pedestrian_scenery::shader(device)
                )
            )
            .replace(
                "const VEHICLE_SAMPLE_COUNT: u32 = 4u;",
                &format!("const VEHICLE_SAMPLE_COUNT: u32 = {samples}u;"),
            )
            .into(),
        ),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[
            Some(camera_layout),
            Some(shadow_layout),
            Some(environment_layout),
            Some(commerce_layout),
        ],
        immediate_size: 0,
    });
    let shadow_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(camera_layout), None, None, Some(commerce_layout)],
        immediate_size: 0,
    });
    let instance_attributes = wgpu::vertex_attr_array![4=>Float32x4,5=>Float32x4,
            6=>Float32x4,7=>Float32x4];
    let skin_attributes = wgpu::vertex_attr_array![8=>Uint16x4,9=>Float32x4];
    let make_pipeline =
        |shadow: bool, masked: bool, instanced: bool, paint: bool, skinned: bool| {
            let targets = [Some(wgpu::ColorTargetState {
                format,
                blend: paint.then_some(wgpu::BlendState::ALPHA_BLENDING),
                // Decals blend by coverage while retaining the pavement’s haze mask.
                write_mask: if paint {
                    wgpu::ColorWrites::COLOR
                } else {
                    wgpu::ColorWrites::ALL
                },
            })];
            let buffers = [
                Some(layout()),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &instance_attributes,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &skin_attributes,
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
                    entry_point: Some(if skinned {
                        if shadow {
                            "vs_pedestrian_shadow"
                        } else {
                            "vs_pedestrian"
                        }
                    } else {
                        match (shadow, instanced) {
                            (true, true) => "vs_vehicle_shadow",
                            (false, true) => "vs_vehicle",
                            (true, false) => "vs_shadow",
                            (false, false) => "vs",
                        }
                    }),
                    compilation_options: Default::default(),
                    buffers: if skinned {
                        &buffers
                    } else if instanced {
                        &buffers[..2]
                    } else {
                        &buffers[..1]
                    },
                },
                fragment: (!(shadow && !masked)).then_some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if shadow {
                        if instanced {
                            "fs_vehicle_shadow"
                        } else {
                            "fs_shadow_game"
                        }
                    } else if skinned {
                        "fs_pedestrian"
                    } else if paint {
                        "fs_paint"
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
                    depth_write_enabled: Some(!paint),
                    depth_compare: Some(if shadow {
                        wgpu::CompareFunction::LessEqual
                    } else {
                        wgpu::CompareFunction::GreaterEqual
                    }),
                    stencil: Default::default(),
                    bias: if shadow {
                        wgpu::DepthBiasState {
                            constant: 2,
                            slope_scale: 2.,
                            clamp: 0.,
                        }
                    } else if paint {
                        // Distant paint and pavement can round to adjacent depth values in either order.
                        wgpu::DepthBiasState {
                            constant: 2,
                            ..Default::default()
                        }
                    } else {
                        Default::default()
                    },
                }),
                multisample: wgpu::MultisampleState {
                    count: if shadow { 1 } else { samples },
                    alpha_to_coverage_enabled: samples > 1 && !shadow && masked && !paint,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
    let pipeline = make_pipeline(false, true, false, false, false);
    let opaque_pipeline = make_pipeline(false, false, false, false, false);
    let graffiti_pipeline = make_pipeline(false, true, false, true, false);
    let shadow_pipeline = make_pipeline(true, true, false, false, false);
    let opaque_shadow_pipeline = make_pipeline(true, false, false, false, false);
    let vehicle_pipeline = make_pipeline(false, true, true, false, false);
    let vehicle_shadow_pipeline = make_pipeline(true, true, true, false, false);
    let pedestrian_pipeline = make_pipeline(false, true, true, false, true);
    let pedestrian_shadow_pipeline = make_pipeline(true, true, true, false, true);
    let ribbon_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("projected grass"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_ribbon"),
            compilation_options: Default::default(),
            buffers: &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: 4,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Unorm8x4],
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 48,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![1=>Float32x4,2=>Float32x4,3=>Float32x4],
                }),
            ],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_ribbon"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(false),
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
    });
    [
        pipeline,
        opaque_pipeline,
        graffiti_pipeline,
        shadow_pipeline,
        opaque_shadow_pipeline,
        vehicle_pipeline,
        vehicle_shadow_pipeline,
        pedestrian_pipeline,
        pedestrian_shadow_pipeline,
        ribbon_pipeline,
    ]
}

impl Renderer {
    pub async fn new(window: Option<Arc<Window>>, width: u32, height: u32) -> Result<Self, String> {
        Self::with_catalog(window, width, height, true).await
    }

    pub(super) async fn with_catalog(
        window: Option<Arc<Window>>,
        width: u32,
        height: u32,
        montreal_actors: bool,
    ) -> Result<Self, String> {
        // Start single-sampled. Only native builds allow changing this later.
        let samples = 1;
        let descriptor = match &window {
            Some(window) => {
                wgpu::InstanceDescriptor::new_with_display_handle(Box::new(window.clone()))
            }
            None => wgpu::InstanceDescriptor::new_without_display_handle(),
        };
        #[cfg(target_arch = "wasm32")]
        let instance = wgpu::util::new_instance_with_webgpu_detection(descriptor).await;
        #[cfg(not(target_arch = "wasm32"))]
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
        #[cfg(target_arch = "wasm32")]
        if let Some(canvas) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("game"))
        {
            canvas
                .set_attribute(
                    "data-renderer",
                    if adapter.get_info().backend == wgpu::Backend::Gl {
                        "webgl"
                    } else {
                        "webgpu"
                    },
                )
                .ok();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let profile = std::env::args().any(|arg| arg == "--profile" || arg == "--profile-120");
        #[cfg(target_arch = "wasm32")]
        let profile = false;
        #[cfg(not(target_arch = "wasm32"))]
        let trace = mtl_world::telemetry::enabled() && !profile;
        #[cfg(target_arch = "wasm32")]
        let trace = false;
        let timestamp_features = if trace {
            adapter.features()
                & (wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES)
        } else if profile {
            adapter.features() & wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let mut required_features = timestamp_features;
        if adapter.get_info().backend == wgpu::Backend::Gl {
            required_features |=
                adapter.features() & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("mtl_jobines"),
                required_features,
                required_limits: if adapter.get_info().backend == wgpu::Backend::Gl {
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
                } else {
                    wgpu::Limits::default()
                },
                memory_hints: wgpu::MemoryHints::MemoryUsage,
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
        let mut camera_entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ];
        camera_entries.extend(super::pedestrian_scenery::Resources::layout(&device));
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &camera_entries,
        });
        let contact_texture = super::contact_texture::ContactTexture::new(&device, &queue);
        let pavement = opencity::runtime::surface_materials::SurfaceTextures::new(&device, &queue);
        let snow = super::snow::Snow::new(&device, &queue);
        let mut shadow_entries = vec![
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
        ];
        shadow_entries.extend(super::contact_texture::ContactTexture::layout(2));
        shadow_entries.extend(opencity::runtime::surface_materials::SurfaceTextures::layout(8));
        shadow_entries.extend(super::snow::Snow::layout());
        if device.limits().max_storage_buffers_per_shader_stage == 0 {
            shadow_entries.retain(|entry| entry.binding != 2);
        }
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow layout"),
            entries: &shadow_entries,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: std::mem::size_of::<Camera>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let local_lights = super::local_lights::LocalLights::new(&device);
        let pedestrians =
            super::pedestrian_scenery::Resources::new(&device, &queue, montreal_actors);
        let mut camera_bindings = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: local_lights.buffer.as_entire_binding(),
            },
        ];
        camera_bindings.extend(pedestrians.entries());
        let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera"),
            layout: &camera_layout,
            entries: &camera_bindings,
        });
        let player_shadow_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("player shadow camera"),
            size: std::mem::size_of::<Camera>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        camera_bindings[0].resource = player_shadow_uniform.as_entire_binding();
        let player_shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("player shadow camera"),
            layout: &camera_layout,
            entries: &camera_bindings,
        });
        let sky = Sky::new(&device, &queue, &camera_layout, format, samples);
        let world_shadow_resolution = SHADOW_RESOLUTION
            .min(device.limits().max_texture_dimension_2d - player_shadow::RESOLUTION);
        let shadow = depth(
            &device,
            world_shadow_resolution + player_shadow::RESOLUTION,
            world_shadow_resolution,
            "sun shadow atlas",
            1,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shadow PCF"),
            compare: Some(wgpu::CompareFunction::LessEqual),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let mut shadow_bindings = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&shadow),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ];
        shadow_bindings.extend(contact_texture.entries(2));
        shadow_bindings.extend(pavement.entries(8));
        shadow_bindings.extend(snow.entries());
        if device.limits().max_storage_buffers_per_shader_stage == 0 {
            shadow_bindings.retain(|entry| entry.binding != 2);
        }
        let shadow_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sun shadow"),
            layout: &shadow_layout,
            entries: &shadow_bindings,
        });
        let commerce = Commerce::new(&device, &queue, montreal_actors);
        let [pipeline, opaque_pipeline, graffiti_pipeline, shadow_pipeline, opaque_shadow_pipeline, vehicle_pipeline, vehicle_shadow_pipeline, pedestrian_pipeline, pedestrian_shadow_pipeline, ribbon_pipeline] =
            world_pipelines(
                &device,
                format,
                &camera_layout,
                &shadow_layout,
                &sky.environment.layout,
                &commerce.layout,
                samples,
            );
        let model_pipeline = super::model_textures::pipeline(
            &device,
            format,
            &camera_layout,
            &shadow_layout,
            &sky.environment.layout,
            &commerce.models.layout,
            samples,
        );
        let depth = depth(&device, width.max(1), height.max(1), "depth", samples);
        let color = color_target(&device, width, height, format, samples);
        let ssao = ssao::Ssao::new(
            &device,
            width.max(1),
            height.max(1),
            format,
            &depth,
            samples,
        );
        let snowfall = (!ssao::passthrough(&device))
            .then(|| snowfall::Snowfall::new(&device, format, &depth, &snow, samples));
        let mission_markers = mission_markers::MissionMarkers::new(&device, format, samples);
        let hud = Hud::new(&device, format);
        let layer_model = surface.as_ref().and_then(|_| {
            layer_model::LayerModel::bundled(
                &adapter,
                &device,
                &queue,
                format,
                [config.width, config.height],
            )
        });
        #[cfg(not(target_arch = "wasm32"))]
        let gpu_profile = (profile && timestamp_features.contains(wgpu::Features::TIMESTAMP_QUERY))
            .then(|| GpuProfile::new(&device));
        #[cfg(target_arch = "wasm32")]
        let gpu_profile = None;
        #[cfg(not(target_arch = "wasm32"))]
        let gpu_trace = (trace && timestamp_features.contains(wgpu::Features::TIMESTAMP_QUERY))
            .then(|| GpuTrace::new(&device));
        #[cfg(target_arch = "wasm32")]
        let gpu_trace = None;
        #[cfg(not(target_arch = "wasm32"))]
        let render_lod = if std::env::args().any(|arg| arg == "--profile-full-render") {
            RenderLod::Full
        } else if std::env::args().any(|arg| arg == "--profile-shadow-lod") {
            RenderLod::Shadows
        } else {
            RenderLod::Adaptive
        };
        #[cfg(target_arch = "wasm32")]
        let render_lod = RenderLod::Adaptive;
        Ok(Self {
            ribbons: projected_grass::Ribbons::new(&device),
            ribbon_pipeline,
            local_lights,
            instance,
            device,
            queue,
            surface,
            config,
            reserved_bottom: 0,
            flight_visibility: 1.,
            construction_span: None,
            underground_exits: None,
            ambient_occlusion: true,
            retro_lens: 0.,
            montreal_grade: false,
            zombie_mode: false,
            render_lod,
            contact_debris: true,
            contact_texture,
            ssao,
            layer_model,
            snowfall,
            mission_markers,
            pipeline,
            opaque_pipeline,
            graffiti_pipeline,
            shadow_pipeline,
            opaque_shadow_pipeline,
            vehicle_pipeline,
            model_pipeline,
            vehicle_shadow_pipeline,
            pedestrian_pipeline,
            pedestrian_shadow_pipeline,
            pedestrians,
            uniform,
            impacts: None,
            wheel_snow: None,
            camera_group,
            shadow_group,
            depth,
            color,
            samples,
            shadow,
            world_shadow_resolution,
            player_shadow_uniform,
            player_shadow_group,
            adapter_name,
            hud,
            cinematic: false,
            overlay: Overlay::Driving,
            full_map: Default::default(),
            menu_selection: MenuItem::Resume,
            daylight: None,
            season: Default::default(),
            winter_weather: true,
            snow,
            sky_settings: SkySettings::default(),
            sky,
            commerce,
            fps: None,
            gpu_trace,
            gpu_profile,
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn antialiasing(&self) -> bool {
        self.samples > 1
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_antialiasing(&mut self, enabled: bool) {
        let samples = if enabled && self.device.limits().max_storage_buffers_per_shader_stage > 0 {
            4
        } else {
            1
        };
        if samples == self.samples {
            return;
        }
        let format = self.config.format.add_srgb_suffix();
        self.model_pipeline = super::model_textures::pipeline(
            &self.device,
            format,
            &self.pipeline.get_bind_group_layout(0),
            &self.pipeline.get_bind_group_layout(1),
            &self.sky.environment.layout,
            &self.commerce.models.layout,
            samples,
        );
        let pipelines = world_pipelines(
            &self.device,
            format,
            &self.pipeline.get_bind_group_layout(0),
            &self.pipeline.get_bind_group_layout(1),
            &self.sky.environment.layout,
            &self.commerce.layout,
            samples,
        );
        [
            self.pipeline,
            self.opaque_pipeline,
            self.graffiti_pipeline,
            self.shadow_pipeline,
            self.opaque_shadow_pipeline,
            self.vehicle_pipeline,
            self.vehicle_shadow_pipeline,
            self.pedestrian_pipeline,
            self.pedestrian_shadow_pipeline,
            self.ribbon_pipeline,
        ] = pipelines;
        self.sky.set_samples(&self.device, samples);
        self.impacts = None;
        self.wheel_snow = None;
        self.samples = samples;
        self.depth = depth(
            &self.device,
            self.config.width,
            self.config.height,
            "depth",
            samples,
        );
        self.color = color_target(
            &self.device,
            self.config.width,
            self.config.height,
            format,
            samples,
        );
        self.ssao = ssao::Ssao::new(
            &self.device,
            self.config.width,
            self.config.height,
            format,
            &self.depth,
            samples,
        );
        self.snowfall = (!ssao::passthrough(&self.device)).then(|| {
            snowfall::Snowfall::new(&self.device, format, &self.depth, &self.snow, samples)
        });
        self.mission_markers = mission_markers::MissionMarkers::new(&self.device, format, samples);
    }
    pub fn recreate_surface(&mut self, window: Arc<Window>) -> Result<(), String> {
        self.surface = Some(
            self.instance
                .create_surface(window)
                .map_err(|e| e.to_string())?,
        );
        self.resize_targets(self.config.width, self.config.height);
        Ok(())
    }
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == self.config.width && height == self.config.height {
            // Surface-loss recovery still needs configuration, but not new render targets.
            if let Some(surface) = &self.surface {
                surface.configure(&self.device, &self.config);
            }
            return;
        }
        self.resize_targets(width, height);
    }
    fn resize_targets(&mut self, width: u32, height: u32) {
        let samples = self.samples;
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        if let Some(s) = &self.surface {
            s.configure(&self.device, &self.config);
        }
        self.depth = depth(&self.device, width, height, "depth", samples);
        self.color = color_target(
            &self.device,
            width,
            height,
            self.config.format.add_srgb_suffix(),
            samples,
        );
        self.ssao.resize(&self.device, width, height, &self.depth);
        if let Some(model) = &mut self.layer_model {
            model.resize(&self.device, [width, height]);
        }
        if let Some(snowfall) = &mut self.snowfall {
            snowfall.resize(&self.device, &self.depth);
        }
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
            [
                &self.camera_group,
                &self.shadow_group,
                &self.sky.environment.group,
            ],
        )
    }
    pub fn update_route(&mut self, points: &[[f32; 2]]) {
        self.hud.update_route(&self.device, points);
    }
    pub fn set_job_marker(&mut self, uv: Option<[f32; 2]>) {
        self.hud.job_marker = uv.map_or([0.; 4], |[x, y]| [x, y, 1., 0.]);
    }
    pub fn set_police_markers(&mut self, markers: impl Iterator<Item = ([f32; 2], bool)>) {
        self.hud.police.fill([0.; 4]);
        for (slot, (uv, walking)) in self.hud.police.iter_mut().zip(markers) {
            *slot = [uv[0], uv[1], if walking { 2. } else { 1. }, 0.];
        }
    }

    pub fn update_vector_map(
        &mut self,
        city: &std::sync::Arc<opencity::runtime::City>,
    ) -> Result<(), String> {
        self.hud.update_vectors(&self.device, &self.queue, city)
    }
    pub fn update_minimap(&mut self, pixels: &[u8]) {
        self.hud.update(&self.queue, pixels);
    }
    pub fn upload(&self, mesh: &Mesh) -> GpuMesh {
        self.upload_vertices(&mesh.vertices)
    }
    pub(super) fn configure_graffiti(&self, settings: super::graffiti::Settings) {
        self.commerce.graffiti.configure(&self.queue, settings);
    }
    pub fn upload_vertices(&self, vertices: &[Vertex]) -> GpuMesh {
        self.upload_with_bounds(vertices, Some(Bounds::of_vertices(vertices)))
    }
    // Transient meshes are already assembled around the player. Avoid scanning
    // every vertex again each frame; conservatively draw them in both passes.
    pub fn upload_dynamic(&self, mesh: &Mesh) -> GpuMesh {
        let mut gpu = GpuMesh {
            skin: None,
            instances: None,
            static_model: false,
            buffer: self.dynamic_buffer(std::mem::size_of_val(mesh.vertices.as_slice()) as u64),
            count: 0,
            opaque_count: 0,
            paint_start: 0,
            bounds: None,
            lod: None,
        };
        self.update_dynamic(&mut gpu, mesh);
        gpu
    }
    pub(super) fn upload_equipment(&self, vertices: &[Vertex]) -> GpuMesh {
        let mut mesh = self.upload_instanced(vertices);
        mesh.static_model = true;
        mesh
    }
    pub fn upload_instanced(&self, vertices: &[Vertex]) -> GpuMesh {
        let mut mesh = self.upload_vertices(vertices);
        mesh.instances = Some(Instances {
            buffer: self.dynamic_buffer(64),
            bounds: Vec::new(),
            scales: Vec::new(),
            force_full: Vec::new(),
            player_shadow: None,
        });
        mesh
    }
    pub(super) fn upload_skinned(
        &self,
        vertices: &[Vertex],
        skin: &[crate::dynamic_world::pedestrian_catalog::SkinVertex],
    ) -> GpuMesh {
        assert_eq!(vertices.len(), skin.len());
        let mut mesh = self.upload_instanced(vertices);
        mesh.skin = Some(
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("shared pedestrian weights"),
                    contents: bytemuck::cast_slice(skin),
                    usage: wgpu::BufferUsages::VERTEX,
                }),
        );
        // Skinning moves feet beyond the T-pose depth. Keep conservative local
        // animation bounds for both camera and sun culling.
        let bounds = mesh.bounds.as_mut().unwrap();
        bounds.min = bounds.min.min(Vec3::new(-2.4, -0.6, -2.4));
        bounds.max = bounds.max.max(Vec3::new(2.4, 2.4, 2.4));
        mesh
    }
    pub fn update_instances(&self, mesh: &mut GpuMesh, transforms: &[[[f32; 4]; 4]]) {
        self.update_actor_instances(mesh, transforms, None);
    }
    pub(super) fn update_actor_instances(
        &self,
        mesh: &mut GpuMesh,
        transforms: &[[[f32; 4]; 4]],
        player_shadow: Option<usize>,
    ) {
        let instances = mesh.instances.as_mut().expect("instanced geometry");
        instances.player_shadow = player_shadow;
        let buffer = &mut instances.buffer;
        let bytes = bytemuck::cast_slice(transforms);
        if bytes.len() as u64 > buffer.size() {
            *buffer = self.dynamic_buffer(bytes.len() as u64);
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(buffer, 0, bytes);
        }
        let local = mesh.bounds.expect("instanced part bounds");
        instances.bounds.clear();
        instances.bounds.extend(
            transforms
                .iter()
                .map(|columns| local.transformed(Mat4::from_cols_array_2d(columns))),
        );
        instances.scales.clear();
        instances.force_full.clear();
        for columns in transforms {
            let matrix = Mat4::from_cols_array_2d(columns);
            instances.scales.push(
                matrix
                    .x_axis
                    .truncate()
                    .length()
                    .max(matrix.y_axis.truncate().length())
                    .max(matrix.z_axis.truncate().length()),
            );
            instances.force_full.push(columns[1][3] > 0.5);
        }
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
        let (vertices, opaque_count, paint_start) = material_vertices(&mesh.vertices);
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
        gpu.paint_start = paint_start;
        gpu.bounds = None;
        gpu.lod = None;
    }
    fn upload_with_bounds(&self, source: &[Vertex], bounds: Option<Bounds>) -> GpuMesh {
        let (vertices, opaque_count, paint_start) = material_vertices(source);
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
            skin: None,
            instances: None,
            static_model: false,
            count: source.len() as u32,
            opaque_count,
            paint_start,
            bounds,
            lod: None,
        }
    }
    pub(super) fn view_projection(&self, eye: Vec3, look: Vec3) -> Mat4 {
        // Reverse scene depth (near=1, far=0) so distant pavement and terrain
        // retain precision without moving the camera near plane or road geometry.
        let aspect = self.config.width as f32 / self.scene_height() as f32;
        let projection = if let Some(span) = self.construction_span {
            Mat4::orthographic_rh(
                -span * aspect / 2.,
                span * aspect / 2.,
                -span / 2.,
                span / 2.,
                (span * 4.).max(100.),
                0.003,
            )
        } else {
            Mat4::perspective_rh(
                58_f32.to_radians(),
                self.config.width as f32 / self.scene_height() as f32,
                10.0 * self.flight_visibility,
                0.003,
            )
        };
        projection * Mat4::look_at_rh(eye, look, Vec3::Z)
    }
    pub(super) fn scene_height(&self) -> u32 {
        self.config
            .height
            .saturating_sub(self.reserved_bottom)
            .max(1)
    }
    fn view_matrices(&self, eye: Vec3, look: Vec3, sun: Vec3) -> (Mat4, Mat4) {
        let focus = Vec3::new(look.x, look.y, 0.15);
        let half = SHADOW_SPAN * 0.5;
        let light = Mat4::orthographic_rh(-half, half, -half, half, 0.01, 10.)
            * Mat4::look_at_rh(
                focus + sun * 4.,
                focus,
                if sun.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z },
            );
        (self.view_projection(eye, look), light)
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
        self.render_scene(target, meshes, eye, look, time, Some(navigation));
    }

    fn world_pass<'a>(
        &self,
        encoder: &'a mut wgpu::CommandEncoder,
        first: bool,
        last: bool,
        timestamp: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) -> wgpu::RenderPass<'a> {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("world"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: self.color.as_ref().unwrap_or(&self.ssao.scene),
                depth_slice: None,
                resolve_target: self
                    .color
                    .as_ref()
                    .filter(|_| last)
                    .map(|_| &self.ssao.scene),
                ops: wgpu::Operations {
                    load: if first {
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: if self.samples > 1 && last {
                        wgpu::StoreOp::Discard
                    } else {
                        wgpu::StoreOp::Store
                    },
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: if first {
                        wgpu::LoadOp::Clear(0.)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: timestamp,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(
            0.,
            0.,
            self.config.width as f32,
            self.scene_height() as f32,
            0.,
            1.,
        );
        pass.set_scissor_rect(0, 0, self.config.width, self.scene_height());
        pass.set_bind_group(0, &self.camera_group, &[]);
        pass.set_bind_group(1, &self.shadow_group, &[]);
        pass.set_bind_group(2, &self.sky.environment.group, &[]);
        pass.set_bind_group(3, &self.commerce.group, &[]);
        pass
    }

    pub(super) fn render_scene(
        &self,
        target: &wgpu::TextureView,
        meshes: &[&GpuMesh],
        eye: Vec3,
        look: Vec3,
        time: f32,
        navigation: Option<MinimapView>,
    ) {
        let _render = mtl_world::telemetry::span("render.cpu");
        let gpu = self.gpu_trace.as_ref().filter(|g| g.sample());
        self.commerce.models.sync(&self.queue);
        self.contact_texture
            .set_enabled(&self.queue, self.contact_debris);
        let lighting = Lighting::with_settings(time, self.daylight, self.sky_settings);
        let sun = lighting.sun;
        let (view, light) = self.view_matrices(eye, look, sun);
        let player_light = player_shadow::projection(meshes, sun);
        let camera_frustum =
            Frustum::from_matrix(view).with_underground_exits(&self.underground_exits);
        let shadow_frustum =
            Frustum::from_matrix(light).with_underground_exits(&self.underground_exits);
        let detail_view = self.detail_view(eye, look);
        let data = Camera {
            view_projection: view.to_cols_array_2d(),
            light_view_projection: light.to_cols_array_2d(),
            eye_time: [eye.x, eye.y, eye.z, time],
            sun_exposure: [sun.x, sun.y, sun.z, 1.35],
            daylight_sky: [
                lighting.daylight,
                f32::from(self.construction_span.is_some()),
                self.season.snow_cover(),
                self.flight_visibility.recip(),
            ],
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
                58_f32.to_radians() / self.scene_height() as f32,
            ],
            star_color: [
                self.sky_settings.star_color[0],
                self.sky_settings.star_color[1],
                self.sky_settings.star_color[2],
                lighting.star_visibility,
            ],
            player_light_view_projection: player_light.unwrap_or(Mat4::ZERO).to_cols_array_2d(),
            shadow_atlas: [
                self.world_shadow_resolution as f32,
                player_shadow::RESOLUTION as f32,
                1. / (self.world_shadow_resolution + player_shadow::RESOLUTION) as f32,
                1. / self.world_shadow_resolution as f32,
            ],
        };
        self.queue
            .write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        if let Some(player_light) = player_light {
            let player_camera = Camera {
                light_view_projection: player_light.to_cols_array_2d(),
                ..data
            };
            self.queue.write_buffer(
                &self.player_shadow_uniform,
                0,
                bytemuck::bytes_of(&player_camera),
            );
        }
        self.mission_markers.set_view(&self.queue, view);
        self.local_lights.upload(
            &self.queue,
            if self.construction_span.is_some() {
                look
            } else {
                eye
            },
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("world frame"),
            });
        let sky = mtl_world::telemetry::span("render.sky_environment");
        self.sky.update(
            &self.queue,
            &mut encoder,
            time,
            self.daylight,
            self.sky_settings,
        );
        drop(sky);
        {
            let _pass = mtl_world::telemetry::span("render.encode.shadow");
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
                timestamp_writes: self
                    .gpu_profile
                    .as_ref()
                    .map(|profile| profile.pass(0))
                    .or_else(|| gpu.map(|g| g.pass(Pass::Shadow))),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let resolution = self.world_shadow_resolution;
            pass.set_viewport(0., 0., resolution as f32, resolution as f32, 0., 1.);
            pass.set_scissor_rect(0, 0, resolution, resolution);
            pass.set_pipeline(&self.opaque_shadow_pipeline);
            pass.set_bind_group(0, &self.camera_group, &[]);
            pass.set_bind_group(3, &self.commerce.group, &[]);
            for mesh in meshes {
                if mesh.instances.is_some()
                    || mesh.opaque_count == 0
                    || !shadow_frustum.includes(mesh.bounds)
                {
                    continue;
                }
                let draw = mesh.static_draw(&detail_view, self.render_lod, true);
                draw.draw(&mut pass, 0..draw.opaque, 0..1);
            }
            pass.set_pipeline(&self.shadow_pipeline);
            for mesh in meshes {
                if mesh.instances.is_some()
                    || mesh.opaque_count == mesh.paint_start
                    || !shadow_frustum.includes(mesh.bounds)
                {
                    continue;
                }
                let draw = mesh.static_draw(&detail_view, self.render_lod, true);
                draw.draw(&mut pass, draw.opaque..draw.paint, 0..1);
            }
            pass.set_pipeline(&self.vehicle_shadow_pipeline);
            for mesh in meshes {
                if mesh.skin.is_some() {
                    continue;
                }
                if let Some(instances) = &mesh.instances {
                    for (draw, range) in
                        mesh.instance_draws(&shadow_frustum, &detail_view, self.render_lod, true)
                    {
                        pass.set_vertex_buffer(1, instances.buffer.slice(..));
                        draw.draw(&mut pass, 0..draw.paint, range);
                    }
                }
            }
            pass.set_pipeline(&self.pedestrian_shadow_pipeline);
            for mesh in meshes {
                if let (Some(skin), Some(instances)) = (&mesh.skin, &mesh.instances) {
                    for (draw, range) in
                        mesh.instance_draws(&shadow_frustum, &detail_view, self.render_lod, true)
                    {
                        pass.set_vertex_buffer(1, instances.buffer.slice(..));
                        pass.set_vertex_buffer(2, skin.slice(..));
                        draw.draw(&mut pass, 0..draw.paint, range);
                    }
                }
            }
            if player_light.is_some() {
                self.draw_player_shadows(&mut pass, meshes);
            }
        }
        {
            let split = gpu.filter(|gpu| gpu.split);
            let timestamp = self
                .gpu_profile
                .as_ref()
                .map(|profile| {
                    let mut writes = profile.pass(1);
                    writes.end_of_pass_write_index = None;
                    writes
                })
                .or_else(|| {
                    gpu.map(|g| {
                        if g.split {
                            g.shader_pass(Shader::Opaque)
                        } else {
                            g.pass(Pass::World)
                        }
                    })
                });
            let mut pass = self.world_pass(&mut encoder, true, split.is_none(), timestamp);
            // Tile-based GPUs only expose timestamps at pass boundaries. Additional
            // load/store operations are confined to explicitly requested shader samples.
            macro_rules! next_shader {
                ($group:ident) => {
                    if let Some(trace) = split {
                        drop(pass);
                        pass = self.world_pass(
                            &mut encoder,
                            false,
                            matches!(Shader::$group, Shader::Sky),
                            Some(trace.shader_pass(Shader::$group)),
                        );
                    } else if let Some(trace) = gpu {
                        trace.boundary(&mut pass, Shader::$group);
                    }
                };
            }
            let phase = mtl_world::telemetry::span("render.encode.opaque");
            pass.set_pipeline(&self.opaque_pipeline);
            for mesh in meshes {
                if mesh.instances.is_some()
                    || mesh.opaque_count == 0
                    || !camera_frustum.includes(mesh.bounds)
                {
                    continue;
                }
                let draw = mesh.static_draw(&detail_view, self.render_lod, false);
                draw.draw(&mut pass, 0..draw.opaque, 0..1);
            }
            drop(phase);
            next_shader!(Cutout);
            let phase = mtl_world::telemetry::span("render.encode.cutout");
            pass.set_pipeline(&self.pipeline);
            for mesh in meshes {
                if mesh.instances.is_some()
                    || mesh.opaque_count == mesh.paint_start
                    || !camera_frustum.includes(mesh.bounds)
                {
                    continue;
                }
                let draw = mesh.static_draw(&detail_view, self.render_lod, false);
                draw.draw(&mut pass, draw.opaque..draw.paint, 0..1);
            }
            drop(phase);
            next_shader!(Vehicles);
            let phase = mtl_world::telemetry::span("render.encode.vehicles");
            pass.set_pipeline(&self.vehicle_pipeline);
            for mesh in meshes {
                if mesh.skin.is_some() || mesh.static_model {
                    continue;
                }
                if let Some(instances) = &mesh.instances {
                    for (draw, range) in
                        mesh.instance_draws(&camera_frustum, &detail_view, self.render_lod, false)
                    {
                        pass.set_vertex_buffer(1, instances.buffer.slice(..));
                        draw.draw(&mut pass, 0..draw.count, range);
                    }
                }
            }
            drop(phase);
            next_shader!(Models);
            let phase = mtl_world::telemetry::span("render.encode.models");
            pass.set_pipeline(&self.model_pipeline);
            pass.set_bind_group(3, &self.commerce.models.group, &[]);
            for mesh in meshes {
                if !mesh.static_model {
                    continue;
                }
                if let Some(instances) = &mesh.instances {
                    for (draw, range) in
                        mesh.instance_draws(&camera_frustum, &detail_view, self.render_lod, false)
                    {
                        pass.set_vertex_buffer(1, instances.buffer.slice(..));
                        draw.draw(&mut pass, 0..draw.count, range);
                    }
                }
            }
            pass.set_bind_group(3, &self.commerce.group, &[]);
            drop(phase);
            next_shader!(Pedestrians);
            let phase = mtl_world::telemetry::span("render.encode.pedestrians");
            pass.set_pipeline(&self.pedestrian_pipeline);
            for mesh in meshes {
                if let (Some(skin), Some(instances)) = (&mesh.skin, &mesh.instances) {
                    for (draw, range) in
                        mesh.instance_draws(&camera_frustum, &detail_view, self.render_lod, false)
                    {
                        pass.set_vertex_buffer(1, instances.buffer.slice(..));
                        pass.set_vertex_buffer(2, skin.slice(..));
                        draw.draw(&mut pass, 0..draw.count, range);
                    }
                }
            }
            drop(phase);
            next_shader!(Graffiti);
            let phase = mtl_world::telemetry::span("render.encode.graffiti");
            pass.set_pipeline(&self.graffiti_pipeline);
            for mesh in meshes {
                if mesh.instances.is_some()
                    || mesh.paint_start == mesh.count
                    || !camera_frustum.includes(mesh.bounds)
                {
                    continue;
                }
                let draw = mesh.static_draw(&detail_view, self.render_lod, false);
                draw.draw(&mut pass, draw.paint..draw.count, 0..1);
            }
            drop(phase);
            next_shader!(Sky);
            let _phase = mtl_world::telemetry::span("render.encode.sky_and_effects");
            self.sky.draw(&mut pass, &self.camera_group);
            if self.ribbons.counts.iter().any(|&n| n > 0) {
                pass.set_pipeline(&self.ribbon_pipeline);
                pass.set_bind_group(0, &self.camera_group, &[]);
                pass.set_bind_group(1, &self.shadow_group, &[]);
                pass.set_bind_group(2, &self.sky.environment.group, &[]);
                pass.set_bind_group(3, &self.commerce.group, &[]);
                self.ribbons.draw(&mut pass);
            }
            if self.season == crate::season::Season::Winter {
                if let Some(snow) = &self.wheel_snow {
                    snow.draw(&mut pass, self);
                }
            }
            if let Some(impacts) = &self.impacts {
                impacts.draw(&mut pass, &self.camera_group);
            }
            if !self.cinematic {
                self.mission_markers.draw(&mut pass);
            }
        }
        let winter = self.season == crate::season::Season::Winter
            && self.winter_weather
            && self.construction_span.is_none();
        if let Some(snowfall) = self.snowfall.as_ref().filter(|_| winter) {
            snowfall.prepare(
                &self.queue,
                view,
                eye,
                time,
                self.config.width,
                self.scene_height(),
                lighting.daylight,
            );
        }
        let composite_target = self
            .layer_model
            .as_ref()
            .map_or(target, |model| &model.scene);
        self.ssao.draw(
            &self.queue,
            &mut encoder,
            composite_target,
            view,
            eye,
            self.config.width,
            self.scene_height(),
            self.ambient_occlusion,
            self.zombie_mode,
            self.montreal_grade
                && self.construction_span.is_none()
                && !self.zombie_mode
                && self.retro_lens == 0.,
            self.flight_visibility.recip(),
            [time, f32::from(winter), lighting.daylight, self.retro_lens],
            self.snowfall.as_ref().filter(|_| winter),
            gpu.map(|g| g.pass(Pass::Occlusion)),
            self.gpu_profile
                .as_ref()
                .map(|profile| {
                    let mut writes = profile.pass(1);
                    writes.beginning_of_pass_write_index = None;
                    writes
                })
                .or_else(|| gpu.map(|g| g.pass(Pass::Composite))),
        );
        if let Some(model) = &self.layer_model {
            let _span = mtl_world::telemetry::span("render.encode.layer_model");
            model.draw(
                &self.queue,
                &mut encoder,
                target,
                [self.config.width, self.scene_height()],
            );
        }
        let hud_navigation = navigation.filter(|_| {
            !self.cinematic && (!super::app::mobile_enabled() || self.overlay == Overlay::Map)
        });
        if let Some(navigation) = hud_navigation {
            let _span = mtl_world::telemetry::span("render.encode.hud");
            self.hud.render(
                &self.queue,
                &mut encoder,
                target,
                self.config.width,
                self.scene_height(),
                navigation,
                self.overlay,
                self.full_map,
                self.menu_selection,
                self.fps,
                self.gpu_profile
                    .as_ref()
                    .map(|profile| profile.pass(2))
                    .or_else(|| gpu.map(|g| g.pass(Pass::Hud))),
            );
        }
        if let Some(profile) = &self.gpu_profile {
            profile.resolve(&mut encoder);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(gpu) = gpu {
            gpu.finish(
                &mut encoder,
                self.queue.get_timestamp_period(),
                hud_navigation.is_some(),
            );
        }
        let _span = mtl_world::telemetry::span("render.submit");
        self.queue.submit(Some(encoder.finish()));
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn snapshot(&self, texture: &wgpu::Texture, path: &Path) -> Result<(), String> {
        let bytes = self.rgba_pixels(texture)?;
        let mut ppm = format!(
            "P6
{} {}
255
",
            self.config.width, self.config.height
        )
        .into_bytes();
        for pixel in bytes.chunks_exact(4) {
            ppm.extend_from_slice(&pixel[..3]);
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
            SurfaceKind::Water,
        ]
        .into_iter()
        .enumerate()
        {
            for corner in 0..3 {
                mesh.vertices.push(Vertex {
                    position: [id as f32, corner as f32, 0.],
                    normal: [0., 0., 1.],
                    color: [1.; 3],
                    surface: if kind == SurfaceKind::Water {
                        [
                            kind as u32 as f32,
                            -0.01 * corner as f32,
                            0.7,
                            Vertex::SHORE_PROFILE,
                        ]
                    } else {
                        [kind as u32 as f32, 0., 0., 0.]
                    },
                });
            }
        }
        let (vertices, opaque, paint_start) = material_vertices(&mesh.vertices);
        assert_eq!(paint_start, 12);
        assert_eq!(opaque, 6);
        assert_eq!(vertices.len(), 12);
        assert_eq!(
            vertices
                .iter()
                .map(|v| v.position[0] as usize)
                .collect::<Vec<_>>(),
            vec![0, 0, 0, 2, 2, 2, 1, 1, 1, 3, 3, 3]
        );
        assert!(vertices[6..]
            .iter()
            .all(|v| v.surface[0] == SurfaceKind::Water as u32 as f32
                || v.surface[0] == SurfaceKind::Fence as u32 as f32));
        for (uploaded, source) in vertices[9..].iter().zip(&mesh.vertices[9..]) {
            assert_eq!(
                uploaded.surface, source.surface,
                "preserve interpolated bed heights"
            );
        }
        mesh.vertices.truncate(3);
        let (vertices, opaque, paint_start) = material_vertices(&mesh.vertices);
        assert_eq!(paint_start, 3);
        assert!(matches!(vertices, Cow::Borrowed(_)));
        assert_eq!(opaque, 3);
    }
    #[test]
    fn road_and_runway_markings_blend_over_the_supporting_pavement() {
        use super::*;
        let marking = |pattern| Vertex {
            position: [0.; 3],
            normal: [0., 0., 1.],
            color: [1.; 3],
            surface: [SurfaceKind::Marking as u32 as f32, 0., 0., pattern],
        };
        let triangles = [[marking(1.); 3], [marking(0.); 3]];
        let (vertices, opaque, paint_start) = material_vertices(triangles.as_flattened());
        assert_eq!((opaque, paint_start), (0, 0));
        assert_eq!(vertices.len(), 6);
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
        let world_sources = [true, false].map(|storage| {
            format!(
                "{}\n{}\n{}",
                super::super::commerce::shader(),
                include_str!("vehicle.wgsl"),
                super::super::pedestrian_scenery::shader_for_storage(storage),
            )
        });
        for source in [
            world_sources[0].as_str(),
            world_sources[1].as_str(),
            opencity::runtime::EDITOR_SHADER,
            include_str!("render/passthrough.wgsl"),
        ] {
            let module = naga::front::wgsl::parse_str(source).unwrap();
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn game_pipelines_create_with_tree_shadow_bindings_when_adapter_is_available() {
        let result = pollster::block_on(super::Renderer::new(None, 64, 64));
        match result {
            Ok(mut renderer) => {
                assert!(!renderer.antialiasing());
                for enabled in [true, false] {
                    renderer.set_antialiasing(enabled);
                    assert_eq!(renderer.antialiasing(), enabled);
                    renderer.resize(80, 72);
                }
            }
            Err(message) if message.to_ascii_lowercase().contains("adapter") => {
                eprintln!("Skipping game pipeline validation: {message}");
            }
            Err(message) => panic!("game pipeline setup failed: {message}"),
        }
    }
}

fn depth_shader(source: &str, samples: u32) -> String {
    match samples {
        1 => source.replace("texture_depth_multisampled_2d", "texture_depth_2d"),
        4 => source.to_owned(),
        _ => unreachable!("unsupported sample count"),
    }
}
