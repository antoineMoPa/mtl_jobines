use super::*;
use crate::dynamic_world::contacts;

const GRADED: &str = include_str!("../../../../tests/fixtures/traffic_lod.road");
const CROSSING: &str = include_str!("../../../../tests/fixtures/signal_cache.road");

fn scene(source: &str) -> (City, Arc<TrafficPaths>) {
    let program = opencity::parse(source).unwrap();
    let city = City::from_program(&program, None).unwrap();
    let graph = Arc::new(TrafficPaths::new(&city, &program));
    (city, graph)
}
fn driver(
    city: &City,
    graph: &Arc<TrafficPaths>,
    path: usize,
    distance: f32,
) -> (Traffic, Vec<Vehicle>) {
    let (point, heading) = graph.paths[path].sample(distance);
    let mut car = Vehicle::spawn(city);
    car.position = point + Vec3::Z * car.ride_height();
    car.yaw = heading.y.atan2(heading.x);
    car.pitch = heading.z.atan2(heading.truncate().length());
    let mut player = Vehicle::spawn(city);
    player.position = Vec3::new(2000., 2000., 0.65);
    let mut traffic = Traffic::new(graph);
    traffic.drivers.push(Driver::new(0, path, distance, 0.));
    (traffic, vec![car, player])
}
fn select(traffic: &mut Traffic, cars: &[Vehicle]) {
    traffic.select_physics(cars, 1, cars[1].position, cars[1].velocity);
}

#[test]
fn distance_hysteresis_and_closing_speed_promote_without_changing_state() {
    let (city, graph) = scene(GRADED);
    let (mut traffic, mut cars) = driver(&city, &graph, 0, 100.);
    let at = cars[0].position;
    let original = (
        cars[0].position,
        cars[0].velocity,
        cars[0].yaw,
        cars[0].angular_momentum,
    );
    cars[1].position = at + Vec3::Y * 100.;
    select(&mut traffic, &cars);
    assert!(matches!(traffic.drivers[0].motion, Motion::Lane(_)));
    cars[1].position = at + Vec3::Y * 70.;
    select(&mut traffic, &cars);
    assert!(matches!(traffic.drivers[0].motion, Motion::Lane(_)));
    cars[1].position = at + Vec3::Y * 50.;
    select(&mut traffic, &cars);
    assert!(matches!(traffic.drivers[0].motion, Motion::Physical));
    cars[1].position = at + Vec3::Y * 70.;
    select(&mut traffic, &cars);
    assert!(matches!(traffic.drivers[0].motion, Motion::Physical));
    cars[1].position = at + Vec3::Y * 120.;
    select(&mut traffic, &cars);
    assert!(matches!(traffic.drivers[0].motion, Motion::Lane(_)));
    cars[1].velocity = -Vec3::Y * 50.;
    select(&mut traffic, &cars);
    assert!(
        matches!(traffic.drivers[0].motion, Motion::Physical),
        "fast approach promoted too late"
    );
    assert_eq!(
        original,
        (
            cars[0].position,
            cars[0].velocity,
            cars[0].yaw,
            cars[0].angular_momentum
        )
    );
}

#[test]
fn tire_hit_wakes_a_distant_lane_follower_and_preserves_its_slot() {
    let (city, graph) = scene(GRADED);
    let (mut traffic, mut cars) = driver(&city, &graph, 0, 100.);
    select(&mut traffic, &cars);
    assert!(matches!(traffic.drivers[0].motion, Motion::Lane(_)));
    traffic.wake_after_tire_hit(0);
    select(&mut traffic, &cars);
    assert!(matches!(traffic.drivers[0].motion, Motion::Physical));
    let position = cars[0].position;
    let center = cars[1].position;
    traffic.populate(&city, &mut cars, center);
    assert!(!traffic.inactive(0));
    assert_eq!(cars[0].position, position);
}

#[test]
fn airborne_displaced_and_recently_colliding_cars_stay_physical() {
    let (city, graph) = scene(GRADED);
    for problem in 0..4 {
        let (mut traffic, mut cars) = driver(&city, &graph, 0, 100.);
        match problem {
            0 => cars[0].grounded = false,
            1 => cars[0].position.y += 2.,
            2 => cars[0].angular_momentum = Vec3::Z * 20.,
            _ => cars[0].collisions += 1,
        }
        select(&mut traffic, &cars);
        assert!(matches!(traffic.drivers[0].motion, Motion::Physical));
    }
}

#[test]
fn lane_motion_uses_its_own_graded_deck_and_keeps_handoff_continuous() {
    let (city, graph) = scene(GRADED);
    for path in 0..4 {
        let (mut traffic, mut cars) = driver(&city, &graph, path, 100.);
        let heading = graph.paths[path].sample(100.).1;
        cars[0].velocity = heading * 10.;
        cars[0].position.y += 0.1;
        select(&mut traffic, &cars);
        assert!(matches!(traffic.drivers[0].motion, Motion::Lane(_)));
        for _ in 0..240 {
            traffic.drivers[0].target_speed = 10.;
            traffic.drivers[0].clearance = f32::INFINITY;
            let before = cars[0].position;
            assert!(traffic.step_lane(0, &mut cars[0], Controls::default(), 1. / 120.));
            assert!(
                cars[0].position.distance(before) < 0.09,
                "lane handoff jumped"
            );
            let sample = graph.paths[path].sample(traffic.drivers[0].distance).0;
            assert!((cars[0].position.z - cars[0].ride_height() - sample.z).abs() < 0.0001);
            assert!(cars[0].position.is_finite() && cars[0].velocity.is_finite());
        }
        assert!((cars[0].velocity.length() - 10.).abs() < 0.001);
        let position = cars[0].position;
        let velocity = cars[0].velocity;
        let momentum = cars[0].angular_momentum;
        cars[1].position = position + Vec3::Y * 20.;
        select(&mut traffic, &cars);
        assert!(matches!(traffic.drivers[0].motion, Motion::Physical));
        assert_eq!(
            (position, velocity, momentum),
            (cars[0].position, cars[0].velocity, cars[0].angular_momentum)
        );
    }
}

#[test]
fn lane_distance_carries_across_a_junction_without_cutting_the_last_metres() {
    let (city, graph) = scene(CROSSING);
    let path = graph
        .paths
        .iter()
        .position(|path| path.speed != SpeedClass::Junction && !path.next.is_empty())
        .unwrap();
    let length = graph.paths[path].length;
    let (mut traffic, mut cars) = driver(&city, &graph, path, length - 0.1);
    let next = graph.paths[path].next[0];
    traffic.drivers[0].next = Some(next);
    traffic.drivers[0].target_speed = 6.;
    cars[0].velocity = graph.paths[path].sample(length - 0.1).1 * 6.;
    select(&mut traffic, &cars);
    let before = cars[0].position;
    assert!(traffic.step_lane(0, &mut cars[0], Controls::default(), 0.1));
    assert_eq!(traffic.drivers[0].path, next);
    assert!((traffic.drivers[0].distance - 0.5).abs() < 0.001);
    assert!(cars[0].position.distance(before) < 0.61);
}

#[test]
fn lane_traffic_obeys_red_then_green_and_keeps_a_stopped_leaders_gap() {
    let (city, graph) = scene(CROSSING);
    let path = graph
        .paths
        .iter()
        .position(|path| path.speed != SpeedClass::Junction && !path.next.is_empty())
        .unwrap();
    let next = *graph.paths[path]
        .next
        .iter()
        .find(|&&id| graph.paths[id].turn == Turn::Straight)
        .unwrap();
    let signal = &graph.signals[graph.paths[next].signal.unwrap()];
    let offset = (signal.junction % 11) as f32 * 3.;
    let (mut traffic, mut cars) = driver(&city, &graph, path, graph.paths[path].length - 25.);
    traffic.drivers[0].next = Some(next);
    cars[0].velocity = graph.paths[path].sample(traffic.drivers[0].distance).1 * 8.;
    for _ in 0..1200 {
        traffic.time = ((signal.phase + 1) % 2) as f32 * 14. - offset;
        select(&mut traffic, &cars);
        let controls = traffic.controls(&city, &cars, None, 1. / 120.);
        assert!(traffic.step_lane(0, &mut cars[0], controls[0].1, 1. / 120.));
    }
    assert_eq!(traffic.drivers[0].path, path, "crossed a red light");
    assert!(cars[0].velocity.length() < 0.01);
    assert!(graph.paths[path].length - traffic.drivers[0].distance > cars[0].length() * 0.5);
    for _ in 0..1200 {
        traffic.time = signal.phase as f32 * 14. - offset;
        let controls = traffic.controls(&city, &cars, None, 1. / 120.);
        assert!(traffic.step_lane(0, &mut cars[0], controls[0].1, 1. / 120.));
    }
    assert_ne!(
        traffic.drivers[0].path, path,
        "green never released lane traffic"
    );
    let (mut traffic, mut cars) = driver(&city, &graph, path, 40.);
    let heading = graph.paths[path].sample(40.).1;
    cars[1].position =
        cars[0].position + heading * ((cars[0].length() + cars[1].length()) * 0.5 + 2.);
    // The nearby leader is an obstacle, while the actual player remains far away.
    traffic.select_physics(&cars, usize::MAX, Vec3::splat(2000.), Vec3::ZERO);
    let before = cars[0].position;
    for _ in 0..120 {
        let controls = traffic.controls(&city, &cars, None, 1. / 120.);
        assert!(traffic.step_lane(0, &mut cars[0], controls[0].1, 1. / 120.));
    }
    assert!(
        cars[0].position.distance(before) < 0.001,
        "ran into a stopped leader"
    );
}

#[test]
fn player_collision_chain_keeps_wrecks_physical_retained_and_wakeable() {
    let (city, graph) = scene(GRADED);
    let (mut traffic, mut cars) = driver(&city, &graph, 0, 100.);
    cars[1].position = cars[0].position + Vec3::X * 4.1;
    cars[0].yaw = 0.;
    cars[1].yaw = 0.;
    cars[1].velocity = -Vec3::X * 10.;
    let events = contacts::resolve_with_contacts(&mut cars);
    assert!(
        !events.is_empty(),
        "test did not produce a real chassis contact"
    );
    traffic.record_contacts(&events, 1);
    let pileup = [(2, 3), (0, 2)].map(|vehicles| contacts::VehicleContact {
        vehicles,
        approach: [0.; 2],
    });
    traffic.record_contacts(&pileup, 1);
    assert!(
        (0..4).all(|id| traffic.interacted.contains_key(&id)),
        "indirect pile-up was not retained"
    );
    // The extra metadata-only pair above verifies order-independent propagation.
    traffic.interacted.remove(&2);
    traffic.interacted.remove(&3);
    cars[1].position = Vec3::splat(2000.);
    select(&mut traffic, &cars);
    assert!(matches!(traffic.drivers[0].motion, Motion::Physical));
    let at = cars[0].position;
    traffic.populate(&city, &mut cars, Vec3::splat(2000.));
    assert_eq!(cars[0].position, at, "player-hit vehicle was recycled");
    assert!(!traffic.inactive(0));
    cars[0].velocity = Vec3::ZERO;
    cars[0].angular_momentum = Vec3::ZERO;
    for _ in 0..3 {
        assert!(!traffic.sleep_if_settled(
            0,
            &mut cars[0],
            Controls::default(),
            Vec3::splat(2000.),
            1.
        ));
    }
    assert!(traffic.sleep_if_settled(0, &mut cars[0], Controls::default(), Vec3::splat(2000.), 1.));
    assert_eq!(cars[0].position, at);
    assert!(!traffic.sleep_if_settled(
        0,
        &mut cars[0],
        Controls::default(),
        at + Vec3::Y,
        1. / 120.
    ));
    assert!(matches!(traffic.interacted[&0], Protected::Awake { .. }));
}
