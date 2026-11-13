use super::*;

#[test]
fn collecteur_keeps_separate_glass_small_cabin_dual_rear_tires_and_lamps() {
    let source = gltf::Gltf::from_slice(include_bytes!(
        "../../data/vehicle/municipal/castor-collecteur/model.glb"
    ))
    .unwrap();
    let definition = definitions()
        .iter()
        .find(|d| d.config.research_id == "vehicle-j05")
        .unwrap();
    assert_eq!(definition.config.name, "Castor Collecteur");
    assert_eq!(definition.config.category, "municipal");
    assert!(definition.config.road_vehicle);
    assert_eq!(definition.config.physics.mass, 15000.);
    assert_eq!(definition.config.physics.max_speed, 36.);
    assert_eq!(
        definition.config.sound.engine,
        crate::audio::EngineKind::Diesel
    );
    assert_eq!(source.images().count(), 0);
    assert_eq!(definition.config.qa["wheelStyle"], "plain-steel-cylinder");
    assert_eq!(definition.config.qa["rearDualTires"], true);
    let nodes: std::collections::HashMap<_, _> =
        source.nodes().map(|n| (n.name().unwrap(), n)).collect();
    assert_eq!(nodes.len(), source.nodes().count());
    for name in [
        "body_shell",
        "cab_roof",
        "door_left",
        "door_right",
        "hopper_inner",
        "hopper_side_left",
        "hopper_side_right",
        "rear_loading_mechanism",
        "loader_safety_step",
        "head_fl",
        "head_fr",
        "taillight_-1",
        "taillight_1",
    ] {
        assert!(
            nodes.contains_key(name),
            "missing refuse truck part: {name}"
        );
    }
    let interior_primitives: usize = nodes
        .iter()
        .filter(|(name, _)| name.starts_with("interior_"))
        .map(|(_, node)| node.mesh().unwrap().primitives().count())
        .sum();
    assert_eq!(interior_primitives, 12);
    assert_eq!(
        definition.config.qa["interiorPrimitives"],
        interior_primitives
    );
    let glass: Vec<_> = nodes
        .iter()
        .filter(|(name, _)| name.starts_with("glass_") || **name == "windshield")
        .collect();
    assert_eq!(glass.len(), 3);
    for (_, node) in glass {
        for primitive in node.mesh().unwrap().primitives() {
            assert_eq!(
                primitive.material().alpha_mode(),
                gltf::material::AlphaMode::Blend
            );
            assert_eq!(
                primitive
                    .material()
                    .pbr_metallic_roughness()
                    .base_color_factor()[3],
                0.7
            );
        }
    }
    let blob = source.blob.as_deref().unwrap();
    for name in [
        "wheel_front_left",
        "wheel_front_right",
        "wheel_rear_left",
        "wheel_rear_right",
    ] {
        let node = &nodes[name];
        let wheel = node.mesh().unwrap();
        assert_eq!(
            wheel.primitives().count(),
            2,
            "rubber and plain steel: {name}"
        );
        let steel = wheel
            .primitives()
            .find(|p| p.material().pbr_metallic_roughness().metallic_factor() > 0.5)
            .unwrap();
        let copies = if name.contains("rear") { 2 } else { 1 };
        assert_eq!(
            steel
                .reader(|_| Some(blob))
                .read_indices()
                .unwrap()
                .into_u32()
                .count(),
            copies * 48 * 4 * 3
        );
        let pivot = Vec3::from(node.transform().decomposed().0);
        assert!((pivot.x - if name.contains("front") { 2.48 } else { -1.02 }).abs() < 1e-4);
        assert!((pivot.y - definition.config.physics.wheel_radius).abs() < 1e-4);
        assert!((pivot.z.abs() * 2. - definition.config.physics.track).abs() < 1e-4);
    }
    assert!((definition.config.physics.wheelbase - 3.5).abs() < 1e-4);
    assert_eq!(definition.headlights.len(), 2);
    assert_eq!(definition.rear_emitters.len(), 2);
    assert!(definition.parts.iter().flat_map(|p| &p.vertices).all(|v| v
        .position
        .into_iter()
        .chain(v.normal)
        .all(f32::is_finite)));
}
