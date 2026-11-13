use super::*;
use crate::{
    dynamic_world::{Controls, METERS_PER_UNIT},
    hermes::Hermes,
};
use glam::Vec3;

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
    let capture = |vehicles: &VehicleScenery, world: &DynamicWorld, time: f32, label: &str| {
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
    let red = capture(&vehicles, &world, 0., "police-red-phase");
    let blue = capture(&vehicles, &world, 0.2, "police-blue-phase");
    let bright = |data: &[u8], channel: usize| {
        data.chunks_exact(4)
            .filter(|p| {
                p[channel] > 180
                    && u16::from(p[channel]) > u16::from(p[1]) * 3 / 2
                    && u16::from(p[channel]) > u16::from(p[2 - channel]) * 2
            })
            .count()
    };
    assert!(
        bright(&red, 0) > bright(&blue, 0) + 4,
        "red roof lens did not pulse"
    );
    assert!(
        bright(&blue, 2) > bright(&red, 2) + 4,
        "blue roof lens did not pulse"
    );
    let taxi = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-j01")
        .unwrap();
    world.preview_vehicle_lamps(taxi, Controls::default());
    vehicles.update(&renderer, &world);
    capture(&vehicles, &world, 0., "taxi-roof-sign");
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
        frames.push(capture(&vehicles, &world, 0., &format!("suv-paint-{name}")));
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
