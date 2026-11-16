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
    background: wgpu::RenderPipeline,
    background_layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
    panorama: wgpu::BindGroup,
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
        let (panorama_layout, panorama) = panorama::create(device, queue);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky background"),
            bind_group_layouts: &[
                Some(camera_layout),
                Some(&environment.layout),
                Some(&panorama_layout),
            ],
            immediate_size: 0,
        });
        let background = Self::background_pipeline(device, &layout, format, samples);
        Self {
            environment,
            background,
            background_layout: layout,
            format,
            panorama,
            last_bake: Cell::new(None),
            updates: Cell::new(0),
        }
    }
    fn background_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky background"),
            source: wgpu::ShaderSource::Wgsl(background_source().into()),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky background"),
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
        })
    }
    pub fn set_samples(&mut self, device: &wgpu::Device, samples: u32) {
        self.background =
            Self::background_pipeline(device, &self.background_layout, self.format, samples);
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
        pass.set_pipeline(&self.background);
        pass.set_bind_group(0, camera, &[]);
        pass.set_bind_group(1, &self.environment.group, &[]);
        pass.set_bind_group(2, &self.panorama, &[]);
        pass.draw(0..3, 0..1);
    }
}
fn background_source() -> String {
    format!(
        "{}\n{}\n{}",
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
