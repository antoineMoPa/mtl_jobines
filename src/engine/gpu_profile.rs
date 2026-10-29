//! Optional per-pass GPU timestamps for the native driving profiler.
pub struct GpuProfile {
    query: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
}
impl GpuProfile {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            query: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("driving GPU pass timings"),
                ty: wgpu::QueryType::Timestamp,
                count: 6,
            }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("GPU timestamp resolve"),
                size: 48,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("GPU timestamp readback"),
                size: 48,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            }),
        }
    }
    pub fn pass(&self, stage: u32) -> wgpu::RenderPassTimestampWrites<'_> {
        wgpu::RenderPassTimestampWrites {
            query_set: &self.query,
            beginning_of_pass_write_index: Some(stage * 2),
            end_of_pass_write_index: Some(stage * 2 + 1),
        }
    }
    pub fn resolve(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.resolve_query_set(&self.query, 0..6, &self.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, 48);
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(&self, device: &wgpu::Device, period: f32) -> Result<[f64; 3], String> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let values = self
            .readback
            .slice(..)
            .get_mapped_range()
            .map_err(|e| e.to_string())?;
        let mut stages = [0.; 3];
        for (stage, pair) in values.chunks_exact(16).enumerate() {
            let begin = u64::from_ne_bytes(pair[..8].try_into().unwrap());
            let end = u64::from_ne_bytes(pair[8..].try_into().unwrap());
            stages[stage] = end.saturating_sub(begin) as f64 * f64::from(period) / 1_000_000.;
        }
        drop(values);
        self.readback.unmap();
        Ok(stages)
    }
}
