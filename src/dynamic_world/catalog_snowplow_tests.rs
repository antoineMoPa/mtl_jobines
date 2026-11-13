use super::*;

const MODEL: &[u8] = include_bytes!("../../data/vehicle/municipal/castor-blizzard/model.glb");

#[test]
fn snowplow_keeps_three_axles_plain_steel_rims_and_a_small_cabin() {
    let source = gltf::Gltf::from_slice(MODEL).unwrap();
    let definition = definitions()
        .iter()
        .find(|d| d.config.research_id == "vehicle-j03")
        .unwrap();
    assert_eq!(definition.config.name, "Castor Blizzard");
    assert_eq!(
        definition.config.sound.engine,
        crate::audio::EngineKind::Diesel
    );
    assert_eq!(source.images().count(), 0);
    assert_eq!(definition.config.qa["interiorPrimitives"], 13);
    assert_eq!(definition.headlights.len(), 2);
    assert_eq!(definition.rear_emitters.len(), 2);
    let names: std::collections::HashSet<_> = source.nodes().filter_map(|n| n.name()).collect();
    assert_eq!(
        names.len(),
        source.nodes().count(),
        "part names must be unique"
    );
    for name in [
        "plow_blade_left",
        "plow_blade_right",
        "dump_body_left",
        "dump_body_right",
        "dump_floor",
        "windshield",
        "rear_glass",
        "glass_left",
        "glass_right",
        "roof_amber_lens",
        "interior_dashboard",
    ] {
        assert!(names.contains(name), "missing snowplow part {name}");
    }
    let binary = source.blob.as_deref().unwrap();
    let wheels: Vec<_> = source
        .nodes()
        .filter(|n| n.name().unwrap().starts_with("wheel_"))
        .collect();
    assert_eq!(wheels.len(), 6);
    for wheel in wheels {
        let name = wheel.name().unwrap();
        let pivot = wheel.transform().decomposed().0;
        let axle = if name.contains("front") {
            2.2
        } else if name.contains("middle") {
            -1.75
        } else {
            -3.2
        };
        assert!((pivot[0] - axle).abs() < 1e-5);
        assert!((pivot[1] - definition.config.physics.wheel_radius).abs() < 1e-5);
        assert!((pivot[2].abs() * 2. - definition.config.physics.track).abs() < 1e-5);
        let mesh = wheel.mesh().unwrap();
        let steel = mesh
            .primitives()
            .find(|p| {
                p.material().pbr_metallic_roughness().base_color_factor() == [0.39, 0.44, 0.47, 1.]
            })
            .unwrap();
        assert!(steel.material().pbr_metallic_roughness().metallic_factor() > 0.6);
        let reader = steel.reader(|_| Some(binary));
        assert_eq!(
            reader.read_indices().unwrap().into_u32().count(),
            48 * 4 * 3
        );
        for position in reader.read_positions().unwrap() {
            let r = position[0].hypot(position[1]);
            assert!(
                r < 1e-5 || (r - 0.445).abs() < 1e-5,
                "rim must be a plain closed cylinder"
            );
            assert!((position[2].abs() - 0.215).abs() < 1e-5);
        }
    }
}

#[test]
fn snowplow_dirt_vertex_colors_survive_the_runtime_loader() {
    let source = gltf::Gltf::from_slice(MODEL).unwrap();
    let binary = source.blob.as_deref().unwrap();
    let definition = definitions()
        .iter()
        .find(|d| d.config.research_id == "vehicle-j03")
        .unwrap();
    let body = definition
        .parts
        .iter()
        .find(|p| p.role == PartRole::Body)
        .unwrap();
    let mut darkest = 1f32;
    let mut lightest = 0f32;
    for node in source
        .nodes()
        .filter(|n| n.name().unwrap().starts_with("dump_"))
    {
        for primitive in node.mesh().unwrap().primitives() {
            let reader = primitive.reader(|_| Some(binary));
            let positions: Vec<_> = reader.read_positions().unwrap().collect();
            let colors: Vec<_> = reader
                .read_colors(0)
                .expect("weathered bin requires vertex colors")
                .into_rgba_f32()
                .collect();
            assert_eq!(positions.len(), colors.len());
            for color in &colors {
                darkest = darkest.min(color[0]);
                lightest = lightest.max(color[0]);
                assert!(color[0] < 0.49 && color[1] < 0.36 && color[2] < 0.14);
            }
            for i in (0..positions.len()).step_by((positions.len() / 8).max(1)) {
                let expected =
                    game_vector(positions[i]) - Vec3::Z * definition.config.physics.height_offset;
                let color = [colors[i][0], colors[i][1], colors[i][2]];
                assert!(
                    body.vertices
                        .iter()
                        .any(|v| Vec3::from_array(v.position).distance(expected) < 1e-5
                            && v.color == color),
                    "runtime lost bin color at {expected:?}"
                );
            }
        }
    }
    assert!(
        lightest - darkest > 0.15,
        "bin lacks dirt buildup and faded paint variation"
    );
    assert!(
        body.vertices.iter().any(|v| v.color == [0.95, 0.59, 0.025]),
        "cab paint changed with bin weathering"
    );
}
