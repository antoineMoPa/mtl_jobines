use super::*;
use crate::dynamic_world::player::HALF_HEIGHT;
fn hermes() -> Hermes {
    let map = include_str!("../../../data/maps/mtl/map.road");
    let metro = map.split("# Underground metro loop:").nth(1).unwrap();
    let metro = metro
        .lines()
        .skip(1)
        .filter(|l| !l.contains("terrain_cut"))
        .collect::<Vec<_>>()
        .join("\n");
    Hermes::test_map_from_source(&format!(
        "road street: carlane sidewalk; end street(-1,0,1,0); {metro}"
    ))
}
#[test]
fn metro_stops_opens_boards_carries_and_alights_only_through_a_stopped_door() {
    let hermes = hermes();
    let mut metro = Metro::new(&hermes);
    assert!(metro.train.is_some());
    let train = metro.train.as_ref().unwrap();
    let yaw = train.yaw;
    let base = train.position - Vec3::Z * train.ride_height();
    let world = |x, y| base + Quat::from_rotation_z(yaw) * Vec3::new(x, y, 1.025 + HALF_HEIGHT);
    let start = world(4.96, 2.2);
    let mut player = Pedestrian::new(hermes.city(), start, yaw - std::f32::consts::FRAC_PI_2);
    let door = world(4.96, 1.2);
    assert!(metro.blocked(door));
    for _ in 0..150 {
        metro.step(1. / 60., None);
    }
    assert!(
        metro.ground(start.truncate().extend(10.2)).is_none(),
        "surface pedestrians must never snap underground"
    );
    assert!(metro.open());
    assert!(!metro.blocked(door));
    assert!(metro.blocked(world(0., 1.2)));
    for _ in 0..60 {
        metro.walk(
            &hermes,
            &mut player,
            WalkControls {
                forward: 1.,
                ..Default::default()
            },
            1. / 60.,
            |_| false,
        );
    }
    assert!(
        metro.aboard,
        "walking through the opening must board; local {:?}",
        metro.local(player.position)
    );
    let local = metro.local(player.position).unwrap();
    let mut visited = vec![];
    for _ in 0..60000 {
        metro.step(1. / 60., Some(&mut player));
        if metro.open() && visited.last() != Some(&metro.stop) {
            visited.push(metro.stop);
        }
        let carried = metro.local(player.position).unwrap();
        assert!(
            carried.distance(local) < 0.03,
            "rider drifted: {carried:?}, initial {local:?}"
        );
        if visited.len() >= 4 {
            break;
        }
    }
    assert_eq!(&visited[..4], &[0, 1, 2, 0]);
    // Exit the same door onto the destination dock while remaining on foot.
    let train = metro.train.as_ref().unwrap();
    player.position = train.position - Vec3::Z * train.ride_height()
        + Quat::from_rotation_z(train.yaw) * Vec3::new(4.96, 0.3, 1.025 + HALF_HEIGHT);
    player.yaw = train.yaw + std::f32::consts::FRAC_PI_2;
    for _ in 0..90 {
        metro.walk(
            &hermes,
            &mut player,
            WalkControls {
                forward: 1.,
                ..Default::default()
            },
            1. / 60.,
            |_| false,
        );
    }
    assert!(!metro.aboard);
    assert!(metro.local(player.position).unwrap().y > 1.6);
}

#[test]
fn pedestrian_can_walk_down_the_complete_station_staircase() {
    let hermes = hermes();
    let mut metro = Metro::new(&hermes);
    let station = metro.map.stations[0].clone();
    let start = station.point(-0.4, 0.06, 0.102);
    let mut player = Pedestrian::at(
        hermes.city(),
        crate::dynamic_world::SpawnPoint {
            point: start.truncate().to_array(),
            heading: Some(station.forward.y.atan2(station.forward.x)),
        },
    )
    .unwrap();
    for _ in 0..1000 {
        metro.walk(
            &hermes,
            &mut player,
            WalkControls {
                forward: 1.,
                ..Default::default()
            },
            1. / 60.,
            |_| false,
        );
    }
    let position = player.position / 100.;
    let along = (position.truncate() - station.center).dot(station.forward);
    assert!(
        along > -0.19,
        "stair descent stopped at {along}, body {position:?}"
    );
    assert!(
        (position.z - HALF_HEIGHT / 100. - FLOOR_HEIGHT).abs() < 0.001,
        "stair descent did not reach the dock {position:?}"
    );
    let anchor = station.point(0., 0.06, FLOOR_HEIGHT + 0.015);
    let camera = metro.camera_eye(anchor, anchor + Vec3::Z * 0.1);
    assert!(
        camera.z < TRACK_HEIGHT + 0.066,
        "camera escaped underground ceiling"
    );
}
