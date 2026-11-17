use super::super::*;
#[test]
fn original_house_glbs_keep_uvs_and_share_decodes_and_texture_layers() {
    for bytes in [
        include_bytes!("../../../../data/maps/mtl/models/house-garden.glb").as_slice(),
        include_bytes!("../../../../data/maps/mtl/models/house-manor.glb").as_slice(),
    ] {
        let mesh = decode_shared(bytes).unwrap();
        let again = decode_shared(bytes).unwrap();
        assert!(std::sync::Arc::ptr_eq(&mesh, &again));
        let textured: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.surface[0] == textures::MATERIAL)
            .collect();
        assert!(!textured.is_empty());
        assert!(textured.iter().any(|v| v.surface[1].abs() > 1.));
        for v in textured {
            assert!(v.surface[3] >= 0. && v.surface[3] < textures::CAPACITY as f32);
            assert!(v.color.iter().all(|c| *c >= 0. && *c <= 1.));
        }
    }
}
#[test]
fn house_streets_load_two_assets_once_and_keep_73_transforms() {
    let root = Path::new(env!("MTL_WORKSPACE_DIR"));
    let source = std::fs::read_to_string(root.join("data/maps/mtl/map.road")).unwrap();
    let program = opencity::parse(&source).unwrap();
    let mut city = City::from_program_with_elevation_loader(&program, |spec| {
        opencity::heightmap::ElevationMap::from_png(
            spec.clone(),
            &std::fs::read(root.join("data/maps/mtl/elevation.png")).unwrap(),
        )
    })
    .unwrap();
    let mut reads = 0;
    city.landscape.retain(|f| {
        f.model
            .as_deref()
            .is_some_and(|p| p.starts_with("models/house-"))
    });
    let houses = city;
    let groups = instances::groups(&houses, |path| {
        reads += 1;
        std::fs::read(root.join("data/maps/mtl").join(path)).map_err(|e| e.to_string())
    })
    .unwrap();
    assert_eq!(reads, 2);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups.iter().map(|g| g.transforms.len()).sum::<usize>(), 73);
    for group in &groups {
        assert!(group.transforms.iter().all(|m| m.is_finite()));
    }
    let feature = &houses.landscape[0];
    let bytes = std::fs::read(
        root.join("data/maps/mtl")
            .join(feature.model.as_ref().unwrap()),
    )
    .unwrap();
    let mesh = decode_shared(&bytes).unwrap();
    let placed = place(&mesh, feature).unwrap();
    let matrix = instances::transform(&mesh, feature).unwrap();
    for (raw, placed) in mesh.vertices.iter().zip(&placed.vertices).step_by(97) {
        assert!(
            matrix
                .transform_point3(Vec3::from_array(raw.position))
                .distance(Vec3::from_array(placed.position))
                < 0.00001
        );
    }
}
