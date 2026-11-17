use super::*;
#[test]
fn authored_metro_is_a_closed_underground_loop_with_walkable_stations_and_stairs() {
    let root = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl");
    let program = opencity::parse(include_str!("../../../data/maps/mtl/map.road")).unwrap();
    let city = City::from_program(&program, Some(&root)).unwrap();
    let map = MetroMap::from_city(&city);
    assert_eq!(map.stations.len(), 3);
    assert!(map.length > 4000. && map.length < 7000.);
    assert!(
        map.samples
            .first()
            .unwrap()
            .1
            .distance(map.samples.last().unwrap().1)
            < 0.00001
    );
    // Every highway, including the inward interchange spur, stays outside the metro path.
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    for road in city
        .roads
        .iter()
        .filter(|r| r.definition.role == opencity::RoadRole::Highway)
    {
        for i in 0..64 {
            let a = road.segment.point_at(i as f32 / 64.);
            let b = road.segment.point_at((i + 1) as f32 / 64.);
            let a = Vec2::new(a.x, a.y);
            let b = Vec2::new(b.x, b.y);
            for pair in map.samples.windows(2) {
                let c = pair[0].1.truncate();
                let d = pair[1].1.truncate();
                assert!(
                    !(cross(a, b, c) * cross(a, b, d) < 0. && cross(c, d, a) * cross(c, d, b) < 0.),
                    "metro crosses {:?}",
                    road.segment.name
                );
            }
        }
    }
    assert!(
        map.stations[0].center.y > 0.5,
        "Acadie belongs on the city side of the interstate"
    );
    assert!(
        map.stations[1].center.y > 7.,
        "Jean-Talon must be farther along the line"
    );
    assert!(
        map.stations[2].center.distance(Vec2::new(17.5, 9.35)) < 0.001,
        "Bonaventure stays in place"
    );
    for (i, station) in map.stations.iter().enumerate() {
        assert_eq!(station.id, StationId::ALL[i]);
        let (p, yaw) = map.sample(station.distance);
        assert!(p.truncate().distance(station.center) < 0.04);
        assert!(Vec2::new(yaw.cos(), yaw.sin()).dot(station.forward) > 0.99);
        for a in [-0.18, 0., 0.18] {
            let point = station.point(a, 0.06, FLOOR_HEIGHT);
            let support = city
                .surface_at(point.truncate().to_array(), FLOOR_HEIGHT + 0.006)
                .unwrap();
            assert!(
                (support.height - FLOOR_HEIGHT).abs() < 0.0001,
                "{} dock {support:?}",
                station.id.title()
            );
        }
        for i in 0..=40 {
            let t = i as f32 / 40.;
            let h = 0.102 + (FLOOR_HEIGHT - 0.102) * t;
            let point = station.point(-0.40 + t * 0.20, 0.06, h);
            let xy = point.truncate().to_array();
            let support = city.surface_at(xy, h + 0.0042).unwrap();
            assert!(
                (support.height - h).abs() < 0.001,
                "{} stair {i}: {:?} expected {h}",
                station.id.title(),
                support
            );
            assert!(
                !city.building_collision(xy, 0.004, h + 0.0005, h + 0.018),
                "{} stair {i} overlaps building at {xy:?}, height {h}",
                station.id.title()
            );
            assert!(
                city.road_barrier_contact(xy, 0.004, h + 0.0005, h + 0.018)
                    .is_none(),
                "{} stair {i} blocked",
                station.id.title()
            );
        }
        assert!(
            city.elevation
                .as_ref()
                .unwrap()
                .original_height_at(station.center.to_array())
                .unwrap()
                > TRACK_HEIGHT + 0.068,
            "station ceiling must be below the terrain"
        );
        let roof = station.point(0., 0.06, 0.102);
        assert!(
            (city
                .surface_at(roof.truncate().to_array(), roof.z + 0.001)
                .unwrap()
                .height
                - 0.102)
                .abs()
                < 0.001
        );
    }
    let mesh = map.scenery().unwrap().unwrap();
    assert!(mesh.mesh.vertices.len() > 10000);
    let layers = crate::hermes::external_models::textures::snapshot();
    assert!(
        layers
            .iter()
            .any(|l| l.normal.chunks_exact(4).any(|p| p[0] < 110 || p[1] < 110)),
        "brick normal texture must contain relief"
    );
    let svg = navigation_svg(&city).unwrap();
    for text in ["metro-line", "Acadie", "Jean-Talon", "Bonaventure"] {
        assert!(svg.contains(text));
    }
}
