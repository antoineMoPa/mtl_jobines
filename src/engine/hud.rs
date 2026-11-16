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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuItem {
    #[default]
    Resume,
    Map,
    Settings,
    Exit,
}
impl MenuItem {
    pub fn previous(self) -> Self {
        match self {
            Self::Resume => Self::Exit,
            Self::Map => Self::Resume,
            Self::Exit => Self::Settings,
            Self::Settings => Self::Map,
        }
    }
    pub fn other(self) -> Self {
        match self {
            Self::Resume => Self::Map,
            Self::Map => Self::Settings,
            Self::Settings => Self::Exit,
            Self::Exit => Self::Resume,
        }
    }
}
pub fn minimap_rect(width: f32, height: f32) -> [f32; 4] {
    let side = (height.min(width) * 0.29)
        .clamp(60., 340.)
        .min(width.min(height) - 16.)
        .max(1.);
    [16., height - side - 16., side, side]
}
pub fn full_map_rect(width: f32, height: f32) -> [f32; 4] {
    [24., 84., width - 48., height - 108.]
}
pub fn full_map_close_rect(width: f32) -> [f32; 4] {
    [width - 66., 34., 38., 38.]
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MapSelection {
    #[default]
    Route,
    Jump,
    JumpBlocked,
}
impl MapSelection {
    fn shader_value(self) -> f32 {
        match self {
            Self::Route => 0.,
            Self::Jump => 1.,
            Self::JumpBlocked => 2.,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct FullMapView {
    center: [f32; 2],
    zoom: f32,
    pub selection: MapSelection,
}
impl Default for FullMapView {
    fn default() -> Self {
        Self {
            center: [0.5; 2],
            zoom: 1.,
            selection: MapSelection::Route,
        }
    }
}
impl FullMapView {
    pub fn raster_view(&self, width: f32, height: f32) -> ([f32; 2], f32) {
        let [_, _, w, h] = full_map_rect(width, height);
        (self.center, w.max(h) / w.min(h).max(1.) / self.zoom)
    }
    pub fn reset_zoom(&mut self) {
        *self = Self {
            selection: self.selection,
            ..Default::default()
        };
    }

    /// Cursor-anchored zoom; keyboard zoom uses the center of the viewport.
    pub fn zoom_at(&mut self, width: f32, height: f32, point: [f32; 2], steps: f32) {
        let Some(anchor) = self.map_uv(width, height, point) else {
            return;
        };
        let previous = self.zoom;
        self.zoom = (self.zoom * (steps * 0.2).exp()).clamp(1., 16.);
        let half_span = 0.5 / self.zoom;
        for (center, anchor) in self.center.iter_mut().zip(anchor) {
            *center = (anchor - (anchor - *center) * previous / self.zoom)
                .clamp(half_span, 1. - half_span);
        }
    }

    /// Inverse of the rotated, zoomed shader projection.
    pub fn map_uv(self, width: f32, height: f32, point: [f32; 2]) -> Option<[f32; 2]> {
        let [left, top, map_width, map_height] = full_map_rect(width, height);
        let size = [map_width, map_height];
        let span = size[0].min(size[1]);
        if span <= 0. || !contains([left, top, size[0], size[1]], point) {
            return None;
        }
        let uv = [
            self.center[0] + (point[1] - top - size[1] * 0.5) / (span * self.zoom),
            self.center[1] - (point[0] - left - size[0] * 0.5) / (span * self.zoom),
        ];
        uv.iter().all(|v| (0. ..=1.).contains(v)).then_some(uv)
    }
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
    texture_region: [f32; 4],
    rect: [f32; 4],
    screen: [f32; 4],
    map: [f32; 4],
    mode: [f32; 4],
    full_map: [f32; 4],
    destination: [f32; 4],
    performance: [f32; 4],
    vehicle: [f32; 4],
    vehicle_letters: [[u32; 4]; 12],
}
pub struct Hud {
    texture: wgpu::Texture,
    uniform: wgpu::Buffer,
    group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    ready: bool,
    route_pipeline: wgpu::RenderPipeline,
    route_buffer: wgpu::Buffer,
    route_count: u32,
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
            size: std::mem::size_of::<Uniform>() as u64,
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
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
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
            multiview_mask: None,
            cache: None,
        });
        let route_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Hermes route"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_route"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 20,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_route"),
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
            multiview_mask: None,
            cache: None,
        });
        let route_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("empty route"),
            size: 20,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });
        Self {
            texture,
            uniform,
            group,
            pipeline,
            ready: false,
            route_pipeline,
            route_buffer,
            route_count: 0,
        }
    }
    pub fn update_route(&mut self, device: &wgpu::Device, points: &[[f32; 2]]) {
        use wgpu::util::DeviceExt;
        let mut vertices: Vec<[f32; 5]> = Vec::new();
        for pair in points.windows(2) {
            if pair[0] == pair[1] {
                continue;
            }
            let [a, b] = [pair[0], pair[1]];
            for (p, q, side) in [
                (a, b, 1.),
                (a, b, -1.),
                (b, a, -1.),
                (b, a, -1.),
                (a, b, -1.),
                (b, a, 1.),
            ] {
                vertices.push([p[0], p[1], q[0], q[1], side]);
            }
        }
        self.route_count = vertices.len() as u32;
        if !vertices.is_empty() {
            self.route_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Hermes route ribbon"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        }
    }
    pub fn update(&mut self, queue: &wgpu::Queue, pixels: &[u8]) {
        assert_eq!(
            pixels.len(),
            TEXTURE_SIZE as usize * TEXTURE_SIZE as usize * 4
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
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
        full_map: FullMapView,
        menu_selection: MenuItem,
        fps: Option<f32>,
        vehicle_name: Option<&str>,
        timestamps: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        if !self.ready {
            return;
        }
        let rect = [0., 0., width as f32, height as f32];
        let mut vehicle_letters = [[32; 4]; 12];
        let mut vehicle_length = 0;
        if let Some(name) = vehicle_name {
            for (index, letter) in name.chars().take(48).enumerate() {
                vehicle_letters[index / 4][index % 4] = letter as u32;
                vehicle_length += 1;
            }
        }
        let data = Uniform {
            texture_region: view.texture_region,
            full_map: [
                full_map.center[0],
                full_map.center[1],
                full_map.zoom,
                full_map.selection.shader_value(),
            ],
            rect,
            screen: [width as f32, height as f32, view.uv_span, view.heading],
            map: [
                view.uv_center[0],
                view.uv_center[1],
                view.player_uv[0],
                view.player_uv[1],
            ],
            destination: [
                view.destination_uv[0],
                view.destination_uv[1],
                match view.route_status {
                    crate::hermes::RouteStatus::Inactive => 0.,
                    crate::hermes::RouteStatus::Active => 1.,
                    crate::hermes::RouteStatus::Unreachable => 2.,
                    crate::hermes::RouteStatus::Arrived => 3.,
                },
                0.,
            ],
            performance: [fps.unwrap_or(0.), 0., 0., 0.],
            vehicle: [vehicle_length as f32, 0., 0., 0.],
            vehicle_letters,
            mode: [
                match overlay {
                    Overlay::Driving => 0.,
                    Overlay::Start | Overlay::Menu => 1.,
                    Overlay::Map => 2.,
                },
                if overlay == Overlay::Start { 1. } else { 0. },
                match menu_selection {
                    MenuItem::Resume => 0.,
                    MenuItem::Map => 1.,
                    MenuItem::Settings => 2.,
                    MenuItem::Exit => 3.,
                },
                0.,
            ],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Hermes minimap"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamps,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(0., 0., width as f32, height as f32, 0., 1.);
        pass.set_scissor_rect(0, 0, width, height);
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.draw(0..if overlay == Overlay::Driving { 18 } else { 6 }, 0..1);
        if matches!(overlay, Overlay::Driving | Overlay::Map) && self.route_count > 0 {
            pass.set_pipeline(&self.route_pipeline);
            pass.set_vertex_buffer(0, self.route_buffer.slice(..));
            pass.draw(0..self.route_count, 0..1);
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn full_map_zoom_keeps_cursor_anchor_and_limits_then_resets_to_overview() {
        for (w, h) in [(1280., 800.), (800., 1280.), (320., 240.)] {
            let mut view = super::FullMapView::default();
            let span = (w - 48.0_f32).min(h - 108.0);
            let point = [w * 0.5 + span * 0.2, h * 0.5 + 30. - span * 0.15];
            let anchor = view.map_uv(w, h, point).unwrap();
            view.zoom_at(w, h, point, 4.);
            let actual = view.map_uv(w, h, point).unwrap();
            assert!(view.zoom > 1.);
            for i in 0..2 {
                assert!((actual[i] - anchor[i]).abs() < 1e-6);
            }
            let center = [w * 0.5, h * 0.5 + 30.];
            let right = view
                .map_uv(w, h, [center[0] + span * 0.1, center[1]])
                .unwrap();
            assert!((right[1] - (view.center[1] - 0.1 / view.zoom)).abs() < 1e-6);
            view.zoom_at(w, h, center, 1000.);
            assert_eq!(view.zoom, 16.);
            view.zoom_at(w, h, center, -1000.);
            assert_eq!(view.zoom, 1.);
            assert_eq!(view.center, [0.5; 2]);
            view.zoom_at(w, h, [44., 44.], 3.);
            assert_eq!(view.zoom, 1., "header scrolling must not zoom the map");
        }
    }

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
    fn map_clicks_match_shader_projection_and_exclude_header_and_padding() {
        for (w, h) in [(1280., 800.), (800., 1280.), (320., 240.)] {
            let center = [w * 0.5, h * 0.5 + 30.];
            assert_eq!(
                super::FullMapView::default().map_uv(w, h, center),
                Some([0.5, 0.5])
            );
            let span = (w - 48.0_f32).min(h - 108.0);
            assert_eq!(
                super::FullMapView::default().map_uv(
                    w,
                    h,
                    [center[0] + span * 0.25, center[1] - span * 0.25]
                ),
                Some([0.25, 0.25])
            );
            // Atlas east points down; atlas north points right after the quarter turn.
            for (offset, expected) in [
                ([0., 0.25], [0.75, 0.5]),
                ([0.25, 0.], [0.5, 0.25]),
                ([0., -0.25], [0.25, 0.5]),
                ([-0.25, 0.], [0.5, 0.75]),
            ] {
                assert_eq!(
                    super::FullMapView::default().map_uv(
                        w,
                        h,
                        [center[0] + span * offset[0], center[1] + span * offset[1]]
                    ),
                    Some(expected)
                );
            }
            assert!(super::FullMapView::default()
                .map_uv(w, h, [44., 44.])
                .is_none());
            assert!(super::FullMapView::default()
                .map_uv(w, h, [w - 20., h - 20.])
                .is_none());
        }
        assert!(super::FullMapView::default()
            .map_uv(1280., 800., [23., 400.])
            .is_none());
        assert!(super::FullMapView::default()
            .map_uv(30., 30., [15., 15.])
            .is_none());
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
