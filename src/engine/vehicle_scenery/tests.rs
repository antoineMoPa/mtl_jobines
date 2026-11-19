use super::*;
use crate::{
    dynamic_world::{Controls, METERS_PER_UNIT},
    hermes::Hermes,
};
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_bus_source_mesh_and_rear_lamps() {
    let hermes =
        Hermes::test_map_from_source(include_str!("../../../tests/fixtures/local_lights.road"));
    let mut world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 1200, 800)).unwrap();
    let model = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-j02")
        .unwrap();
    let mut vehicles = VehicleScenery::new(&renderer);
    let mut rear_frames = Vec::new();
    for (name, daylight, rear, controls) in [
        ("bus-front", 1., false, Controls::default()),
        ("bus-rear", 1., true, Controls::default()),
        ("bus-tail", 0., true, Controls::default()),
        (
            "bus-brake",
            0.,
            true,
            Controls {
                brake: true,
                ..Default::default()
            },
        ),
        (
            "bus-reverse",
            0.,
            true,
            Controls {
                throttle: -1.,
                ..Default::default()
            },
        ),
    ] {
        world.preview_vehicle_lamps(model, controls);
        renderer.daylight = Some(daylight);
        vehicles.update(&renderer, &world);
        let p = world.player_position() / METERS_PER_UNIT;
        let yaw = world.player().yaw;
        let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        let right = Vec3::new(-yaw.sin(), yaw.cos(), 0.);
        let eye = p + forward * if rear { -0.145 } else { 0.145 } + right * 0.12 + Vec3::Z * 0.075;
        let look = p + Vec3::Z * 0.008;
        let png = renderer
            .report_screenshot(
                &vehicles.visible().collect::<Vec<_>>(),
                eye,
                look,
                0.,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        if let Some(folder) = std::env::var_os("BUS_CAPTURE") {
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                std::path::Path::new(&folder).join(format!("{name}.png")),
                &png,
            )
            .unwrap();
        }
        let mut decoder = png::Decoder::new(png.as_slice()).read_info().unwrap();
        let mut pixels = vec![0; decoder.output_buffer_size()];
        decoder.next_frame(&mut pixels).unwrap();
        if daylight == 0. {
            rear_frames.push(pixels);
        }
    }
    let red_increase = rear_frames[0]
        .chunks_exact(4)
        .zip(rear_frames[1].chunks_exact(4))
        .filter(|(a, b)| u16::from(b[0]) > u16::from(a[0]) + 5 && b[0] > b[1])
        .count();
    assert!(
        red_increase > 20,
        "bus brake lenses did not brighten: {red_increase}"
    );
    assert_ne!(
        rear_frames[0], rear_frames[2],
        "bus reverse lenses did not change"
    );
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_service_roof_lights_and_shared_paint_variants() {
    let hermes =
        Hermes::test_map_from_source(include_str!("../../../tests/fixtures/local_lights.road"));
    let mut world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 1000, 700)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let mut vehicles = VehicleScenery::new(&renderer);
    let capture = |renderer: &Renderer,
                   vehicles: &VehicleScenery,
                   world: &DynamicWorld,
                   time: f32,
                   label: &str| {
        let p = world.player_position() / METERS_PER_UNIT;
        let meshes: Vec<_> = vehicles.visible().collect();
        let png = renderer
            .report_screenshot(
                &meshes,
                p + Vec3::new(-0.060, -0.065, 0.042),
                p + Vec3::Z * 0.008,
                time,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        if let Some(folder) = std::env::var_os("VEHICLE_VARIANT_CAPTURE") {
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                std::path::Path::new(&folder).join(format!("{label}.png")),
                &png,
            )
            .unwrap();
        }
        let mut decoder = png::Decoder::new(png.as_slice()).read_info().unwrap();
        let mut data = vec![0; decoder.output_buffer_size()];
        decoder.next_frame(&mut data).unwrap();
        data
    };
    let police = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-j06")
        .unwrap();
    world.preview_vehicle_lamps(police, Controls::default());
    vehicles.update(&renderer, &world);
    for (hour, label) in [(12., "day"), (0., "night")] {
        renderer.sky_settings.solar_hour = Some(hour);
        let first = capture(
            &renderer,
            &vehicles,
            &world,
            0.,
            &format!("police-{label}-first"),
        );
        let second = capture(
            &renderer,
            &vehicles,
            &world,
            0.2,
            &format!("police-{label}-second"),
        );
        assert!(first == second, "police roof lights flashed during {label}");
    }
    renderer.sky_settings.solar_hour = Some(12.);
    let taxi = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-j01")
        .unwrap();
    world.preview_vehicle_lamps(taxi, Controls::default());
    vehicles.update(&renderer, &world);
    capture(&renderer, &vehicles, &world, 0., "taxi-roof-sign");
    let suv = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-12")
        .unwrap();
    world.preview_vehicle_lamps(suv, Controls::default());
    vehicles.update(&renderer, &world);
    let mut frames = Vec::new();
    for (variant, name) in [(0, "original"), (1, "navy"), (2, "ivory"), (3, "burgundy")] {
        for part in &mut vehicles.models[suv] {
            for transform in &mut part.transforms {
                transform[2][3] = variant as f32;
            }
            renderer.update_instances(&mut part.mesh, &part.transforms);
        }
        frames.push(capture(
            &renderer,
            &vehicles,
            &world,
            0.,
            &format!("suv-paint-{name}"),
        ));
    }
    for image in frames.iter().skip(1) {
        assert!(
            frames[0]
                .chunks_exact(4)
                .zip(image.chunks_exact(4))
                .filter(|(a, b)| a[..3] != b[..3])
                .count()
                > 500,
            "shared geometry did not change paint"
        );
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_boat_waterlines_cut_the_ocean_at_the_rendered_pose() {
    use opencity::runtime::{Mesh, SurfaceKind};
    let hermes = Hermes::test_map();
    let mut world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 640, 480)).unwrap();
    renderer.daylight = Some(1.);
    let mut vehicles = VehicleScenery::new(&renderer);
    for (model, boat) in catalog::definitions()
        .iter()
        .enumerate()
        .filter(|(_, d)| d.water_hull.is_some())
    {
        world.preview_vehicle_lamps(model, Controls::default());
        let mut p = world.player_position() / METERS_PER_UNIT;
        p.z -= boat.config.physics.height_offset / METERS_PER_UNIT;
        world.water_level = Some(p.z);
        // Interpolation includes translation and attitude relative to the simulated pose.
        let offset = Vec3::new(0.08, -0.05, 0.);
        let pose = Mat4::from_translation(p + offset)
            * Mat4::from_rotation_y(0.08)
            * Mat4::from_rotation_x(-0.12)
            * Mat4::from_translation(-p);
        vehicles.update_with_player_pose(&renderer, &world, pose);
        p += offset;
        let mut ocean = Mesh::default();
        ocean.quad(
            [[-0.4, -0.4], [0.4, -0.4], [0.4, 0.4], [-0.4, 0.4]]
                .map(|[x, y]| [p.x + x, p.y + y, p.z]),
            [0.025, 0.15, 0.2],
            SurfaceKind::Water,
        );
        let ocean = renderer.upload(&ocean);
        let eye = p + Vec3::new(-0.015, -0.017, 0.068);
        let capture = |meshes: &[&GpuMesh]| {
            let png = renderer
                .report_screenshot(meshes, eye, p, 1., hermes.minimap_view([p.x, p.y], 0.))
                .unwrap();
            let mut reader = png::Decoder::new(png.as_slice()).read_info().unwrap();
            let mut pixels = vec![0; reader.output_buffer_size()];
            reader.next_frame(&mut pixels).unwrap();
            pixels
        };
        let masked = capture(&[&ocean]);
        let empty = capture(&[]);
        renderer.local_lights.hulls.set(Default::default());
        let water = capture(&[&ocean]);
        let pixel = |image: &[u8], x: usize, y: usize| -> [u8; 4] {
            let offset: usize = 4 * (640 * y + x);
            image[offset..offset + 4].try_into().unwrap()
        };
        for (x, y) in [(318, 238), (322, 242)] {
            assert_eq!(
                pixel(&masked, x, y),
                pixel(&empty, x, y),
                "wet interior of {}",
                boat.config.name
            );
            assert_ne!(pixel(&masked, x, y), pixel(&water, x, y));
        }
        assert_eq!(
            pixel(&masked, 100, 100),
            pixel(&water, 100, 100),
            "open water outside hull"
        );
    }
}
