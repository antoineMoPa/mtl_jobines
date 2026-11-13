use super::{render::Renderer, vehicle_scenery::VehicleScenery};
use crate::{
    dynamic_world::{catalog, Controls, DynamicWorld, METERS_PER_UNIT},
    hermes::Hermes,
};
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_collecteur_body_wheels_and_lamps() {
    let hermes =
        Hermes::test_map_from_source(include_str!("../../tests/fixtures/local_lights.road"));
    let mut world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 1200, 800)).unwrap();
    let model = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-j05")
        .unwrap();
    let mut vehicles = VehicleScenery::new(&renderer);
    let mut night_frames = Vec::new();
    for (label, night, rear, controls) in [
        ("collecteur-front", false, false, Controls::default()),
        ("collecteur-rear", false, true, Controls::default()),
        ("collecteur-tail", true, true, Controls::default()),
        (
            "collecteur-brake",
            true,
            true,
            Controls {
                brake: true,
                ..Default::default()
            },
        ),
        (
            "collecteur-reverse",
            true,
            true,
            Controls {
                throttle: -1.,
                ..Default::default()
            },
        ),
    ] {
        world.preview_vehicle_lamps(model, controls);
        renderer.daylight = Some(if night { 0. } else { 1. });
        renderer.sky_settings.solar_hour = Some(if night { 0. } else { 12. });
        vehicles.update(&renderer, &world);
        let mut moving = opencity::runtime::Mesh::default();
        world.write_moving_mesh(&mut moving);
        let lamps = renderer.upload_dynamic(&moving);
        let mut meshes: Vec<_> = vehicles.visible().collect();
        meshes.push(&lamps);
        let p = world.player_position() / METERS_PER_UNIT;
        let yaw = world.player().yaw;
        let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        let right = Vec3::new(-yaw.sin(), yaw.cos(), 0.);
        let eye = p + forward * if rear { -0.105 } else { 0.105 } + right * 0.085 + Vec3::Z * 0.065;
        let png = renderer
            .report_screenshot(
                &meshes,
                eye,
                p + Vec3::Z * 0.012,
                0.,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        if let Some(folder) = std::env::var_os("COLLECTEUR_CAPTURE") {
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                std::path::Path::new(&folder).join(format!("{label}.png")),
                &png,
            )
            .unwrap();
        }
        if night {
            let mut decoder = png::Decoder::new(png.as_slice()).read_info().unwrap();
            let mut pixels = vec![0; decoder.output_buffer_size()];
            decoder.next_frame(&mut pixels).unwrap();
            night_frames.push(pixels);
        }
    }
    let brighter_red = night_frames[0]
        .chunks_exact(4)
        .zip(night_frames[1].chunks_exact(4))
        .filter(|(a, b)| u16::from(b[0]) > u16::from(a[0]) + 5 && b[0] > b[1])
        .count();
    assert!(
        brighter_red > 20,
        "refuse truck brake lenses did not brighten: {brighter_red}"
    );
    assert_ne!(
        night_frames[0], night_frames[2],
        "refuse truck reverse lenses did not change"
    );
}
