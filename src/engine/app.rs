use super::render::Renderer;
use super::{
    hud::{self, MenuItem, Overlay},
    scenery::Scenery,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::dynamic_world::Vehicle;
use crate::dynamic_world::{Controls, DynamicWorld, SpawnPoint, WalkControls, METERS_PER_UNIT};
use crate::hermes::Hermes;
use glam::Vec3;
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
const DT: f32 = 1. / 120.;
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
    #[cfg(target_arch = "wasm32")]
    proxy: winit::event_loop::EventLoopProxy<GameEvent>,
    hermes: Hermes,
    world: DynamicWorld,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    scenery: Option<Scenery>,
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
    route_revision: u64,
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

    fn exit_game(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(not(target_arch = "wasm32"))]
        event_loop.exit();
        // Browsers cannot close a tab they did not open. Return to the title.
        #[cfg(target_arch = "wasm32")]
        {
            let _ = event_loop;
            self.world.reset_player(&self.hermes);
            self.set_overlay(Overlay::Start);
        }
    }
    fn set_overlay(&mut self, overlay: Overlay) {
        if self.cursor.is_some() {
            self.cursor_activity(Instant::now());
        }
        self.overlay = overlay;
        self.menu_selection = MenuItem::Resume;
        self.keys.clear();
        self.dragging = false;
        self.accumulator = 0.;
        self.last = Instant::now();
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
            #[cfg(target_arch = "wasm32")]
            proxy,
            hermes,
            world,
            window: None,
            renderer: None,
            scenery: None,
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
            route_revision: 0,
        })
    }
    fn simulate(&mut self, elapsed: f32) {
        if self.overlay.paused() {
            self.accumulator = 0.;
        } else {
            self.accumulator += elapsed;
            self.game_time += elapsed;
        }
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
        let elapsed = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.simulate(elapsed);

        let p = self.world.player_position() / METERS_PER_UNIT;
        let center = [p.x, p.y];
        self.hermes.update_navigation(p.to_array());
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
        if !self.overlay.paused() {
            self.eye = self.eye.lerp(target_eye, 1. - (-elapsed * 6.).exp());
        }
        let terrain = self
            .hermes
            .city()
            .surface_at([self.eye.x, self.eye.y], self.eye.z + 0.005)
            .map(|s| s.height + 0.015)
            .unwrap_or(-1.);
        self.eye.z = self.eye.z.max(terrain);
        let car = renderer.upload(&self.world.mesh());
        if let Some(pixels) = self.hermes.update_minimap(center) {
            renderer.update_minimap(pixels);
        }
        let navigation = self.hermes.minimap_view(center, self.world.player_yaw());
        renderer.overlay = self.overlay;
        renderer.menu_selection = self.menu_selection;
        let mut meshes = self.scenery.as_ref().unwrap().visible(center);
        meshes.push(&car);
        match renderer.surface.as_ref().unwrap().get_current_texture() {
            Ok(frame) => {
                let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
                    format: Some(renderer.config.format.add_srgb_suffix()),
                    ..Default::default()
                });
                renderer.render(&view, &meshes, self.eye, look, self.game_time, navigation);
                frame.present();
                if self.first_frame {
                    eprintln!("Game ready: first frame presented");
                    self.first_frame = false;
                }
            }
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                renderer.resize(renderer.config.width, renderer.config.height)
            }
            Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
            Err(_) => {}
        }
        if !self.overlay.paused() {
            self.window.as_ref().unwrap().request_redraw();
        }
    }
}
impl ApplicationHandler<GameEvent> for Game {
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(self.cursor_wait(Instant::now()));
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
        if matches!(
            event,
            WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. }
        ) && self.cursor.is_some()
        {
            self.cursor_activity(Instant::now());
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height);
                    self.window.as_ref().unwrap().request_redraw();
                }
            }
            WindowEvent::Focused(false) => {
                if self.overlay != Overlay::Start {
                    self.set_overlay(Overlay::Menu);
                }
                self.show_cursor();
            }
            WindowEvent::Focused(true) => {
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
fn smoke(
    hermes: &mut Hermes,
    output: PathBuf,
    overview: bool,
    profile: bool,
) -> Result<(), String> {
    let city = hermes.city();
    let mut car = Vehicle::spawn(city);
    let origin = car.position;
    for _ in 0..600 {
        car.step(
            city,
            Controls {
                throttle: 1.,
                ..Default::default()
            },
            DT,
        );
    }
    if !car.position.is_finite() || car.position.distance(origin) < 5. {
        return Err(format!(
            "vehicle smoke did not travel: {:?}",
            car.position - origin
        ));
    }
    eprintln!(
        "Driving smoke: moved {:.1} m, {:.1} km/h, {:?}, collisions {}",
        car.position.distance(origin),
        car.velocity.length() * 3.6,
        car.surface,
        car.collisions
    );
    let car = Vehicle::spawn(city);
    let p = car.position / METERS_PER_UNIT;
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800))?;
    let started = Instant::now();
    let scenery = Scenery::new(&renderer, hermes);
    eprintln!(
        "{}: static scenery cached in {:?}",
        renderer.adapter_name,
        started.elapsed()
    );
    let carmesh = renderer.upload(&car.mesh());
    if let Some(pixels) = hermes.update_minimap([p.x, p.y]) {
        renderer.update_minimap(pixels);
    }
    // Exercise the route pipeline in the actual driving and expanded-map GPU snapshots.
    let target_uv = hermes
        .minimap_view([p.x + 4.2, p.y - 4.2], car.yaw)
        .player_uv;
    hermes.select_destination(target_uv, p.to_array());
    renderer.update_route(&hermes.route_uvs());
    let navigation = hermes.minimap_view([p.x, p.y], car.yaw);
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("smoke frame"),
        size: wgpu::Extent3d {
            width: 1280,
            height: 800,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let f = Vec3::new(car.yaw.cos(), car.yaw.sin(), 0.);
    let eye = if overview {
        p + Vec3::new(-1.4, -1.7, 1.5)
    } else {
        p - f * 0.15 + Vec3::Z * 0.075
    };
    let look = if overview {
        p
    } else {
        p + f * 0.04 + Vec3::Z * 0.014
    };
    let mut meshes = scenery.visible([p.x, p.y]);
    meshes.push(&carmesh);
    renderer.render(
        &texture.create_view(&Default::default()),
        &meshes,
        eye,
        look,
        0.,
        navigation,
    );
    renderer.snapshot(&texture, &output)?;
    eprintln!("Rendered {}", output.display());
    for (overlay, suffix) in [
        (Overlay::Start, "start"),
        (Overlay::Menu, "menu"),
        (Overlay::Map, "map"),
    ] {
        renderer.overlay = overlay;
        renderer.render(
            &texture.create_view(&Default::default()),
            &meshes,
            eye,
            look,
            0.,
            navigation,
        );
        let path = output.with_file_name(format!(
            "{}-{suffix}.ppm",
            output.file_stem().unwrap().to_string_lossy()
        ));
        renderer.snapshot(&texture, &path)?;
        eprintln!("Rendered {}", path.display());
    }

    let mut walking_world = DynamicWorld::new(hermes);
    if walking_world.toggle_vehicle(hermes) {
        let walking_mesh = renderer.upload(&walking_world.mesh());
        let mut walking_meshes = scenery.visible([p.x, p.y]);
        walking_meshes.push(&walking_mesh);
        renderer.overlay = Overlay::Driving;
        renderer.render(
            &texture.create_view(&Default::default()),
            &walking_meshes,
            eye,
            look,
            0.,
            navigation,
        );
        let path = output.with_file_name(format!(
            "{}-walking.ppm",
            output.file_stem().unwrap().to_string_lossy()
        ));
        renderer.snapshot(&texture, &path)?;
        eprintln!("Rendered {}", path.display());
    }
    renderer.overlay = Overlay::Driving;
    if profile {
        let mut car = Vehicle::spawn(hermes.city());
        let origin = car.position;
        let mut timings = Vec::new();
        for _ in 0..240 {
            for _ in 0..8 {
                car.step(
                    hermes.city(),
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    DT,
                );
            }
            let p = car.position / METERS_PER_UNIT;
            let center = [p.x, p.y];
            let started = Instant::now();
            assert!(
                hermes.update_minimap(center).is_none(),
                "atlas must never rebuild while driving"
            );
            let carmesh = renderer.upload(&car.mesh());
            let mut meshes = scenery.visible(center);
            meshes.push(&carmesh);
            let f = Vec3::new(car.yaw.cos(), car.yaw.sin(), 0.);
            renderer.render(
                &texture.create_view(&Default::default()),
                &meshes,
                p - f * 0.15 + Vec3::Z * 0.075,
                p + f * 0.04 + Vec3::Z * 0.014,
                0.,
                hermes.minimap_view(center, car.yaw),
            );
            renderer.device.poll(wgpu::Maintain::Wait);
            timings.push(started.elapsed().as_secs_f64() * 1000.);
        }
        timings.sort_by(f64::total_cmp);
        eprintln!("Cached driving: 240 synchronized GPU frames, {:.1} m traveled; median {:.2} ms, p95 {:.2} ms, max {:.2} ms; no static map rebuilds",
            car.position.distance(origin), timings[120], timings[228], timings[239]);
    }
    Ok(())
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
        return smoke(
            &mut hermes,
            output,
            args.iter().any(|a| a == "--overview"),
            args.iter().any(|a| a == "--profile"),
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
        renderer.resize(size.width, size.height);
        self.renderer = Some(renderer);
        self.last = Instant::now();
        self.game_time = 0.;
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
mod tests {
    use super::*;
    #[test]
    fn direction_keys_brake_opposing_motion_then_accelerate_either_way() {
        for (key, direction) in [
            (KeyCode::ArrowUp, 1.),
            (KeyCode::KeyW, 1.),
            (KeyCode::ArrowDown, -1.),
            (KeyCode::KeyS, -1.),
        ] {
            let keys = HashSet::from([key]);
            let (opposing, _) = movement_controls(&keys, -10. * direction);
            assert!(opposing.brake);
            assert_eq!(opposing.throttle, 0.);
            for speed in [-0.1, 0., 0.1, 10. * direction] {
                let (driving, _) = movement_controls(&keys, speed);
                assert!(!driving.brake);
                assert_eq!(driving.throttle, direction);
            }
        }
        let (driving, _) =
            movement_controls(&HashSet::from([KeyCode::ArrowUp, KeyCode::ArrowDown]), 10.);
        assert_eq!(driving.throttle, 0.);
        assert!(!driving.brake);
    }

    #[test]
    fn space_handbrake_and_shift_sprint_are_independent() {
        let (driving, walking) = movement_controls(&HashSet::from([KeyCode::Space]), 10.);
        assert!(driving.handbrake);
        assert!(!driving.brake);
        assert!(!walking.run);
        for key in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
            let (driving, walking) = movement_controls(&HashSet::from([key]), 10.);
            assert!(!driving.handbrake);
            assert!(!driving.brake);
            assert!(walking.run);
        }
    }

    #[test]
    fn cursor_idle_deadline_resets_and_wakes_even_while_paused() {
        let mut game = Game::new(Hermes::embedded().unwrap());
        let now = Instant::now();
        assert_eq!(game.cursor_wait(now), ControlFlow::Wait);
        assert!(!game.cursor_hidden);

        for overlay in [
            Overlay::Start,
            Overlay::Driving,
            Overlay::Menu,
            Overlay::Map,
        ] {
            game.set_overlay(overlay);
            game.cursor_activity(now);
            let moved = now + Duration::from_secs(1);
            game.cursor_activity(moved);
            let deadline = moved + CURSOR_IDLE_DELAY;
            assert_eq!(
                game.cursor_wait(now + CURSOR_IDLE_DELAY),
                ControlFlow::WaitUntil(deadline)
            );
            assert!(!game.cursor_hidden);
            assert_eq!(game.cursor_wait(deadline), ControlFlow::Wait);
            assert!(game.cursor_hidden);
            assert_eq!(game.cursor_hide_at, None);

            game.cursor_activity(deadline);
            assert!(!game.cursor_hidden);
            assert_eq!(
                game.cursor_wait(deadline),
                ControlFlow::WaitUntil(deadline + CURSOR_IDLE_DELAY)
            );
            // Leaving the window or losing focus cancels hiding.
            game.show_cursor();
            assert_eq!(
                game.cursor_wait(deadline + CURSOR_IDLE_DELAY),
                ControlFlow::Wait
            );
            assert!(!game.cursor_hidden);
        }
    }

    #[test]
    fn menu_arrows_select_and_enter_activates_without_leaking_controls() {
        let mut game = Game::new(Hermes::embedded().unwrap());
        for overlay in [Overlay::Start, Overlay::Menu] {
            game.set_overlay(overlay);
            assert_eq!(game.menu_selection, MenuItem::Resume);
            assert_eq!(
                game.menu_key(KeyCode::Enter, ElementState::Pressed, false),
                Some(MenuItem::Resume)
            );
            for key in [
                KeyCode::ArrowDown,
                KeyCode::ArrowDown,
                KeyCode::ArrowUp,
                KeyCode::ArrowUp,
            ] {
                let expected = game.menu_selection.other();
                assert_eq!(game.menu_key(key, ElementState::Pressed, false), None);
                assert_eq!(game.menu_selection, expected);
                assert_eq!(
                    game.menu_key(KeyCode::Enter, ElementState::Pressed, false),
                    Some(expected)
                );
                game.menu_key(key, ElementState::Pressed, true);
                game.menu_key(key, ElementState::Released, false);
                assert_eq!(game.menu_selection, expected);
            }
            assert_eq!(
                game.menu_key(KeyCode::Enter, ElementState::Pressed, true),
                None
            );
            assert_eq!(
                game.menu_key(KeyCode::Enter, ElementState::Released, false),
                None
            );
            assert_eq!(
                game.menu_key(KeyCode::KeyQ, ElementState::Pressed, false),
                Some(MenuItem::Exit)
            );
            assert!(game.keys.is_empty());
            game.menu_key(KeyCode::ArrowDown, ElementState::Pressed, false);
            game.set_overlay(Overlay::Driving);
            assert_eq!(game.menu_selection, MenuItem::Resume);
        }
        for overlay in [Overlay::Driving, Overlay::Map] {
            game.set_overlay(overlay);
            for key in [
                KeyCode::ArrowDown,
                KeyCode::ArrowUp,
                KeyCode::Enter,
                KeyCode::KeyQ,
            ] {
                assert_eq!(game.menu_key(key, ElementState::Pressed, false), None);
                assert_eq!(game.menu_selection, MenuItem::Resume);
            }
        }
    }

    #[test]
    fn explicit_launch_position_starts_playing_immediately() {
        let hermes = Hermes::embedded().unwrap();
        let default = Game::new(Hermes::embedded().unwrap());
        let point = default.world.player_position() / METERS_PER_UNIT;
        let game = Game::new_at(
            hermes,
            Some(SpawnPoint {
                point: [point.x, point.y],
                heading: None,
            }),
        )
        .unwrap();
        assert_eq!(game.overlay, Overlay::Driving);
        assert!((game.world.player_position().x / METERS_PER_UNIT - point.x).abs() < 1e-5);
        assert!((game.world.player_position().y / METERS_PER_UNIT - point.y).abs() < 1e-5);
    }
    #[test]
    fn walking_left_right_keys_turn_without_strafing() {
        let mut game = Game::new(Hermes::embedded().unwrap());
        game.set_overlay(Overlay::Driving);
        assert!(game.world.toggle_vehicle(&game.hermes));
        game.orbit = 0.7;
        for (key, direction) in [
            (KeyCode::KeyA, 1.),
            (KeyCode::ArrowLeft, 1.),
            (KeyCode::KeyD, -1.),
            (KeyCode::ArrowRight, -1.),
        ] {
            game.keys.clear();
            game.keys.insert(key);
            let position = game.world.player_position();
            let heading = game.world.player_yaw();
            game.simulate(DT * 6.);
            assert_eq!(game.world.player_position(), position);
            assert!((game.world.player_yaw() - heading) * direction > 0.1);
            assert_eq!(game.orbit, 0.7);
        }
        game.keys.insert(KeyCode::ArrowLeft);
        let heading = game.world.player_yaw();
        game.simulate(DT * 6.);
        assert_eq!(game.world.player_yaw(), heading);
    }
    #[test]
    fn first_menu_freezes_until_start_then_later_pauses_resume() {
        let mut game = Game::new(Hermes::embedded().unwrap());
        assert_eq!(game.overlay, Overlay::Start);
        let position = game.world.player_position();
        game.keys.insert(KeyCode::KeyW);
        for _ in 0..10 {
            game.simulate(0.1);
        }
        assert_eq!(game.world.player_position(), position);
        assert_eq!(game.game_time, 0.);
        game.set_overlay(game.overlay.escape());
        assert_eq!(game.overlay, Overlay::Driving);
        assert!(game.keys.is_empty());
        game.simulate(DT);
        assert!(game.game_time > 0.);
        game.set_overlay(game.overlay.escape());
        assert_eq!(game.overlay, Overlay::Menu);
        game.set_overlay(game.overlay.escape());
        assert_eq!(game.overlay, Overlay::Driving);
    }
    #[test]
    fn menu_and_map_freeze_the_world_and_resume_without_accumulated_time_or_keys() {
        let mut game = Game::new(Hermes::embedded().unwrap());
        game.set_overlay(Overlay::Driving);
        game.keys.insert(KeyCode::KeyW);
        game.simulate(0.1);
        for overlay in [Overlay::Menu, Overlay::Map] {
            let position = game.world.player().position;
            let velocity = game.world.player().velocity;
            let time = game.game_time;
            game.set_overlay(overlay);
            assert!(game.keys.is_empty());
            for _ in 0..100 {
                game.simulate(0.1);
            }
            assert_eq!(game.world.player().position, position);
            assert_eq!(game.world.player().velocity, velocity);
            assert_eq!(game.game_time, time);
            game.set_overlay(overlay.escape());
            assert_eq!(game.overlay, Overlay::Driving);
            assert_eq!(game.accumulator, 0.);
            game.simulate(DT);
            assert!(game.game_time > time);
        }
    }
}
