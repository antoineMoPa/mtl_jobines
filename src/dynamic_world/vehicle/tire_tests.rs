use super::{tire::Wheel, *};
use crate::dynamic_world::{catalog, DynamicWorld, ShotSurface};
use crate::hermes::Hermes;

fn parking_lot() -> Hermes {
    Hermes::test_map_from_source(
        "pad=concrete(-2,-2,2,-2,2,2,-2,2); elevation(pad,0.1);
         player_start(-0.6,0,0); parked_vehicle(\"vehicle-v2-09\",0,0,0);",
    )
}

#[test]
fn exposed_tires_pop_once_and_stay_flat_when_boarded() {
    let map = parking_lot();
    let mut world = DynamicWorld::new(&map);
    for wheel in Wheel::ALL {
        let car = &world.vehicles[1];
        let center = car.position + car.orientation() * wheel.contact(car);
        let radius = car.definition().unwrap().config.physics.wheel_radius;
        let side = (center.y - car.position.y).signum() * Vec3::Y;
        for (height, damage, expect_pop) in [(0.13, 1, true), (0.48, 2, false)] {
            // Low on the sidewall, below the hub and within the wheel arch.
            let end = center + Vec3::Z * radius * height - side;
            let hand = end + side * 5.;
            let aim = world.weapon_aim_point(map.city(), hand + Vec3::Z * 0.05, end);
            let hit = world.fire_weapon_at(map.city(), hand, aim, damage).unwrap();
            assert_eq!(hit.surface, ShotSurface::Rubber, "{wheel:?}");
            let slot = (world.sound_serial as usize - 1) % world.sound_impacts.len();
            assert_eq!(world.sound_impacts[slot].broken, expect_pop);
            assert!(world.vehicles[1].is_flat(wheel));
        }
    }
    assert!(world.toggle_vehicle(&map));
    world.pedestrian.as_mut().unwrap().position = world.vehicles[1].position + Vec3::Y * 1.5;
    assert!(world.toggle_vehicle(&map));
    assert_eq!(world.player().flat_tires, [true; 4]);
    assert!(world.toggle_vehicle(&map));
    assert!(world.toggle_vehicle(&map));
    assert_eq!(world.player().flat_tires, [true; 4]);
}

#[test]
fn near_body_and_hub_shield_tires() {
    let map = parking_lot();
    let mut world = DynamicWorld::new(&map);
    let car = &world.vehicles[1];
    let ground = car.position + Wheel::FrontLeft.contact(car);
    let radius = car.definition().unwrap().config.physics.wheel_radius;
    let hub = ground + Vec3::Z * radius;
    let hand = hub + Vec3::Y * 5.;
    let hit = world.fire_weapon_at(map.city(), hand, hub, 2).unwrap();
    assert_eq!(hit.surface, ShotSurface::Metal);
    // Park a second car across the ray with its door between the hand and tire.
    world.vehicles[0].position = world.vehicles[1].position + Vec3::new(ground.x, 3., 0.);
    world.vehicles[0].yaw = 0.;
    let end = ground + Vec3::Z * radius * 0.15;
    let hit = world
        .fire_weapon_at(map.city(), end + Vec3::new(0., 6., 1.2), end, 2)
        .unwrap();
    assert_eq!(hit.surface, ShotSurface::Metal);
    assert!(hit.point.y > world.vehicles[0].position.y);
    assert_eq!(world.vehicles[1].flat_tires, [false; 4]);
}

#[test]
fn single_flat_settles_pulls_and_brakes_on_level_ground() {
    let map = parking_lot();
    let mut results = vec![];
    for damage in [None, Some(Wheel::FrontLeft), Some(Wheel::FrontRight)] {
        let mut world = DynamicWorld::new(&map);
        let car = &mut world.vehicles[1];
        if let Some(wheel) = damage {
            car.pop_tire(wheel);
        }
        let ground = car.position.z - car.ride_height();
        for _ in 0..240 {
            car.step(map.city(), Controls::default(), 1. / 120.);
        }
        if let Some(wheel) = damage {
            let mut hub = wheel.contact(car);
            hub.z = -car.ride_height();
            let corner_z = (car.position + car.orientation() * hub).z;
            assert!(
                corner_z < ground - 0.015,
                "{wheel:?} drop {}",
                ground - corner_z
            );
        }
        for _ in 0..240 {
            car.step(
                map.city(),
                Controls {
                    throttle: 1.,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        let speed = car.velocity.length();
        assert!(speed > 5.);
        results.push((car.yaw, speed));
        for _ in 0..360 {
            car.step(
                map.city(),
                Controls {
                    brake: true,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        assert!(car.velocity.length() < 0.3);
        assert!(car.grounded && car.roll.abs() < 0.2);
    }
    let [(straight, full), (left, flat), (right, _)] = results[..] else {
        unreachable!()
    };
    assert!(
        straight.abs() < 0.002 && left > straight + 0.015 && right < straight - 0.015,
        "headings {results:?}"
    );
    assert!(flat < full);
}

#[test]
fn road_wheel_layout_excludes_bikes_rail_and_extra_axles() {
    for def in catalog::definitions() {
        let four_wheel_road_model = matches!(
            def.config.research_id.as_str(),
            "vehicle-v2-01"
                | "vehicle-v2-09"
                | "vehicle-12"
                | "vehicle-j01"
                | "vehicle-j02"
                | "vehicle-j05"
                | "vehicle-j06"
        );
        assert_eq!(
            def.road_wheels.is_some(),
            four_wheel_road_model,
            "{}",
            def.config.name
        );
    }
}

#[test]
fn every_supported_model_has_four_exposed_sidewalls() {
    let map = parking_lot();
    let mut world = DynamicWorld::new(&map);
    for (id, def) in catalog::definitions().iter().enumerate() {
        let Some(parts) = def.road_wheels else {
            continue;
        };
        world.vehicles[1].set_catalog_model(Some(id));
        world.vehicles[1].yaw = 0.42;
        for wheel in Wheel::ALL {
            let transforms = def.part_transforms(&world.vehicles[1]);
            let transform = transforms[parts[wheel as usize]];
            let rubber = transform
                .transform_point3(Vec3::NEG_Z * def.config.physics.wheel_radius * 0.83)
                * METERS_PER_UNIT;
            let side = if matches!(wheel, Wheel::FrontLeft | Wheel::RearLeft) {
                1.
            } else {
                -1.
            };
            let approach = world.vehicles[1].orientation() * Vec3::Y * side;
            let start = rubber + approach * 4.;
            let end = rubber - approach;
            let aim = world.weapon_aim_point(map.city(), start, end);
            let hit = world.fire_weapon_at(map.city(), start, aim, 2);
            assert!(
                world.vehicles[1].is_flat(wheel),
                "{} {wheel:?} {hit:?}",
                def.config.name
            );
            let after = def.part_transforms(&world.vehicles[1]);
            for (index, (intact, damaged)) in transforms.iter().zip(&after).enumerate() {
                if index == parts[wheel as usize] {
                    assert!(damaged.z_axis.z < intact.z_axis.z * 0.8);
                } else {
                    assert_eq!(intact, damaged);
                }
            }
        }
    }
}
