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
#[cfg(test)]
fn walk(
    node: gltf::Node<'_>,
    parent: Mat4,
    binary: &[u8],
    out: &mut Vec<Vertex>,
) -> Result<(), String> {
    walk_materials(node, parent, binary, out, false)
}
fn walk_materials(
    node: gltf::Node<'_>,
    parent: Mat4,
    binary: &[u8],
    out: &mut Vec<Vertex>,
    baked_prop: bool,
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
            let kind = if baked_prop
                && primitive
                    .material()
                    .emissive_factor()
                    .iter()
                    .any(|v| *v > 0.)
            {
                SurfaceKind::Light
            } else {
                SurfaceKind::Prop
            };
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
                        surface: [kind as u32 as f32, 0., 0., 0.],
                    });
                }
            }
        }
    }
    for child in node.children() {
        walk_materials(child, transform, binary, out, baked_prop)?;
    }
    Ok(())
}
/// The authoring tool validates imports with the same decoder as map loading.
pub(crate) fn decode(bytes: &[u8]) -> Result<Mesh, String> {
    decode_model(bytes, false)
}
/// Packaged street props already contain the approved bake in COLOR_0.
/// Keep that shading once, and map their diffuser materials to game lights.
pub(crate) fn decode_street_prop(bytes: &[u8]) -> Result<Mesh, String> {
    decode_model(bytes, true)
}
fn decode_model(bytes: &[u8], baked_prop: bool) -> Result<Mesh, String> {
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("GLB exceeds 64 MiB".into());
    }
    let glb = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
    let binary = glb
        .blob
        .as_deref()
        .ok_or("GLB must contain its binary buffer")?;
    let scene = glb
        .default_scene()
        .or_else(|| glb.scenes().next())
        .ok_or("GLB has no scene")?;
    let mut vertices = Vec::new();
    for node in scene.nodes() {
        walk_materials(node, Mat4::IDENTITY, binary, &mut vertices, baked_prop)?;
    }
    if vertices.is_empty() || vertices.len() % 3 != 0 {
        return Err("GLB needs a nonempty triangle mesh".into());
    }
    let mut mesh = Mesh { vertices };
    let (lo, hi) = bounds(&mesh);
    if (hi - lo).min_element() <= 1e-8 {
        return Err("GLB model has degenerate bounds".into());
    }
    if baked_prop {
        return Ok(mesh);
    }
    let size = hi - lo;
    let radius = (size.min_element() * 0.5).max(size.max_element() * 0.02);
    opencity::runtime::ambient_occlusion::bake(&mut mesh, radius, Some(lo.z));
    let unit = (size.z / 3.).max(size.max_element() / 32.);
    for v in &mut mesh.vertices {
        if v.surface[0] == SurfaceKind::Light as u32 as f32 {
            continue;
        }
        let p = Vec3::from_array(v.position) - lo;
        let uv = if v.normal[2].abs() > 0.7 {
            [p.x / unit, p.y / unit]
        } else {
            [
                if v.normal[1].abs() > v.normal[0].abs() {
                    p.x / unit
                } else {
                    p.y / unit
                },
                p.z / unit,
            ]
        };
        v.surface[1..].copy_from_slice(&[uv[0], uv[1], -1.]);
    }
    Ok(mesh)
}

pub(crate) fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
    mesh.vertices.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), v| {
            let p = Vec3::from_array(v.position);
            (lo.min(p), hi.max(p))
        },
    )
}

pub(crate) fn place(mesh: &Mesh, feature: &opencity::LandscapeFeature) -> Result<Mesh, String> {
    if feature.points.len() != 4 {
        return Err("GLB placement needs four footprint corners".into());
    }
    let (lo, hi) = bounds(mesh);
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
        return Err("Model footprint has a zero-length edge".into());
    }
    let scale = (width / size.x).min(depth / size.y);
    let vertical = feature.height.unwrap_or(size.z * scale) / size.z;
    if !vertical.is_finite() || vertical <= 0. {
        return Err("Invalid model height".into());
    }
    let (sin, cos) = (b[1] - a[1]).atan2(b[0] - a[0]).sin_cos();
    let center = feature
        .points
        .iter()
        .fold([0.; 2], |sum, p| [sum[0] + p[0] / 4., sum[1] + p[1] / 4.]);
    let mut result = mesh.clone();
    for vertex in &mut result.vertices {
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
    }
    Ok(result)
}

pub(crate) fn build(city: &City, base: &Path) -> Result<Mesh, String> {
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
        let raw = decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        result.vertices.extend(place(&raw, feature)?.vertices);
    }
    Ok(result)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn imported_model_bakes_shading_once_and_keeps_geometry_and_original_asset() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("data/maps/mtl/models/fuel-station.glb");
        let bytes = std::fs::read(&path).unwrap();
        let glb = gltf::Gltf::from_slice(&bytes).unwrap();
        let mut original = Vec::new();
        for node in glb.default_scene().unwrap().nodes() {
            super::walk(
                node,
                glam::Mat4::IDENTITY,
                glb.blob.as_ref().unwrap(),
                &mut original,
            )
            .unwrap();
        }
        let mesh = super::decode(&bytes).unwrap();
        let duplicate = super::decode(&bytes).unwrap();
        assert_eq!(mesh.vertices.len(), original.len());
        let mut shaded = 0;
        for ((baked, raw), duplicate) in mesh.vertices.iter().zip(original).zip(duplicate.vertices)
        {
            assert_eq!(baked.position, raw.position);
            assert_eq!(baked.normal, raw.normal);
            assert_eq!(baked.color, duplicate.color);
            assert_eq!(baked.surface[3], -1.);
            for i in 0..3 {
                assert!(baked.color[i] >= raw.color[i] * 0.5199);
                assert!(baked.color[i] <= raw.color[i]);
            }
            shaded += usize::from(baked.color != raw.color);
        }
        assert!(shaded > 0);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
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
