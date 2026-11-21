//! Shared ribbon mesh, instanced over visible 32 cm patches of turf.
use super::{Bounds, Frustum, Renderer};
use glam::{IVec2, Vec2, Vec3};
use opencity::runtime::{City, SurfaceKind};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

const TILE: f32 = 0.0032;
const ROOTS: [u32; 4] = [2048, 512, 128, 24];
const END: [f32; 4] = [0.0065, 0.015, 0.033, 0.07];
const RADIUS: i32 = 23;

#[derive(Default)]
pub(in crate::engine) struct SupportCache(HashMap<IVec2, Option<[f32; 4]>>);

pub(super) struct Ribbons {
    seeds: wgpu::Buffer,
    indices: wgpu::Buffer,
    tiles: wgpu::Buffer,
    pub(super) counts: [u32; 4],
}

impl Ribbons {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let mut state = 4103u32;
        let mut random = || {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            state as f64 / 4294967296.
        };
        let mut seeds = Vec::new();
        let mut indices = Vec::new();
        for cluster in 0..512 {
            let center = [random(), random()];
            for sibling in 0..4 {
                let root = center.map(|p| {
                    if sibling < 2 {
                        (p + 0.015 * random()).fract()
                    } else {
                        random()
                    }
                });
                let seed = [root[0], root[1], random(), random()].map(|v| (v * 256.) as u8);
                seeds.extend(std::iter::repeat_n(seed, 24));
                for blade in 0..3 {
                    for section in 0..3 {
                        let v: u16 = (cluster * 4 + sibling) * 24 + blade * 8 + section * 2;
                        indices.extend([v, v + 1, v + 2, v + 2, v + 1, v + 3]);
                    }
                }
            }
        }
        let buffer = |label, contents, usage| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
        };
        Self {
            seeds: buffer(
                "grass seeds",
                bytemuck::cast_slice(&seeds),
                wgpu::BufferUsages::VERTEX,
            ),
            indices: buffer(
                "grass ribbons",
                bytemuck::cast_slice(&indices),
                wgpu::BufferUsages::INDEX,
            ),
            tiles: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("visible grass tiles"),
                size: (2 * RADIUS + 1).pow(2) as u64 * 48,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            counts: [0; 4],
        }
    }

    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass) {
        pass.set_vertex_buffer(0, self.seeds.slice(..));
        pass.set_vertex_buffer(1, self.tiles.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint16);
        let mut end = self.counts.iter().sum();
        for band in (0..4).rev() {
            let start = end - self.counts[band];
            if start < end {
                pass.draw_indexed(0..ROOTS[band] * 54, 0, start..end);
            }
            end = start;
        }
    }
}

// Store the local tangent plane and a quadrant mask so paved margins clip roots.
fn sample_tile(city: &City, cell: IVec2) -> Option<[f32; 4]> {
    let center = (cell.as_vec2() + 0.5) * TILE;
    let samples: [Option<[f32; 3]>; 4] = std::array::from_fn(|i| {
        let offset = (Vec2::new((i % 2) as f32, (i / 2) as f32) - 0.5) * (TILE * 0.5);
        let s = city.surface_at((center + offset).to_array(), f32::INFINITY)?;
        let slope = Vec2::from_array(s.gradient);
        // Height-map differences at a retaining wall can span two decks.
        if s.kind != SurfaceKind::Grass || slope.length_squared() > 3.0 {
            return None;
        }
        Some([s.height - offset.dot(slope), slope.x, slope.y])
    });
    let plane = samples.iter().flatten().next()?;
    let mut mask = 0u32;
    for (i, sample) in samples.iter().enumerate() {
        // Disjoint grass elevations inside one patch must not share a tangent plane.
        if sample.is_some_and(|s| (s[0] - plane[0]).abs() < 0.00008) {
            mask |= 1 << i;
        }
    }
    Some([plane[0], plane[1], plane[2], mask as f32])
}

impl Renderer {
    pub(in crate::engine) fn prepare_grass(
        &mut self,
        cache: &mut SupportCache,
        city: &City,
        eye: Vec3,
        look: Vec3,
    ) {
        self.ribbons.counts = [0; 4];
        if self.season == crate::season::Season::Winter || self.construction_span.is_some() {
            return;
        }
        let origin = (eye.truncate() / TILE).floor().as_ivec2();
        if cache.0.len() > 5000 {
            cache
                .0
                .retain(|p, _| (*p - origin).abs().max_element() <= RADIUS);
        }
        let frustum = Frustum::from_matrix(self.view_projection(eye, look));
        let mut bands: [Vec<(f32, [f32; 12])>; 4] = Default::default();
        for y in -RADIUS..=RADIUS {
            for x in -RADIUS..=RADIUS {
                let cell = origin + IVec2::new(x, y);
                let center = (cell.as_vec2() + 0.5) * TILE;
                let half = Vec2::splat(TILE * 0.5);
                let horizontal = ((eye.truncate() - center).abs() - half)
                    .max(Vec2::ZERO)
                    .length();
                if horizontal > END[3]
                    || !frustum.intersects(Bounds {
                        min: (center - half).extend(eye.z - END[3]),
                        max: (center + half).extend(eye.z + END[3]),
                    })
                {
                    continue;
                }
                let Some(p) = *cache
                    .0
                    .entry(cell)
                    .or_insert_with(|| sample_tile(city, cell))
                else {
                    continue;
                };
                let dz = TILE * 0.5 * (p[1].abs() + p[2].abs()) + 0.0003;
                let dist = horizontal.hypot(((p[0] - eye.z).abs() - dz).max(0.));
                let Some(band) = END.iter().position(|&e| dist < e) else {
                    continue;
                };
                if !frustum.intersects(Bounds {
                    min: (center - half - 0.0002).extend(p[0] - dz),
                    max: (center + half + 0.0002).extend(p[0] + dz),
                }) {
                    continue;
                }
                bands[band].push((
                    dist,
                    [
                        cell.x as f32,
                        cell.y as f32,
                        p[0],
                        p[3],
                        p[1],
                        p[2],
                        END[band],
                        ROOTS.get(band + 1).copied().unwrap_or(0) as f32,
                        self.config.width as f32,
                        self.scene_height() as f32,
                        0.,
                        0.,
                    ],
                ));
            }
        }
        let mut tiles = Vec::new();
        for (i, band) in bands.iter_mut().enumerate() {
            band.sort_unstable_by(|a, b| b.0.total_cmp(&a.0));
            self.ribbons.counts[i] = band.len() as u32;
            tiles.extend(band.iter().map(|b| b.1));
        }
        if !tiles.is_empty() {
            self.queue
                .write_buffer(&self.ribbons.tiles, 0, bytemuck::cast_slice(&tiles));
        }
    }
}
