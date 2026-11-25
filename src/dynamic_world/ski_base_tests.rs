use super::*;
use crate::{
    dynamic_world::{DynamicWorld, METERS_PER_UNIT},
    hermes::Hermes,
};
use glam::Vec2;

#[test]
fn snowmobile_spawns_beside_the_ski_base_start() {
    let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
    let hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let start = hermes
        .walking_start()
        .or_else(|| hermes.default_start())
        .expect("authored start");
    let mut world = DynamicWorld::new(&hermes);
    // The parked snowmobile is within boarding reach of the authored start.
    let snowmobile = world
        .vehicles
        .iter()
        .find(|v| {
            v.definition
                .is_some_and(|id| definitions()[id].config.research_id == SNOWMOBILE_ID)
        })
        .expect("snowmobile parked on the current map");
    let distance = snowmobile
        .position
        .truncate()
        .distance(Vec2::new(start.point[0], start.point[1]) * METERS_PER_UNIT);
    assert!(distance < 6.5, "snowmobile {distance:.1} m from start");
    // The start stands on supported ground, not water, at the ski base.
    let spawn = world.player_position();
    let surface = hermes
        .city()
        .surface_at(
            (spawn.truncate() / METERS_PER_UNIT).to_array(),
            (spawn.z + 0.4) / METERS_PER_UNIT,
        )
        .expect("spawn surface");
    assert_ne!(surface.kind, SurfaceKind::Water);
    eprintln!(
        "start {:?} on {:?} height {}; snowmobile {distance:.1} m away",
        start.point, surface.kind, surface.height
    );
}
