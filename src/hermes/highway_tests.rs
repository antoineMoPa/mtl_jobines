//! Both highway climbs must carry connected lanes with clear pavement above the terrain.
use super::{routing::Router, traffic_paths::TrafficPaths};
use crate::dynamic_world::METERS_PER_UNIT;
use glam::Vec3;
use opencity::{
    map,
    runtime::{City, SurfaceKind},
    RoadDirection, RoadElementKind,
};

#[test]
fn highway_climbs_have_pavement_and_connected_lanes() {
    let root = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl");
    let program = opencity::parse(include_str!("../../data/maps/mtl/map.road")).unwrap();
    assert_eq!(map::project(&program).unresolved, 0);
    let city = City::from_program(&program, Some(&root)).unwrap();
    let router = Router::new(&city, &program);
    let traffic = TrafficPaths::new(&city, &program);
    let projection = map::project(&program);
    let geometry = opencity::road_geometry::RoadGeometry::compile(&program, &projection);
    // Sample the compiled pavement: adjoining profiles can taper the shoulders
    // and shift lane centres near the crest.
    let point = |road: &opencity::runtime::Road, lane: usize, t: f32| {
        let index = projection
            .segments
            .iter()
            .position(|s| s.source == road.segment.source)
            .unwrap();
        let row = geometry
            .cross_sections(index)
            .iter()
            .min_by(|a, b| (a.t - t).abs().total_cmp(&(b.t - t).abs()))
            .unwrap();
        let a = row.boundaries[lane];
        let b = row.boundaries[lane + 1];
        Vec3::new(
            (a.point.x + b.point.x) / 2.,
            (a.point.y + b.point.y) / 2.,
            a.height,
        )
    };

    // The report location used to be grass inside an otherwise empty excavation.
    let reported = city.surface_at([-0.41013375, 7.4590015], 1.).unwrap();
    assert_eq!(reported.kind, SurfaceKind::Asphalt);
    assert!(reported.road.is_some());

    for (climb_name, parent_name, join_t, parent_t) in [
        ("highway_north_climb", "highway_north", 1., 0.),
        ("highway_south_climb", "highway_south", 0., 1.),
        ("highway_north_east_climb", "highway_north", 0., 1.),
        ("highway_south_east_climb", "highway_south", 1., 0.),
    ] {
        let road = |name| {
            city.roads
                .iter()
                .find(|r| r.segment.name.as_deref() == Some(name))
                .unwrap()
        };
        let climb = road(climb_name);
        let parent = road(parent_name);
        let lanes: Vec<_> = climb
            .definition
            .elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == RoadElementKind::CarLane)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(lanes.len(), 3);
        for lane in lanes {
            assert_eq!(
                climb.definition.lane_direction(lane),
                Some(RoadDirection::Forward)
            );
            let seam = point(climb, lane, join_t);
            assert!(seam.distance(point(parent, lane, parent_t)) < 0.00001);
            for step in 0..=120 {
                let p = point(climb, lane, step as f32 / 120.);
                let surface = city.surface_at([p.x, p.y], p.z + 0.04).unwrap();
                assert_eq!(
                    surface.kind,
                    SurfaceKind::Asphalt,
                    "{climb_name} lane {lane} at {p}"
                );
                assert!((surface.height - p.z).abs() < 0.00001);
                assert!(
                    city.road_barrier_contact([p.x, p.y], 0.008, p.z, p.z + 0.04)
                        .is_none(),
                    "blocked {climb_name} lane {lane} at {p}"
                );
            }

            let (from, to) = if join_t == 1. {
                (point(climb, lane, 0.5), point(parent, lane, 0.05))
            } else {
                (point(parent, lane, 0.95), point(climb, lane, 0.5))
            };
            assert!(
                router.can_route(from, to),
                "disconnected {climb_name} lane {lane}"
            );

            // Traffic needs an actual successor at the seam, not just nearby asphalt.
            let seam = seam * METERS_PER_UNIT;
            let incoming = traffic
                .paths
                .iter()
                .find(|p| p.points.last().unwrap().distance(seam) < 0.001)
                .expect("incoming traffic lane");
            assert!(
                incoming
                    .next
                    .iter()
                    .any(|&i| traffic.paths[i].points[0].distance(seam) < 0.001),
                "traffic cannot cross {climb_name} lane {lane}"
            );
        }
    }

    // The southern crest must reach the unmodified ground, not stop below a terrain wall.
    let elevation = city.elevation.as_ref().unwrap();
    for (name, crest_t) in [
        ("highway_north_east_climb", 1.),
        ("highway_south_east_climb", 0.),
    ] {
        let road = city
            .roads
            .iter()
            .find(|r| r.segment.name.as_deref() == Some(name))
            .unwrap();
        for lane in 1..=3 {
            let p = point(road, lane, crest_t);
            let pavement = road.segment.height_at(crest_t);
            let ground = elevation.original_height_at([p.x, p.y]).unwrap();
            assert!((pavement - ground - 0.002).abs() < 0.00001);
            // Check beyond the authored pavement as well as directly on its last row.
            assert!(
                city.road_barrier_contact([p.x, p.y - 0.005], 0.008, ground, ground + 0.04)
                    .is_none(),
                "terrain wall across {name} lane {lane} crest"
            );
        }
    }
}

#[test]
fn report_85_ring_road_has_drivable_shoulder_openings() {
    let root = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl");
    let program = opencity::parse(include_str!("../../data/maps/mtl/map.road")).unwrap();
    let city = City::from_program(&program, Some(&root)).unwrap();
    let projection = map::project(&program);
    let geometry = opencity::road_geometry::RoadGeometry::compile(&program, &projection);
    for (name, expected_gaps) in [("ring_east_lead", 2), ("highway_north", 0)] {
        let index = projection
            .segments
            .iter()
            .position(|s| s.name.as_deref() == Some(name))
            .unwrap();
        let rows = geometry.cross_sections(index);
        for side in [0, rows[0].boundaries.len() - 1] {
            let mut gaps = 0;
            let mut length = 0.;
            for pair in rows.windows(2) {
                // The underground carriageway has authored ramp mouths outside this stretch.
                if name == "highway_north" && !(0.35..0.75).contains(&pair[0].t) {
                    continue;
                }
                let a = pair[0].boundaries[side];
                let b = pair[1].boundaries[side];
                let xy = [(a.point.x + b.point.x) * 0.5, (a.point.y + b.point.y) * 0.5];
                let blocked = city
                    .road_barrier_contact(xy, 0.01, a.height, a.height + 0.02)
                    .is_some();
                if name == "highway_north" {
                    assert!(blocked, "underground barrier missing at {xy:?}");
                }
                if !blocked {
                    length += (b.point.x - a.point.x).hypot(b.point.y - a.point.y);
                } else {
                    if length >= 0.06 {
                        gaps += 1;
                    }
                    length = 0.;
                }
            }
            gaps += usize::from(length >= 0.06);
            assert_eq!(gaps, expected_gaps, "{name} side {side}");
        }
    }
}
