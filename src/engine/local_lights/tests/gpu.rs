use super::*;
use crate::{
    engine::render::Renderer,
    hermes::{MinimapView, RouteStatus},
};
use opencity::runtime::{Mesh, SurfaceKind};

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_authored_street_and_vehicle_capture() {
    use crate::{
        dynamic_world::{Controls, DynamicWorld, WalkControls, METERS_PER_UNIT},
        engine::{prop_scenery::PropScenery, scenery::Scenery, vehicle_scenery::VehicleScenery},
        hermes::Hermes,
    };
    let mut hermes =
        Hermes::test_map_from_source(include_str!("../../../../tests/fixtures/local_lights.road"));
    let mut world = DynamicWorld::new(&hermes);
    world.step(
        &hermes,
        Controls::default(),
        WalkControls::default(),
        1. / 120.,
    );
    let mut r = pollster::block_on(Renderer::new(None, 1000, 650)).unwrap();
    r.daylight = Some(0.);
    let scenery = Scenery::new(&r, &mut hermes);
    let mut props = PropScenery::default();
    props.update(&r, &world);
    let mut vehicles = VehicleScenery::new(&r);
    vehicles.update(&r, &world);
    let mut moving = Mesh::default();
    world.write_moving_mesh(&mut moving);
    let moving = r.upload_dynamic(&moving);
    let sources = world.local_lights();
    assert!(sources.iter().any(|l| l.beam[2] == 1.));
    assert!(sources.iter().any(|l| l.beam[2] == 0.));
    let p = world.player_position() / METERS_PER_UNIT;
    let mut meshes = scenery.visible([p.x, p.y]);
    meshes.push(&moving);
    meshes.extend(props.visible());
    meshes.extend(vehicles.visible());
    for (name, span, eye, look) in [
        (
            "authored-street",
            None,
            p + Vec3::new(-0.16, -0.12, 0.075),
            p + Vec3::new(0.15, 0., 0.),
        ),
        (
            "authored-iso",
            Some(0.65),
            p + Vec3::new(-0.3, -0.4, 0.4),
            p + Vec3::new(0.1, 0., 0.),
        ),
    ] {
        r.construction_span = span;
        let mut images = Vec::new();
        for (label, lights) in [("before", vec![]), ("after", sources.clone())] {
            r.local_lights.set(lights);
            let png = r
                .report_screenshot(&meshes, eye, look, 0., hermes.minimap_view([p.x, p.y], 0.))
                .unwrap();
            let mut decoder = png::Decoder::new(png.as_slice()).read_info().unwrap();
            let mut pixels = vec![0; decoder.output_buffer_size()];
            decoder.next_frame(&mut pixels).unwrap();
            images.push(pixels);
            if let Some(directory) = std::env::var_os("LOCAL_LIGHT_CAPTURE") {
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(
                    std::path::Path::new(&directory).join(format!("{name}-{label}.png")),
                    png,
                )
                .unwrap();
            }
        }
        let brighter = images[0]
            .chunks_exact(4)
            .zip(images[1].chunks_exact(4))
            .filter(|(a, b)| u16::from(b[0]) > u16::from(a[0]) + 10)
            .count();
        assert!(
            brighter > 500,
            "authored road is not illuminated: {brighter}"
        );
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_lights_illuminate_ground_and_walls_with_direction_and_falloff() {
    let mut r = pollster::block_on(Renderer::new(None, 640, 400)).unwrap();
    r.daylight = Some(0.);
    r.ambient_occlusion = false;
    let mut mesh = Mesh::default();
    mesh.quad(
        [
            [-0.8, -0.8, 0.],
            [0.8, -0.8, 0.],
            [0.8, 0.8, 0.],
            [-0.8, 0.8, 0.],
        ],
        [0.35; 3],
        SurfaceKind::Asphalt,
    );
    mesh.cuboid(
        [0.215, -0.175, 0.],
        [0.225, 0.175, 0.10],
        [0.4; 3],
        SurfaceKind::Prop,
    );
    let mesh = r.upload(&mesh);
    let nav = MinimapView {
        uv_center: [0.5; 2],
        uv_span: 1.,
        heading: 0.,
        player_uv: [0.5; 2],
        destination_uv: [0.5; 2],
        route_status: RouteStatus::Inactive,
    };
    let texture = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("local light regression"),
        size: wgpu::Extent3d {
            width: 640,
            height: 400,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: r.config.format.add_srgb_suffix(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let eye = Vec3::new(-0.35, -0.5, 0.35);
    let look = Vec3::new(0.08, 0., 0.);
    for span in [None, Some(0.65)] {
        r.construction_span = span;
        let mut images = Vec::new();
        for (name, lights) in [
            ("off", vec![]),
            (
                "streetlamp",
                vec![LocalLight::streetlamp(Vec3::new(0., 0., 0.055))],
            ),
            (
                "headlight",
                vec![LocalLight::headlight(
                    Vec3::new(-0.10, 0., 0.008),
                    glam::Quat::IDENTITY,
                    1.,
                )],
            ),
        ] {
            r.local_lights.set(lights);
            r.render(&view, &[&mesh], eye, look, 0., nav);
            let pixels = r.rgba_pixels(&texture).unwrap();
            if let Some(directory) = std::env::var_os("LOCAL_LIGHT_CAPTURE") {
                std::fs::create_dir_all(&directory).unwrap();
                let path = std::path::Path::new(&directory).join(format!(
                    "{}-{name}.png",
                    if span.is_some() { "iso" } else { "street" }
                ));
                let mut png = png::Encoder::new(std::fs::File::create(path).unwrap(), 640, 400);
                png.set_color(png::ColorType::Rgba);
                png.set_depth(png::BitDepth::Eight);
                png.write_header()
                    .unwrap()
                    .write_image_data(&pixels)
                    .unwrap();
            }
            images.push(pixels);
        }
        for lit in &images[1..] {
            let brighter = images[0]
                .chunks_exact(4)
                .zip(lit.chunks_exact(4))
                .filter(|(a, b)| u16::from(b[0]) > u16::from(a[0]) + 10)
                .count();
            assert!(brighter > 100, "no illuminated surfaces: {brighter}");
        }
        // Sample actual projected surface positions, away from the HUD.
        let pixel = |p: Vec3| {
            let clip = r.view_projection(eye, look) * p.extend(1.);
            let ndc = clip.truncate() / clip.w;
            let x = ((ndc.x * 0.5 + 0.5) * 640.) as usize;
            let y = ((0.5 - ndc.y * 0.5) * 400.) as usize;
            (y * 640 + x) * 4
        };
        let lit = pixel(Vec3::new(0.08, 0., 0.0001));
        assert!(
            images[2][lit] > images[0][lit] + 10,
            "beam misses forward pavement"
        );
        let behind = pixel(Vec3::new(-0.18, 0., 0.0001));
        assert_eq!(
            &images[0][behind..behind + 3],
            &images[2][behind..behind + 3],
            "light behind headlamp"
        );
        let wall = pixel(Vec3::new(0.2149, 0., 0.025));
        assert!(
            images[2][wall] > images[0][wall] + 10,
            "headlight misses wall"
        );
    }
}
