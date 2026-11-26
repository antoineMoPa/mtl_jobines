use super::{render::Renderer, vehicle_scenery::VehicleScenery};
use crate::{
    dynamic_world::{Controls, DynamicWorld, WalkControls, METERS_PER_UNIT},
    hermes::Hermes,
    season::Season,
};
use glam::Vec3;
#[test]
#[ignore = "requires a native GPU; SKI_SET_CAPTURE selects the output folder"]
fn native_winter_ski_set_pickup_and_worn_equipment() {
    let output = std::env::var_os("SKI_SET_CAPTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ski-set-captures"));
    std::fs::create_dir_all(&output).unwrap();
    let mut hermes = Hermes::test_map_from_source("road local:carlane;end local(-2,-1,2,-1);ground=land(-5,-5,5,-5,5,5,-5,5);elevation(ground,0.1);player_start(-0.2,0,0);pedestrian_start(0.01,0,180);ski_set(0,0,0);");
    let mut world = DynamicWorld::new(&hermes);
    world.set_season(&hermes, Season::Winter);
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    renderer.season = Season::Winter;
    renderer.winter_weather = false;
    let (terrain, details) = hermes.take_geometry();
    let ground = renderer.upload(&terrain);
    let details = renderer.upload(&details);
    let mut scenery = VehicleScenery::new(&renderer);
    for (label, frames) in [("pickup", 0), ("worn", 0), ("walking", 90), ("removed", 0)] {
        if label == "worn" || label == "removed" {
            assert!(world.toggle_skis());
        }
        for _ in 0..frames {
            world.step(
                &hermes,
                Controls::default(),
                WalkControls {
                    forward: 1.,
                    ..Default::default()
                },
                1. / 60.,
            );
        }
        scenery.update(&renderer, &world);
        let mut moving = opencity::runtime::Mesh::default();
        world.write_moving_mesh(&mut moving);
        let equipment = renderer.upload_dynamic(&moving);
        let mut meshes = vec![&ground, &details, &equipment];
        meshes.extend(scenery.visible());
        let p = world.player_position() / METERS_PER_UNIT;
        let png = renderer
            .report_screenshot(
                &meshes,
                p + Vec3::new(-0.036, -0.044, 0.023),
                p - Vec3::Z * 0.004,
                0.,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        std::fs::write(output.join(format!("skis-{label}.png")), png).unwrap();
    }
}

#[test]
#[ignore = "requires native GPU; SKI_DYNAMICS_CAPTURE selects output directory"]
fn native_ski_dynamics_turn_balance_tuck_and_hockey_stop() {
    let output = std::env::var_os("SKI_DYNAMICS_CAPTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("ski-dynamics-captures"));
    std::fs::create_dir_all(&output).unwrap();
    let mut h=Hermes::test_map_from_source("road local:carlane;end local(-5,-2,5,-2);ground=land(-5,-5,5,-5,5,5,-5,5);elevation(ground,0.1);player_start(-3,-2,0);pedestrian_start(0,0,0);ski_set(0,0,0);");
    let mut world = DynamicWorld::new(&h);
    world.set_season(&h, Season::Winter);
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 900)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    renderer.season = Season::Winter;
    renderer.winter_weather = false;
    let (ground, details) = h.take_geometry();
    let ground = renderer.upload(&ground);
    let details = renderer.upload(&details);
    let mut scenery = VehicleScenery::new(&renderer);
    for (label, input, time) in [
        ("cruise", WalkControls::default(), 0.1),
        (
            "left",
            WalkControls {
                turn: 1.,
                ..Default::default()
            },
            0.7,
        ),
        (
            "right",
            WalkControls {
                turn: -1.,
                ..Default::default()
            },
            0.7,
        ),
        (
            "tuck-left",
            WalkControls {
                turn: 1.,
                run: true,
                ..Default::default()
            },
            0.8,
        ),
        (
            "hockey",
            WalkControls {
                turn: 1.,
                jump: true,
                ..Default::default()
            },
            0.4,
        ),
    ] {
        world.prepare_skiing_run(&h, glam::Vec2::ZERO, 0., 10.);
        for _ in 0..(time * 120.) as usize {
            world.step(&h, Controls::default(), input, 1. / 120.);
        }
        scenery.update(&renderer, &world);
        let mut moving = opencity::runtime::Mesh::default();
        world.write_moving_mesh(&mut moving);
        let equipment = renderer.upload_dynamic(&moving);
        let mut meshes = vec![&ground, &details, &equipment];
        meshes.extend(scenery.visible());
        let p = world.player_position() / METERS_PER_UNIT;
        let yaw = world.player_yaw();
        let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.);
        let eye = p - forward * 0.035 + right * 0.012 + Vec3::Z * 0.009;
        let png = renderer
            .report_screenshot(
                &meshes,
                eye,
                p - Vec3::Z * 0.001,
                0.,
                h.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        std::fs::write(output.join(format!("game-skis-{label}.png")), png).unwrap();
    }
}
