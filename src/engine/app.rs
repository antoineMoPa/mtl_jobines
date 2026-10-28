use super::render::Renderer;
use super::{
    hud::{self, Overlay},
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
use web_time::Instant;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{Window, WindowId},
};
const DT: f32 = 1. / 120.;
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
    overlay: Overlay,
    hood: bool,
    first_frame: bool,
}
impl Game {
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
        self.overlay = overlay;
        self.keys.clear();
        self.dragging = false;
        self.accumulator = 0.;
        self.last = Instant::now();
        if let Some(window) = &self.window {
            window.request_redraw();
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
            overlay: if start.is_some() {
                Overlay::Driving
            } else {
                Overlay::Start
            },
            hood: false,
            first_frame: true,
        })
    }
    fn simulate(&mut self, elapsed: f32) {
        if self.overlay.paused() {
            self.accumulator = 0.;
        } else {
            self.accumulator += elapsed;
            self.game_time += elapsed;
        }
        let held = |k| self.keys.contains(&k);
        let forward = held(KeyCode::KeyW) || held(KeyCode::ArrowUp);
        let backward = held(KeyCode::KeyS) || held(KeyCode::ArrowDown);
        let f = glam::Vec2::new(self.world.player().yaw.cos(), self.world.player().yaw.sin());
        let speed = self.world.player().velocity.truncate().dot(f);
        let input = Controls {
            throttle: if forward {
                1.
            } else if backward && speed < 0.5 {
                -1.
            } else {
                0.
            },
            brake: backward && speed >= 0.5 || held(KeyCode::Space),
            steering: (held(KeyCode::KeyA) || held(KeyCode::ArrowLeft)) as u8 as f32
                - (held(KeyCode::KeyD) || held(KeyCode::ArrowRight)) as u8 as f32,
            handbrake: held(KeyCode::ShiftLeft) || held(KeyCode::ShiftRight),
        };
        let walk = WalkControls {
            forward: forward as u8 as f32 - backward as u8 as f32,
            turn: input.steering,
            run: input.handbrake,
        };
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
        let renderer = self.renderer.as_mut().unwrap();
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
                        if matches!(self.overlay, Overlay::Start | Overlay::Menu)
                            && key == KeyCode::KeyQ
                            && event.state == ElementState::Pressed
                            && !event.repeat
                        {
                            self.exit_game(event_loop);
                            return;
                        }
                        if matches!(self.overlay, Overlay::Start | Overlay::Menu)
                            && key == KeyCode::Enter
                            && event.state == ElementState::Pressed
                            && !event.repeat
                        {
                            self.set_overlay(Overlay::Driving);
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
            } => self.dragging = state == ElementState::Pressed && !self.overlay.paused(),
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
                        _ => {}
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if self.dragging {
                    if let Some(x) = self.cursor {
                        self.orbit -= (position.x as f32 - x[0]) * 0.008;
                    }
                }
                self.cursor = Some([position.x as f32, position.y as f32]);
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
