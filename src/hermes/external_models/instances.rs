//! One shared decoded mesh per asset, with compact map placement transforms.
use super::*;
use std::{collections::HashMap, sync::Arc};
pub struct ModelInstances {
    pub mesh: Arc<Mesh>,
    pub transforms: Vec<Mat4>,
}
pub fn transform(mesh: &Mesh, feature: &opencity::LandscapeFeature) -> Result<Mat4, String> {
    if feature.points.len() != 4 {
        return Err("GLB placement needs four footprint corners".into());
    }
    let (lo, hi) = bounds(mesh);
    let size = hi - lo;
    let [a, b, c] = [feature.points[0], feature.points[1], feature.points[2]];
    let width = (b[0] - a[0]).hypot(b[1] - a[1]);
    let depth = (c[0] - b[0]).hypot(c[1] - b[1]);
    if width <= 1e-8 || depth <= 1e-8 || size.min_element() <= 1e-8 {
        return Err("Degenerate GLB footprint".into());
    }
    let scale = (width / size.x).min(depth / size.y);
    let vertical = feature.height.unwrap_or(size.z * scale) / size.z;
    if !vertical.is_finite() || vertical <= 0. {
        return Err("Invalid GLB height".into());
    }
    let center = feature
        .points
        .iter()
        .fold([0.; 2], |sum, p| [sum[0] + p[0] / 4., sum[1] + p[1] / 4.]);
    Ok(
        Mat4::from_translation(Vec3::new(center[0], center[1], feature.elevation))
            * Mat4::from_rotation_z((b[1] - a[1]).atan2(b[0] - a[0]))
            * Mat4::from_scale(Vec3::new(scale, scale, vertical))
            * Mat4::from_translation(-Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, lo.z)),
    )
}
pub fn load(city: &City, base: &Path) -> Result<Vec<ModelInstances>, String> {
    groups(city, |model| {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = base;
            Ok(crate::web_assets::get(&format!("data/maps/mtl/{model}")).to_vec())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = base.join(model);
            let len = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
            if len > 64 * 1024 * 1024 {
                return Err("GLB exceeds 64 MiB".into());
            }
            std::fs::read(path).map_err(|e| e.to_string())
        }
    })
}
pub fn groups(
    city: &City,
    mut load: impl FnMut(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<ModelInstances>, String> {
    let mut groups = Vec::<ModelInstances>::new();
    let mut assets = HashMap::new();
    for feature in &city.landscape {
        let Some(path) = feature.model.as_deref().filter(|p| p.ends_with(".glb")) else {
            continue;
        };
        if path == super::super::orange_sign::MODEL_PATH {
            continue;
        }
        let index = if let Some(&index) = assets.get(path) {
            index
        } else {
            let mesh = decode_shared(&load(path)?)?;
            let index = groups.len();
            groups.push(ModelInstances {
                mesh,
                transforms: Vec::new(),
            });
            assets.insert(path, index);
            index
        };
        let group = &mut groups[index];
        group.transforms.push(transform(&group.mesh, feature)?);
    }
    Ok(groups)
}
