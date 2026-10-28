use crate::hermes::MinimapView;
use crate::hermes::TEXTURE_SIZE;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Overlay {
    #[default]
    Start,
    Driving,
    Menu,
    Map,
}
impl Overlay {
    pub fn escape(self) -> Self {
        match self {
            Self::Driving => Self::Menu,
            Self::Start | Self::Menu | Self::Map => Self::Driving,
        }
    }
    pub fn paused(self) -> bool {
        self != Self::Driving
    }
}
pub fn minimap_rect(width: f32, height: f32) -> [f32; 4] {
    let side = (height.min(width) * 0.29)
        .clamp(60., 340.)
        .min(width.min(height) - 16.)
        .max(1.);
    [16., height - side - 16., side, side]
}
pub fn resume_rect(width: f32, height: f32) -> [f32; 4] {
    [width * 0.5 - 110., height * 0.5, 220., 56.]
}
pub fn exit_rect(width: f32, height: f32) -> [f32; 4] {
    [width * 0.5 - 110., height * 0.5 + 68., 220., 48.]
}
pub fn contains(rect: [f32; 4], point: [f32; 2]) -> bool {
    point[0] >= rect[0]
        && point[0] <= rect[0] + rect[2]
        && point[1] >= rect[1]
        && point[1] <= rect[1] + rect[3]
}
pub fn minimap_contains(width: f32, height: f32, point: [f32; 2]) -> bool {
    let r = minimap_rect(width, height);
    (point[0] - r[0] - r[2] * 0.5).hypot(point[1] - r[1] - r[3] * 0.5) <= r[2] * 0.5
}

use bytemuck::{Pod, Zeroable};
#[derive(Clone, Copy, Pod, Zeroable)]
#[repr(C)]
struct Uniform {
    rect: [f32; 4],
    screen: [f32; 4],
    map: [f32; 4],
    mode: [f32; 4],
}
pub struct Hud {
    texture: wgpu::Texture,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    ready: bool,
}
impl Hud {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Hermes SVG minimap"),
            size: wgpu::Extent3d {
                width: TEXTURE_SIZE,
                height: TEXTURE_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("minimap view"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("minimap"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("minimap"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("minimap"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hud.wgsl").into()),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("minimap overlay"),
            layout: Some(&layout),
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
            multiview: None,
            cache: None,
        });
        Self {
            texture,
            uniform,
            group,
            pipeline,
            ready: false,
        }
    }
    pub fn update(&mut self, queue: &wgpu::Queue, pixels: &[u8]) {
        assert_eq!(
            pixels.len(),
            TEXTURE_SIZE as usize * TEXTURE_SIZE as usize * 4
        );
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(TEXTURE_SIZE * 4),
                rows_per_image: Some(TEXTURE_SIZE),
            },
            wgpu::Extent3d {
                width: TEXTURE_SIZE,
                height: TEXTURE_SIZE,
                depth_or_array_layers: 1,
            },
        );
        self.ready = true;
    }
    pub fn render(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        width: u32,
        height: u32,
        view: MinimapView,
        overlay: Overlay,
    ) {
        if !self.ready {
            return;
        }
        let rect = [0., 0., width as f32, height as f32];
        let data = Uniform {
            rect,
            screen: [width as f32, height as f32, view.uv_span, view.heading],
            map: [
                view.uv_center[0],
                view.uv_center[1],
                view.player_uv[0],
                view.player_uv[1],
            ],
            mode: [
                match overlay {
                    Overlay::Driving => 0.,
                    Overlay::Start | Overlay::Menu => 1.,
                    Overlay::Map => 2.,
                },
                if overlay == Overlay::Start { 1. } else { 0. },
                0.,
                0.,
            ],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Hermes minimap"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..6, 0..1);
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn circular_hit_target_excludes_square_corners_at_multiple_window_sizes() {
        for (w, h) in [(1280., 800.), (1920., 1080.), (320., 240.)] {
            let r = super::minimap_rect(w, h);
            assert!(super::minimap_contains(
                w,
                h,
                [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5]
            ));
            assert!(!super::minimap_contains(w, h, [r[0] + 1., r[1] + 1.]));
            assert!(!super::minimap_contains(
                w,
                h,
                [r[0] + r[2] + 1., r[1] + r[3] * 0.5]
            ));
        }
        assert_eq!(super::Overlay::Driving.escape(), super::Overlay::Menu);
        assert_eq!(super::Overlay::Menu.escape(), super::Overlay::Driving);
        assert_eq!(super::Overlay::Map.escape(), super::Overlay::Driving);
    }
    #[test]
    fn minimap_shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("hud.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
