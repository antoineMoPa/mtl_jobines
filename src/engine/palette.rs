//! Shared OpenCity palette, with a small painter for the game's wgpu 23 device.
//! egui-wgpu 0.36 targets a newer wgpu, so its renderer cannot share this device.
use super::render::Renderer;
use crate::hermes::Landmark;
#[cfg(not(target_arch = "wasm32"))]
use egui_winit::State as Input;
#[cfg(target_arch = "wasm32")]
#[path = "palette_web_input.rs"]
mod web_input;
use egui_command_palette::{Command, CommandPalette};
use std::collections::HashMap;
#[cfg(target_arch = "wasm32")]
use web_input::Input;
use wgpu::util::DeviceExt;
use winit::{event::WindowEvent, window::Window};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TimeCommand {
    Day,
    Night,
    Automatic,
}
impl TimeCommand {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "time.day" => Some(Self::Day),
            "time.night" => Some(Self::Night),
            "time.automatic" => Some(Self::Automatic),
            _ => None,
        }
    }
    pub fn daylight(self) -> Option<f32> {
        match self {
            Self::Day => Some(1.),
            Self::Night => Some(0.),
            Self::Automatic => None,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(super) enum PaletteAction {
    Time(TimeCommand),
    Respawn(String),
    JumpToLocation,
    #[cfg(not(target_arch = "wasm32"))]
    ExpandNetwork,
    #[cfg(not(target_arch = "wasm32"))]
    GrowNetwork,
    #[cfg(not(target_arch = "wasm32"))]
    BugReport,
}
impl PaletteAction {
    fn from_id(id: &str) -> Option<Self> {
        #[cfg(not(target_arch = "wasm32"))]
        if id == "road.expand" {
            return Some(Self::ExpandNetwork);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if id == "road.grow" {
            return Some(Self::GrowNetwork);
        }
        if id == "map.jump" {
            return Some(Self::JumpToLocation);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if id == "bug.report" {
            return Some(Self::BugReport);
        }
        TimeCommand::from_id(id).map(Self::Time).or_else(|| {
            id.strip_prefix("respawn.")
                .map(|landmark| Self::Respawn(landmark.into()))
        })
    }
}
fn commands(landmarks: &[Landmark]) -> Vec<Command> {
    let mut commands: Vec<_> = [
        (
            "time.day",
            "Time: Set day",
            "Freeze daylight at noon for debugging",
        ),
        (
            "time.night",
            "Time: Set night",
            "Freeze daylight at midnight for debugging",
        ),
        (
            "time.automatic",
            "Time: Resume automatic cycle",
            "Restore the twenty-minute day/night cycle",
        ),
        (
            "sky.settings",
            "Sky: Atmosphere and stars…",
            "Set solar time, location, haze, sun color and night stars",
        ),
        (
            "sound.settings",
            "Sound: Volume…",
            "Adjust engines, tires, surroundings and collisions",
        ),
        (
            "map.jump",
            "Jump to location…",
            "Click the map to move yourself and your current car there",
        ),
    ]
    .into_iter()
    .map(|(id, title, description)| Command {
        id: id.into(),
        title: title.into(),
        description: description.into(),
        shortcut: String::new(),
    })
    .collect();
    #[cfg(not(target_arch = "wasm32"))]
    commands.push(Command {
        id: "bug.report".into(),
        title: "Bug: Report a bug…".into(),
        description: "Save a screenshot, map location and description to bug-reports".into(),
        shortcut: String::new(),
    });
    #[cfg(not(target_arch = "wasm32"))]
    commands.push(Command {
        id: "road.expand".into(),
        title: "Expand road network…".into(),
        description: "Extend roads from their open ends".into(),
        shortcut: String::new(),
    });
    commands.extend(landmarks.iter().map(|landmark| Command {
        id: format!("respawn.{}", landmark.id),
        title: format!("Respawn with car: {}", landmark.title),
        description: "Bring your current car beside the landmark and face the view".into(),
        shortcut: String::new(),
    }));
    #[cfg(not(target_arch = "wasm32"))]
    commands.push(Command {
        id: "road.grow".into(),
        title: "Auto-grow road network…".into(),
        description: "Grow a network using a seed and step count".into(),
        shortcut: String::new(),
    });
    commands
}
pub struct Palette {
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) expansion: Option<super::expansion::Document>,
    pub menu: CommandPalette,
    ctx: egui::Context,
    input: Input,
    painter: Painter,
    modifiers: winit::keyboard::ModifiersState,
    sky_open: bool,
    sound_open: bool,
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) bug_report: super::bug_report::Dialog,
    commands: Vec<Command>,
}
impl Palette {
    /// Refresh map destinations without replacing dialogs, input or egui state.
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn update_landmarks(&mut self, landmarks: &[Landmark]) {
        self.commands = commands(landmarks);
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(super) fn headless(renderer: &Renderer) -> Self {
        use winit::raw_window_handle::{DisplayHandle, HandleError, HasDisplayHandle};
        struct NoDisplay;
        impl HasDisplayHandle for NoDisplay {
            fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
                Err(HandleError::Unavailable)
            }
        }
        let ctx = egui::Context::default();
        Self {
            expansion: None,
            input: Input::new(
                ctx.clone(),
                egui::ViewportId::ROOT,
                &NoDisplay,
                None,
                None,
                None,
            ),
            ctx,
            menu: CommandPalette::default(),
            painter: Painter::new(&renderer.device, renderer.config.format.add_srgb_suffix()),
            modifiers: Default::default(),
            sky_open: false,
            sound_open: false,
            bug_report: Default::default(),
            commands: commands(&[]),
        }
    }

    pub fn new(window: &Window, renderer: &Renderer, landmarks: &[Landmark]) -> Self {
        let ctx = egui::Context::default();
        let input = Input::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(renderer.device.limits().max_texture_dimension_2d as usize),
        );
        Self {
            #[cfg(not(target_arch = "wasm32"))]
            expansion: None,
            menu: CommandPalette::default(),
            ctx,
            input,
            modifiers: Default::default(),
            sky_open: false,
            sound_open: false,
            #[cfg(not(target_arch = "wasm32"))]
            bug_report: Default::default(),
            commands: commands(landmarks),
            painter: Painter::new(&renderer.device, renderer.config.format.add_srgb_suffix()),
        }
    }
    /// Feed all events so modifier state and focus stay current while closed.
    pub fn event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        if let WindowEvent::ModifiersChanged(modifiers) = event {
            self.modifiers = modifiers.state();
        }
        if matches!(event, WindowEvent::Focused(false)) {
            self.modifiers = Default::default();
        }
        let response = self.input.on_window_event(window, event);
        if response.repaint {
            window.request_redraw();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            response.consumed
        }
        #[cfg(target_arch = "wasm32")]
        {
            false
        }
    }
    pub fn dismiss(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.expansion = None;
        }
        self.menu.dismiss();
        self.sky_open = false;
        self.sound_open = false;
        #[cfg(not(target_arch = "wasm32"))]
        self.bug_report.dismiss();
    }
    pub fn is_open(&self) -> bool {
        let open = self.menu.is_open() || self.sky_open || self.sound_open;
        #[cfg(not(target_arch = "wasm32"))]
        let open = open || self.bug_report.is_open();
        #[cfg(not(target_arch = "wasm32"))]
        let open = open || self.expansion.as_ref().is_some_and(|d| d.picker.open);
        open
    }
    pub fn shortcut(&self) -> bool {
        let command = if cfg!(target_os = "macos") {
            self.modifiers.super_key()
        } else if cfg!(target_arch = "wasm32") {
            self.modifiers.control_key() || self.modifiers.super_key()
        } else {
            self.modifiers.control_key()
        };
        command && self.modifiers.shift_key()
    }
    pub(super) fn reserved_bottom(&self, width: u32, height: u32) -> u32 {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(document) = &self.expansion {
            let scale = self.ctx.pixels_per_point();
            let bounds = egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32) / scale,
            );
            let viewport = document.picker.viewport_rect(bounds);
            return ((bounds.height() - viewport.height()) * scale).round() as u32;
        }
        let _ = (width, height);
        0
    }
    pub(super) fn build_camera(&self) -> Option<(glam::Vec3, glam::Vec3, [f32; 2])> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(document) = &self.expansion {
            return document.camera();
        }
        None
    }
    pub fn draw(
        &mut self,
        window: &Window,
        renderer: &mut Renderer,
        target: &wgpu::TextureView,
        sound: &mut crate::audio::Settings,
        view_projection: glam::Mat4,
    ) -> Option<PaletteAction> {
        let mut selected = None;
        #[cfg(not(target_arch = "wasm32"))]
        let bug_link_enabled = !self.is_open();
        let output = self.ctx.run_ui(self.input.take_egui_input(window), |ui| {
            #[cfg(not(target_arch = "wasm32"))]
            egui::Area::new(egui::Id::new("bug_report_link"))
                .anchor(egui::Align2::LEFT_TOP, egui::vec2(12., 8.))
                .show(ui.ctx(), |ui| {
                    if ui
                        .add_enabled(bug_link_enabled, egui::Link::new("[bug]"))
                        .on_hover_text("Report a bug")
                        .clicked()
                    {
                        selected = Some(PaletteAction::BugReport);
                    }
                });
            if let Some(id) = self.menu.show(ui.ctx(), &self.commands) {
                if id == "sound.settings" {
                    self.sound_open = true;
                } else if id == "sky.settings" {
                    self.sky_open = true;
                    if renderer.daylight.is_some() {
                        renderer.sky_settings.solar_hour =
                            Some(renderer.sky_settings.hour(0., renderer.daylight));
                    }
                    renderer.daylight = None;
                } else {
                    selected = PaletteAction::from_id(&id);
                }
            }
            if self.sky_open && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.sky_open = false;
            }
            super::sky_controls::show(ui.ctx(), &mut self.sky_open, &mut renderer.sky_settings);
            if self.sound_open && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                self.sound_open = false;
            }
            sound.show(ui.ctx(), &mut self.sound_open);
            #[cfg(not(target_arch = "wasm32"))]
            self.bug_report.show(ui.ctx());
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(document) = &mut self.expansion {
                document.show(
                    ui,
                    view_projection,
                    [renderer.config.width, renderer.scene_height()],
                );
            }
            #[cfg(target_arch = "wasm32")]
            let _ = view_projection;
        });
        self.input
            .handle_platform_output(window, output.platform_output);
        let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        self.painter.paint(
            renderer,
            target,
            primitives,
            output.textures_delta,
            output.pixels_per_point,
        );
        // Follow egui's immediate animation requests instead of polling while open.
        // Once widgets settle, input/lifecycle events wake the paused scene.
        if self.is_open()
            && output
                .viewport_output
                .values()
                .any(|viewport| viewport.repaint_delay.is_zero())
        {
            window.request_redraw();
        }
        selected
    }
}
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}
struct Painter {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    textures: HashMap<egui::TextureId, (wgpu::Texture, wgpu::BindGroup)>,
}
impl Painter {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("palette texture"),
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
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palette"),
            source: wgpu::ShaderSource::Wgsl(include_str!("palette.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("palette"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("palette"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 32,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
            textures: HashMap::new(),
        }
    }
    fn paint(
        &mut self,
        renderer: &Renderer,
        target: &wgpu::TextureView,
        primitives: Vec<egui::ClippedPrimitive>,
        mut delta: egui::TexturesDelta,
        scale: f32,
    ) {
        let device = &renderer.device;
        let queue = &renderer.queue;
        for (id, updates) in delta.set.drain() {
            for update in updates {
                let egui::ImageData::Color(image) = update.image;
                let size = wgpu::Extent3d {
                    width: image.size[0] as u32,
                    height: image.size[1] as u32,
                    depth_or_array_layers: 1,
                };
                if update.pos.is_none() {
                    let texture = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("palette atlas"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("palette atlas"),
                        layout: &self.layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(
                                    &texture.create_view(&Default::default()),
                                ),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.sampler),
                            },
                        ],
                    });
                    self.textures.insert(id, (texture, group));
                }
                let (texture, _) = &self.textures[&id];
                let pos = update.pos.unwrap_or([0, 0]);
                let pixels: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: pos[0] as u32,
                            y: pos[1] as u32,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size.width * 4),
                        rows_per_image: None,
                    },
                    size,
                );
            }
        }
        if primitives.is_empty() {
            for id in delta.free.drain() {
                self.textures.remove(&id);
            }
            return;
        }
        let [width, height] = [renderer.config.width, renderer.config.height];
        let mut encoder = device.create_command_encoder(&Default::default());
        for primitive in primitives {
            let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else {
                continue;
            };
            if mesh.indices.is_empty() {
                continue;
            }
            let Some((_, group)) = self.textures.get(&mesh.texture_id) else {
                continue;
            };
            let vertices: Vec<Vertex> = mesh
                .vertices
                .iter()
                .map(|v| Vertex {
                    position: [
                        2. * v.pos.x * scale / width as f32 - 1.,
                        1. - 2. * v.pos.y * scale / height as f32,
                    ],
                    uv: [v.uv.x, v.uv.y],
                    color: v.color.to_array().map(|c| c as f32 / 255.),
                })
                .collect();
            let vertex = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("palette vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("palette indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let clip = primitive.clip_rect;
            let x = (clip.min.x * scale).floor().clamp(0., width as f32) as u32;
            let y = (clip.min.y * scale).floor().clamp(0., height as f32) as u32;
            let right = (clip.max.x * scale).ceil().clamp(x as f32, width as f32) as u32;
            let bottom = (clip.max.y * scale).ceil().clamp(y as f32, height as f32) as u32;
            if right == x || bottom == y {
                continue;
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("palette"),
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
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_scissor_rect(x, y, right - x, bottom - y);
            pass.set_bind_group(0, group, &[]);
            pass.set_vertex_buffer(0, vertex.slice(..));
            pass.set_index_buffer(index.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
        }
        queue.submit(Some(encoder.finish()));
        for id in delta.free.drain() {
            self.textures.remove(&id);
        }
    }
}

/// Exercise the palette's atlas uploads, clipping, and blending in the GPU smoke run.
#[cfg(not(target_arch = "wasm32"))]
pub fn smoke(renderer: &Renderer, target: &wgpu::TextureView) {
    let ctx = egui::Context::default();
    let mut menu = CommandPalette::default();
    menu.open();
    let mut painter = Painter::new(&renderer.device, renderer.config.format.add_srgb_suffix());
    for frame in 0..2 {
        let input = egui::RawInput {
            time: Some(frame as f64 * 0.5),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(renderer.config.width as f32, renderer.config.height as f32),
            )),
            ..Default::default()
        };
        let output = ctx.run_ui(input, |ui| {
            menu.show(ui.ctx(), &commands(&[]));
        });
        painter.paint(
            renderer,
            target,
            ctx.tessellate(output.shapes, output.pixels_per_point),
            output.textures_delta,
            output.pixels_per_point,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn step(
        ctx: &egui::Context,
        menu: &mut CommandPalette,
        key: Option<egui::Key>,
    ) -> Option<TimeCommand> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 800.),
            )),
            events: key
                .into_iter()
                .map(|key| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                })
                .collect(),
            ..Default::default()
        };
        let mut chosen = None;
        let mut output = ctx.run_ui(input, |ui| {
            if let Some(id) = menu.show(ui.ctx(), &commands(&[])) {
                chosen = TimeCommand::from_id(&id);
            }
        });
        output.textures_delta.clear();
        chosen
    }
    #[test]
    fn palette_selects_each_lighting_mode_and_dismisses() {
        for (down, expected) in [(0, Some(1.)), (1, Some(0.)), (2, None)] {
            let ctx = egui::Context::default();
            let mut menu = CommandPalette::default();
            menu.open();
            step(&ctx, &mut menu, None);
            for _ in 0..down {
                step(&ctx, &mut menu, Some(egui::Key::ArrowDown));
                step(&ctx, &mut menu, None);
            }
            assert_eq!(
                step(&ctx, &mut menu, Some(egui::Key::Enter))
                    .unwrap()
                    .daylight(),
                expected
            );
            assert!(!menu.is_open());
            menu.open();
            step(&ctx, &mut menu, Some(egui::Key::Escape));
            assert!(!menu.is_open());
        }
    }
    #[test]
    fn landmark_search_selects_respawn_and_dismisses() {
        let landmark = Landmark {
            id: "site_model_01".into(),
            title: "Orange sphere",
            point: [0., 0.],
            target_height: 0.,
            radius: 0.,
        };
        let commands = commands(&[landmark]);
        let ctx = egui::Context::default();
        let mut menu = CommandPalette::default();
        menu.open();
        let mut chosen = None;
        for events in [
            vec![],
            vec![egui::Event::Text("orange sphere".into())],
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(id) = menu.show(ui.ctx(), &commands) {
                        chosen = PaletteAction::from_id(&id);
                    }
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(chosen, Some(PaletteAction::Respawn("site_model_01".into())));
        assert!(!menu.is_open());
    }
    #[test]
    fn palette_shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("palette.wgsl")).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn jump_search_selects_map_picker_and_dismisses_palette() {
        let ctx = egui::Context::default();
        let mut menu = CommandPalette::default();
        menu.open();
        let mut chosen = None;
        for events in [
            vec![],
            vec![egui::Event::Text("jump to location".into())],
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(id) = menu.show(ui.ctx(), &commands(&[])) {
                        chosen = PaletteAction::from_id(&id);
                    }
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(chosen, Some(PaletteAction::JumpToLocation));
        assert!(!menu.is_open());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn bug_report_search_selects_capture_and_dismisses_palette() {
        let ctx = egui::Context::default();
        let mut menu = CommandPalette::default();
        menu.open();
        let mut chosen = None;
        for events in [
            vec![],
            vec![egui::Event::Text("report a bug".into())],
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(id) = menu.show(ui.ctx(), &commands(&[])) {
                        chosen = PaletteAction::from_id(&id);
                    }
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(chosen, Some(PaletteAction::BugReport));
        assert!(!menu.is_open());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "palette/expansion_tests.rs"]
mod expansion_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "palette/follow_tests.rs"]
mod follow_tests;
