use super::*;
use crate::{
    engine::render::Renderer,
    hermes::{MinimapView, RouteStatus},
    season::Season,
};
use glam::Vec3;
use opencity::runtime::SurfaceKind;

#[test]
fn exposure_raster_covers_roof_and_keeps_negative_world_coordinates() {
    let mut heights = vec![EMPTY; SIZE * SIZE];
    raster(
        &mut heights,
        [[-0.1, -0.1, 0.05], [0.1, -0.1, 0.05], [0.1, 0.1, 0.05]],
        [-0.2, -0.2],
    );
    let at = |x: f32, y: f32| ((y + 0.2) / CELL) as usize * SIZE + ((x + 0.2) / CELL) as usize;
    assert!((heights[at(0.02, 0.)] - 0.05).abs() < 0.0001);
    assert_eq!(heights[at(-0.15, -0.15)], EMPTY);
    assert_eq!(pixel((-10, -20), [-20, -30]), Some((10, 10)));
    assert_eq!(pixel((-21, -20), [-20, -30]), None);
}
fn navigation() -> MinimapView {
    MinimapView {
        texture_region: [0., 0., 1., 1.],
        uv_center: [0.5; 2],
        uv_span: 1.,
        heading: 0.,
        player_uv: [0.5; 2],
        destination_uv: [0.5; 2],
        route_status: RouteStatus::Inactive,
    }
}
fn decode(png: &[u8]) -> Vec<u8> {
    let mut r = png::Decoder::new(png).read_info().unwrap();
    let mut out = vec![0; r.output_buffer_size()];
    r.next_frame(&mut out).unwrap();
    out
}
fn sample(renderer: &Renderer, rgba: &[u8], point: Vec3, eye: Vec3, look: Vec3) -> [u8; 4] {
    let clip = renderer.view_projection(eye, look) * point.extend(1.);
    let x = ((clip.x / clip.w * 0.5 + 0.5) * renderer.config.width as f32) as usize;
    let y = ((0.5 - clip.y / clip.w * 0.5) * renderer.config.height as f32) as usize;
    rgba[(y * renderer.config.width as usize + x) * 4
        ..(y * renderer.config.width as usize + x + 1) * 4]
        .try_into()
        .unwrap()
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_winter_summer_roof_shelter_glass_deck_and_cache_roundtrip() {
    let mut renderer = pollster::block_on(Renderer::new(None, 960, 640)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    renderer.ambient_occlusion = false;
    renderer.winter_weather = false;
    let mut mesh = Mesh::default();
    mesh.quad(
        [
            [-0.4, -0.3, 0.],
            [0.4, -0.3, 0.],
            [0.4, 0.3, 0.],
            [-0.4, 0.3, 0.],
        ],
        [0.3; 3],
        SurfaceKind::Asphalt,
    );
    mesh.quad(
        [
            [0., 0., 0.05],
            [0.12, 0., 0.05],
            [0.12, 0.12, 0.05],
            [0., 0.12, 0.05],
        ],
        [0.2; 3],
        SurfaceKind::Roof,
    );
    mesh.quad(
        [
            [-0.22, 0.04, 0.02],
            [-0.12, 0.04, 0.02],
            [-0.12, 0.14, 0.02],
            [-0.22, 0.14, 0.02],
        ],
        [0.1, 0.2, 0.3],
        SurfaceKind::Window,
    );
    mesh.quad(
        [
            [0.18, 0., 0.06],
            [0.28, 0., 0.06],
            [0.28, 0.1, 0.06],
            [0.18, 0.1, 0.06],
        ],
        [0.25; 3],
        SurfaceKind::Asphalt,
    );
    renderer.snow.set_geometry(&[&mesh]);
    let gpu = renderer.upload(&mesh);
    let eye = Vec3::new(-0.3, -0.5, 0.30);
    let look = Vec3::new(0., 0., 0.01);
    let mut tracks = Tracks::default();
    let capture = |r: &Renderer, name: &str| {
        let png = r
            .report_screenshot(&[&gpu], eye, look, 0., navigation())
            .unwrap();
        if let Some(dir) = std::env::var_os("WINTER_CAPTURE") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), &png).unwrap();
        }
        decode(&png)
    };
    let summer = capture(&renderer, "summer");
    renderer.season = Season::Winter;
    renderer.snow.update(&renderer.queue, [0., 0.], &tracks);
    let winter = capture(&renderer, "winter");
    let bright = winter
        .chunks_exact(4)
        .zip(summer.chunks_exact(4))
        .filter(|(w, s)| u16::from(w[0]) > u16::from(s[0]) + 30)
        .count();
    assert!(bright > 1000, "winter did not cover the street: {bright}");
    let roof = Vec3::new(0.06, 0.06, 0.05);
    let glass = Vec3::new(-0.17, 0.09, 0.02);
    let road = Vec3::new(-0.1, -0.12, 0.);
    let sheltered = Vec3::new(0.02, 0.015, 0.);
    let bridge = Vec3::new(0.23, 0.05, 0.06);
    assert!(
        sample(&renderer, &winter, roof, eye, look)[0]
            > sample(&renderer, &summer, roof, eye, look)[0] + 30,
        "roof did not gain snow"
    );
    assert_eq!(
        sample(&renderer, &summer, sheltered, eye, look),
        sample(&renderer, &winter, sheltered, eye, look),
        "snow appeared beneath a roof"
    );
    assert_eq!(
        sample(&renderer, &summer, glass, eye, look),
        sample(&renderer, &winter, glass, eye, look),
        "glass gained snow"
    );
    for y in -60..60 {
        for x in -80..80 {
            tracks.clear_height((x, y), 0.);
        }
    }
    renderer.snow.update(&renderer.queue, [0., 0.], &tracks);
    let cleared = capture(&renderer, "winter-cleared");
    assert_eq!(
        sample(&renderer, &winter, bridge, eye, look),
        sample(&renderer, &cleared, bridge, eye, look),
        "a ground-level pass cleared the bridge"
    );
    assert_eq!(
        sample(&renderer, &winter, roof, eye, look),
        sample(&renderer, &cleared, roof, eye, look),
        "plow cleared a roof"
    );
    assert_ne!(
        sample(&renderer, &winter, road, eye, look),
        sample(&renderer, &cleared, road, eye, look),
        "plow trail not visible"
    );
    // A subsequent small edit must take the dirty-rectangle path, including deck matching.
    for y in 0..20 {
        for x in 36..56 {
            tracks.clear_height((x, y), 0.06);
        }
    }
    renderer.snow.update(&renderer.queue, [0., 0.], &tracks);
    let bridge_cleared = capture(&renderer, "winter-bridge-cleared");
    assert_ne!(
        sample(&renderer, &cleared, bridge, eye, look),
        sample(&renderer, &bridge_cleared, bridge, eye, look),
        "bridge-level plow edit was not uploaded"
    );
    assert_eq!(
        sample(&renderer, &winter, roof, eye, look),
        sample(&renderer, &bridge_cleared, roof, eye, look)
    );
    renderer.snow.update(&renderer.queue, [10., 10.], &tracks);
    renderer.snow.update(&renderer.queue, [0., 0.], &tracks);
    assert_eq!(
        capture(&renderer, "winter-return"),
        bridge_cleared,
        "cache eviction lost trails"
    );
    renderer.season = Season::Summer;
    assert_eq!(
        capture(&renderer, "summer-restored"),
        summer,
        "summer was not restored exactly"
    );
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_winter_actual_map_glb_traffic_capture() {
    use crate::{
        dynamic_world::{Controls, DynamicWorld, WalkControls, METERS_PER_UNIT},
        engine::{prop_scenery::PropScenery, scenery::Scenery, vehicle_scenery::VehicleScenery},
        hermes::Hermes,
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut hermes =
        Hermes::load_with_game_data(&root.join("data/maps/mtl/map.road"), None).unwrap();
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let scenery = Scenery::new(&renderer, &mut hermes);
    let mut world = DynamicWorld::new(&hermes);
    let mut vehicles = VehicleScenery::new(&renderer);
    let mut props = PropScenery::default();
    let mut moving = Mesh::default();
    for (season, name) in [
        (Season::Summer, "map-summer"),
        (Season::Winter, "map-winter"),
        (Season::Summer, "map-summer-restored"),
    ] {
        world.set_season(&hermes, season);
        renderer.season = season;
        if season == Season::Winter {
            for _ in 0..600 {
                world.step(
                    &hermes,
                    Controls {
                        handbrake: true,
                        ..Default::default()
                    },
                    WalkControls::default(),
                    1. / 120.,
                );
            }
        }
        vehicles.update(&renderer, &world);
        props.update(&renderer, &world);
        world.write_moving_mesh(&mut moving);
        let actors = renderer.upload_dynamic(&moving);
        let p = world.player_position() / METERS_PER_UNIT;
        let mut meshes = scenery.visible([p.x, p.y]);
        meshes.push(&actors);
        meshes.extend(vehicles.visible());
        meshes.extend(props.visible());
        let png = renderer
            .report_screenshot(
                &meshes,
                p + Vec3::new(-0.32, -0.38, 0.22),
                p + Vec3::new(0.03, 0.03, 0.),
                0.,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        if let Some(dir) = std::env::var_os("WINTER_CAPTURE") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png).unwrap();
        }
        if season == Season::Winter {
            let eye = p + Vec3::new(-0.06, -0.12, 0.035);
            let look = p + Vec3::new(0., 0.08, 0.015);
            for (eye, look, name) in [
                (eye, look, "map-world-snow-street"),
                (
                    eye + Vec3::X * 0.03,
                    look + Vec3::X * 0.03,
                    "map-world-snow-strafe",
                ),
                (
                    eye,
                    eye + Vec3::new(0.12, 0.16, -0.02),
                    "map-world-snow-turn",
                ),
            ] {
                let png = renderer
                    .report_screenshot(
                        &meshes,
                        eye,
                        look,
                        12.5,
                        hermes.minimap_view([p.x, p.y], 0.),
                    )
                    .unwrap();
                if let Some(dir) = std::env::var_os("WINTER_CAPTURE") {
                    std::fs::create_dir_all(&dir).unwrap();
                    std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png)
                        .unwrap();
                }
            }
            let plows = world
                .vehicle_instances()
                .filter(|(model, _, _, _, _, _)| {
                    crate::dynamic_world::catalog::definitions()[*model]
                        .config
                        .research_id
                        == crate::dynamic_world::catalog::SNOWPLOW_ID
                })
                .count();
            assert!(plows >= 2, "winter NPCs were not present on the actual map");
            assert!(
                !world.snow.cells.is_empty(),
                "actual-map snowplow NPCs left no cleared road cells"
            );
        }
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_winter_smoothed_mask_keeps_resolution() {
    let mut r = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    r.daylight = Some(1.);
    r.sky_settings.solar_hour = Some(12.);
    r.season = Season::Winter;
    r.winter_weather = false;
    r.ambient_occlusion = false;
    r.construction_span = Some(0.3);
    let mut mesh = Mesh::default();
    mesh.quad(
        [
            [-0.4, -0.4, 0.],
            [0.4, -0.4, 0.],
            [0.4, 0.4, 0.],
            [-0.4, 0.4, 0.],
        ],
        [0.3; 3],
        SurfaceKind::Asphalt,
    );
    r.snow.set_geometry(&[&mesh]);
    let mut tracks = Tracks::default();
    for y in -80..80 {
        for x in -80..0 {
            tracks.clear_height((x, y), 0.);
        }
    }
    r.snow.update(&r.queue, [0., 0.], &tracks);
    let gpu = r.upload(&mesh);
    let eye = Vec3::new(0., -0.15, 0.5);
    let look = Vec3::ZERO;
    let png = r
        .report_screenshot(&[&gpu], eye, look, 0., navigation())
        .unwrap();
    if let Some(dir) = std::env::var_os("WINTER_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            std::path::Path::new(&dir).join("snow-edge-smooth.png"),
            &png,
        )
        .unwrap();
    }
    let rgba = decode(&png);
    let dark = sample(&r, &rgba, Vec3::new(-0.012, 0., 0.), eye, look)[0];
    let white = sample(&r, &rgba, Vec3::new(0.012, 0., 0.), eye, look)[0];
    let intermediate: HashSet<_> = (-30..=30)
        .map(|i| sample(&r, &rgba, Vec3::new(i as f32 * 0.0002, 0., 0.), eye, look)[0])
        .filter(|v| u16::from(*v) > u16::from(dark) + 20 && u16::from(*v) + 20 < u16::from(white))
        .collect();
    assert!(
        intermediate.len() >= 5,
        "edge still jumps between full texels: {intermediate:?}"
    );
    assert_eq!(SIZE, 1024);
    assert_eq!(CELL_METERS, 0.5);
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_winter_fog_snowfall_animates_preserves_summer_and_stops_under_cover() {
    let mut r = pollster::block_on(Renderer::new(None, 960, 640)).unwrap();
    r.daylight = Some(1.);
    r.sky_settings.solar_hour = Some(12.);
    r.ambient_occlusion = false;
    r.reserved_bottom = 32;
    let eye = Vec3::new(0., 0., 0.02);
    let look = Vec3::new(0., 0.1, 0.02);
    let capture = |r: &Renderer, time: f32, name: &str| {
        let png = r
            .report_screenshot(&[], eye, look, time, navigation())
            .unwrap();
        if let Some(dir) = std::env::var_os("WINTER_CAPTURE") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), &png).unwrap();
        }
        decode(&png)
    };
    let summer = capture(&r, 0., "weather-summer");
    r.season = Season::Winter;
    r.winter_weather = false;
    let bare = capture(&r, 0., "weather-off");
    assert_eq!(bare, summer);
    r.winter_weather = true;
    let first = capture(&r, 0., "weather-snow-first");
    let second = capture(&r, 1., "weather-snow-second");
    let changed = first
        .chunks_exact(4)
        .zip(second.chunks_exact(4))
        .filter(|(a, b)| a.iter().zip(b.iter()).any(|(a, b)| a.abs_diff(*b) > 12))
        .count();
    assert!(changed > 100, "snowfall did not animate: {changed}");
    let strip = 608 * 960 * 4;
    assert_eq!(
        &first[strip..],
        &summer[strip..],
        "weather covered the reserved UI strip"
    );
    // Most sky pixels should be hazier even outside the sparse moving flakes.
    let fogged = first[..960 * 300 * 4]
        .chunks_exact(4)
        .zip(summer[..960 * 300 * 4].chunks_exact(4))
        .filter(|(a, b)| a[0].abs_diff(b[0]) > 20)
        .count();
    assert!(
        fogged > 10000,
        "winter atmosphere did not change the horizon"
    );
    let mut roof = Mesh::default();
    roof.quad(
        [
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ],
        [0.5; 3],
        SurfaceKind::Roof,
    );
    r.snow.set_geometry(&[&roof]);
    r.snow.update(&r.queue, [eye.x, eye.y], &Tracks::default());
    assert_eq!(
        capture(&r, 0., "weather-sheltered"),
        capture(&r, 1., "weather-sheltered-next"),
        "flakes appeared beneath a roof"
    );
    r.season = Season::Summer;
    assert_eq!(capture(&r, 1., "weather-summer-restored"), summer);
    r.resize(803, 501);
    r.season = Season::Winter;
    assert_eq!(capture(&r, 2., "weather-resized").len(), 803 * 501 * 4);
}
