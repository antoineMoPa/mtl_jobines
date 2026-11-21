use super::{
    world::{MessageLocation, World, CLOSED, DT, INVALID},
    Resume, ACTIVE,
};
use crate::engine::muzzle::Equipment;
use crate::engine::render::{GpuMesh, Renderer};
use crate::props::Kind;
use glam::Vec3;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};
use web_time::Instant;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

struct Ready(Result<(Renderer, super::skydome::Skydome), String>);
struct FloraBatch {
    parts: Vec<GpuMesh>,
    instances: Vec<[[f32; 4]; 4]>,
}
struct Host {
    world: Rc<RefCell<World>>,
    resume: Resume,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    message_ui: Option<(egui::Context, crate::engine::palette::Painter)>,
    terrain: Option<(Vec3, GpuMesh)>,
    actors: Option<GpuMesh>,
    flora: HashMap<Kind, FloraBatch>,
    models: HashMap<String, Equipment>,
    captured: bool,
    audio: crate::audio::Audio,
    #[cfg(not(target_arch = "wasm32"))]
    capture: Option<std::ffi::OsString>,
    held: u32,
    pressed: u32,
    released: u32,
    last: Instant,
    accumulator: f32,
    status: (String, MessageLocation),
    error: Option<String>,
    #[cfg(target_arch = "wasm32")]
    proxy: winit::event_loop::EventLoopProxy<Ready>,
}

pub(super) fn run(world: World, resume: Resume) -> Result<(), String> {
    let event_loop = EventLoop::<Ready>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let host = Host {
        world: Rc::new(RefCell::new(world)),
        resume,
        window: None,
        renderer: None,
        terrain: None,
        message_ui: None,
        actors: None,
        flora: HashMap::new(),
        models: Default::default(),
        captured: false,
        audio: Default::default(),
        #[cfg(not(target_arch = "wasm32"))]
        capture: std::env::var_os("OWE_CAPTURE_PNG"),
        held: 0,
        pressed: 0,
        released: 0,
        last: Instant::now(),
        accumulator: 0.,
        status: (String::new(), MessageLocation::Status),
        error: None,
        #[cfg(target_arch = "wasm32")]
        proxy: event_loop.create_proxy(),
    };
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut host = host;
        event_loop.run_app(&mut host).map_err(|e| e.to_string())?;
        host.error.map_or(Ok(()), Err)
    }
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(host);
        Ok(())
    }
}

impl Host {
    fn stop(&mut self, ev: &ActiveEventLoop, error: String) {
        eprintln!("OWE: {error}");
        #[cfg(target_arch = "wasm32")]
        set_status(&error);
        self.error = Some(error);
        ev.exit();
    }
    fn tick(&mut self, closed: bool) -> Result<bool, String> {
        let now = Instant::now();
        self.accumulator += (now - self.last).as_secs_f32().min(0.12);
        self.last = now;
        let steps = (self.accumulator / DT) as u32;
        self.accumulator -= steps as f32 * DT;
        {
            let mut w = self.world.borrow_mut();
            w.steps_left = steps;
            w.present = false;
            if let Some(map) = w.pending_map.take() {
                w.load_status = if closed {
                    CLOSED
                } else {
                    match w.load(&map) {
                        Ok(()) => {
                            self.terrain = None;
                            0
                        }
                        Err(e) => {
                            w.message = e;
                            INVALID
                        }
                    }
                };
            }
        }
        ACTIVE.with_borrow_mut(|v| *v = Some(self.world.clone()));
        let result = (self.resume)(self.held, self.pressed, self.released, steps, closed);
        ACTIVE.with_borrow_mut(|v| *v = None);
        self.pressed = 0;
        self.released = 0;
        if result < 0 {
            return Err(format!(
                "client ended with error {}: {}",
                -result,
                self.world.borrow().message
            ));
        }
        if closed || result > 0 {
            return Ok(false);
        }
        let w = self.world.borrow();
        if self.status.0 != w.message || self.status.1 != w.message_location {
            if let Some(win) = &self.window {
                if w.message_location == MessageLocation::Status {
                    win.set_title(&format!("{} | {}", w.package.title, w.message));
                } else if self.status.1 == MessageLocation::Status {
                    win.set_title(&w.package.title);
                }
            }
            self.status.0.clone_from(&w.message);
            self.status.1 = w.message_location;
            #[cfg(target_arch = "wasm32")]
            show_status(&w.message, w.message_location != MessageLocation::Status);
        }
        if let Some(frame) = w.impact_audio() {
            self.audio.update(frame, true);
        } else {
            self.audio.silence();
        }
        if !w.present || w.city.is_none() {
            return Ok(true);
        }
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(true);
        };
        renderer.retro_lens = w.retro_lens;
        w.skydome.apply(renderer);
        let target = w.target();
        if self
            .terrain
            .as_ref()
            .is_none_or(|(center, _)| center.distance(target) > 1.)
        {
            self.terrain = Some((target, renderer.upload(&w.terrain()?)));
        }
        let mut shot_sprites = w.impact_sprites();
        renderer.local_lights.flash.set(None);
        let mesh = w.actors();
        if let Some(gpu) = self.actors.as_mut() {
            renderer.update_dynamic(gpu, &mesh)
        } else {
            self.actors = Some(renderer.upload_dynamic(&mesh))
        }
        let mut meshes = vec![
            &self.terrain.as_ref().unwrap().1,
            self.actors.as_ref().unwrap(),
        ];
        for batch in self.flora.values_mut() {
            batch.instances.clear();
        }
        for (kind, transform) in w.flora_instances() {
            let batch = self.flora.entry(kind).or_insert_with(|| FloraBatch {
                parts: kind
                    .far_parts()
                    .iter()
                    .map(|p| renderer.upload_instanced(&p.vertices))
                    .collect(),
                instances: Vec::new(),
            });
            batch.instances.push(transform.to_cols_array_2d());
        }
        for batch in self.flora.values_mut().filter(|b| !b.instances.is_empty()) {
            for part in &mut batch.parts {
                renderer.update_instances(part, &batch.instances);
                meshes.push(part);
            }
        }
        let (eye, look) = if let Some((rotation, height)) = w.first_person {
            let eye = target + Vec3::Z * (height / super::world::SCALE);
            if let Some((name, offset)) = &w.view_model {
                let model = self
                    .models
                    .entry(name.clone())
                    .or_insert_with(|| Equipment::upload(renderer, &w.view_models[name]));
                let matrix = glam::Mat4::from_rotation_translation(
                    rotation,
                    eye + rotation * (*offset / super::world::SCALE),
                );
                model.place(
                    renderer,
                    matrix,
                    rotation,
                    &w.effects.muzzle,
                    &mut shot_sprites,
                );
                meshes.push(&model.gpu);
            }
            (eye, eye + rotation * Vec3::X)
        } else {
            (target + Vec3::new(0., -0.30, 0.36), target)
        };
        renderer.update_impacts(&shot_sprites);
        let mut draw = |view: &wgpu::TextureView| {
            renderer.render_scene(view, &meshes, eye, look, w.time, None);
            if w.message_location == MessageLocation::Status || w.message.is_empty() {
                return;
            }
            let (ctx, painter) = self.message_ui.get_or_insert_with(|| {
                let ctx = egui::Context::default();
                crate::engine::job_feed::install_font(&ctx);
                let painter = crate::engine::palette::Painter::new(
                    &renderer.device,
                    renderer.config.format.add_srgb_suffix(),
                );
                (ctx, painter)
            });
            let size = egui::vec2(renderer.config.width as f32, renderer.scene_height() as f32);
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ui| {
                    let (position, anchor) = if w.message_location == MessageLocation::Center {
                        ((size * 0.5).to_pos2(), egui::Align2::CENTER_CENTER)
                    } else {
                        (egui::pos2(8., 8.), egui::Align2::LEFT_TOP)
                    };
                    crate::engine::tooltip::positioned(ui.ctx(), position, anchor, &w.message);
                },
            );
            painter.paint(
                renderer,
                view,
                ctx.tessellate(output.shapes, output.pixels_per_point),
                output.textures_delta,
                output.pixels_per_point,
            );
        };
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = self.capture.take() {
            std::fs::write(path, renderer.capture_png(draw)?).map_err(|e| e.to_string())?;
            return Ok(false);
        }
        let surface = renderer.surface.as_ref().unwrap().get_current_texture();
        let suboptimal = matches!(&surface, wgpu::CurrentSurfaceTexture::Suboptimal(_));
        match surface {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(renderer.config.format.add_srgb_suffix()),
                    ..Default::default()
                });
                draw(&view);
                renderer.queue.present(frame);
                if suboptimal {
                    renderer.resize(renderer.config.width, renderer.config.height);
                }
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                let size = self.window.as_ref().unwrap().inner_size();
                renderer.resize(size.width, size.height)
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                renderer.recreate_surface(self.window.as_ref().unwrap().clone())?
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("GPU surface validation failed".into())
            }
            wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Timeout => {}
        }
        Ok(true)
    }
}

#[cfg(target_arch = "wasm32")]
fn set_status(text: &str) {
    show_status(text, false);
}

#[cfg(target_arch = "wasm32")]
fn show_status(text: &str, in_hud: bool) {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("status"))
    {
        el.set_class_name(if in_hud { "in-hud" } else { "" });
        el.set_text_content(Some(text));
    }
}

impl ApplicationHandler<Ready> for Host {
    fn resumed(&mut self, ev: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title(&self.world.borrow().package.title)
            .with_inner_size(winit::dpi::LogicalSize::new(960., 640.));
        #[cfg(target_arch = "wasm32")]
        let attrs = {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;
            let Some(canvas) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("game"))
                .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            else {
                return self.stop(ev, "missing game canvas".into());
            };
            let size = winit::dpi::LogicalSize::new(
                canvas.client_width().max(1),
                canvas.client_height().max(1),
            );
            attrs
                .with_inner_size(size)
                .with_canvas(Some(canvas))
                .with_prevent_default(true)
        };
        let window = match ev.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => return self.stop(ev, e.to_string()),
        };
        self.window = Some(window.clone());
        let size = window.inner_size();
        let assets = std::mem::take(&mut self.world.borrow_mut().package.skydome);
        let future = async move {
            let mut renderer =
                Renderer::with_catalog(Some(window), size.width.max(1), size.height.max(1), false)
                    .await?;
            let skydome = assets.prepare(&mut renderer).await?;
            Ok((renderer, skydome))
        };
        #[cfg(not(target_arch = "wasm32"))]
        self.user_event(ev, Ready(pollster::block_on(future)));
        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let _ = proxy.send_event(Ready(future.await));
            });
        }
    }
    fn user_event(&mut self, ev: &ActiveEventLoop, Ready(result): Ready) {
        match result {
            Ok((mut r, skydome)) => {
                self.world.borrow_mut().skydome = skydome;
                // The browser may resize its canvas while the GPU device is being created.
                let window = self.window.as_ref().unwrap();
                let size = window.inner_size();
                r.resize(size.width, size.height);
                self.renderer = Some(r);
                self.last = Instant::now();
                window.request_redraw();
            }
            Err(e) => self.stop(ev, e),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn about_to_wait(&mut self, ev: &ActiveEventLoop) {
        if self.renderer.is_none() || ev.exiting() {
            return;
        }
        let period = std::time::Duration::from_secs_f32(DT);
        let now = Instant::now();
        let next = self.last + period;
        let wake = if now >= next {
            self.window.as_ref().unwrap().request_redraw();
            now + period
        } else {
            next
        };
        ev.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(wake));
    }
    fn device_event(
        &mut self,
        _: &ActiveEventLoop,
        _: winit::event::DeviceId,
        event: winit::event::DeviceEvent,
    ) {
        if self.captured {
            if let winit::event::DeviceEvent::MouseMotion { delta } = event {
                let mut w = self.world.borrow_mut();
                w.look_delta[0] += delta.0 as f32;
                w.look_delta[1] += delta.1 as f32;
            }
        }
    }
    fn window_event(&mut self, ev: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if ev.exiting() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                if self.renderer.is_some() {
                    let _ = self.tick(true);
                }
                ev.exit();
            }
            WindowEvent::MouseInput { state, button, .. }
                if self.world.borrow().first_person.is_some() =>
            {
                let bit = match button {
                    winit::event::MouseButton::Left => 256,
                    winit::event::MouseButton::Right => 512,
                    _ => 0,
                };
                if state == ElementState::Pressed {
                    if self.world.borrow().has_impact_materials() {
                        self.audio.user_gesture();
                    }
                    if !self.captured && bit != 0 {
                        let window = self.window.as_ref().unwrap();
                        self.captured = window
                            .set_cursor_grab(winit::window::CursorGrabMode::Locked)
                            .or_else(|_| {
                                window.set_cursor_grab(winit::window::CursorGrabMode::Confined)
                            })
                            .is_ok();
                        window.set_cursor_visible(!self.captured);
                    }
                    self.pressed |= bit & !self.held;
                    self.held |= bit;
                } else {
                    self.released |= bit & self.held;
                    self.held &= !bit;
                }
            }
            WindowEvent::Focused(false) => {
                if self.captured {
                    let window = self.window.as_ref().unwrap();
                    let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                    window.set_cursor_visible(true);
                    self.captured = false;
                }
                self.world.borrow_mut().look_delta = [0.; 2];
                self.released |= self.held;
                self.held = 0;
                self.pressed = 0;
            }
            WindowEvent::KeyboardInput { event, .. } if !event.repeat => {
                let bit = match event.physical_key {
                    PhysicalKey::Code(code) => match code {
                        KeyCode::ArrowLeft => 1,
                        KeyCode::ArrowRight => 2,
                        KeyCode::ArrowDown => 4,
                        KeyCode::ArrowUp => 8,
                        KeyCode::KeyE => 16,
                        KeyCode::KeyP => 32,
                        KeyCode::Escape => 64,
                        KeyCode::Tab => 128,
                        KeyCode::KeyR => 1024,
                        KeyCode::Digit1 => 2048,
                        KeyCode::Digit2 => 4096,
                        KeyCode::KeyW => 8192,
                        KeyCode::KeyA => 16384,
                        KeyCode::KeyS => 32768,
                        KeyCode::KeyD => 65536,
                        _ => 0,
                    },
                    _ => 0,
                };
                match event.state {
                    ElementState::Pressed => {
                        self.pressed |= bit & !self.held;
                        self.held |= bit
                    }
                    ElementState::Released => {
                        self.released |= bit & self.held;
                        self.held &= !bit
                    }
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested if self.renderer.is_some() => match self.tick(false) {
                Ok(true) => {
                    #[cfg(target_arch = "wasm32")]
                    self.window.as_ref().unwrap().request_redraw();
                }
                Ok(false) => {
                    #[cfg(target_arch = "wasm32")]
                    set_status("Game closed.");
                    ev.exit()
                }
                Err(e) => self.stop(ev, e),
            },
            _ => {}
        }
    }
}
