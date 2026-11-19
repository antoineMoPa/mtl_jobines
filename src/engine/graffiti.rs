//! Concrete receivers are indexed once; paint is generated and cached near the viewer.
use glam::Vec3;
use opencity::runtime::{Mesh, SurfaceKind, Vertex};
use std::collections::BTreeMap;
use wgpu::util::DeviceExt;

pub(super) const MATERIAL: f32 = 42.;
const OFFSET: f32 = 0.00012; // 12 mm; one game map unit is 100 m.
const SPACING: f32 = 0.3;
pub(super) const MAX_VISIBLE: usize = 30;
mod nearby;
pub(super) use nearby::Nearby;
const ART: [&[u8]; 2] = [
    include_bytes!("../../assets/graffiti/glyph-paint.png"),
    include_bytes!("../../assets/graffiti/flower-paint.png"),
];

/// Linear RGB override with a 0..1 blend weight; independent opacity/wash ranges.
/// The app can update these without rebuilding geometry or rerasterizing artwork.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct Settings {
    pub color_override: [f32; 4],
    pub ranges: [f32; 4], // opacity min/max, wash min/max
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            color_override: [0.; 4],
            ranges: [0.04, 0.68, 0.18, 0.76],
        }
    }
}
pub(super) struct PaintTexture {
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub settings: wgpu::Buffer,
}
fn decode(bytes: &[u8]) -> Vec<u8> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .expect("graffiti PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut pixels)
        .expect("graffiti mask pixels");
    assert_eq!((info.width, info.height), (512, 256));
    assert_eq!(info.color_type, png::ColorType::Rgba);
    pixels.truncate(info.buffer_size());
    pixels
}
// Alpha-weight mask/highlight channels so transparent texels cannot darken mip edges.
fn next_mip(source: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..2 {
                for dx in 0..2 {
                    let from = (((y * 2 + dy).min(height - 1) * width
                        + (x * 2 + dx).min(width - 1))
                        * 4) as usize;
                    let a = u32::from(source[from + 3]);
                    for c in 0..3 {
                        sum[c] += u32::from(source[from + c]) * a;
                    }
                    sum[3] += a;
                }
            }
            let to = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                out[to + c] = (sum[c] / sum[3].max(1)) as u8;
            }
            out[to + 3] = ((sum[3] + 2) / 4) as u8;
        }
    }
    out
}
impl PaintTexture {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("paint and fence cutouts"),
            size: wgpu::Extent3d {
                width: 512,
                height: 256,
                depth_or_array_layers: ART.len() as u32 + 1,
            },
            mip_level_count: 10,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        // The fence shares this mipmapped cutout atlas (layer 2).
        let fence = include_bytes!("../../assets/props/airport-fence.png").as_slice();
        for (layer, bytes) in ART.into_iter().chain([fence]).enumerate() {
            let mut pixels = decode(bytes);
            let (mut w, mut h) = (512, 256);
            for level in 0..10 {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: level,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: 0,
                            z: layer as u32,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &pixels,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(w * 4),
                        rows_per_image: Some(h),
                    },
                    wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                );
                if level < 9 {
                    pixels = next_mip(&pixels, w, h);
                    w = (w / 2).max(1);
                    h = (h / 2).max(1);
                }
            }
        }
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("graffiti mip filtering"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let settings = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("graffiti paint ranges and color override"),
            contents: bytemuck::bytes_of(&Settings::default()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            view,
            sampler,
            settings,
        }
    }
    pub fn configure(&self, queue: &wgpu::Queue, settings: Settings) {
        queue.write_buffer(&self.settings, 0, bytemuck::bytes_of(&settings));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PanelKey([i32; 10]);
fn panel_key(t: &[Vertex]) -> Option<PanelKey> {
    let s = t[0].surface;
    let infrastructure = s[3] == -2.
        && (s[0] == SurfaceKind::Prop as u32 as f32 || s[0] == SurfaceKind::Sidewalk as u32 as f32);
    let legacy_concrete = s[0] == SurfaceKind::Building as u32 as f32 && s[3] == 2.;
    if !(infrastructure || legacy_concrete) || t.iter().any(|v| v.normal[2].abs() > 0.00001) {
        return None;
    }
    let mut key = [0; 10];
    for axis in 0..3 {
        let lo = t
            .iter()
            .map(|v| v.position[axis])
            .fold(f32::INFINITY, f32::min);
        let hi = t
            .iter()
            .map(|v| v.position[axis])
            .fold(f32::NEG_INFINITY, f32::max);
        key[axis] = (lo * 1_000_000.).round() as i32;
        key[axis + 3] = (hi * 1_000_000.).round() as i32;
        key[axis + 6] = (t[0].normal[axis] * 10_000.).round() as i32;
    }
    key[9] = i32::from(legacy_concrete);
    Some(PanelKey(key))
}
fn seed(key: PanelKey) -> u32 {
    key.0
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(0x42c0_a175, |h, b| {
            (h ^ u32::from(b)).wrapping_mul(16777619)
        })
}
fn sample(seed: u32, stream: u32) -> f32 {
    let mut x = seed.wrapping_add(stream.wrapping_mul(0x9e37_79b9));
    x = (x ^ x >> 16).wrapping_mul(0x7feb_352d);
    x = (x ^ x >> 15).wrapping_mul(0x846c_a68b);
    (x ^ x >> 16) as f64 as f32 / u32::MAX as f32
}
fn color(seed: u32) -> [f32; 3] {
    match (seed >> 16) % 6 {
        0 => [0.31, 0.095, 0.048],
        1 => [0.055, 0.19, 0.14],
        2 => [0.05, 0.06, 0.045],
        3 => [0.24, 0.15, 0.27],
        4 => [0.55, 0.53, 0.46],
        _ => [0.37, 0.26, 0.08],
    }
}
fn record(seed: u32) -> f32 {
    let opacity = (sample(seed, 7) * 255.).round() as u32;
    let wash = (sample(seed, 8) * 255.).round() as u32;
    ((seed & 1) | (opacity << 1) | (wash << 9) | ((seed & 127) << 17)) as f32
}
fn paint_panel(triangles: &[[Vertex; 3]], seed: u32) -> Option<Mesh> {
    paint_panel_in_cell(triangles, seed, None)
}
fn paint_panel_in_cell(
    triangles: &[[Vertex; 3]],
    seed: u32,
    cell: Option<(i32, i32)>,
) -> Option<Mesh> {
    if triangles.len() != 2 {
        return None;
    }
    let mut points = Vec::<Vec3>::new();
    for v in triangles.iter().flatten() {
        let p = Vec3::from_array(v.position);
        if !points.iter().any(|old| old.distance_squared(p) < 1e-14) {
            points.push(p);
        }
    }
    if points.len() != 4 {
        return None;
    }
    let normal = Vec3::from_array(triangles[0][0].normal);
    let tangent = Vec3::new(-normal.y, normal.x, 0.);
    points.sort_by(|a, b| {
        a.dot(tangent)
            .total_cmp(&b.dot(tangent))
            .then(a.z.total_cmp(&b.z))
    });
    let mut lo = points[0].dot(tangent);
    let mut hi = points[3].dot(tangent);
    if (points[1].dot(tangent) - lo).abs() > 0.000001
        || (points[2].dot(tangent) - hi).abs() > 0.000001
    {
        return None;
    }
    let bottom = points[0]
        .z
        .min(points[1].z)
        .max(points[2].z.min(points[3].z));
    let mut top = points[0]
        .z
        .max(points[1].z)
        .min(points[2].z.max(points[3].z));
    if hi - lo < 0.018 || top - bottom < 0.0095 {
        return None;
    }
    if triangles[0][0].surface[0] == SurfaceKind::Building as u32 as f32 {
        // Stay below the first procedural window band, away from storefront entries.
        top = top.min(bottom + (triangles[0][0].surface[2] * 0.30).min(0.009));
    }
    if hi - lo < 0.018 || top - bottom < 0.0055 {
        return None;
    }
    if let Some(cell) = cell {
        let plane = normal * points[0].dot(normal);
        for (axis, tile) in [(0, cell.0), (1, cell.1)] {
            let lower = tile as f32 * SPACING;
            let upper = lower + SPACING;
            if tangent[axis].abs() < 1e-7 {
                if plane[axis] < lower || plane[axis] > upper {
                    return None;
                }
            } else {
                let a = (lower - plane[axis]) / tangent[axis];
                let b = (upper - plane[axis]) / tangent[axis];
                lo = lo.max(a.min(b));
                hi = hi.min(a.max(b));
            }
        }
        if hi - lo < 0.018 {
            return None;
        }
    }
    let angle = (sample(seed, 2) - 0.5) * 0.14;
    let (s, c) = angle.sin_cos();
    let wanted_height = if top - bottom > 0.025 {
        0.020 + sample(seed, 3) * 0.016
    } else {
        0.006 + sample(seed, 3) * 0.004
    };
    let height = wanted_height
        .min((top - bottom - 0.0016) / (c.abs() + 2. * s.abs()))
        .min((hi - lo - 0.0016) / (2. * c.abs() + s.abs()));
    let width = height * 2.;
    let half_w = (width * c.abs() + height * s.abs()) / 2.;
    let half_h = (height * c.abs() + width * s.abs()) / 2.;
    let along = lo + half_w + 0.0008 + sample(seed, 4) * (hi - lo - 2. * half_w - 0.0016);
    let z = bottom
        + half_h
        + 0.0008
        + sample(seed, 5) * (top - bottom - 2. * half_h - 0.0016).min(0.018);
    let center = tangent * along + normal * (points[0].dot(normal) + OFFSET) + Vec3::Z * z;
    let u = tangent * c + Vec3::Z * s;
    let v = tangent * s - Vec3::Z * c;
    let positions = [
        center - u * width / 2. - v * height / 2.,
        center + u * width / 2. - v * height / 2.,
        center + u * width / 2. + v * height / 2.,
        center - u * width / 2. + v * height / 2.,
    ];
    let uv = [[0., 0.], [1., 0.], [1., 1.], [0., 1.]];
    let mut mesh = Mesh::default();
    for i in [0, 1, 2, 0, 2, 3] {
        mesh.vertices.push(Vertex {
            position: positions[i].to_array(),
            normal: normal.to_array(),
            color: color(seed),
            surface: [MATERIAL, uv[i][0], uv[i][1], record(seed)],
        });
    }
    Some(mesh)
}
/// Pair actual shared edges, rather than triangle bounding boxes. Sloping retaining
/// walls have different triangle bounds even though their full quad is a valid receiver.
fn panels(world: &Mesh) -> BTreeMap<PanelKey, Vec<[Vertex; 3]>> {
    let mut edges = BTreeMap::<[i32; 10], Vec<usize>>::new();
    for (index, t) in world.vertices.chunks_exact(3).enumerate() {
        let Some(key) = panel_key(t) else {
            continue;
        };
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let mut endpoints = [
                t[a].position.map(|v| (v * 1_000_000.).round() as i32),
                t[b].position.map(|v| (v * 1_000_000.).round() as i32),
            ];
            endpoints.sort();
            let mut edge = [0; 10];
            edge[..3].copy_from_slice(&endpoints[0]);
            edge[3..6].copy_from_slice(&endpoints[1]);
            edge[6..].copy_from_slice(&key.0[6..]);
            edges.entry(edge).or_default().push(index);
        }
    }
    let mut out = BTreeMap::new();
    for ids in edges.values().filter(|ids| ids.len() == 2) {
        let a = &world.vertices[ids[0] * 3..ids[0] * 3 + 3];
        let b = &world.vertices[ids[1] * 3..ids[1] * 3 + 3];
        let pair = [[a[0], a[1], a[2]], [b[0], b[1], b[2]]];
        if paint_panel(&pair, 0).is_none() {
            continue;
        }
        let all = [a[0], a[1], a[2], b[0], b[1], b[2]];
        if let Some(key) = panel_key(&all) {
            out.entry(key).or_insert_with(|| pair.to_vec());
        }
    }
    out
}

#[cfg(test)]
mod tests;
