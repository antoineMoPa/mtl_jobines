//! Bounded nearby exposure and four-deck clearing textures; world history stays in Rust.
pub(super) mod banks;
use crate::dynamic_world::snow::{Tracks, CELL_METERS, EMPTY};
use opencity::runtime::Mesh;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};
use wgpu::util::DeviceExt;
const SIZE: usize = 1024;
const CELL: f32 = CELL_METERS / crate::dynamic_world::METERS_PER_UNIT;
const BUCKET: f32 = 0.64;
type Triangle = [[f32; 3]; 3];
#[derive(Default)]
struct Cache {
    triangles: Vec<Triangle>,
    buckets: HashMap<(i32, i32), Vec<usize>>,
    origin: Option<[i32; 2]>,
    epoch: u64,
    revision: u64,
}
pub(super) struct Snow {
    exposure: wgpu::Texture,
    cleared: wgpu::Texture,
    exposure_view: wgpu::TextureView,
    cleared_view: wgpu::TextureView,
    settings: wgpu::Buffer,
    cache: RefCell<Cache>,
}
impl Snow {
    pub fn layout() -> [wgpu::BindGroupLayoutEntry; 3] {
        [5, 6, 7].map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: if binding == 7 {
                wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(16),
                }
            } else {
                wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                }
            },
            count: None,
        })
    }
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let make = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: SIZE as u32,
                    height: SIZE as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let exposure = make("nearby snow exposure", wgpu::TextureFormat::R32Float);
        let cleared = make(
            "cleared snow road elevations",
            wgpu::TextureFormat::Rgba32Float,
        );
        let exposure_view = exposure.create_view(&Default::default());
        let cleared_view = cleared.create_view(&Default::default());
        write(queue, &exposure, &vec![EMPTY; SIZE * SIZE], 1);
        write(queue, &cleared, &vec![EMPTY; SIZE * SIZE * 4], 4);
        let settings = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("snow cache coordinates"),
            contents: bytemuck::cast_slice(&[0_f32, 0., 1. / CELL, SIZE as f32]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            exposure,
            cleared,
            exposure_view,
            cleared_view,
            settings,
            cache: RefCell::new(Cache::default()),
        }
    }
    pub fn entries(&self) -> [wgpu::BindGroupEntry<'_>; 3] {
        [
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::TextureView(&self.exposure_view),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&self.cleared_view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: self.settings.as_entire_binding(),
            },
        ]
    }
    pub fn exposure_resources(&self) -> (&wgpu::TextureView, &wgpu::Buffer) {
        (&self.exposure_view, &self.settings)
    }
    pub fn set_geometry(&self, meshes: &[&Mesh]) {
        self.set_occluders(
            meshes
                .iter()
                .flat_map(|m| m.vertices.chunks_exact(3))
                .filter_map(|triangle| {
                    let kind = triangle[0].surface[0] as u32;
                    (matches!(kind, 0 | 2 | 3 | 6 | 7 | 10 | 12 | 13)
                        && triangle.iter().any(|v| v.normal[2] >= 0.05))
                    .then(|| {
                        [
                            triangle[0].position,
                            triangle[1].position,
                            triangle[2].position,
                        ]
                    })
                }),
        );
    }
    pub fn set_occluders(&self, triangles: impl Iterator<Item = Triangle>) {
        let mut cache = Cache::default();
        for points in triangles {
            let lo = [
                points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
            ];
            let hi = [
                points
                    .iter()
                    .map(|p| p[0])
                    .fold(f32::NEG_INFINITY, f32::max),
                points
                    .iter()
                    .map(|p| p[1])
                    .fold(f32::NEG_INFINITY, f32::max),
            ];
            let id = cache.triangles.len();
            cache.triangles.push(points);
            for y in (lo[1] / BUCKET).floor() as i32..=(hi[1] / BUCKET).floor() as i32 {
                for x in (lo[0] / BUCKET).floor() as i32..=(hi[0] / BUCKET).floor() as i32 {
                    cache.buckets.entry((x, y)).or_default().push(id);
                }
            }
        }
        *self.cache.borrow_mut() = cache;
    }
    // Test the same occluders as the exposure texture when baking seasonal curb geometry.
    fn exposed(&self, p: glam::Vec3) -> bool {
        let cache = self.cache.borrow();
        let bucket = ((p.x / BUCKET).floor() as i32, (p.y / BUCKET).floor() as i32);
        for &id in cache.buckets.get(&bucket).into_iter().flatten() {
            let tri = cache.triangles[id];
            if tri.iter().all(|v| v[2] <= p.z + 0.002) {
                continue;
            }
            let [a, b, c] = tri.map(glam::Vec3::from);
            let ab = b - a;
            let ac = c - a;
            let ap = p - a;
            let det = ab.truncate().perp_dot(ac.truncate());
            if det.abs() < 1e-12 {
                continue;
            }
            let u = ap.truncate().perp_dot(ac.truncate()) / det;
            let v = ab.truncate().perp_dot(ap.truncate()) / det;
            if u >= 0. && v >= 0. && u + v <= 1. && a.z + u * ab.z + v * ac.z > p.z + 0.002 {
                return false;
            }
        }
        true
    }
    pub fn update(&self, queue: &wgpu::Queue, center: [f32; 2], tracks: &Tracks) {
        // Align the cache to world cells and move only after travelling 64 metres.
        let mut cache = self.cache.borrow_mut();
        let requested =
            center.map(|v| ((v / BUCKET).round() * BUCKET / CELL).round() as i32 - SIZE as i32 / 2);
        // Hysteresis avoids refreshes when a resting car straddles a cell boundary.
        let origin = cache
            .origin
            .filter(|origin| {
                (0..2).all(|axis| {
                    (center[axis] - (origin[axis] + SIZE as i32 / 2) as f32 * CELL).abs() <= BUCKET
                })
            })
            .unwrap_or(requested);
        if cache.origin != Some(origin) || cache.epoch != tracks.epoch {
            let mut heights = vec![EMPTY; SIZE * SIZE];
            let base = origin.map(|v| v as f32 * CELL);
            let mut selected = HashSet::new();
            for y in (base[1] / BUCKET).floor() as i32
                ..=((base[1] + SIZE as f32 * CELL) / BUCKET).floor() as i32
            {
                for x in (base[0] / BUCKET).floor() as i32
                    ..=((base[0] + SIZE as f32 * CELL) / BUCKET).floor() as i32
                {
                    if let Some(ids) = cache.buckets.get(&(x, y)) {
                        selected.extend(ids.iter().copied());
                    }
                }
            }
            for id in selected {
                raster(&mut heights, cache.triangles[id], base);
            }
            write(queue, &self.exposure, &heights, 1);
            let mut cleared = vec![EMPTY; SIZE * SIZE * 4];
            for (&key, layers) in &tracks.cells {
                if let Some((x, y)) = pixel(key, origin) {
                    cleared[(y * SIZE + x) * 4..(y * SIZE + x + 1) * 4].copy_from_slice(layers);
                }
            }
            write(queue, &self.cleared, &cleared, 4);
            queue.write_buffer(
                &self.settings,
                0,
                bytemuck::cast_slice(&[base[0], base[1], 1. / CELL, SIZE as f32]),
            );
            cache.origin = Some(origin);
            cache.epoch = tracks.epoch;
            cache.revision = tracks.revision;
            return;
        }
        if cache.revision == tracks.revision {
            return;
        }
        if tracks
            .changes
            .front()
            .is_some_and(|(revision, _)| *revision > cache.revision + 1)
        {
            // A long pause or headless consumer missed the bounded dirty journal.
            cache.origin = None;
            drop(cache);
            self.update(queue, center, tracks);
            return;
        }
        let changed: HashSet<_> = tracks
            .changes
            .iter()
            .filter(|(rev, _)| *rev > cache.revision)
            .map(|(_, key)| *key)
            .collect();
        // Separate 32 m rectangles prevent distant plows uploading the space between them.
        let mut tiles: HashMap<(usize, usize), Vec<(usize, usize)>> = HashMap::new();
        for position in changed.into_iter().filter_map(|key| pixel(key, origin)) {
            tiles
                .entry((position.0 / 64, position.1 / 64))
                .or_default()
                .push(position);
        }
        for positions in tiles.values() {
            let minx = positions.iter().map(|p| p.0).min().unwrap();
            let maxx = positions.iter().map(|p| p.0).max().unwrap();
            let miny = positions.iter().map(|p| p.1).min().unwrap();
            let maxy = positions.iter().map(|p| p.1).max().unwrap();
            let width = maxx - minx + 1;
            let height = maxy - miny + 1;
            let mut values = vec![EMPTY; width * height * 4];
            for y in miny..=maxy {
                for x in minx..=maxx {
                    if let Some(layers) = tracks
                        .cells
                        .get(&(x as i32 + origin[0], y as i32 + origin[1]))
                    {
                        let at = ((y - miny) * width + x - minx) * 4;
                        values[at..at + 4].copy_from_slice(layers);
                    }
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    origin: wgpu::Origin3d {
                        x: minx as u32,
                        y: miny as u32,
                        z: 0,
                    },
                    ..self.cleared.as_image_copy()
                },
                bytemuck::cast_slice(&values),
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width as u32 * 16),
                    rows_per_image: Some(height as u32),
                },
                wgpu::Extent3d {
                    width: width as u32,
                    height: height as u32,
                    depth_or_array_layers: 1,
                },
            );
        }
        cache.revision = tracks.revision;
    }
}
fn pixel((x, y): (i32, i32), origin: [i32; 2]) -> Option<(usize, usize)> {
    let x = x - origin[0];
    let y = y - origin[1];
    (x >= 0 && y >= 0 && x < SIZE as i32 && y < SIZE as i32).then_some((x as usize, y as usize))
}
fn write(queue: &wgpu::Queue, texture: &wgpu::Texture, values: &[f32], channels: u32) {
    queue.write_texture(
        texture.as_image_copy(),
        bytemuck::cast_slice(values),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SIZE as u32 * channels * 4),
            rows_per_image: Some(SIZE as u32),
        },
        wgpu::Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 1,
        },
    );
}
fn raster(output: &mut [f32], triangle: Triangle, origin: [f32; 2]) {
    let p = triangle.map(|p| [(p[0] - origin[0]) / CELL, (p[1] - origin[1]) / CELL, p[2]]);
    let [a, b, c] = p;
    let det = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    if det.abs() < 1e-8 {
        return;
    }
    let lo = [
        p.iter()
            .map(|p| p[0])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.) as usize,
        p.iter()
            .map(|p| p[1])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.) as usize,
    ];
    let hi = [
        p.iter()
            .map(|p| p[0])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(SIZE as f32 - 1.) as usize,
        p.iter()
            .map(|p| p[1])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .min(SIZE as f32 - 1.) as usize,
    ];
    if p.iter().all(|p| p[0] < 0.) || p.iter().all(|p| p[1] < 0.) {
        return;
    }
    for y in lo[1]..=hi[1] {
        for x in lo[0]..=hi[0] {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let u = ((b[1] - c[1]) * (px - c[0]) + (c[0] - b[0]) * (py - c[1])) / det;
            let v = ((c[1] - a[1]) * (px - c[0]) + (a[0] - c[0]) * (py - c[1])) / det;
            let w = 1. - u - v;
            if u >= -0.001 && v >= -0.001 && w >= -0.001 {
                output[y * SIZE + x] = output[y * SIZE + x].max(u * a[2] + v * b[2] + w * c[2]);
            }
        }
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
