//! Embedded vehicle albedo atlases; UVs stay on the source surfaces.
use crate::dynamic_world::catalog;

pub(super) fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Sampler) {
    upload_pngs(
        device,
        queue,
        catalog::definitions()
            .iter()
            .filter_map(|definition| definition.texture_png.as_deref())
            .chain(std::iter::once(
                include_bytes!("../../data/vehicle/rear-plate.png").as_slice(),
            )),
    )
}
pub(super) fn upload_pngs<'a>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    images: impl Iterator<Item = &'a [u8]>,
) -> (wgpu::TextureView, wgpu::Sampler) {
    let images: Vec<_> = images
        .map(|bytes| {
            let mut decoder = png::Decoder::new(bytes);
            decoder
                .set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
            let mut reader = decoder.read_info().expect("embedded vehicle PNG");
            let mut buffer = vec![0; reader.output_buffer_size()];
            let info = reader.next_frame(&mut buffer).expect("vehicle PNG pixels");
            let pixels: Vec<u8> = buffer[..info.buffer_size()]
                .chunks_exact(info.color_type.samples())
                .flat_map(|p| match info.color_type {
                    png::ColorType::Rgb => [p[0], p[1], p[2], 255],
                    png::ColorType::Rgba => [p[0], p[1], p[2], p[3]],
                    png::ColorType::Grayscale => [p[0], p[0], p[0], 255],
                    png::ColorType::GrayscaleAlpha => [p[0], p[0], p[0], p[1]],
                    png::ColorType::Indexed => unreachable!("PNG palette is expanded"),
                })
                .collect();
            (pixels, info.width, info.height)
        })
        .collect();
    let width = images.iter().map(|(_, w, _)| *w).max().unwrap_or(1);
    let height = images.iter().map(|(_, _, h)| *h).max().unwrap_or(1);
    let mip_levels = 32 - width.max(height).leading_zeros();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("embedded vehicle albedo"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: {
                let count = images.len().max(1) as u32;
                if device.limits().max_storage_buffers_per_shader_stage == 0 {
                    // Force a 2D array rather than wgpu GL's D2/cube heuristic.
                    if count == 1 || count % 6 == 0 {
                        count + 1
                    } else {
                        count
                    }
                } else {
                    count
                }
            },
        },
        mip_level_count: mip_levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    // Compare the linear sum against sRGB half-byte boundaries. This keeps the
    // four-texel filter gamma correct without evaluating powf for every texel.
    let linear: [f32; 256] = std::array::from_fn(|i| srgb_to_linear(i as f32 / 255.));
    let thresholds: [f32; 255] =
        std::array::from_fn(|i| 4. * srgb_to_linear((i as f32 + 0.5) / 255.));
    for (layer, (source, source_width, source_height)) in images.into_iter().enumerate() {
        let mut pixels = if (source_width, source_height) == (width, height) {
            source
        } else {
            let mut pixels = vec![0; (width * height * 4) as usize];
            for y in 0..height {
                for x in 0..width {
                    let from = ((y * source_height / height) * source_width
                        + x * source_width / width) as usize
                        * 4;
                    let to = (y * width + x) as usize * 4;
                    pixels[to..to + 4].copy_from_slice(&source[from..from + 4]);
                }
            }
            pixels
        };
        let (mut w, mut h) = (width, height);
        for level in 0..mip_levels {
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
            if level + 1 == mip_levels {
                break;
            }
            let (next_w, next_h) = ((w / 2).max(1), (h / 2).max(1));
            let mut next = vec![0; (next_w * next_h * 4) as usize];
            for y in 0..next_h {
                for x in 0..next_w {
                    let offset = |dx: u32, dy: u32| {
                        (((2 * y + dy).min(h - 1) * w + (2 * x + dx).min(w - 1)) * 4) as usize
                    };
                    let samples = [offset(0, 0), offset(1, 0), offset(0, 1), offset(1, 1)];
                    let to = ((y * next_w + x) * 4) as usize;
                    for channel in 0..3 {
                        let sum: f32 = samples
                            .iter()
                            .map(|i| linear[pixels[i + channel] as usize])
                            .sum();
                        next[to + channel] =
                            thresholds.partition_point(|&boundary| boundary <= sum) as u8;
                    }
                    let alpha: u16 = samples.iter().map(|i| u16::from(pixels[i + 3])).sum();
                    next[to + 3] = ((alpha + 2) / 4) as u8;
                }
            }
            pixels = next;
            (w, h) = (next_w, next_h);
        }
    }
    let view = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("vehicle albedo filtering"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        ..Default::default()
    });
    (view, sampler)
}

fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
