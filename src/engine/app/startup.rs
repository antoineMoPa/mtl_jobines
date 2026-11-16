use super::*;

impl Game {
    pub(super) fn finish_startup(&mut self, mut renderer: Renderer) {
        eprintln!("GPU: {}", renderer.adapter_name);
        renderer.configure_graffiti(super::super::graffiti::Settings::default());
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
        self.palette = Some(Palette::new(
            self.window.as_ref().unwrap(),
            &renderer,
            &self.hermes.landmarks(),
        ));
        self.renderer = Some(renderer);
        #[cfg(not(target_arch = "wasm32"))]
        self.audio.prepare();
        self.last = Instant::now();
        self.game_time = 0.;
        // Ensure startup gets another opportunity even if the initial redraw is coalesced
        // before the native window becomes visible. Successful presentation cancels this.
        self.retry_surface(Duration::from_millis(100));
        eprintln!("Startup ready; requesting first frame");
        self.window.as_ref().unwrap().request_redraw();
    }
}
