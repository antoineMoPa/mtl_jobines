use crate::{
    engine::render::Renderer,
    hermes::{MinimapView, RouteStatus},
};
use glam::Vec3;
use opencity::runtime::{Mesh, SurfaceKind};
use std::path::Path;
fn target(r: &Renderer) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("SSAO QA"),
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
fn save(r: &Renderer, pixels: &[u8], name: &str) {
    if let Some(dir) = std::env::var_os("REPORT36_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        let mut png = png::Encoder::new(
            std::fs::File::create(Path::new(&dir).join(name)).unwrap(),
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
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_ssao_shades_contacts_preserves_sky_hud_and_resizes_in_both_projections() {
    let mut r = pollster::block_on(Renderer::new(None, 640, 400)).unwrap();
    r.daylight = Some(1.);
    r.reserved_bottom = 42;
    let mut mesh = Mesh::default();
    mesh.quad(
        [[-1., -1., 0.], [1., -1., 0.], [1., 1., 0.], [-1., 1., 0.]],
        [0.5; 3],
        SurfaceKind::Prop,
    );
    mesh.cuboid(
        [0., -0.05, 0.],
        [0.02, 0.18, 0.10],
        [0.6; 3],
        SurfaceKind::Prop,
    );
    let world = r.upload(&mesh);
    for span in [None, Some(0.35)] {
        r.construction_span = span;
        let texture = target(&r);
        let view = texture.create_view(&Default::default());
        let eye = Vec3::new(-0.25, -0.35, 0.18);
        let look = Vec3::new(0., 0., 0.015);
        let mut images = Vec::new();
        for enabled in [false, true] {
            r.ambient_occlusion = enabled;
            r.render(&view, &[&world], eye, look, 0., navigation());
            images.push(r.rgba_pixels(&texture).unwrap());
        }
        let darkened = images[0]
            .chunks_exact(4)
            .zip(images[1].chunks_exact(4))
            .filter(|(a, b)| u16::from(a[0]) > u16::from(b[0]) + 3)
            .count();
        assert!(darkened > 20, "no contact AO in span {span:?}: {darkened}");
        let strip = (358 * 640 * 4) as usize;
        assert_eq!(
            &images[0][strip..],
            &images[1][strip..],
            "reserved strip changed"
        );
        // The minimap is composited after AO, including its translucent edge.
        for y in 250..350 {
            for x in 15..110 {
                let p = (y * 640 + x) * 4;
                assert_eq!(&images[0][p..p + 4], &images[1][p..p + 4]);
            }
        }
        if span.is_none() {
            assert_eq!(&images[0][..640 * 4], &images[1][..640 * 4], "sky changed");
        }
        save(
            &r,
            &images[1],
            if span.is_some() {
                "ao-fixture-iso.png"
            } else {
                "ao-fixture-street.png"
            },
        );
    }
    r.resize(643, 403);
    r.construction_span = None;
    let texture = target(&r);
    r.render(
        &texture.create_view(&Default::default()),
        &[&world],
        Vec3::new(-0.2, -0.3, 0.2),
        Vec3::ZERO,
        0.,
        navigation(),
    );
    assert_eq!(r.rgba_pixels(&texture).unwrap().len(), 643 * 403 * 4);
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_report_camera_ao_comparison() {
    use crate::{
        dynamic_world::{DynamicWorld, SpawnPoint, METERS_PER_UNIT},
        engine::{prop_scenery::PropScenery, scenery::Scenery, vehicle_scenery::VehicleScenery},
        hermes::Hermes,
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("bug-reports/36/report.json")).unwrap())
            .unwrap();
    let point: [f32; 3] = serde_json::from_value(report["map_coordinates"].clone()).unwrap();
    let eye = Vec3::from_array(serde_json::from_value(report["camera_position"].clone()).unwrap());
    let look = Vec3::from_array(serde_json::from_value(report["camera_look_at"].clone()).unwrap());
    let mut hermes =
        Hermes::load_with_game_data(&root.join("data/maps/mtl/map.road"), None).unwrap();
    let mut r = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    r.daylight = Some(1.);
    let world = DynamicWorld::with_start(
        &hermes,
        Some(SpawnPoint {
            point: [point[0], point[1]],
            heading: Some((look.y - eye.y).atan2(look.x - eye.x)),
        }),
    )
    .unwrap();
    let scenery = Scenery::new(&r, &mut hermes);
    let mut moving = Mesh::default();
    world.write_moving_mesh(&mut moving);
    let car = r.upload_dynamic(&moving);
    let mut vehicles = VehicleScenery::new(&r);
    vehicles.update(&r, &world);
    let mut props = PropScenery::default();
    props.update(&r, &world);
    let p = world.player_position() / METERS_PER_UNIT;
    if let Some(pixels) = hermes.update_minimap([p.x, p.y]) {
        r.update_minimap(pixels);
    }
    let mut meshes = scenery.visible([p.x, p.y]);
    meshes.push(&car);
    meshes.extend(vehicles.visible());
    meshes.extend(props.visible());
    let texture = target(&r);
    let view = texture.create_view(&Default::default());
    for enabled in [false, true] {
        r.ambient_occlusion = enabled;
        for _ in 0..3 {
            r.render(
                &view,
                &meshes,
                eye,
                look,
                0.,
                hermes.minimap_view([p.x, p.y], world.player_yaw()),
            );
            r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
        let start = std::time::Instant::now();
        for _ in 0..8 {
            r.render(
                &view,
                &meshes,
                eye,
                look,
                0.,
                hermes.minimap_view([p.x, p.y], world.player_yaw()),
            );
            r.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        }
        eprintln!(
            "report camera AO {enabled}: {:.2} ms/frame including CPU submission",
            start.elapsed().as_secs_f64() * 1000. / 8.
        );
        let pixels = r.rgba_pixels(&texture).unwrap();
        save(
            &r,
            &pixels,
            if enabled {
                "report-36-ao.png"
            } else {
                "report-36-before.png"
            },
        );
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn antialiasing_switch_preserves_scene_and_depth_effects() {
    let mut r = pollster::block_on(Renderer::new(None, 128, 96)).unwrap();
    let mut mesh = Mesh::default();
    mesh.quad(
        [[-1., -1., 0.], [1., -1., 0.], [1., 1., 0.], [-1., 1., 0.]],
        [0.5; 3],
        SurfaceKind::Prop,
    );
    let world = r.upload(&mesh);
    for (enabled, width) in [(false, 128), (true, 144), (false, 160)] {
        r.set_antialiasing(enabled);
        r.resize(width, 96);
        r.season = crate::season::Season::Winter;
        let texture = target(&r);
        r.render(
            &texture.create_view(&Default::default()),
            &[&world],
            Vec3::new(-0.2, -0.3, 0.2),
            Vec3::ZERO,
            0.,
            navigation(),
        );
        let pixels = r.rgba_pixels(&texture).unwrap();
        assert_eq!(pixels.len(), (width * 96 * 4) as usize);
        assert!(pixels.chunks_exact(4).any(|p| p[0] > 10));
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_flight_visibility_reveals_landmarks_beyond_the_street_far_plane() {
    let mut renderer = pollster::block_on(Renderer::new(None, 320, 240)).unwrap();
    renderer.daylight = Some(1.);
    let eye = Vec3::new(0., 0., 3.);
    let look = Vec3::new(0., 15., 3.);
    let mut mesh = Mesh::default();
    mesh.cuboid(
        [-2., 15., 0.],
        [2., 16., 6.],
        [0.5, 0.04, 0.02],
        SurfaceKind::Prop,
    );
    let landmark = renderer.upload(&mesh);
    let texture = target(&renderer);
    let view = texture.create_view(&Default::default());
    for scale in [1., 4.] {
        renderer.flight_visibility = scale;
        renderer.render_scene(&view, &[], eye, look, 0., None);
        let sky = renderer.rgba_pixels(&texture).unwrap();
        renderer.render_scene(&view, &[&landmark], eye, look, 0., None);
        let scene = renderer.rgba_pixels(&texture).unwrap();
        let i = (120 * 320 + 160) * 4;
        let contrast = (0..3)
            .map(|c| scene[i + c].abs_diff(sky[i + c]))
            .max()
            .unwrap();
        if scale == 1. {
            assert_eq!(contrast, 0);
        } else {
            assert!(
                contrast > 16,
                "distant landmark lost in flight haze: {contrast}"
            );
        }
    }
}
