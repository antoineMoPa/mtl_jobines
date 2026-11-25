//! cargo run --example building_memory --features memory-profile -- /tmp/building-memory.json
fn main() {
    use mtl_world::hermes::{external_models, Hermes};
    use opencity::runtime::architecture::BuildingRecipe;
    use serde_json::json;
    use std::{
        collections::{BTreeMap, HashMap, HashSet},
        mem::size_of,
        path::Path,
    };
    let hermes = Hermes::load_with_game_data(Path::new("data/maps/mtl/map.road"), None).unwrap();
    let city = hermes.city();
    let profile = city.building_memory();
    let details = hermes.building_detail_memory();
    let mut detail_rows = HashMap::<usize, (usize, usize, usize, usize)>::new();
    for d in &details {
        let row = detail_rows.entry(d.feature).or_default();
        row.0 += d.scenery_vertices;
        row.1 += d.prop_vertices;
        row.2 += d.prop_capacity_bytes;
        row.3 += d.prop_meshes;
    }
    let mut buildings = Vec::new();
    for row in &profile.buildings {
        let f = &city.landscape[row.feature];
        let recipe = BuildingRecipe::from_feature(f);
        let detail = detail_rows.get(&row.feature).copied().unwrap_or_default();
        let kinds: BTreeMap<_, _> = row
            .surfaces
            .iter()
            .map(|(kind, n)| (format!("{kind:?}"), *n))
            .collect();
        buildings.push(json!({"feature":row.feature,"name":f.name,"model":f.model,"use":format!("{:?}",f.building_use),
            "heightMeters":f.height.unwrap_or(0.)*100.,"points":f.points,"style":format!("{:?}",f.building_style),
            "recipe":recipe.map(|r| json!({"family":format!("{:?}",r.family),"widthMeters":r.width*100.,"depthMeters":r.depth*100.,"floorHeightMeters":r.floor_height*100.,"section":r.section,"podiumFloors":r.podium_floors,"roofUnits":r.roof_units})),
            "triangles":row.triangles,"proxyTriangles":row.proxy_triangles,"surfaces":kinds,
            "triangleBytes":row.triangles*profile.triangle_bytes,"indexBytes":(row.cell_references+row.support_references+row.building_references)*profile.pointer_bytes,
            "cellReferences":row.cell_references,"supportReferences":row.support_references,"buildingReferences":row.building_references,
            "volumeBytes":row.volume_bytes,"recipeBytes":row.recipe_bytes,"recipeReferences":row.recipe_references,"compiledAttributedBytes":row.triangles*profile.triangle_bytes+(row.cell_references+row.support_references+row.building_references)*profile.pointer_bytes+row.volume_bytes+row.recipe_bytes+row.recipe_references*profile.pointer_bytes,
            "expandedRenderVertexBytes":(row.triangles-row.proxy_triangles)*3*52,
            "detailSceneryBytes":detail.0*52,"detailPropUsedBytes":detail.1*52,"detailPropCapacityBytes":detail.2,"detailPropMeshes":detail.3
        }));
    }
    let mut paths = Vec::<String>::new();
    let mut placements = HashMap::<String, Vec<String>>::new();
    for (id, f) in city.landscape.iter().enumerate() {
        let Some(path) = f
            .model
            .as_deref()
            .filter(|p| p.ends_with(".glb") && *p != "models/orange-sign.glb")
        else {
            continue;
        };
        if !placements.contains_key(path) {
            paths.push(path.to_string());
        }
        placements
            .entry(path.to_string())
            .or_default()
            .push(f.name.clone().unwrap_or_else(|| format!("feature_{id}")));
    }
    let offset = hermes.external_models.len() - paths.len();
    let mut assets = Vec::new();
    let mut backing_ids = HashMap::new();
    let mut layer_users = BTreeMap::<usize, Vec<String>>::new();
    for (path, group) in paths.iter().zip(&hermes.external_models[offset..]) {
        assert_eq!(placements[path].len(), group.transforms.len());
        let mesh = &group.mesh;
        let next_backing = backing_ids.len();
        let backing = *backing_ids
            .entry(std::sync::Arc::as_ptr(mesh))
            .or_insert(next_backing);
        let unique: HashSet<[u32; 13]> = mesh
            .vertices
            .iter()
            .map(|v| {
                v.position
                    .into_iter()
                    .chain(v.normal)
                    .chain(v.color)
                    .chain(v.surface)
                    .map(f32::to_bits)
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap()
            })
            .collect();
        let mut layers = BTreeMap::<usize, usize>::new();
        for v in &mesh.vertices {
            if v.surface[0] == external_models::textures::MATERIAL {
                *layers.entry(v.surface[3] as usize).or_default() += 1;
            }
        }
        for layer in layers.keys() {
            layer_users.entry(*layer).or_default().push(path.clone());
        }
        let bytes = std::fs::read(Path::new("data/maps/mtl").join(path)).unwrap();
        let raw = gltf::binary::Glb::from_slice(&bytes).unwrap();
        let doc = gltf::Document::from_json(serde_json::from_slice(&raw.json).unwrap()).unwrap();
        let mut nodes = Vec::new();
        for node in doc.nodes() {
            if let Some(mesh) = node.mesh() {
                let triangles: usize = mesh
                    .primitives()
                    .map(|p| {
                        p.indices()
                            .map(|a| a.count())
                            .or_else(|| p.get(&gltf::Semantic::Positions).map(|a| a.count()))
                            .unwrap_or(0)
                            / 3
                    })
                    .sum();
                nodes.push(json!({"name":node.name(),"triangles":triangles}));
            }
        }
        nodes.sort_by_key(|n| std::cmp::Reverse(n["triangles"].as_u64().unwrap()));
        let images: Vec<_> = doc
            .images()
            .map(|image| match image.source() {
                gltf::image::Source::View { view, .. } => {
                    let binary = raw.bin.as_deref().unwrap();
                    let png = &binary[view.offset()..view.offset() + view.length()];
                    let dims = png::Decoder::new(png)
                        .read_info()
                        .ok()
                        .map(|r| [r.info().width, r.info().height]);
                    json!({"compressedBytes":view.length(),"dimensions":dims})
                }
                _ => json!({"external":true}),
            })
            .collect();
        assets.push(json!({"path":path,"backing":backing,"meshArcOwners":std::sync::Arc::strong_count(mesh),"nodes":nodes,"placements":placements[path],"instances":group.transforms.len(),"fileBytes":bytes.len(),
            "vertices":mesh.vertices.len(),"capacityVertices":mesh.vertices.capacity(),"triangles":mesh.vertices.len()/3,
            "decodedMeshUsedBytes":mesh.vertices.len()*52,"decodedMeshCapacityBytes":mesh.vertices.capacity()*52,"transformBytes":group.transforms.capacity()*size_of::<glam::Mat4>(),
            "uniqueFullVertices":unique.len(),"indexedVertexAndIndexBytes":unique.len()*52+mesh.vertices.len()*4,
            "sourcePositionCount":doc.meshes().flat_map(|m|m.primitives()).filter_map(|p|p.get(&gltf::Semantic::Positions)).map(|a|a.count()).sum::<usize>(),
            "textureLayerVertices":layers,"images":images}));
    }
    let mut landmarks = Vec::new();
    for f in &city.landscape {
        let Some(id) = f.model.as_deref().filter(|id| id.starts_with("study_")) else {
            continue;
        };
        let mesh = mtl_world::hermes::landmarks::source_mesh(id).unwrap();
        landmarks.push(json!({"name":f.name,"model":id,"triangles":mesh.vertices.len()/3,"expandedRenderVertexBytes":mesh.vertices.len()*52}));
    }
    let layers = external_models::textures::snapshot();
    let result = json!({"sourceCommit":std::env::var("BUILDING_AUDIT_COMMIT").unwrap_or_else(|_| "unspecified".into()),"target":"native diagnostic; byte sizes follow native layout, counts are platform-independent",
        "pointerBytes":profile.pointer_bytes,"compiledTriangleBytes":profile.triangle_bytes,
        "totals":{"buildings":buildings.len(),"landscapeFeatures":city.landscape.len(),"allTriangles":profile.triangle_count,"triangleCapacity":profile.triangle_capacity,
            "allTriangleUsedBytes":profile.triangle_count*profile.triangle_bytes,"allTriangleCapacityBytes":profile.triangle_capacity*profile.triangle_bytes,
            "cellReferenceBytes":profile.cell_reference_count*profile.pointer_bytes,"cellReferenceCapacityBytes":profile.cell_reference_capacity*profile.pointer_bytes,
            "supportReferenceBytes":profile.support_reference_count*profile.pointer_bytes,"supportReferenceCapacityBytes":profile.support_reference_capacity*profile.pointer_bytes,
            "proceduralRecipeCount":profile.recipe_count,"proceduralRecipeBytes":profile.recipe_bytes,"proceduralRecipeIndexBytes":profile.recipe_index_bytes,"buildingMeshCacheBytes":profile.building_mesh_cache_bytes,"buildingMeshCacheBudgetBytes":profile.building_mesh_cache_budget,
            "staticTextureLayersUsed":layers.len(),"staticTextureCpuBytes":layers.iter().map(|l|l.colour.len()+l.normal.len()).sum::<usize>(),
            "staticTextureArrayCapacity":external_models::textures::CAPACITY},
        "buildings":buildings,"sharedAssets":assets,"landmarks":landmarks,"sharedTextureLayerUsers":layer_users,
        "notes":["Per-building compiled bytes include triangle payload, used spatial-index references and volume Vec allocation. Shared Vec slack, hash buckets, metadata, allocators and custom colliders are not distributed.","Imported meshes are shared once per asset; never multiply decoded mesh RAM by placement count.","Expanded render vertex bytes are a geometry-size estimate; tiled scenery is streamed, not all resident.","Texture layers are shared; GPU arrays reserve 32 layers independent of actual use. No GPU process/RSS measurement is made here."]});
    let output = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/building-memory.json".into());
    std::fs::write(&output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    eprintln!("Building memory report written to {output}");
}
