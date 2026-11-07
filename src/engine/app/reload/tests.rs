use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

const ROAD: &str = "road street: carlane carlane; end street(-2,0,2,0); player_start(0,0,0);";
struct TempMap(PathBuf);
impl TempMap {
    fn new(source: &str) -> Self {
        static SERIAL: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "map-reload-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let map = Self(directory.join("map.road"));
        std::fs::write(&map.0, source).unwrap();
        map
    }
}
impl Drop for TempMap {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap());
    }
}
fn wait_for_reload(watcher: &mut LiveReload, clock: &mut Instant) -> Result<Hermes, String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        *clock += INTERVAL;
        if let Some(result) = watcher.poll(*clock) {
            return result;
        }
        assert!(Instant::now() < deadline, "reload worker did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn write_elevation(path: &Path, value: u8) {
    let mut encoder = png::Encoder::new(std::fs::File::create(path).unwrap(), 1024, 1024);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&vec![value; 1024 * 1024])
        .unwrap();
}

#[test]
fn saves_reload_after_debounce_and_recover_from_invalid_or_missing_source() {
    let file = TempMap::new(ROAD);
    let mut watcher = LiveReload::new(file.0.clone());
    let mut clock = watcher.next_check;
    assert!(watcher.poll(clock).is_none());
    assert!(watcher.worker.is_none());
    std::fs::write(&file.0, format!("{ROAD} street(0,-2,0,2);")).unwrap();
    clock += INTERVAL;
    assert!(watcher.poll(clock).is_none());
    assert!(
        watcher.worker.is_none(),
        "save must settle before compilation"
    );
    assert!(watcher
        .poll(clock + INTERVAL - Duration::from_millis(1))
        .is_none());
    let hermes = wait_for_reload(&mut watcher, &mut clock).unwrap();
    assert_eq!(hermes.city().roads.len(), 2);

    // Atomic-save replacement followed by a partial/invalid edit.
    let replacement = file.0.with_extension("new");
    std::fs::write(&replacement, "road incomplete:").unwrap();
    std::fs::rename(replacement, &file.0).unwrap();
    assert!(wait_for_reload(&mut watcher, &mut clock).is_err());
    std::fs::remove_file(&file.0).unwrap();
    assert!(wait_for_reload(&mut watcher, &mut clock).is_err());
    std::fs::write(&file.0, ROAD).unwrap();
    assert_eq!(
        wait_for_reload(&mut watcher, &mut clock)
            .unwrap()
            .city()
            .roads
            .len(),
        1
    );
}

#[test]
fn elevation_only_saves_update_terrain_and_superseded_results_are_discarded() {
    let source = "elevation_map(\"terrain.png\",-2,-2,4,4,0,1); player_start(0,0,0);";
    let file = TempMap::new(source);
    let elevation = file.0.with_file_name("terrain.png");
    write_elevation(&elevation, 0);
    let mut watcher = LiveReload::new(file.0.clone());
    let mut clock = watcher.next_check;
    write_elevation(&elevation, 255);
    let hermes = wait_for_reload(&mut watcher, &mut clock).unwrap();
    assert_eq!(
        hermes
            .city()
            .elevation
            .as_ref()
            .unwrap()
            .height_at([0., 0.]),
        Some(1.)
    );

    // Inject a completed stale job, then save a newer source before it is consumed.
    let (sender, receiver) = mpsc::channel();
    sender.send((watcher.observed.clone(), Ok(hermes))).unwrap();
    watcher.worker = Some(receiver);
    std::fs::write(&file.0, ROAD).unwrap();
    clock += INTERVAL;
    assert!(
        watcher.poll(clock).is_none(),
        "stale terrain must never replace a newer save"
    );
    let latest = wait_for_reload(&mut watcher, &mut clock).unwrap();
    assert!(latest.city().elevation.is_none());
    assert_eq!(latest.city().roads.len(), 1);
}

#[test]
fn map_replacement_preserves_pause_and_player_then_falls_back_to_authored_spawn() {
    let mut game = Game::new(Hermes::test_map_from_source(ROAD));
    game.set_overlay(Overlay::Map);
    let original_position = game.world.player_position();
    let original_model = game.world.player().name().to_owned();
    game.hermes.select_destination([0.6, 0.5], [0., 0., 0.]);
    let valid = Hermes::test_map_from_source(&format!("{ROAD} street(0,-2,0,2);"));
    game.apply_reloaded_map(valid).unwrap();
    assert_eq!(game.hermes.city().roads.len(), 2);
    assert_eq!(game.overlay, Overlay::Map);
    assert_eq!(game.world.player_position(), original_position);
    assert_eq!(game.world.player().name(), original_model);
    assert!(game.hermes.route_uvs().is_empty());
    assert!(game.scene_dirty);
    assert_eq!(game.accumulator, 0.);

    let distant = Hermes::test_map_from_source(
        "road street: carlane carlane; end street(10,10,12,10); player_start(11,10,90);",
    );
    game.apply_reloaded_map(distant).unwrap();
    assert!((game.world.player_position().x - 1100.).abs() < 0.01);
    assert!((game.world.player_position().y - 1000.).abs() < 0.01);
    let before = game.world.player_position();
    let unsupported = Hermes::test_map_from_source("player_start(100,100,0);");
    assert!(game.apply_reloaded_map(unsupported).is_err());
    assert_eq!(game.world.player_position(), before);
    assert_eq!(game.hermes.city().roads.len(), 1);
    assert!(game
        .apply_reloaded_map(Hermes::test_map_from_source(""))
        .is_err());
    assert_eq!(game.world.player_position(), before);

    game.live_reload = Some(LiveReload::new(PathBuf::from("unused.road")));
    assert!(
        matches!(
            game.wait_for_event(Instant::now()),
            ControlFlow::WaitUntil(_)
        ),
        "a paused map must wake up to check saves"
    );
}

#[test]
fn canonical_island_has_water_parking_and_a_driveable_interior() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/map.road");
    let hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let city = hermes.city();
    assert_eq!(city.roads.len(), 1);
    let road = &city.roads[0];
    assert_eq!(road.definition.direction, opencity::RoadDirection::Forward);
    assert_eq!(
        road.definition
            .elements
            .iter()
            .filter(|element| element.kind == opencity::RoadElementKind::CarLane)
            .count(),
        2
    );
    assert_eq!(city.landscape.len(), 2);
    assert_eq!(city.paths.len(), 1);
    for path in &city.paths {
        assert!(path.points.last().unwrap()[0] < path.points[0][0]);
    }
    assert!(city.junctions.is_empty());
    assert_eq!(hermes.landmarks().len(), 1);
    assert_eq!(hermes.landmarks()[0].id, "site_model_01");
    let parking = city.surface_at([-1.2, 1.0], 0.15).unwrap();
    assert_eq!(parking.kind, opencity::runtime::SurfaceKind::Sidewalk);
    assert!((parking.height - 0.102).abs() < 0.0001);
    let service = city.surface_at([-0.55398536, 0.8062226], 0.15).unwrap();
    assert_eq!(service.kind, opencity::runtime::SurfaceKind::Asphalt);
    let terrain = city.elevation.as_ref().unwrap();
    assert_eq!(terrain.spec.water_level, Some(0.));
    for point in [[0., 0.], [40., 0.], [-40., 0.], [0., 40.], [0., -40.]] {
        assert!((terrain.height_at(point).unwrap() - 0.1).abs() < 0.0001);
    }
    for point in [[70., 0.], [-70., 0.], [0., 70.], [0., -70.], [50., 50.]] {
        assert!(terrain.height_at(point).unwrap() < 0.);
        assert_eq!(terrain.surface_height_at(point), Some(0.));
    }
    let mut world = DynamicWorld::new(&hermes);
    let initial = world.player_position();
    for _ in 0..600 {
        world.step(
            &hermes,
            Controls {
                throttle: 1.,
                ..Default::default()
            },
            WalkControls::default(),
            1. / 120.,
        );
    }
    assert!(world.player_position().distance(initial) > 2.);
    assert_eq!(world.player().collisions, 0);
    assert!(world.player().grounded);
    let without_spawn = std::fs::read_to_string(path)
        .unwrap()
        .replace("player_start(0, 0, 0);", "");
    let elevation =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/elevation.png"))
            .unwrap();
    let hermes = Hermes::from_map_bytes(without_spawn.as_bytes(), &elevation, None).unwrap();
    assert!(DynamicWorld::with_start(&hermes, None).is_ok());
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn map_reload_preserves_bug_report_capture_and_description() {
    let mut game = Game::new(Hermes::test_map_from_source(ROAD));
    let renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    let mut palette = Palette::headless(&renderer);
    palette.bug_report.begin(
        super::super::super::bug_report::Report::new(
            game.hermes.city(),
            [0.; 3],
            [1., 2., 3.],
            [4., 5., 6.],
        ),
        Ok(vec![1, 2, 3]),
    );
    palette
        .bug_report
        .type_description("A road issue.\nStill typing…");
    let captured = palette.bug_report.draft_snapshot();
    game.palette = Some(palette);
    game.renderer = Some(renderer);
    for source in [format!("{ROAD} street(0,-2,0,2);"), ROAD.into()] {
        game.apply_reloaded_map(Hermes::test_map_from_source(&source))
            .unwrap();
        let dialog = &game.palette.as_ref().unwrap().bug_report;
        assert!(dialog.is_open());
        assert_eq!(dialog.draft_snapshot(), captured);
    }
    let dialog = &mut game.palette.as_mut().unwrap().bug_report;
    dialog.type_description(" More detail.");
    assert_eq!(
        dialog.draft_snapshot().0["description"],
        "A road issue.\nStill typing… More detail."
    );
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn live_saves_replace_gpu_scenery_and_atlas_then_remove_them_again() {
    let source = "elevation_map(\"terrain.png\",-2,-2,4,4,0,1); player_start(0,0,0);";
    let file = TempMap::new(source);
    write_elevation(&file.0.with_file_name("terrain.png"), 0);
    let mut watcher = LiveReload::new(file.0.clone());
    let mut clock = watcher.next_check;
    let hermes = Hermes::load_with_game_data(&file.0, None).unwrap();
    let mut game = Game::new(hermes);
    game.renderer = Some(pollster::block_on(Renderer::new(None, 1280, 800)).unwrap());
    game.set_overlay(Overlay::Map);
    let output = std::env::temp_dir().join("fresh-map-reload-qa");
    std::fs::create_dir_all(&output).unwrap();

    // Build and apply exactly the snapshots delivered by the real file watcher.
    std::fs::write(&file.0, format!("{source}\n# first save\n")).unwrap();
    game.apply_reloaded_map(wait_for_reload(&mut watcher, &mut clock).unwrap())
        .unwrap();
    capture(&mut game, &output.join("flat.ppm"));
    let populated = format!(
        "{source}\nroad street: carlane carlane; end street(-1,0,1,0);\n\
        test_plot = building(0.15, 0.2,0.2,0.5,0.2,0.5,0.5,0.2,0.5);"
    );
    std::fs::write(&file.0, populated).unwrap();
    game.apply_reloaded_map(wait_for_reload(&mut watcher, &mut clock).unwrap())
        .unwrap();
    assert_eq!(game.hermes.city().roads.len(), 1);
    assert!(!game.hermes.city().landscape.is_empty());
    capture(&mut game, &output.join("populated.ppm"));
    assert_ne!(
        std::fs::read(output.join("flat.ppm")).unwrap(),
        std::fs::read(output.join("populated.ppm")).unwrap()
    );
    std::fs::write(&file.0, "road unfinished:").unwrap();
    assert!(wait_for_reload(&mut watcher, &mut clock).is_err());
    assert_eq!(game.hermes.city().roads.len(), 1);
    std::fs::write(&file.0, source).unwrap();
    game.apply_reloaded_map(wait_for_reload(&mut watcher, &mut clock).unwrap())
        .unwrap();
    assert!(game.hermes.city().roads.is_empty());
    assert!(game.hermes.city().landscape.is_empty());
    assert_eq!(game.overlay, Overlay::Map);
    capture(&mut game, &output.join("cleared.ppm"));
    eprintln!("Reload GPU evidence: {}", output.display());
}

fn capture(game: &mut Game, path: &Path) {
    let p = game.world.player_position() / METERS_PER_UNIT;
    let navigation = game
        .hermes
        .minimap_view([p.x, p.y], game.world.player_yaw());
    let renderer = game.renderer.as_mut().unwrap();
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("map reload QA"),
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
    let mut meshes = game.scenery.as_ref().unwrap().visible([p.x, p.y]);
    meshes.push(game.dynamic_mesh.as_ref().unwrap());
    meshes.extend(game.prop_scenery.visible());
    meshes.extend(game.vehicle_scenery.as_ref().unwrap().visible());
    renderer.render(
        &texture.create_view(&Default::default()),
        &meshes,
        p + Vec3::new(-0.8, -1., 1.),
        p,
        0.,
        navigation,
    );
    renderer.snapshot(&texture, path).unwrap();
}
