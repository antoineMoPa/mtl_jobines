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

#[test]
fn both_snowmobiles_park_nearby_on_clear_ski_base_ground() {
    use crate::dynamic_world::DynamicWorld;
    let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
    let hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let world = DynamicWorld::new(&hermes);
    let find = |id| {
        world
            .vehicles
            .iter()
            .enumerate()
            .find(|(_, v)| v.definition().is_some_and(|d| d.config.research_id == id))
            .unwrap()
    };
    let (small_slot, small) = find(SNOWMOBILE_ID);
    let (large_slot, large) = find(ALPINE_ID);
    assert!((small.position.distance(large.position) - 5.).abs() < 0.05);
    assert!(
        large.definition().unwrap().config.dimensions.length
            > small.definition().unwrap().config.dimensions.length * 1.5
    );
    for (slot, vehicle) in [(small_slot, small), (large_slot, large)] {
        assert!(world.clear_vehicle_position(hermes.city(), vehicle, Some(slot)));
        assert!(vehicle.grounded);
    }
}

#[test]
fn alpine_doors_open_close_and_the_vehicle_can_be_boarded() {
    use crate::dynamic_world::{doors::Door, DynamicWorld, VehicleId};
    let hermes = Hermes::test_map_from_source(
        "road local: carlane; end local(-3,-2,3,-2); ground=land(-10,-10,10,-10,10,10,-10,10); elevation(ground,0.1); player_start(-0.2,0,0); parked_vehicle(\"vehicle-winter-alpine-340\",0,0,180);"
    );
    let mut world = DynamicWorld::new(&hermes);
    assert!(world.toggle_vehicle(&hermes));
    let car = &world.vehicles[1];
    let access = car.access_point(Door::FrontLeft).unwrap();
    world.pedestrian.as_mut().unwrap().position = access.extend(car.position.z + 0.6);
    assert!(world.toggle_vehicle(&hermes));
    assert_eq!(world.player_vehicle, VehicleId(1));
    for _ in 0..240 {
        world.step(&hermes, Default::default(), Default::default(), 1. / 60.);
    }
    let def = world.player().definition().unwrap();
    assert_eq!(def.config.research_id, ALPINE_ID);
    assert!(world.toggle_door());
    for _ in 0..60 {
        world.step(&hermes, Default::default(), Default::default(), 1. / 60.);
    }
    assert!(world.player().doors.fraction(Door::FrontLeft) > 0.99);
    assert!(world.toggle_door());
    for _ in 0..60 {
        world.step(&hermes, Default::default(), Default::default(), 1. / 60.);
    }
    assert_eq!(world.player().doors.fraction(Door::FrontLeft), 0.);
    for door in [Door::FrontLeft, Door::FrontRight] {
        let car = &mut world.vehicles[1];
        let shut = def.part_transforms(car);
        car.doors.set_fraction(door, 1.);
        let open = def.part_transforms(car);
        assert_eq!(shut[0], open[0], "fixed cabin stays put");
        let moving = def
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| matches!(p.role, PartRole::Door(d, _) if d == door))
            .collect::<Vec<_>>();
        assert!(!moving.is_empty());
        for (i, _) in moving {
            assert_ne!(shut[i], open[i]);
        }
        car.doors.set_fraction(door, 0.);
        assert_eq!(shut, def.part_transforms(car));
    }
}

#[test]
fn both_belts_follow_signed_distance_and_match_the_rendered_mesh() {
    let hermes = Hermes::test_map_from_source(
        "ground=land(-10,-10,10,-10,10,10,-10,10); elevation(ground,0.1);",
    );
    for id in [SNOWMOBILE_ID, ALPINE_ID] {
        let index = definitions()
            .iter()
            .position(|d| d.config.research_id == id)
            .unwrap();
        let def = &definitions()[index];
        let mut v = Vehicle::spawn_at(hermes.city(), "0,0,0".parse().unwrap()).unwrap();
        v.set_catalog_model(Some(index));
        let shoes: Vec<_> = def
            .parts
            .iter()
            .enumerate()
            .filter(|(_, p)| matches!(p.role, PartRole::TrackShoe(_)))
            .collect();
        assert_eq!(shoes.len(), if id == ALPINE_ID { 112 } else { 88 });
        let rest = def.part_transforms(&v);
        for travel in [-0.037, 0.037, 0.71, 2.9] {
            v.wheel_angle = travel / v.definition().unwrap().config.physics.wheel_radius;
            let transforms = def.part_transforms(&v);
            assert_eq!(transforms[0], rest[0]);
            for &(i, _) in &shoes {
                assert_ne!(transforms[i], rest[i]);
            }
            assert_eq!(
                transforms,
                def.part_transforms(&v),
                "stopped belts hold their phase"
            );
            let cpu = def.mesh(&v);
            let mut offset = 0;
            for (i, p) in def.parts.iter().enumerate() {
                for (source, actual) in p.vertices.iter().zip(&cpu.vertices[offset..]) {
                    let expected = transforms[i].transform_point3(source.position.into());
                    assert!(expected.distance(Vec3::from(actual.position)) < 0.00001);
                }
                offset += p.vertices.len();
            }
        }
        v.wheel_angle = 0.;
        assert_eq!(rest, def.part_transforms(&v));
        let (i, _) = shoes[0];
        let point = Vec3::ZERO;
        let x = rest[i].transform_point3(point).x;
        v.wheel_angle = 0.037 / v.definition().unwrap().config.physics.wheel_radius;
        assert!(
            def.part_transforms(&v)[i].transform_point3(point).x < x,
            "bottom run goes backward as vehicle advances"
        );
    }
}

#[test]
fn alpine_drives_on_snow_and_crawls_on_summer_grass() {
    let hermes = Hermes::test_map_from_source("road local: carlane; end local(-3,-2,3,-2); ground=land(-10,-10,10,-10,10,10,-10,10); elevation(ground,0.1);");
    let model = definitions()
        .iter()
        .position(|d| d.config.research_id == ALPINE_ID)
        .unwrap();
    let run = |season, throttle| {
        let mut v = Vehicle::spawn_at(hermes.city(), "0,0,0".parse().unwrap()).unwrap();
        v.set_catalog_model(Some(model));
        v.season = season;
        for _ in 0..600 {
            v.step(
                hermes.city(),
                Controls {
                    throttle,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        (v.position.x, v.wheel_angle)
    };
    let (snow, spin) = run(Season::Winter, 1.);
    let (grass, _) = run(Season::Summer, 1.);
    let (reverse, reverse_spin) = run(Season::Winter, -1.);
    assert!(snow > 10. && snow > grass * 10.);
    assert!(grass > 0. && grass < 5.);
    assert!(reverse < -1. && reverse_spin < 0. && spin > 0.);
}
