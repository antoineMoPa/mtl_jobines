//! Native integration check for the production bicycle mesh, texture and shadows.
use super::{render::Renderer, vehicle_scenery::VehicleScenery};
use crate::{
    dynamic_world::{catalog, DynamicWorld, METERS_PER_UNIT},
    hermes::Hermes,
};
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_bicycle_default_vehicle_and_transparent_spokes() {
    let hermes =
        Hermes::test_map_from_source(include_str!("../../tests/fixtures/local_lights.road"));
    let world = DynamicWorld::new(&hermes);
    assert!(world.player().is_bicycle());
    let mut renderer = pollster::block_on(Renderer::new(None, 1200, 900)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let mut vehicles = VehicleScenery::new(&renderer);
    vehicles.update(&renderer, &world);
    let meshes: Vec<_> = vehicles.visible().collect();
    let definition = catalog::definitions()
        .iter()
        .find(|d| d.is_bicycle())
        .unwrap();
    let without_spokes: Vec<_> = definition
        .part_vertices()
        .zip(definition.part_transforms(world.player()))
        .map(|(vertices, transform)| {
            let vertices: Vec<_> = vertices
                .chunks_exact(3)
                .filter(|t| t[0].surface[0] >= 0.)
                .flatten()
                .copied()
                .collect();
            let mut mesh = renderer.upload_instanced(&vertices);
            let mut instance = transform.to_cols_array_2d();
            instance[1][3] = 1.;
            renderer.update_instances(&mut mesh, &[instance]);
            mesh
        })
        .collect();
    let p = world.player_position() / METERS_PER_UNIT;
    let yaw = world.player().yaw;
    let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
    let left = Vec3::new(-forward.y, forward.x, 0.);
    for (name, eye) in [
        (
            "game-bike-left",
            p + forward * 0.023 + left * 0.033 + Vec3::Z * 0.016,
        ),
        (
            "game-bike-right",
            p + forward * 0.023 - left * 0.033 + Vec3::Z * 0.016,
        ),
    ] {
        let png = renderer
            .report_screenshot(
                &meshes,
                eye,
                p + Vec3::Z * 0.003,
                0.,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        let baseline = renderer
            .report_screenshot(
                &without_spokes.iter().collect::<Vec<_>>(),
                eye,
                p + Vec3::Z * 0.003,
                0.,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        let pixels = |bytes: &[u8]| {
            let mut decoder = png::Decoder::new(bytes).read_info().unwrap();
            let mut pixels = vec![0; decoder.output_buffer_size()];
            decoder.next_frame(&mut pixels).unwrap();
            pixels
        };
        let a = pixels(&png);
        let b = pixels(&baseline);
        let changed = a
            .chunks_exact(4)
            .zip(b.chunks_exact(4))
            .filter(|(a, b)| (0..3).map(|i| a[i].abs_diff(b[i]) as u32).sum::<u32>() > 30)
            .count();
        if let Some(folder) = std::env::var_os("BICYCLE_CAPTURE") {
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                std::path::Path::new(&folder).join(format!("{name}.png")),
                &png,
            )
            .unwrap();
            std::fs::write(
                std::path::Path::new(&folder).join(format!("{name}-without-spokes.png")),
                &baseline,
            )
            .unwrap();
        }
        assert!(
            (10..4000).contains(&changed),
            "spokes must show thin lines, not opaque cards: {changed} changed pixels"
        );
    }
}
