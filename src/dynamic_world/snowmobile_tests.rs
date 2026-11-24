use super::*;
use crate::{
    dynamic_world::{Controls, Vehicle},
    hermes::Hermes,
    season::Season,
};

fn model() -> usize {
    definitions()
        .iter()
        .position(|d| d.config.research_id == SNOWMOBILE_ID)
        .unwrap()
}

#[test]
fn snowmobile_progress_on_snow_and_grass() {
    let hermes = Hermes::test_map_from_source("road local: carlane; end\n local(-3,-2,3,-2); ground=land(-10,-10,10,-10,10,10,-10,10); elevation(ground,0.1);");
    let run = |season| {
        let mut v = Vehicle::spawn_at(hermes.city(), "0,0,0".parse().unwrap()).unwrap();
        v.set_catalog_model(Some(model()));
        v.season = season;
        for _ in 0..600 {
            v.step(
                hermes.city(),
                Controls {
                    throttle: 1.,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        (v.position.x, v.velocity.x)
    };
    let (snow_distance, snow_speed) = run(Season::Winter);
    let (grass_distance, grass_speed) = run(Season::Summer);
    eprintln!("5s drive: snow {snow_distance:.2}m / {snow_speed:.2}m/s; grass {grass_distance:.2}m / {grass_speed:.2}m/s");
    assert!(snow_speed > 10., "snow speed {snow_speed}");
    assert!(
        grass_speed > 0.05 && grass_speed < 1.,
        "grass speed {grass_speed}"
    );
    assert!(snow_distance > grass_distance * 10.);
    assert!(!definitions()[model()].config.road_vehicle);
}

#[test]
fn ski_base_uses_real_terrain_support() {
    let source = include_str!("../../data/maps/mtl/map.road");
    let program = opencity::parse(source).unwrap();
    let spec = opencity::heightmap::ElevationMapSpec::from_program(&program)
        .unwrap()
        .unwrap();
    let map = opencity::heightmap::ElevationMap::from_png(
        spec,
        include_bytes!("../../data/maps/mtl/elevation.png"),
    )
    .unwrap();
    for point in [
        [4.5, -82.],
        [4.1, -82.6],
        [4.1, -82.4],
        [4.3, -82.4],
        [4.041, -82.6525],
        [4.159, -82.5475],
    ] {
        let height = map.height_at(point).unwrap();
        eprintln!("ski terrain {point:?}: {height}");
        assert!((height - 0.100001).abs() < 0.00001);
    }
}

#[test]
fn snowmobile_can_be_entered_switched_and_changed_between_seasons() {
    use crate::dynamic_world::{DynamicWorld, VehicleId};
    let hermes = Hermes::test_map_from_source(
        "road local: carlane; end local(-1,-1,1,-1); ground=land(-2,-2,2,-2,2,2,-2,2); elevation(ground,0.1);
         player_start(-0.2,0,0); parked_vehicle(\"vehicle-winter-snowmobile\",0,0,180);"
    );
    let mut world = DynamicWorld::new(&hermes);
    let original = world.vehicles[1].position;
    assert!(world.toggle_vehicle(&hermes));
    world.pedestrian.as_mut().unwrap().position = original + glam::Vec3::Y * 1.2;
    assert!(world.toggle_vehicle(&hermes));
    assert_eq!(world.player_vehicle, VehicleId(1));
    assert_eq!(world.player().definition, Some(model()));
    assert_eq!(
        world
            .seated_drivers()
            .filter(|driver| driver.model
                == crate::dynamic_world::pedestrian_catalog::player_model(world.season()))
            .count(),
        1
    );
    assert!(!world.toggle_door());
    world.set_season(&hermes, Season::Winter);
    world.step(
        &hermes,
        Controls {
            throttle: 1.,
            ..Default::default()
        },
        Default::default(),
        1. / 60.,
    );
    assert_eq!(world.player().season, Season::Winter);
    world.set_season(&hermes, Season::Summer);
    world.step(&hermes, Controls::default(), Default::default(), 1. / 60.);
    assert_eq!(world.player().season, Season::Summer);
    world.reset_player(&hermes);
    assert!(world.player().position.distance(original) < 0.001);
    assert!(world.switch_vehicle(model()));
}
