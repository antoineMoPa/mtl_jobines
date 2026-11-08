//! Native, map-relative GLBs stay on disk and outside serialized map meshes.
use glam::{Mat4, Vec3};
use opencity::runtime::{City, Mesh, SurfaceKind, Vertex};
use std::path::{Path, PathBuf};

pub(crate) fn paths(program: &opencity::Program, base: &Path) -> Result<Vec<PathBuf>, String> {
    let projected = opencity::project_landscape_features(program);
    if !projected.errors.is_empty() {
        return Err(format!("landscape: {:?}", projected.errors));
    }
    let mut paths: Vec<_> = projected
        .features
        .iter()
        .filter_map(|f| f.model.as_deref())
        .filter(|p| p.ends_with(".glb"))
        .map(|p| base.join(p))
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}
fn game(p: Vec3) -> Vec3 {
    Vec3::new(p.x, -p.z, p.y)
}
fn walk(
    node: gltf::Node<'_>,
    parent: Mat4,
    binary: &[u8],
    out: &mut Vec<Vertex>,
) -> Result<(), String> {
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if !transform.is_finite() || transform.determinant().abs() < 1e-12 {
        return Err("GLB node has an invalid transform".into());
    }
    let normal_transform = transform.inverse().transpose();
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                return Err("GLB models require triangle primitives".into());
            }
            if primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_texture()
                .is_some()
            {
                return Err(
                    "GLB base-color textures are not supported by the static model renderer".into(),
                );
            }
            let reader = primitive.reader(|b| match b.source() {
                gltf::buffer::Source::Bin => Some(binary),
                _ => None,
            });
            let positions = reader.read_positions().ok_or("GLB has no positions")?;
            if positions.len() > 1_000_000 {
                return Err("GLB primitive exceeds the vertex budget".into());
            }
            let positions: Vec<_> = positions.collect();
            let normals = reader.read_normals().map(|v| v.collect::<Vec<_>>());
            let colors = reader
                .read_colors(0)
                .map(|v| v.into_rgba_f32().collect::<Vec<_>>());
            let indices: Vec<_> = reader
                .read_indices()
                .map(|v| v.into_u32().collect())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            if indices.len() % 3 != 0 || out.len() + indices.len() > 1_000_000 {
                return Err("GLB exceeds the triangle budget".into());
            }
            let factor = primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_factor();
            for triangle in indices.chunks_exact(3) {
                let p = triangle
                    .iter()
                    .map(|i| {
                        positions
                            .get(*i as usize)
                            .copied()
                            .map(Vec3::from_array)
                            .ok_or("GLB position index out of bounds")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let face = (p[1] - p[0]).cross(p[2] - p[0]);
                for (corner, &index) in triangle.iter().enumerate() {
                    let normal = match &normals {
                        Some(n) => Vec3::from_array(
                            *n.get(index as usize)
                                .ok_or("GLB normal index out of bounds")?,
                        ),
                        None => face,
                    };
                    let normal = game(normal_transform.transform_vector3(normal));
                    let length = normal.length();
                    if length < 1e-12 {
                        continue;
                    }
                    let position = game(transform.transform_point3(p[corner]));
                    let tint = match &colors {
                        Some(c) => *c
                            .get(index as usize)
                            .ok_or("GLB color index out of bounds")?,
                        None => [1.; 4],
                    };
                    let color = [0, 1, 2].map(|i| factor[i] * tint[i]);
                    if !position.is_finite()
                        || !normal.is_finite()
                        || color.iter().any(|v| !v.is_finite())
                    {
                        return Err("GLB contains nonfinite vertices".into());
                    }
                    out.push(Vertex {
                        position: position.to_array(),
                        normal: (normal / length).to_array(),
                        color,
                        surface: [SurfaceKind::Prop as u32 as f32, 0., 0., 0.],
                    });
                }
            }
        }
    }
    for child in node.children() {
        walk(child, transform, binary, out)?;
    }
    Ok(())
}
pub(super) fn build(city: &City, base: &Path) -> Result<Mesh, String> {
    let mut result = Mesh::default();
    for feature in &city.landscape {
        let Some(path) = feature.model.as_deref().filter(|p| p.ends_with(".glb")) else {
            continue;
        };
        let path = base.join(path);
        let metadata = std::fs::metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if metadata.len() > 64 * 1024 * 1024 {
            return Err(format!("{}: GLB exceeds 64 MiB", path.display()));
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let glb = gltf::Gltf::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let binary = glb
            .blob
            .as_deref()
            .ok_or("GLB must contain its binary buffer")?;
        let mut vertices = Vec::new();
        let scene = glb
            .default_scene()
            .or_else(|| glb.scenes().next())
            .ok_or("GLB has no scene")?;
        for node in scene.nodes() {
            walk(node, Mat4::IDENTITY, binary, &mut vertices)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        if vertices.is_empty() || vertices.len() % 3 != 0 || feature.points.len() != 4 {
            return Err("GLB placement needs a nonempty model and four footprint corners".into());
        }
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for v in &vertices {
            let p = Vec3::from_array(v.position);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        let size = hi - lo;
        if size.min_element() <= 1e-8 {
            return Err("GLB model has degenerate bounds".into());
        }
        let a = feature.points[0];
        let b = feature.points[1];
        let c = feature.points[2];
        let width = (b[0] - a[0]).hypot(b[1] - a[1]);
        let depth = (c[0] - b[0]).hypot(c[1] - b[1]);
        if width <= 1e-8 || depth <= 1e-8 {
            return Err(format!(
                "{}: model footprint has a zero-length edge",
                path.display()
            ));
        }
        let scale = (width / size.x).min(depth / size.y);
        let vertical = feature.height.unwrap_or(size.z * scale) / size.z;
        let (sin, cos) = (b[1] - a[1]).atan2(b[0] - a[0]).sin_cos();
        let center = feature
            .points
            .iter()
            .fold([0.; 2], |sum, p| [sum[0] + p[0] / 4., sum[1] + p[1] / 4.]);
        for mut vertex in vertices {
            let p = Vec3::from_array(vertex.position);
            let x = (p.x - (lo.x + hi.x) * 0.5) * scale;
            let y = (p.y - (lo.y + hi.y) * 0.5) * scale;
            vertex.position = [
                center[0] + cos * x - sin * y,
                center[1] + sin * x + cos * y,
                feature.elevation + (p.z - lo.z) * vertical,
            ];
            let n = vertex.normal;
            let nz = n[2] * scale / vertical;
            let length = n[0].hypot(n[1]).hypot(nz);
            vertex.normal = [
                (cos * n[0] - sin * n[1]) / length,
                (sin * n[0] + cos * n[1]) / length,
                nz / length,
            ];
            result.vertices.push(vertex);
        }
    }
    Ok(result)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn packaged_map_data_excludes_external_glb_vertices() {
        let dir = std::env::temp_dir().join(format!("external-model-cache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("data/maps/mtl/models/fuel-station.glb"),
            dir.join("model.glb"),
        )
        .unwrap();
        let path = dir.join("map.road");
        std::fs::write(
            &path,
            concat!(
                "station=building(0.0759,\"model.glb\",{collision:none},",
                "0,0,0.344,0,0.344,0.244,0,0.244);"
            ),
        )
        .unwrap();
        let mut map = super::super::Hermes::load_with_game_data(&path, None).unwrap();
        assert!(map.external_vertex_count > 0);
        let key = map.cache.as_ref().unwrap().0;
        let package = dir.join("map.game-data.gz");
        map.write_game_data(&package).unwrap();
        let packed = super::super::game_data::GameData::read(&package, key).unwrap();
        let mut reloaded =
            super::super::Hermes::load_with_game_data(&path, Some(&package)).unwrap();
        assert!(reloaded.external_vertex_count > 0);
        assert_eq!(
            reloaded.take_geometry().0.vertices.len(),
            packed.world.vertices.len() + reloaded.external_vertex_count
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
