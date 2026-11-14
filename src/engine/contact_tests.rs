//! Matched readbacks through the game's production shader and report renderer.
use super::{render::Renderer, scenery::Scenery};
use crate::hermes::{Hermes, MinimapView, RouteStatus};
use glam::Vec3;
use opencity::runtime::{City, Mesh, Region, SurfaceKind};
use std::path::Path;

fn navigation() -> MinimapView {
    MinimapView {
        uv_center: [0.5; 2],
        uv_span: 1.,
        heading: 0.,
        player_uv: [0.5; 2],
        destination_uv: [0.5; 2],
        route_status: RouteStatus::Inactive,
    }
}
fn target(r: &Renderer) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("contact texture regression"),
        size: wgpu::Extent3d {
            width: r.config.width,
            height: r.config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: r.config.format.add_srgb_suffix(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn save(r: &Renderer, pixels: &[u8], name: &str) {
    if let Some(directory) = std::env::var_os("CONTACT_DEBRIS_CAPTURE") {
        std::fs::create_dir_all(&directory).unwrap();
        let mut png = png::Encoder::new(
            std::fs::File::create(Path::new(&directory).join(name)).unwrap(),
            r.config.width,
            r.config.height,
        );
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
    }
}
fn differences(before: &[u8], after: &[u8]) -> usize {
    before
        .chunks_exact(4)
        .zip(after.chunks_exact(4))
        .filter(|(a, b)| (0..3).any(|i| a[i].abs_diff(b[i]) > 2))
        .count()
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_contact_texture_covers_curbs_and_preserves_flush_pavement() {
    let program = opencity::parse(include_str!(
        "../../opencity/tests/fixtures/contact_debris.road"
    ))
    .unwrap();
    let city = City::from_program(&program, None).unwrap();
    let start = std::time::Instant::now();
    let mesh = city.mesh(Region::WholeMap, 0.04).unwrap();
    eprintln!(
        "contact fixture mesh preparation {:?}, {} vertices",
        start.elapsed(),
        mesh.vertices.len()
    );
    assert!(
        mesh.vertices.iter().any(|v| v.surface[3] < -4.),
        "junction has no baked contacts"
    );
    let mut r = pollster::block_on(Renderer::new(None, 1024, 640)).unwrap();
    eprintln!("contact adapter: {}", r.adapter_name);
    r.daylight = Some(1.);
    r.ambient_occlusion = false;
    let gpu = r.upload(&mesh);
    let output = target(&r);
    let view = output.create_view(&Default::default());
    for (name, span, eye, look) in [
        (
            "curb",
            None,
            Vec3::new(-0.05, -0.08, 0.035),
            Vec3::new(0., 0.025, 0.),
        ),
        (
            "curve",
            None,
            Vec3::new(-0.08, 0.10, 0.045),
            Vec3::new(0., 0.2, 0.),
        ),
        (
            "junction",
            None,
            Vec3::new(0.35, -0.12, 0.07),
            Vec3::new(0.45, 0., 0.),
        ),
        (
            "iso",
            Some(0.95),
            Vec3::new(-0.3, -0.5, 0.6),
            Vec3::new(0.15, 0.08, 0.),
        ),
    ] {
        r.construction_span = span;
        let mut images = Vec::new();
        for enabled in [false, true] {
            r.contact_debris = enabled;
            r.render(&view, &[&gpu], eye, look, 0., navigation());
            let pixels = r.rgba_pixels(&output).unwrap();
            assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
            save(
                &r,
                &pixels,
                &format!(
                    "fixture-{name}-{}.png",
                    if enabled { "after" } else { "before" }
                ),
            );
            images.push(pixels);
        }
        assert!(
            differences(&images[0], &images[1]) > 20,
            "no contact shading in {name}"
        );
    }
    let mut flush = Mesh::default();
    flush.quad(
        [
            [-0.3, -0.3, 0.],
            [0.3, -0.3, 0.],
            [0.3, 0.3, 0.],
            [-0.3, 0.3, 0.],
        ],
        [0.5; 3],
        SurfaceKind::Asphalt,
    );
    let flush = r.upload(&flush);
    r.construction_span = None;
    let mut images = Vec::new();
    for enabled in [false, true] {
        r.contact_debris = enabled;
        r.render(
            &view,
            &[&flush],
            Vec3::new(-0.1, -0.2, 0.1),
            Vec3::ZERO,
            0.,
            navigation(),
        );
        images.push(r.rgba_pixels(&output).unwrap());
    }
    assert_eq!(
        images[0], images[1],
        "unmarked pavement acquired contact deposits"
    );
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_winter_contacts_brighten_plowed_edges_and_restore_summer() {
    use crate::{dynamic_world::snow::Tracks, season::Season};
    let program = opencity::parse(include_str!(
        "../../opencity/tests/fixtures/contact_debris.road"
    ))
    .unwrap();
    let city = City::from_program(&program, None).unwrap();
    let mesh = city.mesh(Region::WholeMap, 0.04).unwrap();
    let mut r = pollster::block_on(Renderer::new(None, 1024, 640)).unwrap();
    r.daylight = Some(1.);
    r.ambient_occlusion = false;
    r.winter_weather = false;
    r.snow.set_geometry(&[&mesh]);
    let mut tracks = Tracks::default();
    for y in -80..100 {
        for x in -80..160 {
            tracks.clear_height((x, y), 0.);
        }
    }
    r.snow.update(&r.queue, [0., 0.], &tracks);
    let gpu = r.upload(&mesh);
    let output = target(&r);
    let view = output.create_view(&Default::default());
    let eye = Vec3::new(-0.05, -0.08, 0.035);
    let look = Vec3::new(0., 0.025, 0.);
    let mut summer = Vec::new();
    for (season, name) in [
        (Season::Summer, "summer"),
        (Season::Winter, "winter"),
        (Season::Summer, "summer-return"),
    ] {
        r.season = season;
        let mut images = Vec::new();
        for enabled in [false, true] {
            r.contact_debris = enabled;
            r.render(&view, &[&gpu], eye, look, 0., navigation());
            let pixels = r.rgba_pixels(&output).unwrap();
            save(&r, &pixels, &format!("edge-{name}-{enabled}.png"));
            images.push(pixels);
        }
        let mut brighter = 0;
        let mut darker = 0;
        for (a, b) in images[0].chunks_exact(4).zip(images[1].chunks_exact(4)) {
            let delta: i32 = (0..3).map(|i| i32::from(b[i]) - i32::from(a[i])).sum();
            brighter += usize::from(delta > 6);
            darker += usize::from(delta < -6);
        }
        if season == Season::Winter {
            assert!(brighter > 100, "snow contacts invisible: {brighter}");
            assert!(brighter > darker * 10, "winter retained dark debris");
        } else {
            assert!(darker > 100, "summer lost its grime");
            if summer.is_empty() {
                summer = images;
            } else {
                assert_eq!(summer, images, "winter contacts survived season switch");
            }
        }
    }
}

#[test]
#[ignore = "requires a native GPU adapter and the selected local bug report"]
fn native_contact_texture_report_camera_and_cost() {
    use crate::{
        dynamic_world::{DynamicWorld, SpawnPoint, METERS_PER_UNIT},
        engine::{prop_scenery::PropScenery, vehicle_scenery::VehicleScenery},
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let report_id = std::env::var("CONTACT_DEBRIS_REPORT")
        .map(|value| {
            value
                .parse::<u32>()
                .expect("CONTACT_DEBRIS_REPORT must be a report number")
        })
        .unwrap_or(38);
    let report: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(format!("bug-reports/{report_id}/report.json"))).unwrap(),
    )
    .unwrap();
    let point: [f32; 3] = serde_json::from_value(report["map_coordinates"].clone()).unwrap();
    let eye = Vec3::from_array(serde_json::from_value(report["camera_position"].clone()).unwrap());
    let look = Vec3::from_array(serde_json::from_value(report["camera_look_at"].clone()).unwrap());
    let start = std::time::Instant::now();
    let mut hermes =
        Hermes::load_with_game_data(&root.join("data/maps/mtl/map.road"), None).unwrap();
    let mut r = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    let world = DynamicWorld::with_start(
        &hermes,
        Some(SpawnPoint {
            point: [point[0], point[1]],
            heading: Some((look.y - eye.y).atan2(look.x - eye.x)),
        }),
    )
    .unwrap();
    let scenery = Scenery::new(&r, &mut hermes);
    eprintln!(
        "report {report_id} load + GPU/scene preparation: {:?}; adapter {}",
        start.elapsed(),
        r.adapter_name
    );
    let mut moving = Mesh::default();
    world.write_moving_mesh(&mut moving);
    let moving = r.upload_dynamic(&moving);
    let mut vehicles = VehicleScenery::new(&r);
    vehicles.update(&r, &world);
    let mut props = PropScenery::default();
    props.update(&r, &world);
    r.local_lights.set(world.local_lights());
    let p = world.player_position() / METERS_PER_UNIT;
    if let Some(pixels) = hermes.update_minimap([p.x, p.y]) {
        r.update_minimap(pixels);
    }
    let mut meshes = scenery.visible([p.x, p.y]);
    meshes.push(&moving);
    meshes.extend(vehicles.visible());
    meshes.extend(props.visible());
    let output = target(&r);
    let view = output.create_view(&Default::default());
    for (name, span, camera, target, daylight) in [
        (format!("report{report_id}-day"), None, eye, look, 1.),
        (format!("report{report_id}-headlights"), None, eye, look, 0.),
        (
            format!("report{report_id}-iso"),
            Some(1.4),
            p + Vec3::new(-0.8, -0.8, 0.9),
            p,
            1.,
        ),
    ] {
        r.construction_span = span;
        r.daylight = Some(daylight);
        let mut images = Vec::new();
        for enabled in [false, true] {
            r.contact_debris = enabled;
            for _ in 0..4 {
                r.render(
                    &view,
                    &meshes,
                    camera,
                    target,
                    0.,
                    hermes.minimap_view([p.x, p.y], 0.),
                );
                r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            }
            let start = std::time::Instant::now();
            let mut material = Vec::new();
            for _ in 0..12 {
                r.render(
                    &view,
                    &meshes,
                    camera,
                    target,
                    0.,
                    hermes.minimap_view([p.x, p.y], 0.),
                );
                r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
                if let Some(profile) = &r.gpu_profile {
                    material.push(
                        profile
                            .read(&r.device, r.queue.get_timestamp_period())
                            .unwrap()[1],
                    );
                }
            }
            let frame_ms = start.elapsed().as_secs_f64() * 1000. / 12.;
            material.sort_by(f64::total_cmp);
            eprintln!(
                "{name} enabled={enabled}: {frame_ms:.3} ms/frame CPU+GPU+sync; \
                 median scene GPU {:?} ms",
                material.get(material.len() / 2)
            );
            let pixels = r.rgba_pixels(&output).unwrap();
            save(
                &r,
                &pixels,
                &format!("{name}-{}.png", if enabled { "after" } else { "before" }),
            );
            images.push(pixels);
        }
        let changed = differences(&images[0], &images[1]);
        eprintln!("{name}: {changed} pixels changed");
        assert!(changed > 20, "report contacts invisible");
    }
}
