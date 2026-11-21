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
        map.stations[0].center.x > 0.5,
        "Acadie belongs on the city side of the interstate"
    );
    assert!(
        map.stations[1].center.x > 7.,
        "Jean-Talon must be farther along the line"
    );
    assert!(
        map.stations[2].center.distance(Vec2::new(9.35, -17.5)) < 0.001,
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
            let point = station.point(-0.50 + t * 0.20, 0.06, h);
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
        let mut room = city
            .mesh(
                opencity::runtime::Region::radius(station.center.to_array(), 0.45).unwrap(),
                0.04,
            )
            .unwrap();
        map.open_portals(&mut room);
        for direction in [-1., 1.] {
            assert!(
                !ray_blocked(
                    &room,
                    station.point(0., 0., TRACK_HEIGHT + 0.02),
                    station.point(0.42 * direction, 0., TRACK_HEIGHT + 0.02)
                ),
                "{} tunnel portal {direction} is sealed",
                station.id.title()
            );
        }
        let roof = station.point(0., 0.06, 0.1005);
        assert!(
            (city
                .surface_at(roof.truncate().to_array(), roof.z + 0.001)
                .unwrap()
                .height
                - 0.1005)
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
            .any(|l| l.normal.chunks_exact(4).any(|p| p[0] < 123 || p[1] < 123)),
        "concrete normal texture must contain relief"
    );
    let svg = navigation_svg(&city).unwrap();
    for text in ["metro-line", "Acadie", "Jean-Talon", "Bonaventure"] {
        assert!(svg.contains(text));
    }
}

fn ray_blocked(mesh: &opencity::runtime::Mesh, from: Vec3, to: Vec3) -> bool {
    let ray = to - from;
    mesh.vertices.chunks_exact(3).any(|triangle| {
        let a = Vec3::from_array(triangle[0].position);
        let b = Vec3::from_array(triangle[1].position);
        let c = Vec3::from_array(triangle[2].position);
        let e1 = b - a;
        let e2 = c - a;
        let p = ray.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-10 {
            return false;
        }
        let t = from - a;
        let u = t.dot(p) / det;
        if !(0.0..=1.0).contains(&u) {
            return false;
        }
        let q = t.cross(e1);
        let v = ray.dot(q) / det;
        if v < 0. || u + v > 1. {
            return false;
        }
        let distance = e2.dot(q) / det;
        distance > 0. && distance < 1.
    })
}

#[test]
fn tunnel_shell_overlaps_both_station_mouths_even_on_long_samples() {
    let station = Station {
        id: StationId::Acadie,
        center: Vec2::ZERO,
        forward: Vec2::X,
        distance: 0.,
    };
    let map = MetroMap {
        stations: vec![station],
        ..Default::default()
    };
    let spans = map.tunnel_spans(
        Vec3::new(-0.5, 0., TRACK_HEIGHT),
        Vec3::new(0.5, 0., TRACK_HEIGHT),
    );
    assert_eq!(spans.len(), 2);
    assert!((spans[0].1.x + 0.315).abs() < 1e-6);
    assert!((spans[1].0.x - 0.315).abs() < 1e-6);
    // A train-sized ray remains within the vault; rays above the crown meet the bulkhead.
    let mesh = map.scenery().unwrap().unwrap();
    for direction in [-1., 1.] {
        assert!(!ray_blocked(
            &mesh.mesh,
            Vec3::new(0., 0., TRACK_HEIGHT + 0.041),
            Vec3::new(direction * 0.4, 0., TRACK_HEIGHT + 0.041)
        ));
        assert!(ray_blocked(
            &mesh.mesh,
            Vec3::new(0., 0., TRACK_HEIGHT + 0.065),
            Vec3::new(direction * 0.4, 0., TRACK_HEIGHT + 0.065)
        ));
    }
    assert!(!map.enclosed_view(map.stations[0].point(-0.40, 0.06, 0.)));
}
