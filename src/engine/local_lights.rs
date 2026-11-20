//! A compact world grid keeps fragment work proportional to nearby emitters.
use crate::dynamic_world::{LocalLight, Wake, Waterline};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use std::cell::RefCell;

const MAX_LIGHTS: usize = 128;
const GRID: usize = 16;
const CELL: f32 = 0.20;
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LightGrid {
    origin: [f32; 4],
    lights: [LocalLight; MAX_LIGHTS],
    cells: [[u32; 4]; GRID * GRID],
    wakes: [Wake; 32],
    hulls: [Waterline; 8],
}
fn grid(sources: &[LocalLight], center: Vec3) -> LightGrid {
    let mut result = LightGrid::zeroed();
    let origin = center.truncate() - glam::Vec2::splat(GRID as f32 * CELL * 0.5);
    result.origin = [origin.x, origin.y, 1. / CELL, GRID as f32];
    let mut selected: Vec<_> = sources
        .iter()
        .filter(|light| {
            let p = light.position().truncate() - origin;
            let r = light.position_range[3];
            p.x + r >= 0.
                && p.y + r >= 0.
                && p.x - r < GRID as f32 * CELL
                && p.y - r < GRID as f32 * CELL
        })
        .collect();
    selected.sort_by(|a, b| {
        a.position()
            .distance_squared(center)
            .total_cmp(&b.position().distance_squared(center))
    });
    for (index, &&light) in selected.iter().take(MAX_LIGHTS).enumerate() {
        result.lights[index] = light;
        let p = light.position().truncate() - origin;
        let radius = glam::Vec2::splat(light.position_range[3]);
        let lo = ((p - radius) / CELL)
            .floor()
            .as_ivec2()
            .max(glam::IVec2::ZERO);
        let hi = ((p + radius) / CELL)
            .floor()
            .as_ivec2()
            .min(glam::IVec2::splat(GRID as i32 - 1));
        for y in lo.y..=hi.y {
            for x in lo.x..=hi.x {
                result.cells[y as usize * GRID + x as usize][index / 32] |= 1 << (index % 32);
            }
        }
    }
    result
}
pub(super) struct LocalLights {
    pub buffer: wgpu::Buffer,
    pub flash: std::cell::Cell<Option<LocalLight>>,
    sources: RefCell<Vec<LocalLight>>,
    pub wakes: std::cell::Cell<[Wake; 32]>,
    pub hulls: std::cell::Cell<[Waterline; 8]>,
}
impl LocalLights {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("local light grid"),
                size: std::mem::size_of::<LightGrid>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            flash: Default::default(),
            sources: RefCell::new(Vec::new()),
            wakes: Default::default(),
            hulls: Default::default(),
        }
    }
    pub fn set(&self, sources: Vec<LocalLight>) {
        self.sources.replace(sources);
    }
    pub fn upload(&self, queue: &wgpu::Queue, center: Vec3) {
        let mut sources = self.sources.borrow_mut();
        let count = sources.len();
        sources.extend(self.flash.get());
        let mut data = grid(&sources, center);
        sources.truncate(count);
        data.hulls = self.hulls.get();
        data.wakes = self.wakes.get();
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&data));
    }
}

#[cfg(test)]
mod tests;
