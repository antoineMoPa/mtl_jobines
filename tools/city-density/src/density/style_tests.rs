//! The authored eastern districts retain their distinct street-level architecture.
use super::Index;

fn city() -> opencity::Program {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/mtl");
    opencity::parse(&std::fs::read_to_string(root.join("map.road")).unwrap()).unwrap()
}

#[test]
fn downtown_transitions_to_brick_and_existing_monument_assets() {
    let features = opencity::project_landscape_features(&city());
    let mut towers = 0;
    let mut brick = 0;
    for f in features.features.iter().filter(|f| {
        f.name
            .as_deref()
            .is_some_and(|n| n.starts_with("generated_area_infill_"))
    }) {
        let center = f
            .points
            .iter()
            .fold([0.; 2], |p, q| [p[0] + q[0] / 4., p[1] + q[1] / 4.]);
        if !(7.5..15.7).contains(&center[1]) {
            continue;
        }
        let style = f.building_style.unwrap();
        if (21.2..24.35).contains(&center[0]) {
            assert_eq!(f.building_use.keyword(), "mixed_use");
            assert_ne!(style.architecture.family.keyword(), "legacy");
            assert!(f.height.unwrap() >= 0.19);
            if f.height.unwrap() >= 0.5 {
                towers += 1;
            }
        } else if (24.35..26.).contains(&center[0]) {
            assert!(matches!(
                style.material.keyword(),
                "red_brick" | "brown_brick"
            ));
            assert_eq!(style.architecture.family.keyword(), "heritage");
            brick += 1;
        }
    }
    assert!(towers >= 20);
    assert!(brick >= 20);
    let mut lots = Index::default();
    for (name, asset) in [
        ("city_hall", "study_09"),
        ("cathedral", "study_12"),
        ("chateau", "study_20"),
    ] {
        let name = format!("brick_landmark_{name}");
        let f = features
            .features
            .iter()
            .find(|f| f.name.as_deref() == Some(&name))
            .unwrap();
        assert_eq!(f.model.as_deref(), Some(asset));
        assert!(f
            .points
            .iter()
            .all(|p| (24.40..=25.30).contains(&p[0]) && (8.25..=10.82).contains(&p[1])));
        assert!(!lots.overlaps(&f.points, None, false));
        lots.insert(f.points.clone());
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../data/landmarks/{}.mesh", &asset[6..]));
        assert!(std::fs::read(path).unwrap().starts_with(b"LMK1"));
    }
}

#[test]
fn brick_quarter_has_connected_streets_inside_the_ring() {
    let program = city();
    let projection = opencity::map::project(&program);
    let junctions = opencity::map::intersections(&program, &projection);
    for (row, arms) in [2, 3, 4, 2].into_iter().enumerate() {
        let name = format!("brick_junction_{row}");
        let junction = junctions.iter().find(|j| j.point_ref == name).unwrap();
        assert_eq!(junction.kind.arm_count(), arms);
        assert!((junction.position.x - 25.4).abs() < 0.00001);
        let attached = projection
            .segments
            .iter()
            .filter(|r| {
                r.start_ref.as_deref() == Some(&name) || r.end_ref.as_deref() == Some(&name)
            })
            .count();
        assert_eq!(attached, arms);
    }
}
