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
    let mut riders = super::pedestrian_scenery::PedestrianScenery::new(&renderer);
    riders.update(&renderer, &world);
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
                &without_spokes
                    .iter()
                    .chain(riders.visible())
                    .collect::<Vec<_>>(),
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

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_bicycle_rider_pedaling_and_steering_poses() {
    let hermes = Hermes::test_map();
    let mut world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 1400, 1100)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let mut vehicles = VehicleScenery::new(&renderer);
    for (name, phase, steer, standing) in [
        ("rider-straight", 0., 0., 0.),
        ("rider-left", 1.57, 0.55, 0.),
        ("rider-right", 3.14, -0.55, 0.),
        ("rider-pedal-up", 4.71, 0., 0.),
        ("rider-standing", 1.57, 0., 1.),
        ("rider-standing-turn", 3.14, 0.55, 1.),
    ] {
        world.preview_bicycle_pose(phase, steer, standing);
        vehicles.update(&renderer, &world);
        let p = world.player_position() / METERS_PER_UNIT;
        let yaw = world.player().yaw;
        let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        let left = Vec3::new(-forward.y, forward.x, 0.);
        let png = renderer
            .report_screenshot(
                &vehicles.visible().collect::<Vec<_>>(),
                p + forward * 0.022 + left * 0.030 + Vec3::Z * 0.012,
                p + Vec3::Z * 0.007,
                0.,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        if let Some(folder) = std::env::var_os("BICYCLE_CAPTURE") {
            std::fs::write(
                std::path::Path::new(&folder).join(format!("{name}.png")),
                png,
            )
            .unwrap();
        }
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_bicycle_npcs_on_lane_and_regular_street() {
    let mut hermes=Hermes::test_map_from_source("road cycle: sidewalk bikelane carlane middle carlane bikelane sidewalk; end road street: sidewalk carlane middle carlane sidewalk; end a=map_point(-1,0); b=map_point(0,0); c=map_point(1,0); first=cycle(a,-0.67,0,-0.33,0,b); second=street(b,0.33,0,0.67,0,c); land(-2,-1,2,-1,2,1,-2,1); player_start(-0.9,0);");
    let mut world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 1400, 1000)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let mut vehicles = VehicleScenery::new(&renderer);
    let (ground, detail) = hermes.take_geometry();
    let ground = renderer.upload(&ground);
    let detail = renderer.upload(&detail);
    for (label, seconds) in [("npc-bike-lane", 0.), ("npc-regular-street", 30.)] {
        for _ in 0..(seconds * 60.) as usize {
            world.step(&hermes, Default::default(), Default::default(), 1. / 60.);
        }
        vehicles.update(&renderer, &world);
        let actor = world
            .pedestrian_instances()
            .find(|i| {
                i.pose.is_some()
                    && i.model < 6
                    && (label == "npc-bike-lane" || i.transform.w_axis.x > 0.1)
            })
            .expect("visible NPC cyclist");
        let p = actor.transform.w_axis.truncate();
        let mut meshes = vec![&ground, &detail];
        meshes.extend(vehicles.visible());
        let png = renderer
            .report_screenshot(
                &meshes,
                p + Vec3::new(0.025, -0.035, 0.019),
                p + Vec3::Z * 0.009,
                0.,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        if let Some(folder) = std::env::var_os("BICYCLE_CAPTURE") {
            std::fs::write(
                std::path::Path::new(&folder).join(format!("{label}.png")),
                png,
            )
            .unwrap();
        }
    }
}
