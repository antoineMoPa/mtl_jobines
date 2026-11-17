//! Authored junctions must connect streets rather than open onto unused stubs.
use opencity::{map, network};
use std::collections::HashMap;

const SOURCE: &str = include_str!("../../data/maps/mtl/map.road");

#[test]
fn neighborhood_intersections_have_no_terminal_approach_stubs() {
    let program = opencity::parse(SOURCE).unwrap();
    let projection = map::project(&program);
    assert_eq!(projection.unresolved, 0);
    let mut degree = HashMap::new();
    for segment in &projection.segments {
        for reference in [&segment.start_ref, &segment.end_ref].into_iter().flatten() {
            *degree.entry(reference.as_str()).or_insert(0) += 1;
        }
    }
    let mut stubs = Vec::new();
    for junction in map::intersections(&program, &projection) {
        for (index, is_end) in map::intersection_arms(&projection, &junction.point_ref) {
            let segment = &projection.segments[index];
            // Highway/service-road boundaries are intentional map entrances.
            if !matches!(
                segment.road_name.as_str(),
                "cross_street" | "cycling_street"
            ) {
                continue;
            }
            let outer = if is_end {
                &segment.start_ref
            } else {
                &segment.end_ref
            };
            if let Some(outer) = outer {
                if degree.get(outer.as_str()) == Some(&1) {
                    stubs.push(format!("{} -> {outer}", junction.point_ref));
                }
            }
        }
    }
    assert!(stubs.is_empty(), "unused intersection arms: {stubs:?}");
}

#[test]
fn report_80_is_a_three_way_and_the_hill_access_is_a_turn() {
    let program = opencity::parse(SOURCE).unwrap();
    let projection = map::project(&program);
    let junctions = map::intersections(&program, &projection);
    for (name, kind) in [
        ("junction_36", map::IntersectionKind::ThreeWay),
        ("junction_28", map::IntersectionKind::RightTurn),
    ] {
        let junction = junctions.iter().find(|j| j.point_ref == name).unwrap();
        assert_eq!(junction.kind, kind);
        let arms = map::intersection_arms(&projection, name);
        assert_eq!(arms.len(), kind.arm_count());
        // Neither junction should retain an east-facing road or lane movement.
        for (index, is_end) in arms {
            let segment = &projection.segments[index];
            let tangent = segment.tangent_at(if is_end { 1. } else { 0. });
            let outward_x = if is_end { -tangent.x } else { tangent.x };
            assert!(outward_x < tangent.y.abs(), "{name} has an eastern stub");
        }
        let instance = network::default_three_way_instance(&program, &projection, junction)
            .or_else(|| network::default_right_turn_instance(&program, &projection, junction))
            .expect("junction must compile its pavement, sidewalks and lane movements");
        assert!(!instance.geometry.movements.is_empty());
        assert_eq!(instance.geometry.ports.len(), kind.arm_count() * 2);
        if kind == map::IntersectionKind::ThreeWay {
            assert_eq!(instance.geometry.movements.len(), 6);
            assert!(instance.geometry.bands.iter().any(|band| {
                band.name.starts_with("closed_")
                    && band.material
                        == network::Material::RoadElement(opencity::RoadElementKind::Sidewalk)
            }));
        }
    }
    assert!(!projection
        .segments
        .iter()
        .any(|s| { matches!(s.name.as_deref(), Some("road_29" | "road_38")) }));
}
