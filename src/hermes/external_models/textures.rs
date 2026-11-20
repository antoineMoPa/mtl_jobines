//! Embedded material images shared by imported meshes and the GPU texture arrays.
use std::sync::{Arc, Mutex, OnceLock};
pub const SIZE: u32 = 512;
pub const CAPACITY: u32 = 32;
pub const MATERIAL: f32 = 42.;
pub const EQUIPMENT_TEXTURE: f32 = 43.;
pub const EQUIPMENT_SOLID: f32 = 44.;
#[derive(Clone)]
pub struct Layer {
    pub colour: Arc<[u8]>,
    pub normal: Arc<[u8]>,
}
static LAYERS: OnceLock<Mutex<Vec<Layer>>> = OnceLock::new();
pub fn snapshot() -> Vec<Layer> {
    LAYERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}
fn image(texture: gltf::Texture<'_>, binary: &[u8]) -> Result<Vec<u8>, String> {
    let gltf::image::Source::View {
        view,
        mime_type: "image/png",
    } = texture.source().source()
    else {
        return Err("Static GLB textures require embedded PNG images".into());
    };
    let bytes = binary
        .get(view.offset()..view.offset() + view.length())
        .ok_or("Invalid GLB image buffer view")?;
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    if reader.info().width > 4096 || reader.info().height > 4096 {
        return Err("GLB image exceeds 4096 pixels".into());
    }
    let mut data = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
    let mut pixels = vec![0; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let i = ((y * info.height / SIZE * info.width + x * info.width / SIZE) as usize)
                * info.color_type.samples();
            let p = &data[i..];
            let rgba = match info.color_type {
                png::ColorType::Rgb => [p[0], p[1], p[2], 255],
                png::ColorType::Rgba => [p[0], p[1], p[2], p[3]],
                png::ColorType::Grayscale => [p[0], p[0], p[0], 255],
                png::ColorType::GrayscaleAlpha => [p[0], p[0], p[0], p[1]],
                _ => return Err("Unsupported PNG channels".into()),
            };
            let out = ((y * SIZE + x) * 4) as usize;
            pixels[out..out + 4].copy_from_slice(&rgba);
        }
    }
    Ok(pixels)
}
pub(super) fn register(
    material: gltf::Material<'_>,
    binary: &[u8],
) -> Result<Option<usize>, String> {
    let pbr = material.pbr_metallic_roughness();
    let Some(base) = pbr.base_color_texture() else {
        return Ok(None);
    };
    if base.tex_coord() != 0
        || material
            .normal_texture()
            .is_some_and(|n| n.tex_coord() != 0)
    {
        return Err("Static GLB materials require TEXCOORD_0".into());
    }
    if pbr.metallic_factor() > 0.001
        || pbr.metallic_roughness_texture().is_some()
        || material.alpha_mode() != gltf::material::AlphaMode::Opaque
    {
        return Err("Static GLB textures currently support opaque dielectric materials with constant roughness".into());
    }
    let colour = image(base.texture(), binary)?;
    let mut normal = if let Some(n) = material.normal_texture() {
        image(n.texture(), binary)?
    } else {
        [128, 128, 255, 255].repeat((SIZE * SIZE) as usize)
    };
    let strength = material.normal_texture().map_or(1., |n| n.scale());
    for pixel in normal.chunks_exact_mut(4) {
        let x = (pixel[0] as f32 / 127.5 - 1.) * strength;
        let y = (pixel[1] as f32 / 127.5 - 1.) * strength;
        let z = pixel[2] as f32 / 127.5 - 1.;
        let length = x.hypot(y).hypot(z).max(0.001);
        for (i, v) in [x, y, z].iter().enumerate() {
            pixel[i] = ((v / length * 0.5 + 0.5) * 255.).round() as u8;
        }
        pixel[3] = (pbr.roughness_factor().clamp(0.04, 1.) * 255.).round() as u8;
    }
    let mut layers = LAYERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(i) = layers
        .iter()
        .position(|l| l.colour.as_ref() == colour && l.normal.as_ref() == normal)
    {
        return Ok(Some(i));
    }
    if layers.len() >= CAPACITY as usize {
        return Err("Static GLB material texture budget exceeded (texture array is full)".into());
    }
    let index = layers.len();
    layers.push(Layer {
        colour: colour.into(),
        normal: normal.into(),
    });
    Ok(Some(index))
}

/// Game-authored RGBA material layers use the same mipmapped colour/normal arrays.
pub fn register_pixels(colour: Vec<u8>, normal: Vec<u8>) -> Result<usize, String> {
    let size = (SIZE * SIZE * 4) as usize;
    if colour.len() != size || normal.len() != size {
        return Err("Material images must be 512×512 RGBA".into());
    }
    let mut layers = LAYERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(index) = layers
        .iter()
        .position(|l| l.colour.as_ref() == colour && l.normal.as_ref() == normal)
    {
        return Ok(index);
    }
    if layers.len() >= CAPACITY as usize {
        return Err("Material texture budget exceeded".into());
    }
    let index = layers.len();
    layers.push(Layer {
        colour: colour.into(),
        normal: normal.into(),
    });
    Ok(index)
}

// Equipment layers store metallic in colour alpha and roughness in normal alpha.
pub(super) fn register_equipment(
    material: gltf::Material<'_>,
    binary: &[u8],
) -> Result<Option<usize>, String> {
    let pbr = material.pbr_metallic_roughness();
    let base = pbr.base_color_texture();
    let normal = material.normal_texture();
    let orm = pbr.metallic_roughness_texture();
    if base.is_none() && normal.is_none() && orm.is_none() {
        return Ok(None);
    }
    let count = (SIZE * SIZE) as usize;
    let mut colour = match base {
        Some(t) => image(t.texture(), binary)?,
        None => vec![255; count * 4],
    };
    let mut detail = match normal {
        Some(t) => image(t.texture(), binary)?,
        None => [128, 128, 255, 255].repeat(count),
    };
    let channels = orm.map(|t| image(t.texture(), binary)).transpose()?;
    for (i, (c, n)) in colour
        .chunks_exact_mut(4)
        .zip(detail.chunks_exact_mut(4))
        .enumerate()
    {
        c[3] = (pbr.metallic_factor() * channels.as_ref().map_or(255, |v| v[i * 4 + 2]) as f32)
            .round() as u8;
        n[3] = (pbr.roughness_factor() * channels.as_ref().map_or(255, |v| v[i * 4 + 1]) as f32)
            .round() as u8;
    }
    register_pixels(colour, detail).map(Some)
}
