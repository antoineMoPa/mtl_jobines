//! Native GPU captures of the animated snowmobile belt.
use super::{render::Renderer, vehicle_scenery::VehicleScenery};
use crate::{
    dynamic_world::{catalog, Controls, DynamicWorld},
    hermes::Hermes,
    season::Season,
};
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU adapter; SNOWMOBILE_CAPTURE names the output directory"]
fn native_snowmobile_belt_views() {
    let output = std::env::var_os("SNOWMOBILE_CAPTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("snowmobile-belt-captures"));
    std::fs::create_dir_all(&output).unwrap();
    // Winter snowfield; the mounted start becomes the snowmobile directly.
    let mut hermes = Hermes::test_map_from_source(
        "road local: carlane; end
         local(-3,-2,3,-2); ground=land(-10,-10,10,-10,10,10,-10,10); elevation(ground,0.1);
         player_start(-1,0,90);",
    );
    let model = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == catalog::SNOWMOBILE_ID)
        .unwrap();
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    renderer.season = Season::Winter;
    let mut world = DynamicWorld::new(&hermes);
    world.set_season(&hermes, Season::Winter);
    assert!(world.switch_vehicle(model));
    let start = world.player_position();
    let mut scenery = VehicleScenery::new(&renderer);
    let (terrain, details) = hermes.take_geometry();
    let gpu_terrain = renderer.upload(&terrain);
    let gpu_details = renderer.upload(&details);
    // The snowmobile drives off across the snow; the belt visibly flows.
    for (label, frames) in [("rest", 0), ("creep", 24), ("driven", 240)] {
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
        let meshes: Vec<_> = scenery.visible().collect();
        let player = world.player_position();
        // Chase slightly behind the moving snowmobile so the belt stays framed.
        let behind =
            player + (player - start).try_normalize().unwrap_or(Vec3::X) * 260. - Vec3::Z * 110.;
        let png = renderer
            .report_screenshot(
                &[&gpu_terrain, &gpu_details]
                    .into_iter()
                    .chain(meshes.iter().copied())
                    .collect::<Vec<_>>(),
                player + Vec3::new(-180., -220., 120.),
                player - Vec3::Z * 20.,
                0.6,
                hermes.minimap_view([player.x, player.y], 0.),
            )
            .unwrap();
        std::fs::write(output.join(format!("snowmobile-belt-{label}.png")), png).unwrap();
        eprintln!("capture {}: player at {:?}", label, player);
    }
    eprintln!("captures written to {}", output.display());
}
