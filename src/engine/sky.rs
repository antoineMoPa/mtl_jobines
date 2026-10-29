//! Small shared GPU environment. Camera motion only changes sampling, never the bake.
use super::lighting::{Lighting, SkySettings};
use bytemuck::{Pod, Zeroable};
use std::cell::Cell;

// Six 512² RGBA16F faces use 12 MiB; shared by the background, glass and fog.
const SIZE: u32 = 512;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Environment {
    sun: [f32; 4],
    color_haze: [f32; 4],
    night: [f32; 4],
}
#[derive(Clone, Copy, PartialEq)]
struct AtmosphereKey {
    latitude: f32,
    day_of_year: u32,
    north_rotation: f32,
    turbidity: f32,
    sun_temperature: f32,
    sun_intensity: f32,
    horizon_glow: f32,
}
#[derive(Clone, Copy, PartialEq)]
struct BakeKey {
    atmosphere: AtmosphereKey,
    hour_tick: i32,
}
impl BakeKey {
    fn at(time: f32, manual: Option<f32>, settings: SkySettings) -> Self {
        Self {
            atmosphere: AtmosphereKey {
                latitude: settings.latitude,
                day_of_year: settings.day_of_year,
                north_rotation: settings.north_rotation,
                turbidity: settings.turbidity,
                sun_temperature: settings.sun_temperature,
                sun_intensity: settings.sun_intensity,
                horizon_glow: settings.horizon_glow,
            },
            hour_tick: (settings.hour(time, manual) * 200.).floor() as i32,
        }
    }
}
pub struct Sky {
    pub layout: wgpu::BindGroupLayout,
    pub group: wgpu::BindGroup,
    faces: Vec<wgpu::TextureView>,
    uniform: wgpu::Buffer,
    generator_group: wgpu::BindGroup,
    generator: wgpu::RenderPipeline,
    background: wgpu::RenderPipeline,
    last_bake: Cell<Option<BakeKey>>,
    updates: Cell<u64>,
}
impl Sky {
    pub fn new(
        device: &wgpu::Device,
        camera_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shared sky cube"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
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
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shared atmosphere and stars"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let cube = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            ..Default::default()
        });
        let faces = (0..6)
            .map(|face| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: face,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sky linear sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shared sky"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&cube),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let generator_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atmosphere settings"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("atmosphere settings"),
            size: std::mem::size_of::<Environment>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let generator_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atmosphere settings"),
            layout: &generator_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });
        let pipeline =
            |label, source: &str, layouts: &[&wgpu::BindGroupLayout], format, samples, depth| {
                let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(label),
                    source: wgpu::ShaderSource::Wgsl(source.into()),
                });
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some(label),
                    bind_group_layouts: layouts,
                    push_constant_ranges: &[],
                });
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs"),
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    primitive: Default::default(),
                    depth_stencil: depth,
                    multisample: wgpu::MultisampleState {
                        count: samples,
                        ..Default::default()
                    },
                    multiview: None,
                    cache: None,
                })
            };
        let generator = pipeline(
            "bake atmosphere",
            include_str!("sky_environment.wgsl"),
            &[&generator_layout],
            wgpu::TextureFormat::Rgba16Float,
            1,
            None,
        );
        let background = pipeline(
            "sky background",
            &background_source(),
            &[camera_layout, &layout],
            format,
            4,
            Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::LessEqual,
                stencil: Default::default(),
                bias: Default::default(),
            }),
        );
        Self {
            layout,
            group,
            faces,
            uniform,
            generator_group,
            generator,
            background,
            last_bake: Cell::new(None),
            updates: Cell::new(0),
        }
    }
    /// Four refreshes/second during the cycle; settings edits refresh immediately.
    /// Manual time is static. All six faces use one state in the same submission.
    pub fn update(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        time: f32,
        manual: Option<f32>,
        settings: SkySettings,
    ) {
        let key = BakeKey::at(time, manual, settings);
        if self.last_bake.get() == Some(key) {
            return;
        }
        let lighting = Lighting::with_settings(time, manual, settings);
        let color = lighting.sun_color;
        let environment = Environment {
            sun: [
                lighting.solar.x,
                lighting.solar.y,
                lighting.solar.z,
                settings.sun_intensity,
            ],
            color_haze: [color[0], color[1], color[2], settings.turbidity],
            night: [settings.horizon_glow, 0., 0., 0.],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&environment));
        for (face, target) in self.faces.iter().enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sky cube face"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.generator);
            pass.set_bind_group(0, &self.generator_group, &[]);
            pass.draw(0..3, face as u32..face as u32 + 1);
        }
        self.last_bake.set(Some(key));
        self.updates.set(self.updates.get() + 1);
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn update_count(&self) -> u64 {
        self.updates.get()
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, camera: &'a wgpu::BindGroup) {
        pass.set_pipeline(&self.background);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, &self.group, &[]);
        pass.draw(0..3, 0..1);
    }
}
fn background_source() -> String {
    format!(
        "{}\n{}",
        opencity::runtime::SKY_STARS_SHADER,
        include_str!("sky_background.wgsl")
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bake_cache_tracks_time_and_settings_and_holds_manual_time() {
        let settings = SkySettings::default();
        assert!(BakeKey::at(0., None, settings) == BakeKey::at(0.1, None, settings));
        assert!(BakeKey::at(0., None, settings) != BakeKey::at(0.3, None, settings));
        assert!(BakeKey::at(0., Some(0.), settings) == BakeKey::at(300., Some(0.), settings));
        let held = SkySettings {
            solar_hour: Some(19.),
            ..settings
        };
        assert!(BakeKey::at(0., None, held) == BakeKey::at(300., None, held));
        let hazy = SkySettings {
            turbidity: 8.,
            ..settings
        };
        assert!(BakeKey::at(0., None, settings) != BakeKey::at(0., None, hazy));
    }
    #[test]
    fn native_star_settings_do_not_rebake_the_atmosphere() {
        let settings = SkySettings::default();
        let edited = SkySettings {
            star_density: 0.4,
            star_size: 0.5,
            star_brightness: 4.,
            star_color: [0.5, 0.8, 1.],
            ..settings
        };
        assert!(BakeKey::at(0., None, settings) == BakeKey::at(0., None, edited));
    }
    #[test]
    fn sky_shaders_validate() {
        for source in [
            include_str!("sky_environment.wgsl").to_owned(),
            background_source(),
        ] {
            let module = naga::front::wgsl::parse_str(&source).unwrap();
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
