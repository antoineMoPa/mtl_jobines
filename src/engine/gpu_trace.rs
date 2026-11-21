//! Sample real GPU durations without waiting for readback on the frame thread.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum Pass {
    Shadow,
    World,
    Occlusion,
    Composite,
    Hud,
}
const PASSES: [(Pass, &str); 5] = [
    (Pass::Shadow, "gpu.shadow"),
    (Pass::World, "gpu.world"),
    (Pass::Occlusion, "gpu.ambient_occlusion"),
    (Pass::Composite, "gpu.composite_and_weather"),
    (Pass::Hud, "gpu.hud"),
];
// In-pass boundaries between successive shader families in the world pass.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum Shader {
    Opaque,
    Cutout,
    Vehicles,
    Models,
    Pedestrians,
    Graffiti,
    Sky,
}
#[cfg(not(target_arch = "wasm32"))]
const SHADERS: [&str; 7] = [
    "gpu.world.opaque",
    "gpu.world.cutout",
    "gpu.world.vehicles",
    "gpu.world.models",
    "gpu.world.pedestrians",
    "gpu.world.graffiti",
    "gpu.world.sky_and_effects",
];
const PASS_QUERIES: u32 = PASSES.len() as u32 * 2;
pub struct GpuTrace {
    queries: wgpu::QuerySet,
    #[cfg(not(target_arch = "wasm32"))]
    resolve: wgpu::Buffer,
    #[cfg(not(target_arch = "wasm32"))]
    readback: wgpu::Buffer,
    busy: Arc<AtomicBool>,
    detailed: bool,
    pub split: bool,
    next: std::cell::Cell<u8>,
}
impl GpuTrace {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(device: &wgpu::Device) -> Self {
        let detailed = device
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);
        let split = !detailed && std::env::var("MTL_TRACE_SHADERS").as_deref() == Ok("1");
        let count = PASS_QUERIES
            + if split {
                SHADERS.len() as u32 * 2
            } else if detailed {
                SHADERS.len() as u32 - 1
            } else {
                0
            };
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("game trace timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count,
        });
        let buffer = |usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("game trace GPU samples"),
                size: u64::from(count) * 8,
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            queries,
            resolve: buffer(wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
            readback: buffer(wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ),
            busy: Arc::new(AtomicBool::new(false)),
            detailed,
            split,
            next: 0.into(),
        }
    }
    pub fn sample(&self) -> bool {
        let frame = self.next.get();
        self.next.set((frame + 1) % 8);
        frame == 0 && !self.busy.swap(true, Ordering::AcqRel)
    }
    pub fn pass(&self, pass: Pass) -> wgpu::RenderPassTimestampWrites<'_> {
        let index = pass as u32 * 2;
        wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(index),
            end_of_pass_write_index: Some(index + 1),
        }
    }
    pub fn shader_pass(&self, shader: Shader) -> wgpu::RenderPassTimestampWrites<'_> {
        let index = PASS_QUERIES + shader as u32 * 2;
        wgpu::RenderPassTimestampWrites {
            query_set: &self.queries,
            beginning_of_pass_write_index: Some(index),
            end_of_pass_write_index: Some(index + 1),
        }
    }
    pub fn boundary(&self, pass: &mut wgpu::RenderPass<'_>, shader: Shader) {
        if self.detailed {
            pass.write_timestamp(&self.queries, PASS_QUERIES + shader as u32 - 1);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn finish(&self, encoder: &mut wgpu::CommandEncoder, period: f32, hud: bool) {
        let size = self.readback.size();
        encoder.resolve_query_set(&self.queries, 0..(size / 8) as u32, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, size);
        let buffer = self.readback.clone();
        let busy = self.busy.clone();
        let detailed = self.detailed;
        let split = self.split;
        let sample = mtl_world::telemetry::GpuSample::current();
        encoder.map_buffer_on_submit(&self.readback, wgpu::MapMode::Read, .., move |result| {
            if result.is_ok() {
                if let Some(sample) = sample {
                    let bytes = buffer.slice(..).get_mapped_range().unwrap();
                    let tick =
                        |i: usize| u64::from_ne_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
                    let measure = |name, start, end| {
                        if let Some(delta) = tick(end).checked_sub(tick(start)) {
                            sample.duration(name, delta as f64 * f64::from(period) / 1e6);
                        }
                    };
                    for (pass, name) in PASSES {
                        if matches!(pass, Pass::Hud) && !hud {
                            continue;
                        }
                        if split && matches!(pass, Pass::World) {
                            continue;
                        }
                        let start = pass as usize * 2;
                        measure(name, start, start + 1);
                    }
                    if split {
                        for (i, name) in SHADERS.iter().enumerate() {
                            let start = PASS_QUERIES as usize + 2 * i;
                            measure(name, start, start + 1);
                        }
                    } else if detailed {
                        let mut start = Pass::World as usize * 2;
                        for (i, name) in SHADERS.iter().enumerate() {
                            let end = if i + 1 == SHADERS.len() {
                                Pass::World as usize * 2 + 1
                            } else {
                                PASS_QUERIES as usize + i
                            };
                            measure(name, start, end);
                            start = end;
                        }
                    }
                }
                buffer.unmap();
            }
            busy.store(false, Ordering::Release);
        });
    }
}
