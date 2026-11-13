use super::*;
use crate::{
    dynamic_world::{Controls, DynamicWorld, RearLight, METERS_PER_UNIT},
    engine::{render::Renderer, scenery::Scenery, vehicle_scenery::VehicleScenery},
    hermes::{Hermes, RouteSignal},
};
use opencity::runtime::{Mesh, SurfaceKind};

fn capture(
    r: &Renderer,
    meshes: &[&crate::engine::render::GpuMesh],
    eye: Vec3,
    look: Vec3,
    hermes: &Hermes,
    name: &str,
) -> Vec<u8> {
    let png = r
        .report_screenshot(meshes, eye, look, 0., hermes.minimap_view([0.; 2], 0.))
        .unwrap();
    if let Some(dir) = std::env::var_os("LOCAL_LIGHT_CAPTURE") {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), &png).unwrap();
    }
    let mut decoder = png::Decoder::new(png.as_slice()).read_info().unwrap();
    let mut pixels = vec![0; decoder.output_buffer_size()];
    decoder.next_frame(&mut pixels).unwrap();
    pixels
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_daylight_extinguishes_all_projected_lights() {
    let hermes =
        Hermes::test_map_from_source(include_str!("../../../../tests/fixtures/local_lights.road"));
    let mut r = pollster::block_on(Renderer::new(None, 640, 400)).unwrap();
    r.construction_span = Some(0.5);
    let mut mesh = Mesh::default();
    mesh.quad(
        [[-1., -1., 0.], [1., -1., 0.], [1., 1., 0.], [-1., 1., 0.]],
        [0.35; 3],
        SurfaceKind::Asphalt,
    );
    let mesh = r.upload(&mesh);
    let lights = vec![
        LocalLight::streetlamp(Vec3::new(0., 0., 0.055)),
        LocalLight::headlight(Vec3::new(-0.12, 0., 0.008), glam::Quat::IDENTITY, 1.),
        LocalLight::rear(
            Vec3::new(0.1, 0., 0.008),
            glam::Quat::IDENTITY,
            RearLight::Brake,
            1.,
        ),
        LocalLight::rear(
            Vec3::new(0., 0.1, 0.008),
            glam::Quat::IDENTITY,
            RearLight::Indicator,
            1.,
        ),
        LocalLight::rear(
            Vec3::new(0., -0.1, 0.008),
            glam::Quat::IDENTITY,
            RearLight::Reverse,
            1.,
        ),
    ];
    for (hour, label) in [(1., "day"), (0., "night")] {
        r.daylight = Some(hour);
        let mut images = Vec::new();
        for (enabled, suffix) in [(false, "off"), (true, "on")] {
            r.local_lights
                .set(if enabled { lights.clone() } else { vec![] });
            images.push(capture(
                &r,
                &[&mesh],
                Vec3::new(-0.3, -0.4, 0.4),
                Vec3::ZERO,
                &hermes,
                &format!("{label}-spill-{suffix}"),
            ));
        }
        if label == "day" {
            assert_eq!(
                images[0], images[1],
                "projected light remains visible in daylight"
            );
        } else {
            assert!(
                images[0]
                    .iter()
                    .zip(&images[1])
                    .filter(|(a, b)| a != b)
                    .count()
                    > 1000
            );
        }
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_rear_lamps_and_spill_follow_braking_and_flashing() {
    let mut hermes =
        Hermes::test_map_from_source(include_str!("../../../../tests/fixtures/local_lights.road"));
    let mut world = DynamicWorld::new(&hermes);
    let mut r = pollster::block_on(Renderer::new(None, 900, 600)).unwrap();
    r.daylight = Some(0.);
    let scenery = Scenery::new(&r, &mut hermes);
    let mut vehicles = VehicleScenery::new(&r);
    let p = world.player_position() / METERS_PER_UNIT;
    let eye = p + Vec3::new(-0.10, -0.065, 0.045);
    let look = p + Vec3::new(-0.035, 0., 0.);
    let mut images = Vec::new();
    for (name, brake, signal, phase) in [
        ("rear-tail", false, RouteSignal::Off, 0.),
        ("rear-brake", true, RouteSignal::Off, 0.),
        ("rear-left-on", true, RouteSignal::Left, 0.),
        ("rear-left-off", true, RouteSignal::Left, 0.4),
        ("rear-right-on", true, RouteSignal::Right, 0.),
    ] {
        world.preview_vehicle_lamps(
            0,
            Controls {
                brake,
                ..Default::default()
            },
        );
        world.preview_vehicle_signal(signal, phase);
        vehicles.update(&r, &world);
        r.local_lights.set(
            world
                .local_lights()
                .into_iter()
                .filter(|l| l.position_range[3] < 0.1)
                .collect(),
        );
        let mut moving = Mesh::default();
        world.write_moving_mesh(&mut moving);
        let moving = r.upload_dynamic(&moving);
        let mut meshes = scenery.visible([p.x, p.y]);
        meshes.push(&moving);
        meshes.extend(vehicles.visible());
        images.push(capture(&r, &meshes, eye, look, &hermes, name));
    }
    let increased_red = images[0]
        .chunks_exact(4)
        .zip(images[1].chunks_exact(4))
        .filter(|(a, b)| u16::from(b[0]) > u16::from(a[0]) + 5 && b[0] > b[1])
        .count();
    assert!(
        increased_red > 100,
        "braking does not increase red illumination: {increased_red}"
    );
    assert_ne!(images[1], images[2], "left indicator never flashes");
    assert_eq!(
        images[1], images[3],
        "off phase does not return to brake lighting"
    );
    assert_ne!(
        images[2], images[4],
        "left and right indicators illuminate the same side"
    );
    // A point on the pavement behind an authored emitter must brighten too.
    let source = world
        .local_lights()
        .into_iter()
        .find(|l| l.position_range[3] < 0.1)
        .unwrap();
    let mut ground = source.position() - Vec3::X * 0.012;
    ground.z = hermes
        .city()
        .surface_at(ground.truncate().to_array(), p.z)
        .unwrap()
        .height
        + 0.0001;
    let clip = r.view_projection(eye, look) * ground.extend(1.);
    let ndc = clip.truncate() / clip.w;
    let x = ((ndc.x * 0.5 + 0.5) * 900.) as usize;
    let y = ((0.5 - ndc.y * 0.5) * 600.) as usize;
    let pixel = (y * 900 + x) * 4;
    assert!(
        images[1][pixel] > images[0][pixel] + 5,
        "braking only changes the lens, not the ground"
    );
}
