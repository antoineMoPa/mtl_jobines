use super::frame_rate::FrameRate;
use super::palette::Palette;
use super::prop_scenery::PropScenery;
use super::render::{GpuMesh, Renderer};
#[cfg(not(target_arch = "wasm32"))]
use super::smoke::{self, SkyOverrides, SmokeOptions};
use super::vehicle_scenery::VehicleScenery;
use super::{
    hud::{self, MenuItem, Overlay},
    scenery::Scenery,
};
use crate::dynamic_world::{Controls, DynamicWorld, SpawnPoint, WalkControls, METERS_PER_UNIT};
use crate::hermes::Hermes;
use glam::Vec3;
use opencity::runtime::Mesh;
#[cfg(not(target_arch = "wasm32"))]
use opencity::runtime::Region;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
use std::{collections::HashSet, sync::Arc};
use web_time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};
pub(super) const DT: f32 = 1. / 120.;
const CURSOR_IDLE_DELAY: Duration = Duration::from_secs(2);
fn movement_controls(keys: &HashSet<KeyCode>, speed: f32) -> (Controls, WalkControls) {
    let held = |key| keys.contains(&key);
    let forward = held(KeyCode::KeyW) || held(KeyCode::ArrowUp);
    let backward = held(KeyCode::KeyS) || held(KeyCode::ArrowDown);
    let direction = forward as u8 as f32 - backward as u8 as f32;
    let brake = direction * speed < -0.5;
    let driving = Controls {
        throttle: if brake { 0. } else { direction },
        brake,
        steering: (held(KeyCode::KeyA) || held(KeyCode::ArrowLeft)) as u8 as f32
            - (held(KeyCode::KeyD) || held(KeyCode::ArrowRight)) as u8 as f32,
        handbrake: held(KeyCode::Space),
    };
    let walking = WalkControls {
        forward: direction,
        turn: driving.steering,
        run: held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight),
    };
    (driving, walking)
}
enum GameEvent {
    #[cfg(target_arch = "wasm32")]
    RendererReady(Box<Result<Renderer, String>>),
}
struct Game {
    audio: crate::audio::Audio,
    sound_settings: crate::audio::Settings,
    sound_eye: Option<Vec3>,
    hidden: bool,
    minimized: bool,
    #[cfg(target_arch = "wasm32")]
    proxy: winit::event_loop::EventLoopProxy<GameEvent>,
    hermes: Hermes,
    world: DynamicWorld,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    scenery: Option<Scenery>,
    prop_scenery: PropScenery,
    vehicle_scenery: Option<VehicleScenery>,
    dynamic_mesh: Option<GpuMesh>,
    dynamic_vertices: Mesh,
    palette: Option<Palette>,
    keys: HashSet<KeyCode>,
    last: Instant,
    game_time: f32,
    accumulator: f32,
    mesh_center: [f32; 2],
    eye: Vec3,
    zoom: f32,
    orbit: f32,
    dragging: bool,
    cursor: Option<[f32; 2]>,
    cursor_hide_at: Option<Instant>,
    cursor_hidden: bool,
    overlay: Overlay,
    menu_selection: MenuItem,
    hood: bool,
    first_frame: bool,
    first_frame_attempts: u32,
    surface_retry: Option<Instant>,
    scene_dirty: bool,
    route_revision: u64,
    frame_rate: FrameRate,
}
impl Game {
    fn show_cursor(&mut self) {
        self.cursor_hide_at = None;
        if self.cursor_hidden {
            self.cursor_hidden = false;
            if let Some(window) = &self.window {
                window.set_cursor_visible(true);
            }
        }
    }

    fn cursor_activity(&mut self, now: Instant) {
        self.show_cursor();
        self.cursor_hide_at = Some(now + CURSOR_IDLE_DELAY);
    }

    fn cursor_wait(&mut self, now: Instant) -> ControlFlow {
        if let Some(deadline) = self.cursor_hide_at {
            if now < deadline {
                return ControlFlow::WaitUntil(deadline);
            }
            self.cursor_hide_at = None;
            self.cursor_hidden = true;
            if let Some(window) = &self.window {
                window.set_cursor_visible(false);
            }
        }
        ControlFlow::Wait
    }

    fn retry_surface(&mut self, delay: Duration) {
        self.surface_retry = Some(Instant::now() + delay);
    }
    fn wake_surface(&mut self, now: Instant) -> bool {
        if self.surface_retry.is_some_and(|deadline| deadline <= now) {
            self.surface_retry = None;
            true
        } else {
            false
        }
    }
    fn wait_for_event(&mut self, now: Instant) -> ControlFlow {
        let cursor = self.cursor_wait(now);
        match (cursor, self.surface_retry) {
            (ControlFlow::WaitUntil(deadline), Some(retry)) => {
                ControlFlow::WaitUntil(deadline.min(retry))
            }
            (_, Some(retry)) => ControlFlow::WaitUntil(retry),
            _ => cursor,
        }
    }
    fn exit_game(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(not(target_arch = "wasm32"))]
        event_loop.exit();
        // Browsers cannot close a tab they did not open. Return to the title.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = event_loop;
            self.world.reset_player(&self.hermes);
            self.scene_dirty = true;
            self.set_overlay(Overlay::Start);
        }
    }
    fn set_overlay(&mut self, overlay: Overlay) {
        self.audio.silence();
        self.sound_eye = None;
        if self.cursor.is_some() {
            self.cursor_activity(Instant::now());
        }
        self.overlay = overlay;
        if overlay == Overlay::Driving {
            self.audio.user_gesture();
        }
        self.menu_selection = MenuItem::Resume;
        self.keys.clear();
        self.dragging = false;
        self.accumulator = 0.;
        self.last = Instant::now();
        self.frame_rate = FrameRate::default();
        if let Some(renderer) = &mut self.renderer {
            renderer.fps = None;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn menu_key(&mut self, key: KeyCode, state: ElementState, repeat: bool) -> Option<MenuItem> {
        if !matches!(self.overlay, Overlay::Start | Overlay::Menu)
            || state != ElementState::Pressed
            || repeat
        {
            return None;
        }
        match key {
            KeyCode::ArrowUp | KeyCode::ArrowDown => {
                self.menu_selection = self.menu_selection.other();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                None
            }
            KeyCode::Enter => Some(self.menu_selection),
            KeyCode::KeyQ => Some(MenuItem::Exit),
            _ => None,
        }
    }

    #[cfg(test)]
    fn new(
        hermes: Hermes,
        #[cfg(target_arch = "wasm32")] proxy: winit::event_loop::EventLoopProxy<GameEvent>,
    ) -> Self {
        Self::new_at(
            hermes,
            None,
            #[cfg(target_arch = "wasm32")]
            proxy,
        )
        .expect("default map spawn")
    }
    fn new_at(
        hermes: Hermes,
        start: Option<SpawnPoint>,
        #[cfg(target_arch = "wasm32")] proxy: winit::event_loop::EventLoopProxy<GameEvent>,
    ) -> Result<Self, String> {
        let world = DynamicWorld::with_start(&hermes, start)?;
        let p = world.player_position() / METERS_PER_UNIT;
        Ok(Self {
            audio: crate::audio::Audio::default(),
            sound_settings: crate::audio::Settings::default(),
            sound_eye: None,
            hidden: false,
            minimized: false,
            #[cfg(target_arch = "wasm32")]
            proxy,
            hermes,
            world,
            window: None,
            renderer: None,
            scenery: None,
            prop_scenery: PropScenery::default(),
            vehicle_scenery: None,
            dynamic_mesh: None,
            dynamic_vertices: Mesh::default(),
            palette: None,
            keys: HashSet::new(),
            last: Instant::now(),
            game_time: 0.,
            accumulator: 0.,
            mesh_center: [p.x, p.y],
            eye: p - Vec3::new(0.16, 0., -0.08),
            zoom: 1.,
            orbit: 0.,
            dragging: false,
            cursor: None,
            cursor_hide_at: None,
            cursor_hidden: false,
            overlay: if start.is_some() {
                Overlay::Driving
            } else {
                Overlay::Start
            },
            menu_selection: MenuItem::Resume,
            hood: false,
            first_frame: true,
            first_frame_attempts: 0,
            surface_retry: None,
            scene_dirty: true,
            route_revision: 0,
            frame_rate: FrameRate::default(),
        })
    }
    fn simulation_paused(&self) -> bool {
        self.hidden
            || self.minimized
            || self.overlay.paused()
            || self.palette.as_ref().is_some_and(|p| p.is_open())
    }
    fn horn_requested(&self) -> bool {
        !self.simulation_paused()
            && self.world.is_driving()
            && (self.keys.contains(&KeyCode::Enter) || self.keys.contains(&KeyCode::NumpadEnter))
    }
    fn refresh_scene(&self) -> bool {
        !self.simulation_paused() || self.scene_dirty
    }
    fn simulate(&mut self, elapsed: f32) {
        if self.simulation_paused() {
            self.accumulator = 0.;
            return;
        }
        self.accumulator += elapsed;
        self.game_time += elapsed;
        let f = glam::Vec2::new(self.world.player().yaw.cos(), self.world.player().yaw.sin());
        let speed = self.world.player().velocity.truncate().dot(f);
        let (input, walk) = movement_controls(&self.keys, speed);
        while self.accumulator >= DT {
            self.world.step(&self.hermes, input, walk, DT);
            self.accumulator -= DT;
        }
    }
    fn frame(&mut self, event_loop: &ActiveEventLoop) {
        if self.renderer.is_none() {
            return;
        }
        let now = Instant::now();
        if self.first_frame {
            self.first_frame_attempts += 1;
            if self.first_frame_attempts == 1 {
                eprintln!("Drawing first frame...");
            }
        }
        let elapsed = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.simulate(elapsed);
        let paused = self.simulation_paused();
        let refresh_scene = self.refresh_scene();

        let p = self.world.player_position() / METERS_PER_UNIT;
        let center = [p.x, p.y];
        if !paused {
            self.hermes.update_navigation(p.to_array());
        }
        let horn = self.horn_requested();
        let renderer = self.renderer.as_mut().unwrap();
        if self.route_revision != self.hermes.route_revision() {
            renderer.update_route(&self.hermes.route_uvs());
            self.route_revision = self.hermes.route_revision();
        }
        let heading = self.world.player_yaw() + self.orbit;
        let forward = Vec3::new(heading.cos(), heading.sin(), 0.);
        let look = p + Vec3::new(0., 0., 0.014) + forward * 0.045;
        let target_eye = if self.hood && self.world.is_driving() {
            p + forward * 0.009 + Vec3::Z * 0.011
        } else {
            p - forward * (0.15 * self.zoom) + Vec3::Z * (0.075 * self.zoom)
        };
        if !paused {
            self.eye = self.eye.lerp(target_eye, 1. - (-elapsed * 6.).exp());
        }
        if refresh_scene {
            let terrain = self
                .hermes
                .city()
                .surface_at([self.eye.x, self.eye.y], self.eye.z + 0.005)
                .map(|s| s.height + 0.015)
                .unwrap_or(-1.);
            self.eye.z = self.eye.z.max(terrain);
        }
        let sound_eye = self.eye * METERS_PER_UNIT;
        let sound_velocity = self.sound_eye.map_or(Vec3::ZERO, |previous| {
            if elapsed > 0.0001 {
                ((sound_eye - previous) / elapsed).clamp_length_max(100.)
            } else {
                Vec3::ZERO
            }
        });
        self.sound_eye = if paused { None } else { Some(sound_eye) };
        let mut sound_frame = self.world.sound_frame(
            sound_eye,
            look - self.eye,
            sound_velocity,
            self.sound_settings,
        );
        if let Some(car) = sound_frame.cars.iter_mut().find(|car| car.player) {
            car.horn = horn;
        }
        self.audio.update(sound_frame, !paused);
        let car = self.dynamic_mesh.as_mut().unwrap();
        if refresh_scene {
            self.world.write_moving_mesh(&mut self.dynamic_vertices);
            renderer.update_dynamic(car, &self.dynamic_vertices);
            self.prop_scenery.update(renderer, &self.world);
            self.vehicle_scenery
                .as_mut()
                .unwrap()
                .update(renderer, &self.world);
            if let Some(pixels) = self.hermes.update_minimap(center) {
                renderer.update_minimap(pixels);
            }
            self.scene_dirty = false;
        }
        let navigation = self.hermes.minimap_view(center, self.world.player_yaw());
        renderer.overlay = self.overlay;
        renderer.menu_selection = self.menu_selection;
        renderer.fps = self.frame_rate.value;
        let mut meshes = self.scenery.as_ref().unwrap().visible(center);
        meshes.push(car);
        meshes.extend(self.prop_scenery.visible());
        meshes.extend(self.vehicle_scenery.as_ref().unwrap().visible());
        let texture = renderer.surface.as_ref().unwrap().get_current_texture();
        let suboptimal = matches!(&texture, wgpu::CurrentSurfaceTexture::Suboptimal(_));
        match texture {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(renderer.config.format.add_srgb_suffix()),
                    ..Default::default()
                });
                renderer.render(&view, &meshes, self.eye, look, self.game_time, navigation);
                if let Some(palette) = &mut self.palette {
                    if let Some(command) = palette.draw(
                        self.window.as_ref().unwrap(),
                        renderer,
                        &view,
                        &mut self.sound_settings,
                    ) {
                        renderer.daylight = command.daylight();
                        renderer.sky_settings.solar_hour = None;
                        let window = self.window.as_ref().unwrap();
                        window.set_title(match command {
                            super::palette::TimeCommand::Day => "MTL Jobines · day",
                            super::palette::TimeCommand::Night => "MTL Jobines · night",
                            super::palette::TimeCommand::Automatic => {
                                "MTL Jobines · automatic daylight"
                            }
                        });
                        window.request_redraw();
                    }
                }
                renderer.queue.present(frame);
                self.surface_retry = None;
                if suboptimal {
                    // The texture is usable: present it before reconfiguring the surface.
                    renderer.resize(renderer.config.width, renderer.config.height);
                }
                if !paused {
                    self.frame_rate.presented(Instant::now());
                }
                if self.first_frame {
                    eprintln!("Game ready: first frame presented");
                    self.first_frame = false;
                }
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                renderer.resize(renderer.config.width, renderer.config.height);
                self.retry_surface(Duration::from_millis(100));
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                if let Err(error) = renderer.recreate_surface(self.window.as_ref().unwrap().clone())
                {
                    eprintln!("Cannot recreate game surface: {error}");
                    event_loop.exit();
                } else {
                    self.retry_surface(Duration::from_millis(100));
                }
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                if self.first_frame_attempts == 1 {
                    eprintln!("First surface acquisition timed out; retrying shortly");
                }
                self.retry_surface(Duration::from_millis(100));
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                if self.first_frame {
                    if self.first_frame_attempts == 1 {
                        eprintln!("Window surface not visible yet; waiting for first drawable");
                    }
                    self.retry_surface(Duration::from_millis(250));
                }
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                eprintln!("Cannot acquire game surface: GPU validation failed");
                event_loop.exit();
            }
        }
        if !self.simulation_paused() && self.surface_retry.is_none() {
            self.window.as_ref().unwrap().request_redraw();
        }
    }
}
impl ApplicationHandler<GameEvent> for Game {
    fn suspended(&mut self, _event_loop: &ActiveEventLoop) {
        self.audio.silence();
        self.sound_eye = None;
        if self.overlay != Overlay::Start {
            self.set_overlay(Overlay::Menu);
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.simulation_paused() {
            self.audio.silence();
        }
        let now = Instant::now();
        if self.wake_surface(now) {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        event_loop.set_control_flow(self.wait_for_event(now));
    }

    #[cfg(target_arch = "wasm32")]
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: GameEvent) {
        match event {
            GameEvent::RendererReady(result) => match *result {
                Ok(renderer) => {
                    self.finish_startup(renderer);
                    web_status(None);
                }
                Err(error) => {
                    web_status(Some(&format!("Could not start the game: {error}")));
                    event_loop.exit();
                }
            },
        }
    }
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("MTL Jobines")
            .with_inner_size(winit::dpi::LogicalSize::new(1440., 900.));
        #[cfg(target_arch = "wasm32")]
        let attributes = {
            use wasm_bindgen::JsCast;
            use winit::platform::web::WindowAttributesExtWebSys;
            let canvas = web_sys::window()
                .unwrap()
                .document()
                .unwrap()
                .get_element_by_id("game")
                .unwrap()
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .unwrap();
            let width = canvas.client_width().max(1) as f64;
            let height = canvas.client_height().max(1) as f64;
            attributes
                .with_canvas(Some(canvas))
                .with_prevent_default(true)
                .with_inner_size(winit::dpi::LogicalSize::new(width, height))
        };
        let window = Arc::new(event_loop.create_window(attributes).expect("window"));
        let size = window.inner_size();
        self.window = Some(window.clone());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let renderer =
                pollster::block_on(Renderer::new(Some(window.clone()), size.width, size.height))
                    .expect("wgpu renderer");
            self.finish_startup(renderer);
        }
        #[cfg(target_arch = "wasm32")]
        {
            let proxy = self.proxy.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = Renderer::new(Some(window), size.width, size.height).await;
                let _ = proxy.send_event(GameEvent::RendererReady(Box::new(result)));
            });
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let audio_gesture = match &event {
            WindowEvent::KeyboardInput { event, .. } => {
                event.state == ElementState::Pressed && !event.repeat
            }
            WindowEvent::MouseInput { state, .. } => *state == ElementState::Pressed,
            WindowEvent::Touch(touch) => touch.phase == winit::event::TouchPhase::Started,
            _ => false,
        };
        if !self.simulation_paused() && audio_gesture {
            self.audio.user_gesture();
        }
        if matches!(
            event,
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
        ) && self.cursor.is_some()
        {
            self.cursor_activity(Instant::now());
        }
        if let (Some(palette), Some(window)) = (&mut self.palette, &self.window) {
            palette.event(window, &event);
            let open = palette.is_open();
            let shortcut = matches!(&event, WindowEvent::KeyboardInput { event, .. }
                if event.physical_key == PhysicalKey::Code(KeyCode::KeyP)
                    && event.state == ElementState::Pressed && !event.repeat && palette.shortcut());
            if shortcut {
                self.audio.silence();
                self.sound_eye = None;
                if open {
                    palette.dismiss();
                } else {
                    palette.menu.open();
                }
                self.keys.clear();
                self.dragging = false;
                self.accumulator = 0.;
                self.show_cursor();
                self.window.as_ref().unwrap().request_redraw();
                return;
            }
            if open {
                // Keep platform lifecycle events flowing, but capture all game input.
                if matches!(
                    event,
                    WindowEvent::KeyboardInput { .. }
                        | WindowEvent::Ime(_)
                        | WindowEvent::MouseInput { .. }
                        | WindowEvent::MouseWheel { .. }
                        | WindowEvent::CursorMoved { .. }
                        | WindowEvent::Touch(_)
                ) {
                    self.show_cursor();
                    self.window.as_ref().unwrap().request_redraw();
                    return;
                }
            }
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.minimized = size.width == 0 || size.height == 0;
                if self.minimized {
                    self.audio.silence();
                    self.sound_eye = None;
                }
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                    self.window.as_ref().unwrap().request_redraw();
                }
            }
            WindowEvent::Focused(false) => {
                self.audio.silence();
                if let Some(palette) = &mut self.palette {
                    palette.dismiss();
                }
                if self.overlay != Overlay::Start {
                    self.set_overlay(Overlay::Menu);
                }
                self.show_cursor();
            }
            WindowEvent::Occluded(occluded) => {
                self.hidden = occluded;
                self.sound_eye = None;
                if occluded {
                    self.audio.silence();
                }
                self.surface_retry = None;
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::Focused(true) => {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
                if self.cursor.is_some() {
                    self.cursor_activity(Instant::now());
                }
            }
            WindowEvent::CursorEntered { .. } => self.cursor_activity(Instant::now()),
            WindowEvent::CursorLeft { .. } => {
                self.show_cursor();
                self.cursor = None;
                self.dragging = false;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if key == KeyCode::Escape {
                        if event.state == ElementState::Pressed && !event.repeat {
                            self.set_overlay(self.overlay.escape());
                        }
                        return;
                    }
                    if self.overlay.paused() {
                        match self.menu_key(key, event.state, event.repeat) {
                            Some(MenuItem::Exit) => self.exit_game(event_loop),
                            Some(MenuItem::Resume) => self.set_overlay(Overlay::Driving),
                            None => {}
                        }
                        return;
                    }
                    if event.state == ElementState::Pressed {
                        self.keys.insert(key);
                        if !event.repeat {
                            match key {
                                KeyCode::KeyR => {
                                    self.world.reset_player(&self.hermes);
                                    self.orbit = 0.;
                                }
                                KeyCode::KeyF => {
                                    if self.world.toggle_vehicle(&self.hermes) {
                                        self.orbit = 0.;
                                        self.hood = false;
                                    }
                                }
                                KeyCode::Tab => self.hood = !self.hood,
                                KeyCode::BracketLeft | KeyCode::BracketRight => {
                                    if let Some(renderer) = &mut self.renderer {
                                        let hour = renderer
                                            .sky_settings
                                            .hour(self.game_time, renderer.daylight);
                                        let current = renderer
                                            .daylight
                                            .unwrap_or((hour.min(24. - hour) / 12.).clamp(0., 1.));
                                        let delta = if key == KeyCode::BracketRight {
                                            0.1
                                        } else {
                                            -0.1
                                        };
                                        let daylight = (current + delta).clamp(0., 1.);
                                        renderer.daylight = Some(daylight);
                                        renderer.sky_settings.solar_hour = None;
                                        if let Some(window) = &self.window {
                                            window.set_title(&format!(
                                                "MTL Jobines · daylight {daylight:.1}"
                                            ));
                                        }
                                    }
                                }
                                KeyCode::KeyT => {
                                    if let Some(renderer) = &mut self.renderer {
                                        renderer.daylight = None;
                                        renderer.sky_settings.solar_hour = None;
                                    }
                                    if let Some(window) = &self.window {
                                        window.set_title("MTL Jobines · automatic daylight");
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else {
                        self.keys.remove(&key);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !self.overlay.paused() => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 50.,
                };
                self.zoom = (self.zoom * (-y * 0.1).exp()).clamp(0.45, 3.0);
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => {
                if self.overlay == Overlay::Map && state == ElementState::Pressed {
                    self.hermes.clear_destination();
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                } else {
                    self.dragging = state == ElementState::Pressed && !self.overlay.paused();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let (Some(point), Some(window)) = (self.cursor, &self.window) {
                    let size = window.inner_size();
                    let (w, h) = (size.width as f32, size.height as f32);
                    match self.overlay {
                        Overlay::Driving if hud::minimap_contains(w, h, point) => {
                            self.set_overlay(Overlay::Map)
                        }
                        Overlay::Start | Overlay::Menu
                            if hud::contains(hud::exit_rect(w, h), point) =>
                        {
                            self.exit_game(event_loop)
                        }
                        Overlay::Start | Overlay::Menu
                            if hud::contains(hud::resume_rect(w, h), point) =>
                        {
                            self.set_overlay(Overlay::Driving)
                        }
                        Overlay::Map
                            if hud::contains([w - 68., 20., 48., 48.], point)
                                || !hud::contains([20., 20., w - 40., h - 40.], point) =>
                        {
                            self.set_overlay(Overlay::Driving)
                        }
                        Overlay::Map => {
                            if let Some(uv) = hud::map_uv(w, h, point) {
                                let p = self.world.player_position() / METERS_PER_UNIT;
                                self.hermes.select_destination(uv, p.to_array());
                                window.request_redraw();
                            }
                        }
                        _ => {}
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_activity(Instant::now());
                if self.dragging {
                    if let Some(x) = self.cursor {
                        self.orbit -= (position.x as f32 - x[0]) * 0.008;
                    }
                }
                self.cursor = Some([position.x as f32, position.y as f32]);
                if matches!(self.overlay, Overlay::Start | Overlay::Menu) {
                    if let Some(window) = &self.window {
                        let size = window.inner_size();
                        let (w, h) = (size.width as f32, size.height as f32);
                        let point = self.cursor.unwrap();
                        let selection = if hud::contains(hud::resume_rect(w, h), point) {
                            Some(MenuItem::Resume)
                        } else if hud::contains(hud::exit_rect(w, h), point) {
                            Some(MenuItem::Exit)
                        } else {
                            None
                        };
                        if let Some(selection) = selection {
                            if selection != self.menu_selection {
                                self.menu_selection = selection;
                                window.request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            _ => {}
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    let start = args
        .iter()
        .position(|a| a == "--start")
        .map(|i| {
            args.get(i + 1)
                .ok_or_else(|| "--start requires x,y[,heading-degrees]".to_string())?
                .parse::<SpawnPoint>()
        })
        .transpose()?;
    let option = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from)
    };
    let path = option("--map").unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("opencity/examples/default_map.road")
    });
    let started = Instant::now();
    eprintln!("Loading map: {}", path.display());
    let mut hermes = Hermes::load_with_game_data(&path, option("--game-data").as_deref())?;
    let city = hermes.city();
    eprintln!(
        "Loaded {} roads, {} buildings, {} routes in {:?}",
        city.roads.len(),
        city.query(Region::WholeMap).buildings.len(),
        city.paths.len(),
        started.elapsed()
    );
    if let Some(output) = option("--build-game-data") {
        hermes.write_game_data(&output)?;
        eprintln!(
            "Game data ready: {} ({:.1} MiB)",
            output.display(),
            std::fs::metadata(&output).map_err(|e| e.to_string())?.len() as f64 / 1048576.
        );
        return Ok(());
    }
    if let Some(path) = option("--export-minimap") {
        std::fs::write(&path, hermes.minimap_svg()).map_err(|e| e.to_string())?;
        if option("--smoke").is_none() {
            return Ok(());
        }
    }
    if let Some(output) = option("--smoke") {
        let size = args
            .iter()
            .position(|a| a == "--smoke-size")
            .map(|i| {
                let value = args
                    .get(i + 1)
                    .ok_or("--smoke-size requires WIDTHxHEIGHT")?;
                let (width, height) = value
                    .split_once('x')
                    .ok_or("--smoke-size requires WIDTHxHEIGHT")?;
                let width = width.parse::<u32>().map_err(|_| "invalid smoke width")?;
                let height = height.parse::<u32>().map_err(|_| "invalid smoke height")?;
                if width == 0 || height == 0 || width > 8192 || height > 8192 {
                    return Err("smoke dimensions must be between 1 and 8192");
                }
                Ok((width, height))
            })
            .transpose()?
            .unwrap_or((1280, 800));
        return smoke::run(
            &mut hermes,
            output,
            SmokeOptions {
                overview: args.iter().any(|a| a == "--overview"),
                vehicle_lineup: args.iter().any(|a| a == "--vehicle-lineup"),
                hood: args.iter().any(|a| a == "--hood"),
                profile: args
                    .iter()
                    .any(|a| a == "--profile" || a == "--profile-120"),
                realtime: args.iter().any(|a| a == "--profile-realtime"),
                target_120: args.iter().any(|a| a == "--profile-120"),
                night: args.iter().any(|a| a == "--night"),
                start,
                size,
                sky: SkyOverrides::from_args(&args)?,
            },
        );
    }
    let event_loop = EventLoop::<GameEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    event_loop
        .run_app(&mut Game::new_at(hermes, start)?)
        .map_err(|e| e.to_string())
}

impl Game {
    fn finish_startup(&mut self, mut renderer: Renderer) {
        eprintln!("GPU: {}", renderer.adapter_name);
        let started = Instant::now();
        eprintln!("Preparing static scenery...");
        self.scenery = Some(Scenery::new(&renderer, &mut self.hermes));
        self.world.write_moving_mesh(&mut self.dynamic_vertices);
        self.dynamic_mesh = Some(renderer.upload_dynamic(&self.dynamic_vertices));
        self.prop_scenery.update(&renderer, &self.world);
        let mut vehicles = VehicleScenery::new(&renderer);
        vehicles.update(&renderer, &self.world);
        self.vehicle_scenery = Some(vehicles);
        eprintln!(
            "Scenery ready in {:?}; preparing map atlas...",
            started.elapsed()
        );
        if let Some(pixels) = self.hermes.update_minimap(self.mesh_center) {
            renderer.update_minimap(pixels);
        }
        eprintln!("Startup caches ready in {:?}", started.elapsed());
        // A resize may have arrived while browser GPU initialization was pending.
        let size = self.window.as_ref().unwrap().inner_size();
        let refresh = self
            .window
            .as_ref()
            .unwrap()
            .current_monitor()
            .and_then(|monitor| monitor.refresh_rate_millihertz());
        eprintln!(
            "Window: {}x{} physical pixels; display refresh {:?} Hz",
            size.width,
            size.height,
            refresh.map(|rate| rate as f32 / 1000.)
        );
        renderer.resize(size.width, size.height);
        eprintln!("Window configured; initializing command palette...");
        self.palette = Some(Palette::new(self.window.as_ref().unwrap(), &renderer));
        self.renderer = Some(renderer);
        self.last = Instant::now();
        self.game_time = 0.;
        // Ensure startup gets another opportunity even if the initial redraw is coalesced
        // before the native window becomes visible. Successful presentation cancels this.
        self.retry_surface(Duration::from_millis(100));
        eprintln!("Startup ready; requesting first frame");
        self.window.as_ref().unwrap().request_redraw();
    }
}

#[cfg(target_arch = "wasm32")]
fn web_status(error: Option<&str>) {
    if let Some(element) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("loading"))
    {
        element.set_text_content(error);
        element
            .set_attribute(
                "data-state",
                if error.is_some() { "error" } else { "ready" },
            )
            .ok();
    }
}

#[cfg(target_arch = "wasm32")]
pub fn start_web() -> Result<(), String> {
    use winit::platform::web::EventLoopExtWebSys;
    let hermes = Hermes::embedded()?;
    let search = web_sys::window()
        .ok_or("Browser window unavailable")?
        .location()
        .search()
        .map_err(|_| "Could not read launch position")?;
    let start = search
        .trim_start_matches('?')
        .split('&')
        .find_map(|part| part.strip_prefix("start="))
        .map(str::parse::<SpawnPoint>)
        .transpose()?;
    let event_loop = EventLoop::<GameEvent>::with_user_event()
        .build()
        .map_err(|e| e.to_string())?;
    let proxy = event_loop.create_proxy();
    event_loop.spawn_app(Game::new_at(hermes, start, proxy)?);
    Ok(())
}

#[cfg(test)]
mod tests;
