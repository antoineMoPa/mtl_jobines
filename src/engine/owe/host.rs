use super::{
    world::{World, CLOSED, DT, INVALID},
    Resume, ACTIVE,
};
use crate::engine::render::{GpuMesh, Renderer};
use glam::Vec3;
use std::{cell::RefCell, rc::Rc, sync::Arc};
use web_time::Instant;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};

struct Ready(Result<Renderer, String>);
struct Host {
    world: Rc<RefCell<World>>,
    resume: Resume,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    terrain: Option<(Vec3, GpuMesh)>,
    actors: Option<GpuMesh>,
    held: u32,
    pressed: u32,
    released: u32,
    last: Instant,
    accumulator: f32,
    title: String,
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
        actors: None,
        held: 0,
        pressed: 0,
        released: 0,
        last: Instant::now(),
        accumulator: 0.,
        title: String::new(),
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
        if self.title != w.message {
            self.title = w.message.clone();
            if let Some(win) = &self.window {
                win.set_title(&format!("{} | {}", w.package.title, self.title));
            }
            #[cfg(target_arch = "wasm32")]
            set_status(&self.title);
        }
        if !w.present || w.city.is_none() {
            return Ok(true);
        }
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(true);
        };
        let target = w.target();
        if self
            .terrain
            .as_ref()
            .is_none_or(|(center, _)| center.distance(target) > 1.)
        {
            self.terrain = Some((target, renderer.upload(&w.terrain()?)));
        }
        let mesh = w.actors();
        if let Some(gpu) = self.actors.as_mut() {
            renderer.update_dynamic(gpu, &mesh)
        } else {
            self.actors = Some(renderer.upload_dynamic(&mesh))
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
                let meshes = [
                    &self.terrain.as_ref().unwrap().1,
                    self.actors.as_ref().unwrap(),
                ];
                renderer.render_scene(
                    &view,
                    &meshes,
                    target + Vec3::new(0., -0.30, 0.36),
                    target,
                    w.time,
                    None,
                );
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
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("status"))
    {
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
        let future =
            Renderer::with_catalog(Some(window), size.width.max(1), size.height.max(1), false);
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
            Ok(mut r) => {
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
            WindowEvent::Focused(false) => {
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
