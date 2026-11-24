//! A fixed GPU ring. Two instanced quads share each wheel sample: packed snow and powder.
use super::Renderer;
use crate::dynamic_world::{DynamicWorld, METERS_PER_UNIT};
use glam::Vec3;
use opencity::runtime::{City, SurfaceKind};
use std::{collections::HashMap, ops::Range};

const CAPACITY: usize = 4096;
const POWDER_LIFE: f32 = 0.75;
const TRACK_LIFE: f32 = 45.;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Stamp {
    from_time: [f32; 4],
    to_width: [f32; 4],
    outward_seed: [f32; 4],
}

pub(super) struct WheelSnow {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    stamps: Vec<Stamp>,
    previous: HashMap<(usize, usize), (Vec3, f32)>,
    born: Box<[f32; CAPACITY]>,
    oldest: usize,
    powder: usize,
    end: usize,
    last_update: Option<f32>,
}

impl WheelSnow {
    fn new(r: &Renderer) -> Self {
        let gpu = &r.device;
        let shader=gpu.create_shader_module(wgpu::ShaderModuleDescriptor{
            label:Some("winter wheel materials"),
            source:wgpu::ShaderSource::Wgsl(format!("const POWDER_LIFE:f32={POWDER_LIFE};\nconst TRACK_LIFE:f32={TRACK_LIFE};\n{}\n{}",include_str!("../snow_cache.wgsl"),include_str!("wheel_snow.wgsl")).into()),
        });
        let layout = gpu.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("winter wheel layout"),
            bind_group_layouts: &[
                Some(&r.pipeline.get_bind_group_layout(0)),
                Some(&r.pipeline.get_bind_group_layout(1)),
            ],
            immediate_size: 0,
        });
        let pipeline = gpu.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("winter wheel tracks and spray"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: size_of::<Stamp>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &wgpu::vertex_attr_array![0=>Float32x4,1=>Float32x4,2=>Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: r.config.format.add_srgb_suffix(),
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: r.samples,
                ..Default::default()
            },
            multiview_mask: None,
            cache: None,
        });
        Self {
            pipeline,
            buffer: r.dynamic_buffer((CAPACITY * size_of::<Stamp>()) as u64),
            previous: Default::default(),
            stamps: Vec::new(),
            born: Box::new([0.; CAPACITY]),
            oldest: 0,
            powder: 0,
            end: 0,
            last_update: None,
        }
    }
    fn update(
        &mut self,
        queue: &wgpu::Queue,
        world: &DynamicWorld,
        city: &City,
        eye: Vec3,
        time: f32,
    ) {
        if self.last_update == Some(time) {
            return;
        }
        self.last_update = Some(time);
        self.stamps.clear();
        // An absent slot (stopped, airborne, off-camera or recycled) starts a new trail.
        for wheel in world.snow_wheels(eye * METERS_PER_UNIT) {
            let point = wheel.point / METERS_PER_UNIT;
            let (previous, seen) = self.previous.entry(wheel.key).or_insert((point, time));
            *seen = time;
            let travel = point.distance(*previous) * METERS_PER_UNIT;
            if travel < 0.55 {
                continue;
            }
            let old = std::mem::replace(previous, point);
            if travel > 5. {
                continue;
            }
            let mut ends = [old, point];
            let mut slope = glam::Vec2::ZERO;
            let supported = ends.iter_mut().all(|p| {
                let Some(g) = city.surface_at(p.truncate().to_array(), p.z + 0.004) else {
                    return false;
                };
                if (g.height - p.z).abs() >= 0.004
                    || !matches!(
                        g.kind,
                        SurfaceKind::Asphalt | SurfaceKind::Sidewalk | SurfaceKind::Grass
                    )
                {
                    return false;
                }
                p.z = g.height;
                slope = glam::Vec2::from(g.gradient);
                true
            });
            if !supported {
                continue;
            }
            let delta = (ends[1] - ends[0]).truncate();
            let cross_slope = slope.dot(delta.perp()) / delta.length().max(0.00001);
            let seed = ((self.end + self.stamps.len()) % 4093) as f32;
            self.stamps.push(Stamp {
                from_time: ends[0].extend(time).to_array(),
                to_width: ends[1].extend(wheel.width / METERS_PER_UNIT).to_array(),
                outward_seed: wheel
                    .outward
                    .truncate()
                    .extend(cross_slope)
                    .extend(if wheel.sprays { seed } else { -1. })
                    .to_array(),
            });
        }
        self.previous.retain(|_, (_, seen)| *seen == time);
        let n = self.stamps.len().min(CAPACITY);
        let stamps = &self.stamps[self.stamps.len() - n..];
        let slot = self.end % CAPACITY;
        let (first, rest) = stamps.split_at(n.min(CAPACITY - slot));
        for (offset, slice) in [(slot, first), (0, rest)] {
            if !slice.is_empty() {
                queue.write_buffer(
                    &self.buffer,
                    (offset * size_of::<Stamp>()) as u64,
                    bytemuck::cast_slice(slice),
                );
                self.born[offset..offset + slice.len()].fill(time);
            }
        }
        self.end += n;
        self.oldest = self.oldest.max(self.end.saturating_sub(CAPACITY));
        while self.oldest < self.end && time - self.born[self.oldest % CAPACITY] >= TRACK_LIFE {
            self.oldest += 1;
        }
        self.powder = self.powder.max(self.oldest);
        while self.powder < self.end && time - self.born[self.powder % CAPACITY] >= POWDER_LIFE {
            self.powder += 1;
        }
    }

    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, r: &Renderer) {
        if self.oldest == self.end {
            return;
        }
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &r.camera_group, &[]);
        pass.set_bind_group(1, &r.shadow_group, &[]);
        pass.set_vertex_buffer(0, self.buffer.slice(..));
        for (start, vertices) in [(self.oldest, 0..6), (self.powder, 6..12)] {
            for instances in ring_ranges(start, self.end) {
                if !instances.is_empty() {
                    pass.draw(vertices.clone(), instances);
                }
            }
        }
    }
}
fn ring_ranges(start: usize, end: usize) -> [Range<u32>; 2] {
    let slot = start % CAPACITY;
    let first = (end - start).min(CAPACITY - slot);
    [
        slot as u32..(slot + first) as u32,
        0..(end - start - first) as u32,
    ]
}
impl Renderer {
    pub(in crate::engine) fn update_wheel_snow(
        &mut self,
        world: &DynamicWorld,
        city: &City,
        eye: Vec3,
        time: f32,
        reset: bool,
    ) {
        if reset {
            self.wheel_snow = None;
        }
        if self.season != crate::season::Season::Winter {
            return;
        }
        if self.wheel_snow.is_none() {
            self.wheel_snow = Some(WheelSnow::new(self));
        }
        self.wheel_snow
            .as_mut()
            .unwrap()
            .update(&self.queue, world, city, eye, time);
    }
}
#[cfg(test)]
mod tests;
