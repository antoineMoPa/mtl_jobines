//! Authored road connectivity, tire support and imported bridge placement must agree.
use super::{external_models, routing::Router, traffic_paths::TrafficPaths};
use crate::dynamic_world::METERS_PER_UNIT;
use glam::Vec3;
use opencity::{
    map,
    runtime::{City, SurfaceKind},
    RoadElementKind,
};

#[test]
fn south_shore_crossing_has_clear_lanes_and_all_eight_connected_ramps() {
    let base = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl");
    let program = opencity::parse(include_str!("../../data/maps/mtl/map.road")).unwrap();
    let projection = map::project(&program);
    assert_eq!(projection.unresolved, 0);
    let city = City::from_program(&program, Some(&base)).unwrap();
    let geometry = opencity::road_geometry::RoadGeometry::compile(&program, &projection);
    let router = Router::new(&city, &program);
    let traffic = TrafficPaths::new(&city, &program);
    let (cycling, _) = TrafficPaths::for_bicycles(&city);
    for path in &cycling.paths {
        for point in &path.points {
            let p = *point / METERS_PER_UNIT;
            let on_bridge = (11.72..12.28).contains(&p.x) && (-36.1..-30.9).contains(&p.y);
            let on_cloverleaf = (8.0..16.0).contains(&p.x) && (-30.8..-26.0).contains(&p.y);
            assert!(
                !on_bridge && !on_cloverleaf,
                "cycling route on autoroute at {p}"
            );
        }
    }

    let mut ramps = 0;
    for (id, road) in city.roads.iter().enumerate() {
        let name = road.segment.name.as_deref().unwrap_or("");
        let crossing = name.starts_with("champlain_");
        let ramp = name.starts_with("south_clover_")
            && (name.contains("_loop_") || name.contains("_connector"));
        let local = name.starts_with("south_shore_");
        if !crossing && !ramp && !local {
            continue;
        }
        if ramp && (name.ends_with("_loop_b") || name.ends_with("_connector_down")) {
            ramps += 1;
        }
        for (reference, t) in [(&road.segment.start_ref, 0.), (&road.segment.end_ref, 1.)] {
            if let Some((parent, _, parent_t)) = reference
                .as_deref()
                .and_then(|r| map::lane_attachment(&program, r))
            {
                let parent = city
                    .roads
                    .iter()
                    .find(|r| r.segment.name.as_deref() == Some(parent.as_str()))
                    .unwrap();
                assert!(
                    (road.segment.height_at(t) - parent.segment.height_at(parent_t)).abs()
                        < 0.00001,
                    "height step at {name}"
                );
            }
        }
        let rows = geometry.cross_sections(id);
        for (lane, _) in road
            .definition
            .elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == RoadElementKind::CarLane)
        {
            for row in rows {
                let a = row.boundaries[lane];
                let b = row.boundaries[lane + 1];
                let p = Vec3::new(
                    (a.point.x + b.point.x) * 0.5,
                    (a.point.y + b.point.y) * 0.5,
                    a.height,
                );
                let surface = city.surface_at([p.x, p.y], p.z + 0.01).unwrap();
                assert_eq!(
                    surface.kind,
                    SurfaceKind::Asphalt,
                    "{name} lane {lane} at {p}"
                );
                assert!(
                    (surface.height - p.z).abs() < if ramp { 0.002 } else { 0.0001 },
                    "{name} wrong tire height at {p}: {}",
                    surface.height
                );
                assert!(
                    !city.building_collision([p.x, p.y], 0.008, p.z, p.z + 0.03),
                    "building across {name} lane {lane} at {p}"
                );
                assert!(
                    city.road_barrier_contact([p.x, p.y], 0.008, p.z, p.z + 0.03)
                        .is_none(),
                    "barrier across {name} lane {lane} at {p}"
                );
            }
            let row = &rows[rows.len() / 2];
            let a = row.boundaries[lane];
            let b = row.boundaries[lane + 1];
            let midpoint = Vec3::new(
                (a.point.x + b.point.x) * 0.5,
                (a.point.y + b.point.y) * 0.5,
                a.height,
            ) * METERS_PER_UNIT;
            let (index, path) = traffic
                .paths
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| a.project(midpoint).1.total_cmp(&b.project(midpoint).1))
                .unwrap();
            assert!(
                path.project(midpoint).1 < 0.5,
                "missing traffic on {name}: {}m",
                path.project(midpoint).1
            );
            assert!(!path.next.is_empty(), "traffic dead end on {name}");
            assert!(
                traffic.paths.iter().any(|p| p.next.contains(&index)),
                "no incoming traffic on {name}"
            );
        }
    }
    assert_eq!(ramps, 8, "four loops and four direct connectors");
    for start in [
        Vec3::new(7., -27.75, 0.102001),
        Vec3::new(17., -27.75, 0.102001),
        Vec3::new(12., -25.4, 0.102001),
    ] {
        let shore = Vec3::new(12., -42., 0.102001);
        assert!(
            router.can_route(start, shore),
            "no route to South Shore from {start}"
        );
        assert!(
            router.can_route(shore, start),
            "no route back from South Shore to {start}"
        );
    }
    let dry = city.surface_at([8., -43.], 1.).unwrap();
    assert_eq!(dry.kind, SurfaceKind::Grass);
    assert!((dry.height - 0.1).abs() < 0.0001);
    let water = city.surface_at([12., -33.], 0.1).unwrap();
    assert_eq!(
        water.kind,
        SurfaceKind::Water,
        "river must remain open below the bridge"
    );
}

#[test]
fn imported_champlain_fits_the_pavement_and_leaves_structural_strips_clear() {
    let program = opencity::parse(include_str!("../../data/maps/mtl/map.road")).unwrap();
    let features = opencity::project_landscape_features(&program);
    let feature = features
        .features
        .iter()
        .find(|f| f.name.as_deref() == Some("champlain_model"))
        .unwrap();
    let mesh = external_models::decode(include_bytes!(
        "../../data/maps/mtl/models/champlain-bridge.glb"
    ))
    .unwrap();
    let transform = external_models::instances::transform(&mesh, feature).unwrap();
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    for v in &mesh.vertices {
        let p = transform.transform_point3(Vec3::from_array(v.position));
        lo = lo.min(p);
        hi = hi.max(p);
    }
    assert!(lo.distance(Vec3::new(11.72, -36.1, -0.02)) < 0.0001);
    assert!(hi.distance(Vec3::new(12.28, -30.9, 1.09)) < 0.0001);
    assert!(!feature.building_style.unwrap().collidable);
    // Original deck z=24m must agree with the authored pavement within its 2cm overlay.
    let deck = transform.transform_point3(Vec3::new(0., 0., 24.));
    assert!((deck.z - 0.24).abs() < 0.00001);
    // Three 4.5m lanes fit inside each widened carriageway, outside tower/stay strips.
    for (center, side) in [(11.7984, -1.), (12.2016, 1.)] {
        for offset in [-0.0675, 0.0675] {
            let across: f32 = (center + offset - 12.) * side;
            assert!(across > 9.45 * 1.4 / 100. && across < 19.35 * 1.4 / 100.);
        }
    }
}
