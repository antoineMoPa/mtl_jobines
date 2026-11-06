//! Native readback uses a separate render target so palette windows stay out of reports.
use super::render::{GpuMesh, Renderer};
use crate::hermes::MinimapView;
use glam::Vec3;

impl Renderer {
    pub(super) fn report_screenshot(
        &self,
        meshes: &[&GpuMesh],
        eye: Vec3,
        look: Vec3,
        time: f32,
        navigation: MinimapView,
    ) -> Result<Vec<u8>, String> {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bug report screenshot"),
            size: wgpu::Extent3d {
                width: self.config.width,
                height: self.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.config.format.add_srgb_suffix(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        self.render(
            &texture.create_view(&Default::default()),
            meshes,
            eye,
            look,
            time,
            navigation,
        );
        let rgba = self.rgba_pixels(&texture)?;
        encode_png(&rgba, self.config.width, self.config.height)
    }

    pub(super) fn rgba_pixels(&self, texture: &wgpu::Texture) -> Result<Vec<u8>, String> {
        let row = (texture.width() * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot readback"),
            size: row as u64 * texture.height() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
        self.queue.submit(Some(encoder.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| error.to_string())?;
        rx.recv()
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        let bytes = buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|error| error.to_string())?;
        rgba_rows(
            &bytes,
            texture.width(),
            texture.height(),
            row,
            texture.format(),
        )
    }
}

fn rgba_rows(
    bytes: &[u8],
    width: u32,
    height: u32,
    row: u32,
    format: wgpu::TextureFormat,
) -> Result<Vec<u8>, String> {
    let bgra = match format {
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => false,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => true,
        _ => return Err(format!("Unsupported screenshot format: {format:?}")),
    };
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for source in bytes.chunks_exact(row as usize).take(height as usize) {
        for pixel in source[..width as usize * 4].chunks_exact(4) {
            let [r, g, b] = if bgra {
                [pixel[2], pixel[1], pixel[0]]
            } else {
                [pixel[0], pixel[1], pixel[2]]
            };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Ok(rgba)
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(rgba)
            .map_err(|error| error.to_string())?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn report_screenshot_renders_and_reads_back_a_real_gpu_target() {
        let renderer = pollster::block_on(Renderer::new(None, 65, 40)).unwrap();
        let navigation = MinimapView {
            uv_center: [0.5; 2],
            uv_span: 1.,
            heading: 0.,
            player_uv: [0.5; 2],
            destination_uv: [0.5; 2],
            route_status: crate::hermes::RouteStatus::Inactive,
        };
        let png = renderer
            .report_screenshot(&[], Vec3::new(0., -1., 0.5), Vec3::ZERO, 0., navigation)
            .unwrap();
        let mut reader = png::Decoder::new(png.as_slice()).read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (65, 40));
        let mut pixels = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut pixels).unwrap();
        assert!(pixels.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[..3] != [0; 3]));
    }

    #[test]
    fn png_readback_strips_padding_maps_bgra_and_keeps_rows_in_order() {
        let pixels = [1, 2, 3, 0, 90, 90, 90, 90, 4, 5, 6, 0, 90, 90, 90, 90];
        for (format, expected) in [
            (
                wgpu::TextureFormat::Rgba8UnormSrgb,
                vec![1, 2, 3, 255, 4, 5, 6, 255],
            ),
            (
                wgpu::TextureFormat::Bgra8UnormSrgb,
                vec![3, 2, 1, 255, 6, 5, 4, 255],
            ),
        ] {
            let rgba = rgba_rows(&pixels, 1, 2, 8, format).unwrap();
            assert_eq!(rgba, expected);
            let png = encode_png(&rgba, 1, 2).unwrap();
            let mut reader = png::Decoder::new(png.as_slice()).read_info().unwrap();
            let mut decoded = vec![0; reader.output_buffer_size()];
            let frame = reader.next_frame(&mut decoded).unwrap();
            assert_eq!((frame.width, frame.height), (1, 2));
            assert_eq!(&decoded[..frame.buffer_size()], expected);
        }
    }
}
