//! Small shared GPU environment. Camera motion only changes sampling, never the bake.
use super::lighting::{Lighting, SkySettings};
use std::cell::Cell;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod capture_tests;
mod panorama;

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
    pub environment: opencity::runtime::surface_materials::EnvironmentMap,
    backgrounds: Vec<(wgpu::ShaderModule, wgpu::RenderPipeline)>,
    pub(super) shader_index: usize,
    samples: u32,
    background_layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    panoramas: Vec<wgpu::BindGroup>,
    pub(super) texture_index: usize,
    panorama_layout: wgpu::BindGroupLayout,
    last_bake: Cell<Option<BakeKey>>,
    updates: Cell<u64>,
}
impl Sky {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        camera_layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let environment = opencity::runtime::surface_materials::EnvironmentMap::new(device);
        let panorama_layout = panorama::layout(device);
        let panorama = panorama::create(
            device,
            queue,
            &panorama_layout,
            include_bytes!("../../assets/sky/cloud-highlights.png"),
            false,
        )
        .expect("embedded sky panorama");
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky background"),
            bind_group_layouts: &[
                Some(camera_layout),
                Some(&environment.layout),
                Some(&panorama_layout),
            ],
            immediate_size: 0,
        });
        let module = Self::shader_module(device, DEFAULT_SHADER);
        let background = Self::background_pipeline(device, &layout, format, samples, &module);
        Self {
            environment,
            backgrounds: vec![(module, background)],
            shader_index: 0,
            samples,
            background_layout: layout,
            format,
            panoramas: vec![panorama],
            texture_index: 0,
            panorama_layout,
            last_bake: Cell::new(None),
            updates: Cell::new(0),
        }
    }
    pub(super) fn add_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        png: &[u8],
    ) -> Result<usize, String> {
        let group = panorama::create(device, queue, &self.panorama_layout, png, true)?;
        self.panoramas.push(group);
        Ok(self.panoramas.len() - 1)
    }

    pub(super) async fn add_shader(
        &mut self,
        device: &wgpu::Device,
        hook: &str,
    ) -> Result<usize, String> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = Self::shader_module(device, hook);
        let pipeline = Self::background_pipeline(
            device,
            &self.background_layout,
            self.format,
            self.samples,
            &module,
        );
        if let Some(error) = scope.pop().await {
            return Err(error.to_string());
        }
        self.backgrounds.push((module, pipeline));
        Ok(self.backgrounds.len() - 1)
    }

    fn shader_module(device: &wgpu::Device, hook: &str) -> wgpu::ShaderModule {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky background"),
            source: wgpu::ShaderSource::Wgsl(background_source(hook).into()),
        })
    }

    fn background_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        format: wgpu::TextureFormat,
        samples: u32,
        shader: &wgpu::ShaderModule,
    ) -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky background"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
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
        })
    }
    pub fn set_samples(&mut self, device: &wgpu::Device, samples: u32) {
        self.samples = samples;
        for (module, pipeline) in &mut self.backgrounds {
            *pipeline = Self::background_pipeline(
                device,
                &self.background_layout,
                self.format,
                samples,
                module,
            );
        }
    }
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
        self.environment.bake(
            queue,
            encoder,
            [
                [
                    lighting.solar.x,
                    lighting.solar.y,
                    lighting.solar.z,
                    settings.sun_intensity,
                ],
                [color[0], color[1], color[2], settings.turbidity],
                [settings.horizon_glow, 0., 0., 0.],
            ],
        );
        self.last_bake.set(Some(key));
        self.updates.set(self.updates.get() + 1);
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn update_count(&self) -> u64 {
        self.updates.get()
    }
    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, camera: &'a wgpu::BindGroup) {
        pass.set_pipeline(&self.backgrounds[self.shader_index].1);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, &self.environment.group, &[]);
        pass.set_bind_group(2, &self.panoramas[self.texture_index], &[]);
        pass.draw(0..3, 0..1);
    }
}
const DEFAULT_SHADER: &str = "
fn owe_skydome(input: SkydomeInput) -> vec3<f32> {
    return mix(input.sky, input.panorama.rgb, select(0.0, input.panorama.a, custom_panorama != 0u));
}";

fn background_source(hook: &str) -> String {
    format!(
        "{}\n{}\n{}\n{hook}",
        opencity::runtime::FILMIC_SHADER,
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
            include_str!("../../opencity/src/runtime/sky_filter.wgsl").to_owned(),
            include_str!("../../opencity/src/runtime/sky_environment.wgsl").to_owned(),
            background_source(DEFAULT_SHADER),
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
