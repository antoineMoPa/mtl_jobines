//! Compare deterministic regional geometry, support and collision before/after residency changes.
use mtl_world::hermes::Hermes;
use opencity::runtime::{architecture::BuildingRecipe, Region};
use serde_json::json;
use std::{path::Path, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let source = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("data/maps/mtl/map.road");
    let output = args
        .get(2)
        .map(String::as_str)
        .unwrap_or("/tmp/procedural-residency.json");
    let started = Instant::now();
    let hermes = Hermes::load_with_game_data(Path::new(source), None).unwrap();
    let startup_ms = started.elapsed().as_secs_f64() * 1000.;
    let city = hermes.city();
    let profile = city.building_memory();
    let mut regions = Vec::new();
    for f in &city.landscape {
        let Some(recipe) = BuildingRecipe::from_feature(f) else {
            continue;
        };
        let center: [f32; 2] =
            std::array::from_fn(|a| f.points.iter().map(|p| p[a] / f.points.len() as f32).sum());
        let started = Instant::now();
        let mesh = city
            .mesh_with_external_models(
                Region::Radius {
                    center,
                    radius: 0.8,
                },
                0.04,
            )
            .unwrap();
        // Sort triangle fingerprints: source storage order must not affect geometry equivalence.
        let mut fingerprints: Vec<_> = mesh
            .vertices
            .chunks_exact(3)
            .map(|t| {
                let mut hash = 0xcbf29ce484222325_u64;
                for v in t {
                    for value in v
                        .position
                        .into_iter()
                        .chain(v.normal)
                        .chain(v.color)
                        .chain(v.surface)
                    {
                        for byte in value.to_bits().to_le_bytes() {
                            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                        }
                    }
                }
                hash
            })
            .collect();
        fingerprints.sort_unstable();
        let mut hash = 0xcbf29ce484222325_u64;
        for triangle in fingerprints {
            for byte in triangle.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        let samples: Vec<_> = [-0.4,0.,0.4].into_iter().flat_map(|dx|[-0.4,0.,0.4].into_iter().map(move|dy|[center[0]+dx,center[1]+dy])).map(|p| {
            json!({"point":p,"collision":city.building_collision(p,0.008, f.elevation, f.elevation+0.018),
                "ground":city.surface_at(p,10.).map(|s|json!({"heightBits":s.height.to_bits(),"gradientBits":s.gradient.map(f32::to_bits),"kind":format!("{:?}",s.kind),"road":s.road}))})
        }).collect();
        regions.push(json!({"name":f.name,"family":format!("{:?}",recipe.family),"center":center,"triangles":mesh.vertices.len()/3,"hash":format!("{hash:016x}"),"milliseconds":started.elapsed().as_secs_f64()*1000.,"samples":samples}));
    }
    let result = json!({"map":source,"startupMilliseconds":startup_ms,"storedTriangles":profile.triangle_count,"triangleCapacity":profile.triangle_capacity,
        "storedTriangleBytes":profile.triangle_count*profile.triangle_bytes,"triangleAllocationBytes":profile.triangle_capacity*profile.triangle_bytes,
        "spatialIndexAllocationBytes":(profile.cell_reference_capacity+profile.support_reference_capacity)*profile.pointer_bytes,
        "buildingVolumeBytes":profile.buildings.iter().map(|b|b.volume_bytes).sum::<usize>(),"regions":regions});
    std::fs::write(output, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    eprintln!("Residency probe written to {output}");
}
