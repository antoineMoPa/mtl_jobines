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
    let program = opencity::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let city = hermes.city();
    let road = city
        .roads
        .iter()
        .find(|r| r.segment.name.as_deref() == Some("service_north_segment"))
        .unwrap();
    assert_eq!(road.definition.direction, opencity::RoadDirection::Forward);
    assert_eq!(
        road.definition
            .elements
            .iter()
            .filter(|element| element.kind == opencity::RoadElementKind::CarLane)
            .count(),
        2
    );
    for path in city.paths.iter().filter(|p| p.component == "service_north") {
        assert!(path.points.last().unwrap()[0] > path.points[0][0]);
    }
    assert_eq!(hermes.landmarks().len(), 1);
    assert_eq!(hermes.landmarks()[0].id, "site_model_01");
    let k = 0.0296475_f32;
    let c = 1. / k.hypot(1.);
    let point = |u: f32, v: f32| [c * u - k * c * v, k * c * u + c * v];
    // Both lots reach the crossing's sidewalk; the service road keeps its
    // carriageway while adding a walkable outside edge.
    for (u, v) in [(1.59, -1.3), (1.78, -1.3), (3., -0.74)] {
        let surface = city.surface_at(point(u, v), 0.15).unwrap();
        assert_eq!(surface.kind, opencity::runtime::SurfaceKind::Sidewalk);
    }
    let station = city
        .landscape
        .iter()
        .find(|f| f.name.as_deref() == Some("fuel_station"))
        .unwrap();
    assert!(!station.building_style.as_ref().unwrap().collidable);
    assert!((station.height.unwrap() - 0.108).abs() < 1e-5);
    // The forecourt is open; the separate shop shell remains solid.
    assert!(!city.building_collision(point(2.155, -1.18), 0.009, 0.11, 0.14));
    assert!(city.building_collision(point(2.155, -1.285), 0.009, 0.11, 0.14));
    let parking = city.surface_at([1.2, -1.0], 0.15).unwrap();
    assert_eq!(parking.kind, opencity::runtime::SurfaceKind::Sidewalk);
    assert!((parking.height - 0.102).abs() < 0.0001);
    let service = city.surface_at(point(0.53, -0.655), 0.15).unwrap();
    assert_eq!(service.kind, opencity::runtime::SurfaceKind::Asphalt);
    let terrain = city.elevation.as_ref().unwrap();
    assert_eq!(terrain.spec.water_level, Some(0.));
    for name in ["service_north_segment", "service_south_segment"] {
        let service = city
            .roads
            .iter()
            .find(|road| road.segment.name.as_deref() == Some(name))
            .unwrap();
        assert_eq!(
            service
                .definition
                .elements
                .iter()
                .map(|e| e.kind)
                .collect::<Vec<_>>(),
            [
                opencity::RoadElementKind::CarLane,
                opencity::RoadElementKind::CarLane,
                opencity::RoadElementKind::Sidewalk,
            ]
        );
    }
    // The trench reaches both carriageway edges, with no upper grass ledge.
    for (inside, outside) in [(-0.6320, -0.6330), (-0.0680, -0.0670)] {
        assert!(terrain.height_at(point(2.2, inside)).unwrap().abs() < 1e-4);
        assert!((terrain.height_at(point(2.2, outside)).unwrap() - 0.1).abs() < 1e-4);
        let lower = city.surface_at(point(2.2, inside), 0.01).unwrap();
        assert_eq!(lower.kind, opencity::runtime::SurfaceKind::Asphalt);
        assert!((lower.height - 0.002).abs() < 1e-4);
        assert_eq!(
            city.surface_at(point(2.2, outside), 0.15).unwrap().kind,
            opencity::runtime::SurfaceKind::Asphalt
        );
    }
    let crossing = [1.7006906, -0.51140493];
    assert!((city.surface_at(crossing, 0.01).unwrap().height - 0.002).abs() < 0.0001);
    assert!((city.surface_at(crossing, 0.15).unwrap().height - 0.102).abs() < 0.0001);
    for road in city
        .roads
        .iter()
        .filter(|r| r.segment.road_name == "access_ramp" || r.segment.road_name == "access_mouth")
    {
        for step in 1..40 {
            let t = step as f32 / 40.;
            let center = road.segment.point_at(t);
            let tangent = road.segment.tangent_at(t);
            let (sin, cos) = tangent.y.atan2(tangent.x).sin_cos();
            let z = road.segment.height_at(t);
            // A car must fit across the widened pavement, including the inward
            // shoulder and service-road mouth, without touching a wall.
            for offset in [-0.0275, 0., 0.0275] {
                let p = [center.x - sin * offset, center.y + cos * offset];
                let support = city.surface_at(p, z + 0.005).unwrap();
                assert_eq!(support.kind, opencity::runtime::SurfaceKind::Asphalt);
                assert!(
                    (support.height - z).abs() < 0.004,
                    "{:?} at {t}, offset {offset}: {support:?}",
                    road.segment.name
                );
                assert!(terrain.height_at(p).unwrap() <= z + 0.0001);
                assert!(
                    city.road_barrier_contact(p, 0.009, z + 0.001, z + 0.02)
                        .is_none(),
                    "{:?} at {t}, offset {offset}",
                    road.segment.name
                );
            }
        }
    }
    // Widening the ramps inward must leave all six highway lanes unobstructed.
    for road in city.roads.iter().filter(|road| {
        matches!(
            road.segment.name.as_deref(),
            Some("highway_north" | "highway_south")
        )
    }) {
        for (lane, element) in road.definition.elements.iter().enumerate() {
            if element.kind != opencity::RoadElementKind::CarLane {
                continue;
            }
            for step in 1..40 {
                let t = step as f32 / 40.;
                let p = opencity::map::element_position(&road.segment, &road.definition, lane, t)
                    .unwrap();
                assert!(
                    city.road_barrier_contact([p.x, p.y], 0.009, 0.003, 0.022)
                        .is_none(),
                    "{:?}, lane {lane} at {t}",
                    road.segment.name
                );
            }
        }
    }
    for point in [[10., 0.], [40., 0.], [-40., 0.], [0., 40.], [0., -40.]] {
        assert!((terrain.height_at(point).unwrap() - 0.1).abs() < 0.0001);
    }
    for point in [[70., 0.], [-70., 0.], [0., 70.], [0., -70.], [50., 50.]] {
        assert!(terrain.height_at(point).unwrap() < 0.);
        assert_eq!(terrain.surface_height_at(point), Some(0.));
    }
    let spawn = hermes.default_start().unwrap();
    assert!(
        (spawn.point[0] - 1.1318498).abs() < 1e-5 && (spawn.point[1] + 0.94828856).abs() < 1e-5
    );
    let mut car = crate::dynamic_world::Vehicle::spawn_at(
        city,
        SpawnPoint {
            point: point(0.53, -0.655),
            heading: Some(0.02964),
        },
    )
    .unwrap();
    let initial = car.position;
    for _ in 0..600 {
        car.step(
            city,
            Controls {
                throttle: 1.,
                ..Default::default()
            },
            1. / 120.,
        );
    }
    assert!(car.position.distance(initial) > 2.);
    assert_eq!(car.collisions, 0);
    assert!(car.grounded);
    // Drive across the former highway dead end and onto its uphill continuation.
    let mut car = crate::dynamic_world::Vehicle::spawn_at(
        city,
        SpawnPoint {
            point: point(-5.2, -0.24),
            heading: Some(k.atan() + std::f32::consts::PI),
        },
    )
    .unwrap();
    let origin = car.position;
    for _ in 0..600 {
        car.step(
            city,
            Controls {
                throttle: 1.,
                ..Default::default()
            },
            1. / 120.,
        );
    }
    assert!(car.position.distance(origin) > 30.);
    assert!(car.position.z > origin.z + 1.);
    assert_eq!(car.collisions, 0);
    assert!(car.grounded);
    let extension = city
        .roads
        .iter()
        .find(|road| road.segment.name.as_deref() == Some("cross_island_extension"))
        .unwrap();
    let end = extension.segment.point_at(1.);
    assert!((59.8..60.).contains(&end.x.hypot(end.y)));
    for v in [0.9, 20., 40., 59.8] {
        let surface = city.surface_at(point(1.7197884, v), 0.15).unwrap();
        assert_eq!(surface.kind, opencity::runtime::SurfaceKind::Asphalt);
        assert!((surface.height - 0.102).abs() < 1e-4);
    }
    let mut without_spawn = opencity::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    without_spawn
        .statements
        .retain(|s| !matches!(s,opencity::Statement::Call(c) if c.name=="player_start"));
    let without_spawn = without_spawn.to_road_source();
    let elevation =
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/elevation.png"))
            .unwrap();
    let hermes = Hermes::from_map_bytes(
        without_spawn.as_bytes(),
        &elevation,
        None,
        Some(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("data/maps/mtl")
                .as_path(),
        ),
    )
    .unwrap();
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

#[test]
fn glb_only_saves_reload_relative_models_and_recover_after_bad_files() {
    let file = TempMap::new(concat!(
        "player_start(1,1,0); ground=land(-2,-2,2,-2,2,2,-2,2);",
        "station=building(0.0759,\"model.glb\",{collision:none},",
        "0,0,0.344,0,0.344,0.244,0,0.244);"
    ));
    let asset = file.0.with_file_name("model.glb");
    let original = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/models/fuel-station.glb"),
    )
    .unwrap();
    std::fs::write(&asset, &original).unwrap();
    let mut watcher = LiveReload::new(file.0.clone());
    let mut clock = watcher.next_check;
    std::fs::write(&asset, b"invalid GLB").unwrap();
    assert!(wait_for_reload(&mut watcher, &mut clock).is_err());
    std::fs::write(&asset, &original).unwrap();
    let mut hermes = wait_for_reload(&mut watcher, &mut clock).unwrap();
    let first = hermes.take_geometry().0;
    let doc = gltf::Gltf::from_slice(&original).unwrap();
    let accessor = doc
        .meshes()
        .next()
        .unwrap()
        .primitives()
        .next()
        .unwrap()
        .get(&gltf::Semantic::Positions)
        .unwrap();
    let json_length = u32::from_le_bytes(original[12..16].try_into().unwrap()) as usize;
    let offset = 28 + json_length + accessor.view().unwrap().offset() + accessor.offset();
    let mut edited = original.clone();
    let position = f32::from_le_bytes(edited[offset..offset + 4].try_into().unwrap());
    edited[offset..offset + 4].copy_from_slice(&(position + 0.5).to_le_bytes());
    std::fs::write(&asset, &edited).unwrap();
    let mut updated = wait_for_reload(&mut watcher, &mut clock).unwrap();
    let second = updated.take_geometry().0;
    assert_eq!(first.vertices.len(), second.vertices.len());
    assert!(first
        .vertices
        .iter()
        .zip(&second.vertices)
        .any(|(a, b)| a.position != b.position));
    std::fs::remove_file(&asset).unwrap();
    assert!(wait_for_reload(&mut watcher, &mut clock).is_err());
    std::fs::write(&asset, &original).unwrap();
    assert!(wait_for_reload(&mut watcher, &mut clock).is_ok());
}
