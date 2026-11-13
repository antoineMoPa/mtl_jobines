use super::*;
use crate::hermes::traffic_paths::TrafficPaths;
fn map(source: &str) -> (City, Arc<TrafficPaths>) {
    let program = opencity::parse(source).unwrap();
    let city = City::from_program(&program, None).unwrap();
    let paths = TrafficPaths::new(&city, &program);
    (city, Arc::new(paths))
}
const STRAIGHT: &str = "road street: sidewalk carlane middle carlane sidewalk; end
    street(-3,0,3,0); street(-3,0.4,3,0.4); street(-3,-0.4,3,-0.4);
    land(-4,-2,4,-2,4,2,-4,2);";
const CROSS: &str = "road street: sidewalk carlane middle carlane sidewalk; end
    center=map_point(0,0); north=map_point(0,2); east=map_point(2,0);
    south=map_point(0,-2); west=map_point(-2,0);
    street(north,0,1.4,0,0.3,center); street(center,0.3,0,1.4,0,east);
    street(center,0,-0.3,0,-1.4,south); street(west,-1.4,0,-0.3,0,center);
    intersection4(center,street); land(-3,-3,3,-3,3,3,-3,3);";
fn one_driver(
    city: &City,
    graph: &Arc<TrafficPaths>,
    path: usize,
    distance: f32,
) -> (Traffic, Vec<Vehicle>) {
    let mut traffic = Traffic::new(graph);
    let (point, heading) = graph.paths[path].sample(distance);
    let mut car = Vehicle::spawn_at(
        city,
        SpawnPoint {
            point: (point.truncate() / 100.).to_array(),
            heading: Some(heading.y.atan2(heading.x)),
        },
    )
    .unwrap();
    car.definition = Some(0);
    car.position.z = point.z + car.ride_height();
    traffic.drivers.push(Driver::new(0, path, distance, 0.));
    (traffic, vec![car])
}
fn tick(traffic: &mut Traffic, city: &City, cars: &mut [Vehicle], dt: f32) {
    for (index, input) in traffic.controls(city, cars, None, dt) {
        cars[index].step(city, input, dt);
    }
    super::super::contacts::resolve(cars);
}
#[test]
fn population_is_nearby_separated_and_reuses_slots_after_travel() {
    let (city, graph) = map(STRAIGHT);
    let mut traffic = Traffic::new(&graph);
    assert!(
        Arc::ptr_eq(&traffic.graph, &graph),
        "traffic must share its map graph"
    );
    let mut cars = Vec::new();
    traffic.populate(&city, &mut cars, Vec3::ZERO);
    assert_eq!(traffic.drivers.len(), TARGET);
    let buses = cars
        .iter()
        .filter(|v| v.definition().is_some_and(|d| d.config.category == "bus"))
        .count();
    assert!(
        (1..=2).contains(&buses),
        "ambient traffic must include a bounded bus population"
    );
    for (i, a) in cars.iter().enumerate() {
        assert!(a.position.length() < SPAWN_RADIUS + 1.);
        assert!(a.position.truncate().length() >= 30.);
        for b in &cars[..i] {
            assert!(a.position.distance(b.position) >= (a.length() + b.length()) * 0.5 + 5.);
        }
    }
    for driver in &traffic.drivers {
        let half_length = cars[driver.vehicle].length() * 0.5;
        assert!(driver.distance >= half_length + 1.5);
        assert!(graph.paths[driver.path].length - driver.distance >= half_length + 1.5);
    }
    let capacity = cars.len();
    traffic.take_spawns();
    for center in [Vec3::X * 600., Vec3::ZERO, Vec3::X * -600., Vec3::ZERO]
        .into_iter()
        .cycle()
        .take(100)
    {
        traffic.populate(&city, &mut cars, center);
        assert!(cars.len() <= capacity, "travelling must reuse NPC slots");
        assert!(traffic.take_spawns().len() <= TARGET);
    }
    assert_eq!(traffic.drivers.len(), TARGET);
}
#[test]
fn bus_pursuit_uses_its_wheelbase_and_holds_clear_of_a_leader() {
    let (city, graph) = map(STRAIGHT);
    let bus = catalog::definitions()
        .iter()
        .position(|d| d.config.category == "bus")
        .unwrap();
    let (mut traffic, mut cars) = one_driver(&city, &graph, 0, 120.);
    cars[0].definition = Some(bus);
    cars[0].position.z = graph.paths[0].sample(120.).0.z + cars[0].ride_height();
    cars[0].velocity = graph.paths[0].sample(120.).1 * 8.;
    cars[0].yaw += 0.12;
    let bus_controls = traffic.controls(&city, &cars, None, 1. / 120.)[0].1;
    let bus_steer = bus_controls.steering.abs();
    cars[0].definition = Some(0);
    let short_steer = traffic.controls(&city, &cars, None, 1. / 120.)[0]
        .1
        .steering
        .abs();
    assert!(
        bus_steer > short_steer * 1.1,
        "bus used passenger-car steering geometry"
    );
    cars[0].definition = Some(bus);
    cars[0].yaw -= 0.12;
    let forward = graph.paths[0].sample(120.).1;
    let mut leader = Vehicle::spawn(&city);
    leader.definition = Some(0);
    leader.position =
        cars[0].position + forward * ((cars[0].length() + leader.length()) * 0.5 + 2.);
    cars.push(leader);
    let input = traffic.controls(&city, &cars, None, 1. / 120.)[0].1;
    assert!(input.brake);
    assert_eq!(input.throttle, 0.);
}
#[test]
fn speed_classes_follow_semantic_roles_and_carriageways_have_opposite_directions() {
    let (_, graph) = map("road local: carlane middle carlane; end
        road freeway: carlane middle carlane; role highway; end
        road ramp: carlane; direction forward; role ramp; end
        road merge: carlane; direction forward; role merge; end
        local(-2,0,2,0); freeway(-2,1,2,1); ramp(-2,2,2,2); merge(-2,3,2,3);");
    let classes: Vec<_> = graph.paths.iter().map(|p| p.speed).collect();
    assert_eq!(
        classes,
        vec![
            SpeedClass::Local,
            SpeedClass::Local,
            SpeedClass::Highway,
            SpeedClass::Highway,
            SpeedClass::Ramp,
            SpeedClass::Merge
        ]
    );
    assert!(graph.paths[0].sample(0.).1.x < 0.);
    assert!(graph.paths[1].sample(0.).1.x > 0.);
    assert!(SpeedClass::Highway.meters_per_second() > SpeedClass::Local.meters_per_second());
}
#[test]
fn crossing_links_permitted_turns_and_has_four_signals() {
    let (_, graph) = map(CROSS);
    assert_eq!(graph.signals.len(), 4);
    let incoming: Vec<_> = graph
        .paths
        .iter()
        .filter(|p| p.speed != SpeedClass::Junction && !p.next.is_empty())
        .collect();
    assert_eq!(incoming.len(), 4);
    for lane in incoming {
        assert_eq!(
            lane.next.len(),
            3,
            "each approach needs left, straight and right"
        );
        for &next in &lane.next {
            assert!(graph.paths[next].signal.is_some());
            assert!(
                !graph.paths[next].next.is_empty(),
                "turn must reach an exit lane"
            );
        }
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn current_map_one_way_crossings_have_visible_signals_at_road_height() {
    let hermes = crate::hermes::Hermes::load_with_game_data(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/map.road"),
        None,
    )
    .unwrap();
    let graph = hermes.traffic_paths();
    let traffic = Traffic::new(graph);
    for name in ["profile:north_cross", "profile:south_cross"] {
        let junction = hermes
            .city()
            .components
            .iter()
            .position(|c| c.id == name)
            .unwrap();
        let component = &hermes.city().components[junction];
        let signals: Vec<_> = graph
            .signals
            .iter()
            .filter(|s| s.junction == junction)
            .collect();
        assert_eq!(
            signals.len(),
            3,
            "{name}: three incoming one-way approaches"
        );
        assert!(signals.iter().any(|s| s.phase == 0));
        assert!(signals.iter().any(|s| s.phase == 1));
        for signal in &signals {
            assert!((signal.position.z / 100. - component.origin.z).abs() < 1e-5);
            let base = signal
                .mast_base
                .expect("every canonical approach has a sidewalk");
            let (standing, _) = models::instantiate(signal);
            let mut model = opencity::runtime::Mesh::default();
            standing.append(&mut model, None);
            let mut foot_vertices = 0;
            for vertex in &model.vertices {
                if (vertex.position[2] - base.z / 100.).abs() > 1e-6 {
                    continue;
                }
                let support = hermes
                    .city()
                    .surface_at(
                        [vertex.position[0], vertex.position[1]],
                        base.z / 100. + 0.001,
                    )
                    .unwrap();
                assert_eq!(
                    support.kind,
                    opencity::runtime::SurfaceKind::Sidewalk,
                    "{name}: footplate extends off sidewalk"
                );
                assert!((support.height - vertex.position[2]).abs() < 0.0008);
                foot_vertices += 1;
            }
            assert!(foot_vertices > 0);
        }
        let mut visible = opencity::runtime::Mesh::default();
        traffic.append_signals(&mut visible, signals[0].position);
        assert!(!visible.vertices.is_empty());
        assert!(visible
            .vertices
            .iter()
            .any(|v| { v.surface[0] == opencity::runtime::SurfaceKind::Light as u32 as f32 }));
    }
}
#[test]
fn red_light_stops_before_line_then_green_releases_traffic() {
    let (city, graph) = map(CROSS);
    let path = graph
        .paths
        .iter()
        .position(|p| !p.next.is_empty() && graph.paths[p.next[0]].signal.is_some())
        .unwrap();
    let signal = graph.paths[graph.paths[path].next[0]].signal.unwrap();
    let phase = graph.signals[signal].phase;
    let bus = catalog::definitions()
        .iter()
        .position(|d| d.config.category == "bus")
        .unwrap();
    for model in [0, bus] {
        let (mut traffic, mut cars) =
            one_driver(&city, &graph, path, graph.paths[path].length - 25.);
        cars[0].definition = Some(model);
        cars[0].position.z =
            graph.paths[path].sample(traffic.drivers[0].distance).0.z + cars[0].ride_height();
        traffic.time = ((phase + 1) % 2) as f32 * 14.;
        for _ in 0..540 {
            tick(&mut traffic, &city, &mut cars, 1. / 60.);
        }
        assert_eq!(traffic.drivers[0].path, path, "crossed on red");
        assert!(cars[0].velocity.length() < 0.6, "failed to stop");
        let distance = graph.paths[path]
            .project(cars[0].position - Vec3::Z * cars[0].ride_height())
            .0;
        assert!(graph.paths[path].length - distance > cars[0].length() * 0.5);
        traffic.time = phase as f32 * 14.;
        for _ in 0..600 {
            tick(&mut traffic, &city, &mut cars, 1. / 60.);
        }
        assert_ne!(traffic.drivers[0].path, path, "failed to leave on green");
    }
}
#[test]
fn hitting_a_signal_detaches_it_and_turns_the_whole_junction_dark() {
    let (city, graph) = map(CROSS);
    let mut traffic = Traffic::new(&graph);
    let signal = &graph.signals[0];
    let base = signal.mast_base.unwrap();
    let selected = traffic
        .standing_signal_batches(base)
        .find(|batch| batch.id == 0)
        .unwrap();
    let mut car = Vehicle::spawn(&city);
    car.set_catalog_model(Some(0));
    car.yaw = signal.forward.y.atan2(signal.forward.x);
    let before = base - signal.forward * 8. + Vec3::Z * car.ride_height();
    car.position = base + signal.forward * 5. + Vec3::Z * car.ride_height();
    car.velocity = signal.forward * 14.;
    traffic.vehicle_move(&mut car, before, &city);
    assert!(!traffic.signal_props.group_intact(0));
    assert!(traffic.failed_junctions.contains(&signal.junction));
    assert!(car.collisions > 0);
    assert!(traffic
        .standing_signal_batches(base)
        .all(|batch| batch.id != 0 && batch.active.is_none()));
    let mut stale = opencity::runtime::Mesh::default();
    traffic.append_signal_batch(&selected, &mut stale);
    assert!(
        stale.vertices.is_empty(),
        "stale selection restored a broken signal"
    );
    let mut mesh = opencity::runtime::Mesh::default();
    traffic.append_signals(&mut mesh, base);
    assert!(
        !mesh.vertices.is_empty(),
        "fallen pole and standing signals remain visible"
    );
    assert!(mesh
        .vertices
        .iter()
        .all(|v| v.surface[0] != opencity::runtime::SurfaceKind::Light as u32 as f32));
    for _ in 0..1201 {
        traffic.step_debris(1. / 120.);
    }
    assert!(
        !traffic.signal_props.group_intact(0),
        "signals do not respawn after debris expires"
    );
    assert!(traffic.failed_junctions.contains(&signal.junction));
}

#[test]
fn crossings_without_supported_signal_bases_use_all_way_stops() {
    let (_, graph) = map(&CROSS.replace("sidewalk", "verge"));
    assert!(!graph.signals.is_empty());
    assert!(graph
        .signals
        .iter()
        .all(|signal| signal.mast_base.is_none()));
    let traffic = Traffic::new(&graph);
    assert!(graph
        .signals
        .iter()
        .all(|signal| traffic.failed_junctions.contains(&signal.junction)));
    let mut mesh = opencity::runtime::Mesh::default();
    traffic.append_signals(&mut mesh, Vec3::ZERO);
    assert!(
        mesh.vertices.is_empty(),
        "unsupported poles have no render or collision mesh"
    );
}

#[test]
fn failed_signal_requires_a_full_stop_then_releases_even_on_red() {
    let (city, graph) = map(CROSS);
    let path = graph
        .paths
        .iter()
        .position(|p| !p.next.is_empty() && graph.paths[p.next[0]].signal.is_some())
        .unwrap();
    let signal = graph.paths[graph.paths[path].next[0]].signal.unwrap();
    let junction = graph.signals[signal].junction;
    let (mut traffic, mut cars) = one_driver(&city, &graph, path, graph.paths[path].length - 25.);
    traffic.failed_junctions.insert(junction);
    let mut stopped_ticks = 0;
    let mut crossed = false;
    for _ in 0..900 {
        // Hold the normal signal red to prove failure mode overrides its clock.
        traffic.time = ((graph.signals[signal].phase + 1) % 2) as f32 * 14.;
        tick(&mut traffic, &city, &mut cars, 1. / 60.);
        if traffic.drivers[0].path == path && cars[0].velocity.length() < 0.5 {
            stopped_ticks += 1;
        }
        if traffic.drivers[0].path != path {
            crossed = true;
            break;
        }
    }
    assert!(crossed, "failed signal left traffic stuck");
    assert!(
        stopped_ticks >= 60,
        "driver rolled through without a one-second stop: {stopped_ticks}"
    );
}

#[test]
fn all_way_stop_reserves_one_crossing_in_arrival_order() {
    let (city, graph) = map(CROSS);
    let approaches: Vec<_> = graph
        .paths
        .iter()
        .enumerate()
        .filter_map(|(id, path)| {
            path.next
                .first()
                .and_then(|&next| graph.paths[next].signal.map(|signal| (id, next, signal)))
        })
        .collect();
    let (path, next, signal) = approaches[0];
    let (mut traffic, mut cars) = one_driver(&city, &graph, path, graph.paths[path].length - 4.5);
    let junction = graph.signals[signal].junction;
    traffic.failed_junctions.insert(junction);
    traffic.drivers[0].next = Some(next);
    traffic.drivers[0].stop_wait = 1.;
    traffic.drivers[0].stop_arrival = Some(1.);
    let (other_path, other_next, _) = approaches
        .iter()
        .copied()
        .find(|(_, _, id)| *id != signal)
        .unwrap();
    let (other_traffic, mut other_cars) = one_driver(
        &city,
        &graph,
        other_path,
        graph.paths[other_path].length - 4.5,
    );
    cars.push(other_cars.remove(0));
    let mut driver = other_traffic.drivers.into_iter().next().unwrap();
    driver.vehicle = 1;
    driver.next = Some(other_next);
    driver.stop_wait = 1.;
    driver.stop_arrival = Some(2.);
    traffic.drivers.push(driver);
    // Exact stop-line placement is based on each model's length.
    for driver in &traffic.drivers {
        let car = &mut cars[driver.vehicle];
        let at = graph.paths[driver.path].length - car.length() * 0.5 - 1.5;
        car.position = graph.paths[driver.path].sample(at).0 + Vec3::Z * car.ride_height();
    }
    traffic.time = 3.;
    traffic.update_stop_priority(&cars, 1. / 60.);
    assert_eq!(traffic.crossing.get(&junction), Some(&0));
    traffic.update_stop_priority(&cars, 1. / 60.);
    assert_eq!(
        traffic.crossing.get(&junction),
        Some(&0),
        "second approach must wait"
    );
    traffic.release(0);
    cars[0].position += Vec3::Z * 20.;
    traffic.update_stop_priority(&cars, 1. / 60.);
    assert_eq!(traffic.crossing.get(&junction), Some(&1));
}
#[test]
fn drivers_move_follow_speed_limit_and_brake_for_parked_cars() {
    let (city, graph) = map(STRAIGHT);
    let (mut traffic, mut cars) = one_driver(&city, &graph, 1, 120.);
    let initial = cars[0].position;
    for _ in 0..600 {
        tick(&mut traffic, &city, &mut cars, 1. / 60.);
    }
    assert!(cars[0].position.distance(initial) > 25.);
    assert!(cars[0].velocity.length() <= SpeedClass::Local.meters_per_second() + 1.);
    let mut parked = Vehicle::spawn(&city);
    parked.definition = Some(0);
    parked.position = cars[0].position + Vec3::X * 25.;
    parked.yaw = cars[0].yaw;
    let parked_at = parked.position;
    cars.push(parked);
    for _ in 0..600 {
        tick(&mut traffic, &city, &mut cars, 1. / 60.);
    }
    assert!(cars[0].position.x < parked_at.x - 4.);
    assert!(cars[0].velocity.length() < 0.7);
    assert_eq!(cars[0].collisions, 0);
}
#[test]
fn collision_displacement_rejoins_a_lane_or_stops_when_unreachable() {
    let (city, graph) = map(STRAIGHT);
    let (mut traffic, mut cars) = one_driver(&city, &graph, 1, 120.);
    cars[0].position.y += 4.;
    for _ in 0..600 {
        tick(&mut traffic, &city, &mut cars, 1. / 60.);
    }
    assert!(!traffic.drivers[0].stopped);
    let ground = cars[0].position - Vec3::Z * cars[0].ride_height();
    assert!(graph.paths[traffic.drivers[0].path].project(ground).1 < 1.5);
    cars[0].position.y = 150.;
    cars[0].velocity = Vec3::X * 3.;
    for _ in 0..300 {
        tick(&mut traffic, &city, &mut cars, 1. / 60.);
    }
    assert!(traffic.drivers[0].stopped);
    assert!(cars[0].velocity.length() < 0.5);
}
#[test]
fn physical_contact_deflects_npc_and_takeover_removes_ai_control() {
    let (city, graph) = map(STRAIGHT);
    let (mut traffic, mut cars) = one_driver(&city, &graph, 1, 120.);
    let mut player = Vehicle::spawn(&city);
    player.position =
        cars[0].position - Vec3::X * ((player.length() + cars[0].length()) * 0.5 - 0.1);
    player.yaw = cars[0].yaw;
    player.velocity = Vec3::X * 12.;
    cars.push(player);
    super::super::contacts::resolve(&mut cars);
    assert!(cars[0].collisions > 0);
    assert!(cars[0].velocity.x > 0.);
    traffic.release(0);
    assert!(traffic.controls(&city, &cars, None, 1. / 60.).is_empty());
}
#[test]
fn paths_do_not_connect_unrelated_crossings_or_stacked_decks() {
    let (_, graph) = map("road street: carlane; direction forward; end
        origin=map_point(-2,0); a=map_point(2,0); b=map_point(2,0); c=map_point(4,0);
        street(origin,-0.7,0,0.7,0,a); street(0,-2,0,2);
        street(b,2.6,0,3.4,0,c); elevated=street(2,0,4,0); grade(elevated,0.5,0.5);");
    assert!(graph.paths.iter().all(|p| p.next.is_empty()));
}
#[test]
fn ramp_attachment_splits_and_connects_only_its_lane() {
    let (_, graph) = map("road freeway: carlane middle carlane; role highway; end
        road ramp: carlane; direction forward; role ramp; end
        main=freeway(-3,0,3,0); exit=lane_point(main,2,0.5);
        finish=map_point(1,-1); ramp(exit,0.3,-0.035,0.8,-0.7,finish);");
    let ramp = graph
        .paths
        .iter()
        .position(|p| p.speed == SpeedClass::Ramp)
        .unwrap();
    assert!(graph
        .paths
        .iter()
        .any(|p| p.speed == SpeedClass::Highway && p.next.contains(&ramp)));
    assert!(!graph.paths[0].next.contains(&ramp));
}
#[test]
fn highway_driver_reaches_highway_speed() {
    let (city, graph) = map("road freeway: verge carlane middle carlane verge;
        role highway; end freeway(-20,0,20,0); land(-21,-2,21,-2,21,2,-21,2);");
    let (mut traffic, mut cars) = one_driver(&city, &graph, 1, 100.);
    for _ in 0..1200 {
        tick(&mut traffic, &city, &mut cars, 1. / 60.);
    }
    assert!(cars[0].velocity.length() > 24.);
    assert!(cars[0].velocity.length() < 29.);
}
#[test]
fn default_city_keeps_thirty_physical_npcs_and_moves_them() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("opencity/examples/default_map.road");
    let hermes = crate::hermes::Hermes::load_with_game_data(&path, None).unwrap();
    let mut world = super::super::DynamicWorld::new(&hermes);
    assert_eq!(world.traffic.count(), TARGET);
    let original: Vec<_> = world
        .traffic
        .drivers
        .iter()
        .map(|d| (d.vehicle, world.vehicles[d.vehicle].position))
        .collect();
    let simulation_started = std::time::Instant::now();
    for _ in 0..900 {
        world.step(
            &hermes,
            Controls::default(),
            super::super::WalkControls::default(),
            1. / 60.,
        );
    }
    eprintln!("900 simulation ticks: {:?}", simulation_started.elapsed());
    let moved = original
        .iter()
        .filter(|(id, position)| world.vehicles[*id].position.distance(*position) > 5.)
        .count();
    eprintln!(
        "default traffic: {moved}/{TARGET} moved over 5 m, {} stopped after recovery",
        world.traffic.drivers.iter().filter(|d| d.stopped).count()
    );
    assert!(
        moved >= 10,
        "traffic should move through the real default road network"
    );
    assert_eq!(world.traffic.count(), TARGET);
    assert!(world.vehicles.iter().all(|v| v.position.is_finite()));
    let mut mesh = opencity::runtime::Mesh::default();
    world.write_moving_mesh(&mut mesh);
    assert!(!mesh.vertices.is_empty());
}
#[test]
fn left_turn_waits_for_oncoming_straight_traffic_then_takes_the_gap() {
    let (city, graph) = map(CROSS);
    let lane = graph
        .paths
        .iter()
        .position(|p| {
            p.speed != SpeedClass::Junction
                && p.next.iter().any(|&id| graph.paths[id].turn == Turn::Left)
        })
        .unwrap();
    let left = *graph.paths[lane]
        .next
        .iter()
        .find(|&&id| graph.paths[id].turn == Turn::Left)
        .unwrap();
    let signal = graph.paths[left].signal.unwrap();
    let opposite = graph
        .signals
        .iter()
        .find(|s| s.forward.dot(graph.signals[signal].forward) < -0.7)
        .unwrap();
    let (mut traffic, mut cars) = one_driver(&city, &graph, lane, graph.paths[lane].length - 3.6);
    traffic.drivers[0].next = Some(left);
    traffic.time = graph.signals[signal].phase as f32 * 14.;
    let mut oncoming = Vehicle::spawn(&city);
    oncoming.definition = Some(0);
    oncoming.position =
        opposite.position - opposite.forward * 20. + Vec3::Z * oncoming.ride_height();
    oncoming.yaw = opposite.forward.y.atan2(opposite.forward.x);
    oncoming.velocity = opposite.forward * 8.;
    cars.push(oncoming);
    let input = traffic.controls(&city, &cars, None, 1. / 120.)[0].1;
    assert!(
        input.brake && input.throttle == 0.,
        "left turn must wait for approaching traffic"
    );
    assert!(
        !left_turn_blocked(
            &graph,
            signal,
            graph.paths[left].length,
            0,
            &cars,
            &[Some(Turn::Left), Some(Turn::Left)]
        ),
        "opposing left turns can proceed together"
    );
    cars.pop();
    let input = traffic.controls(&city, &cars, None, 1. / 120.)[0].1;
    assert!(
        input.throttle > 0.,
        "left turn must take the gap when oncoming traffic clears"
    );
}
#[test]
fn four_way_graph_pairs_opposite_approaches_on_each_phase() {
    let (_, graph) = map(CROSS);
    for phase in 0..2 {
        let approaches: Vec<_> = graph.signals.iter().filter(|s| s.phase == phase).collect();
        assert_eq!(approaches.len(), 2);
        assert!(approaches[0].forward.dot(approaches[1].forward) < -0.9);
    }
}

#[test]
fn spawned_traffic_fades_and_reused_slots_start_invisible() {
    let (city, graph) = map(STRAIGHT);
    let mut traffic = Traffic::new(&graph);
    let mut cars = Vec::new();
    traffic.populate(&city, &mut cars, Vec3::ZERO);
    let first = traffic.drivers[0].vehicle;
    assert_eq!(traffic.opacity(first), 0.);
    traffic.time += 0.4;
    assert!((traffic.opacity(first) - 0.5).abs() < 0.0001);
    traffic.time += 0.4;
    assert_eq!(traffic.opacity(first), 1.);
    traffic.populate(&city, &mut cars, Vec3::X * 600.);
    traffic.populate(&city, &mut cars, Vec3::ZERO);
    assert!(!traffic.drivers.is_empty());
    for driver in &traffic.drivers {
        assert_eq!(traffic.opacity(driver.vehicle), 0.);
        assert!(
            cars[driver.vehicle]
                .position
                .truncate()
                .distance(glam::Vec2::ZERO)
                >= 30.
        );
    }
    let released = traffic.drivers[0].vehicle;
    traffic.release(released);
    assert_eq!(traffic.opacity(released), 1.);
}

#[test]
fn street_furniture_bases_touch_supporting_surface() {
    let (city, _) = map(STRAIGHT);
    let features = city.street_features(opencity::runtime::Region::WholeMap);
    assert!(!features.is_empty());
    for feature in features {
        let p = feature.position;
        let support = city.surface_at([p[0], p[1]], p[2] + 0.001).unwrap();
        assert!((p[2] - support.height).abs() < 0.00001);
    }
}
