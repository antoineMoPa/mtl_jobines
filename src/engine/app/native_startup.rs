use super::*;
use crate::engine::boot::Boot;
use std::sync::mpsc::{self, Receiver};
use winit::window::WindowLevel;

struct NativeApp {
    boot: Option<Boot>,
    game: Option<Game>,
    result: Option<Receiver<Result<Hermes, String>>>,
    proxy: winit::event_loop::EventLoopProxy<GameEvent>,
    path: PathBuf,
    game_data: Option<PathBuf>,
    start: Option<SpawnPoint>,
    profile_shutdown: bool,
    startup_error: Option<String>,
    rule: Option<crate::VehicleReturnFn>,
}

pub(super) fn run(
    path: PathBuf,
    game_data: Option<PathBuf>,
    rule: Option<crate::VehicleReturnFn>,
    start: Option<SpawnPoint>,
    profile_shutdown: bool,
) -> Result<(), String> {
    let event_loop = EventLoop::<GameEvent>::with_user_event()
        .build()
        .map_err(|error| error.to_string())?;
    let mut app = NativeApp {
        boot: None,
        game: None,
        result: None,
        proxy: event_loop.create_proxy(),
        path,
        game_data,
        start,
        profile_shutdown,
        startup_error: None,
        rule,
    };
    let result = event_loop
        .run_app(&mut app)
        .map_err(|error| error.to_string());
    if let Some(error) = app.startup_error {
        return Err(error);
    }
    if let Some(mut game) = app.game {
        let started = Instant::now();
        if profile_shutdown {
            macro_rules! release {
                ($label:literal, $value:expr) => {{
                    let stage = Instant::now();
                    drop($value);
                    eprintln!(concat!("Shutdown ", $label, ": {:?}"), stage.elapsed());
                }};
            }
            release!("audio", std::mem::take(&mut game.audio));
            release!("window handle", game.window.take());
            release!("renderer", game.renderer.take());
            release!("scenery", game.scenery.take());
            release!("props", std::mem::take(&mut game.prop_scenery));
            release!("vehicles", game.vehicle_scenery.take());
            release!("dynamic mesh", game.dynamic_mesh.take());
            release!("palette", game.palette.take());
        }
        drop(game);
        if profile_shutdown {
            eprintln!("Shutdown game resources: {:?}", started.elapsed());
        }
    }
    result
}

impl ApplicationHandler<GameEvent> for NativeApp {
    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        // Captured camera input arrives as device motion, not cursor positions.
        if let Some(game) = &mut self.game {
            game.device_event(event_loop, id, event);
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(game) = &mut self.game {
            game.resumed(event_loop);
            return;
        }
        if self.boot.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("MTL Jobines")
            .with_inner_size(winit::dpi::LogicalSize::new(800., 482.))
            .with_window_level(WindowLevel::AlwaysOnTop);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.startup_error = Some(error.to_string());
                event_loop.exit();
                return;
            }
        };
        match pollster::block_on(Boot::new(window.clone())) {
            Ok(mut boot) => {
                boot.draw();
                window.request_redraw();
                self.boot = Some(boot);
            }
            Err(error) => {
                self.startup_error = Some(error);
                event_loop.exit();
                return;
            }
        }
        let path = self.path.clone();
        let game_data = self.game_data.clone();
        let proxy = self.proxy.clone();
        let (sender, receiver) = mpsc::channel();
        self.result = Some(receiver);
        std::thread::spawn(move || {
            let _span = mtl_world::telemetry::span("map.startup.worker");
            eprintln!("Loading map: {}", path.display());
            let started = Instant::now();
            let result = Hermes::load_with_game_data(&path, game_data.as_deref());
            if let Ok(hermes) = &result {
                let city = hermes.city();
                eprintln!(
                    "Loaded {} roads, {} buildings, {} routes in {:?}",
                    city.roads.len(),
                    city.query(Region::WholeMap).buildings.len(),
                    city.paths.len(),
                    started.elapsed()
                );
            }
            let _ = sender.send(result);
            let _ = proxy.send_event(GameEvent::MapLoaded);
        });
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: GameEvent) {
        match event {
            GameEvent::MapLoaded => {
                let _span = mtl_world::telemetry::span("map.startup.apply");
                let result = self.result.take().and_then(|receiver| receiver.recv().ok());
                match result {
                    Some(Ok(hermes)) => match Game::new_at(hermes, self.start) {
                        Ok(mut game) => {
                            if let Some(rule) = self.rule {
                                game.kairos.set_vehicle_return_rule(
                                    move |away, previous, elapsed| {
                                        rule(u32::from(away), previous, elapsed)
                                    },
                                );
                            }
                            game.live_reload = Some(reload::LiveReload::new(self.path.clone()));
                            game.profile_shutdown = self.profile_shutdown;
                            game.resumed(event_loop);
                            self.game = Some(game);
                        }
                        Err(error) => {
                            self.startup_error = Some(error);
                            event_loop.exit();
                        }
                    },
                    Some(Err(error)) => {
                        self.startup_error = Some(error);
                        event_loop.exit();
                    }
                    None => {
                        self.startup_error = Some("Map loader stopped unexpectedly".into());
                        event_loop.exit();
                    }
                }
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self
            .game
            .as_ref()
            .and_then(|game| game.window.as_ref())
            .map(|window| window.id())
            == Some(id)
        {
            let game = self.game.as_mut().unwrap();
            game.window_event(event_loop, id, event);
            if !game.first_frame {
                if let Some(boot) = self.boot.take() {
                    boot.window.set_visible(false);
                    game.window.as_ref().unwrap().focus_window();
                }
            }
            return;
        }
        if let Some(boot) = &mut self.boot {
            if boot.window.id() == id {
                match event {
                    WindowEvent::RedrawRequested => boot.draw(),
                    WindowEvent::Occluded(false) => boot.window.request_redraw(),
                    WindowEvent::Resized(_) => boot.resize(),
                    WindowEvent::CloseRequested => event_loop.exit(),
                    _ => {}
                }
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(game) = &mut self.game {
            game.about_to_wait(event_loop);
        } else {
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(game) = &mut self.game {
            game.suspended(event_loop);
        }
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(game) = &mut self.game {
            game.exiting(event_loop);
        }
        if let Some(boot) = &self.boot {
            boot.window.set_visible(false);
        }
    }
}
