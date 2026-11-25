//! Native renderer proof of the paired snowmobiles and Alpine articulation.
use super::{render::Renderer, vehicle_scenery::VehicleScenery};
use crate::{
    dynamic_world::{catalog, Controls, DynamicWorld, METERS_PER_UNIT},
    hermes::Hermes,
    season::Season,
};
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU; ALPINE_CAPTURE selects the output folder"]
fn native_alpine_doors_and_tracks() {
    let output = std::env::var_os("ALPINE_CAPTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("alpine-captures"));
    std::fs::create_dir_all(&output).unwrap();
    let mut hermes = Hermes::test_map_from_source(
        "road local: carlane; end local(-3,-2,3,-2); ground=land(-10,-10,10,-10,10,10,-10,10); elevation(ground,0.1); player_start(0,0,0); parked_vehicle(\"vehicle-winter-snowmobile\",0,0.05,0);"
    );
    let mut world = DynamicWorld::new(&hermes);
    world.set_season(&hermes, Season::Winter);
    let model = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == catalog::ALPINE_ID)
        .unwrap();
    assert!(world.switch_vehicle(model));
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    renderer.season = Season::Winter;
    renderer.winter_weather = false;
    let mut scenery = VehicleScenery::new(&renderer);
    let (terrain, details) = hermes.take_geometry();
    let ground = renderer.upload(&terrain);
    let details = renderer.upload(&details);
    for (label, frames, throttle, toggle) in [
        ("closed", 0, 0., false),
        ("open", 60, 0., true),
        ("driven", 120, 1., true),
    ] {
        if toggle {
            assert!(world.toggle_door());
        }
        for _ in 0..frames {
            world.step(
                &hermes,
                Controls {
                    throttle,
                    ..Default::default()
                },
                Default::default(),
                1. / 60.,
            );
        }
        scenery.update(&renderer, &world);
        let mut meshes = vec![&ground, &details];
        meshes.extend(scenery.visible());
        let p = world.player_position() / METERS_PER_UNIT;
        for (view, offset, look) in [
            (
                "pair",
                Vec3::new(0.095, 0.125, 0.065),
                Vec3::new(0., 0.022, 0.006),
            ),
            (
                "belt",
                Vec3::new(-0.035, 0.048, 0.012),
                Vec3::new(-0.008, 0., 0.001),
            ),
        ] {
            let png = renderer
                .report_screenshot(
                    &meshes,
                    p + offset,
                    p + look,
                    0.,
                    hermes.minimap_view([p.x, p.y], 0.),
                )
                .unwrap();
            std::fs::write(output.join(format!("alpine-{label}-{view}.png")), png).unwrap();
        }
    }
    // The original Motoneige uses the same arc-length mechanism, with its own loop size.
    let classic = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == catalog::SNOWMOBILE_ID)
        .unwrap();
    assert!(world.switch_vehicle(classic));
    for (label, frames) in [("classic-rest", 0), ("classic-driven", 90)] {
        for _ in 0..frames {
            world.step(
                &hermes,
                Controls {
                    throttle: 1.,
                    ..Default::default()
                },
                Default::default(),
                1. / 60.,
            );
        }
        scenery.update(&renderer, &world);
        let mut meshes = vec![&ground, &details];
        meshes.extend(scenery.visible());
        let p = world.player_position() / METERS_PER_UNIT;
        let png = renderer
            .report_screenshot(
                &meshes,
                p + Vec3::new(-0.024, 0.03, 0.012),
                p + Vec3::new(-0.006, 0., 0.001),
                0.,
                hermes.minimap_view([p.x, p.y], 0.),
            )
            .unwrap();
        std::fs::write(output.join(format!("{label}.png")), png).unwrap();
    }
}
