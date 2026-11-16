use super::*;

#[test]
fn tapered_three_lane_connections_reach_both_sides_of_a_divided_highway() {
    let program = opencity::parse(
        "road climb: verge carlane carlane carlane verge verge verge; direction forward; role highway; end
         road approach: carlane carlane carlane verge; direction forward; role highway; end
         road divided: verge carlane carlane carlane middle carlane carlane carlane verge; role highway; end
         east_start = map_point(-1, -0.145);\n         west_end = map_point(-1, 0.145);\n         east_crest = map_point(0, -0.145);
         west_crest = map_point(0, 0.145);
         parent_start = map_point(1, 0);
         parent_end = map_point(2, 0);
         incoming = climb(east_start, -0.7, -0.145, -0.3, -0.145, east_crest);
         outgoing = climb(west_crest, -0.3, 0.145, -0.7, 0.145, west_end);
         parent = divided(parent_start, 1.3, 0, 1.7, 0, parent_end);
         entry = lane_point(parent, 5, 0);
         exit = lane_point(parent, 3, 0);
         entry_road = approach(east_crest, 0.3, -0.145, 0.7, -0.035, entry);
         exit_road = approach(exit, 0.7, 0.035, 0.3, 0.145, west_crest);"
    ).unwrap();
    let city = City::from_program(&program, None).unwrap();
    let traffic = TrafficPaths::new(&city, &program);
    // Every approach lane must have a successor, including the two lanes that
    // aren't themselves named by the lane_point attachment.
    for path in &traffic.paths {
        let end = *path.points.last().unwrap();
        if end.x.abs() < 0.001 || (end.x - 100.).abs() < 0.001 {
            assert_eq!(path.next.len(), 1, "disconnected lane at {end}");
            assert!(end.distance(traffic.paths[path.next[0]].points[0]) < 0.001);
        }
        // Traffic must follow the rendered taper/alignment, not the untapered
        // cross section's lane centres.
        for p in &path.points {
            let p = *p / METERS_PER_UNIT;
            assert_eq!(
                city.surface_at([p.x, p.y], p.z + 0.01).unwrap().kind,
                opencity::runtime::SurfaceKind::Asphalt
            );
        }
    }
    assert_eq!(traffic.paths.len(), 18);
}

#[test]
fn coincident_endpoints_with_distinct_names_stay_disconnected() {
    let program = opencity::parse(
        "road lane: carlane; direction forward; end
         a = map_point(0, 0); b = map_point(0, 0); start = map_point(-1, 0); finish = map_point(1, 0);
         first = lane(start, -0.7, 0, -0.3, 0, a);
         second = lane(b, 0.3, 0, 0.7, 0, finish);",
    )
    .unwrap();
    let city = City::from_program(&program, None).unwrap();
    let traffic = TrafficPaths::new(&city, &program);
    assert!(traffic.paths.iter().all(|p| p.next.is_empty()));
}
