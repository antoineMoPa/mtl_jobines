use super::{render::Renderer, vehicle_scenery::VehicleScenery};
use crate::{
    dynamic_world::{catalog, Controls, DynamicWorld, METERS_PER_UNIT},
    hermes::Hermes,
};
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU adapter"]
fn aircraft_position_lights_visible_at_night() {
    let hermes =
        Hermes::test_map_from_source(include_str!("../../tests/fixtures/local_lights.road"));
    let mut world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 1200, 800)).unwrap();
    renderer.set_antialiasing(true);
    let mut vehicles = VehicleScenery::new(&renderer);

    for id in ["vehicle-air-estuaire", "vehicle-air-fleche"] {
        let index = catalog::definitions()
            .iter()
            .position(|def| def.config.research_id == id)
            .unwrap();
        world.preview_vehicle_lamps(index, Controls::default());
        vehicles.update(&renderer, &world);
        let plane = world.player();
        let center = plane.position / METERS_PER_UNIT;
        let length = plane.length() / METERS_PER_UNIT;
        let forward = Vec3::new(plane.yaw.cos(), plane.yaw.sin(), 0.);
        let port = Vec3::new(-forward.y, forward.x, 0.);
        let mut beacon_off_pixels = 0;
        for (label, daylight, offset, target) in [
            (
                "day",
                1.,
                forward * 0.85 + port * 0.58 + Vec3::Z * 0.32,
                Vec3::ZERO,
            ),
            (
                "nose",
                1.,
                forward * 0.69 + port * 0.26 + Vec3::Z * 0.16,
                forward * 0.32,
            ),
            ("night", 0., forward * 0.85 + Vec3::Z * 0.32, Vec3::ZERO),
            ("beacon", 0., forward * 0.85 + Vec3::Z * 0.32, Vec3::ZERO),
        ] {
            renderer.daylight = Some(daylight);
            renderer.sky_settings.solar_hour = Some(if daylight > 0. { 12. } else { 0. });
            let png = renderer
                .report_screenshot(
                    &vehicles.visible().collect::<Vec<_>>(),
                    center + length * offset,
                    center + length * target,
                    if label == "beacon" { 0.5 / 0.85 } else { 0. },
                    hermes.minimap_view([center.x, center.y], 0.),
                )
                .unwrap();
            if let Some(folder) = std::env::var_os("AIRCRAFT_CAPTURE") {
                std::fs::write(
                    std::path::Path::new(&folder).join(format!("{id}-{label}.png")),
                    &png,
                )
                .unwrap();
            }
            if daylight == 0. {
                let mut decoder = png::Decoder::new(png.as_slice()).read_info().unwrap();
                let mut rgba = vec![0; decoder.output_buffer_size()];
                decoder.next_frame(&mut rgba).unwrap();
                let beacon_pixels = rgba
                    .chunks_exact(4)
                    .enumerate()
                    .filter(|(i, p)| {
                        (560..640).contains(&(i % 1200)) && p[0] > 180 && p[1] < 110 && p[2] < 110
                    })
                    .count();
                if label == "night" {
                    beacon_off_pixels = beacon_pixels;
                } else {
                    assert!(
                        beacon_pixels > beacon_off_pixels,
                        "{id} dorsal beacon did not flash"
                    );
                }
                // Seen nose-on, port is on the viewer's right and starboard on the left.
                for (left, channel) in [(true, 1usize), (false, 0usize)] {
                    let lit = rgba
                        .chunks_exact(4)
                        .enumerate()
                        .filter(|(i, p)| {
                            let x = i % 1200;
                            let in_wing = if left { x < 500 } else { x > 700 };
                            in_wing
                                && p[channel] > 180
                                && p[channel].saturating_sub(p[1 - channel].max(p[2])) > 70
                        })
                        .count();
                    assert!(
                        lit >= 4,
                        "{id} has only {lit} bright wingtip pixels on left={left}"
                    );
                }
            }
        }
    }
}
