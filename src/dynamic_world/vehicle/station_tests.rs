use super::*;
use crate::dynamic_world::buildings::Buildings;

fn station() -> City {
    City::from_program(
        &opencity::parse(concat!(
            "road street:carlane;end street(-2,-2,-1,-2);",
            "site=building(0.12,{collision:none},0,0,0.4,0,0.4,0.3,0,0.3);",
            "shop=building(0.05,0.05,0.23,0.35,0.23,0.35,0.3,0.05,0.3);",
            "pump=building(0.025,0.09,0.09,0.11,0.09,0.11,0.11,0.09,0.11);",
            "roof=building(0.01,0.04,0.05,0.36,0.05,0.36,0.18,0.04,0.18);",
            "elevation(roof,0.06);"
        ))
        .unwrap(),
        None,
    )
    .unwrap()
}

#[test]
fn vehicles_cross_open_forecourt_and_hit_individual_objects() {
    let city = station();
    let grid = Buildings::new(&city);
    for indexed in [false, true] {
        // Cross the site boundary, drive beneath the canopy, then hit the pump/shop.
        for (y, obstacle) in [(3., None), (15., None), (10., Some(9.)), (26., Some(5.))] {
            let mut car = Vehicle::spawn(&city);
            car.position = Vec3::new(-5., y, RIDE_HEIGHT);
            car.yaw = 0.;
            car.velocity = Vec3::X * 25.;
            let mut candidates = Vec::new();
            grid.nearby(Vec2::new(20., y), 30., &mut candidates);
            car.move_with_building_candidates(&city, 2., indexed.then_some(&candidates));
            if let Some(edge) = obstacle {
                assert!(
                    car.collisions > 0,
                    "missed object at y={y}, indexed={indexed}"
                );
                assert!(car.position.x < edge);
            } else {
                assert_eq!(
                    car.collisions, 0,
                    "blocked forecourt at y={y}, indexed={indexed}"
                );
                assert!((car.position.x - 45.).abs() < 0.001);
            }
        }
    }
}

#[test]
fn follow_camera_ignores_site_bounds_and_stops_at_individual_objects() {
    let city = station();
    let grid = Buildings::new(&city);
    let anchor = Vec3::new(-0.05, 0.15, 0.02);
    let desired = Vec3::new(0.45, 0.15, 0.02);
    assert_eq!(grid.camera_eye(&city, anchor, desired), desired);
    let pump = grid.camera_eye(&city, Vec3::new(0., 0.1, 0.02), Vec3::new(0.2, 0.1, 0.02));
    assert!((0.08..0.09).contains(&pump.x), "{pump:?}");
    let roof = grid.camera_eye(&city, anchor.with_z(0.065), desired.with_z(0.065));
    assert!((0.03..0.04).contains(&roof.x), "{roof:?}");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn canonical_station_forecourt_is_driveable_and_shop_remains_solid() {
    let map = crate::hermes::Hermes::load_with_game_data(
        &std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road"),
        None,
    )
    .unwrap();
    let city = map.city();
    let site = city
        .landscape
        .iter()
        .find(|f| f.name.as_deref() == Some("fuel_station"))
        .unwrap();
    let a = Vec2::from_array(site.points[0]);
    let width = Vec2::from_array(site.points[1]) - a;
    let depth = Vec2::from_array(site.points[2]) - Vec2::from_array(site.points[1]);
    let forward = width / width.length();
    let grid = Buildings::new(city);
    for indexed in [false, true] {
        let start = a + depth * 0.1 - forward * 0.04;
        let support = city.surface_at(start.to_array(), f32::INFINITY).unwrap();
        let mut car = Vehicle::spawn(city);
        car.position =
            (start * METERS_PER_UNIT).extend(support.height * METERS_PER_UNIT + RIDE_HEIGHT);
        car.yaw = forward.y.atan2(forward.x);
        car.velocity = (forward * 20.).extend(0.);
        let mut candidates = Vec::new();
        grid.nearby(car.position.truncate(), 65., &mut candidates);
        car.move_with_building_candidates(city, 3., indexed.then_some(&candidates));
        assert_eq!(
            car.collisions, 0,
            "site bounds blocked forecourt, indexed={indexed}"
        );
        assert!((car.position.truncate() - start * METERS_PER_UNIT).dot(forward) > 59.9);

        let shop = city
            .landscape
            .iter()
            .find(|f| f.name.as_deref() == Some("station_shop"))
            .unwrap();
        let front = (Vec2::from_array(shop.points[0]) + Vec2::from_array(shop.points[1])) * 0.5;
        let toward_shop = depth / depth.length();
        car.position = ((front - toward_shop * 0.05) * METERS_PER_UNIT).extend(car.position.z);
        car.yaw = toward_shop.y.atan2(toward_shop.x);
        car.velocity = (toward_shop * 10.).extend(0.);
        grid.nearby(car.position.truncate(), 20., &mut candidates);
        car.move_with_building_candidates(city, 1., indexed.then_some(&candidates));
        assert!(car.collisions > 0, "shop is not solid, indexed={indexed}");
        assert!((car.position.truncate() / METERS_PER_UNIT - front).dot(toward_shop) < 0.);
    }
}

#[test]
fn catalog_crash_releases_only_the_struck_end_and_keeps_debris_in_world_space() {
    use super::damage::{Piece, Zone};
    use crate::dynamic_world::catalog;
    let city = station();
    for id in ["vehicle-v2-09", "vehicle-v2-01", "vehicle-12"] {
        let index = catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == id)
            .unwrap();
        let mut car = Vehicle::spawn(&city);
        car.set_catalog_model(Some(index));
        car.position = Vec3::new(-5., 10., car.ride_height());
        car.yaw = 0.;
        car.velocity = Vec3::X * 25.;
        car.move_with_building_candidates(&city, 0.7, None);
        let def = &catalog::definitions()[index];
        assert!(car.collisions > 0);
        assert!(car.damaged_instance(0, glam::Mat4::IDENTITY).unwrap().1 > 0);
        let released: Vec<_> = def
            .crash_pieces()
            .filter(|(i, _, _)| {
                car.damage
                    .as_ref()
                    .unwrap()
                    .loose
                    .iter()
                    .any(|p| p.part == *i)
            })
            .collect();
        assert_eq!(released.len(), 5, "{id}");
        assert!(released
            .iter()
            .all(|(_, piece, _)| piece.zone() == Zone::Nose));
        let panel = released
            .iter()
            .find(|(_, p, _)| matches!(p, Piece::Panel(_)))
            .unwrap()
            .0;
        let pose = car.damaged_instance(panel, glam::Mat4::IDENTITY).unwrap().0;
        car.position.x += 15.;
        assert_eq!(
            car.damaged_instance(panel, glam::Mat4::from_translation(car.position))
                .unwrap()
                .0,
            pose
        );
        car.damage.as_mut().unwrap().tick(0.2);
        assert_ne!(
            car.damaged_instance(panel, glam::Mat4::IDENTITY).unwrap().0,
            pose
        );
        car.damage.as_mut().unwrap().tick(5.);
        assert!(car.damaged_instance(panel, glam::Mat4::IDENTITY).is_none());
        car.set_catalog_model(Some(index));
        assert!(car.damage.is_none());
    }
}
