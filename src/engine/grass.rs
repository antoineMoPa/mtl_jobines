//! Photographic ground cover with a complete, linear-light filtered mip chain.
pub(super) fn layout() -> [wgpu::BindGroupLayoutEntry; 2] {
    [
        wgpu::BindGroupLayoutEntry {
            binding: 12,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 13,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        },
    ]
}

fn srgb_to_linear(value: u8) -> f32 {
    let v = value as f32 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let v = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn mip_chain() -> Vec<(u32, u32, Vec<u8>)> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(include_bytes!(
        "../../assets/terrain/grass.png"
    )))
    .read_info()
    .expect("ground grass PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("ground grass pixels");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    pixels.truncate(info.buffer_size());
    let mut levels = vec![(info.width, info.height, pixels)];
    while levels.last().is_some_and(|(w, h, _)| *w > 1 || *h > 1) {
        let (w, h, source) = levels.last().unwrap();
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut target = vec![255; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                // Integer source ranges also retain the last row/column of odd mips.
                let (x0, x1) = (x * w / nw, (x + 1) * w / nw);
                let (y0, y1) = (y * h / nh, (y + 1) * h / nh);
                for c in 0..3 {
                    let mut sum = 0.0;
                    for sy in y0..y1 {
                        for sx in x0..x1 {
                            sum += srgb_to_linear(source[((sy * w + sx) * 4 + c) as usize]);
                        }
                    }
                    target[((y * nw + x) * 4 + c) as usize] =
                        linear_to_srgb(sum / ((x1 - x0) * (y1 - y0)) as f32);
                }
            }
        }
        levels.push((nw, nh, target));
    }
    levels
}

pub(super) fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Sampler) {
    let levels = mip_chain();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("stochastic grass ground"),
        size: wgpu::Extent3d {
            width: levels[0].0,
            height: levels[0].1,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (width, height, pixels)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(*height),
            },
            wgpu::Extent3d {
                width: *width,
                height: *height,
                depth_or_array_layers: 1,
            },
        );
    }
    (
        texture.create_view(&Default::default()),
        device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("grass trilinear anisotropic filtering"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 8,
            ..Default::default()
        }),
    )
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn native_grass_scales_and_snow() {
        use crate::{engine::render::Renderer, hermes::Hermes, season::Season};
        use glam::Vec3;
        use opencity::runtime::{Mesh, SurfaceKind};
        let mut hermes = Hermes::test_map_from_source("land(-1,-1,1,-1,1,1,-1,1);");
        let mut renderer = pollster::block_on(Renderer::new(None, 900, 900)).unwrap();
        renderer.daylight = Some(1.);
        if let Some(pixels) = hermes.update_minimap([0., 0.]) {
            renderer.update_minimap(pixels);
        }
        let mut ground = Mesh::default();
        ground.quad(
            [[-2., -2., 0.], [2., -2., 0.], [2., 2., 0.], [-2., 2., 0.]],
            [0.12, 0.23, 0.075],
            SurfaceKind::Grass,
        );
        let ground = renderer.upload(&ground);
        let mut captures = Vec::new();
        for (season, name, eye) in [
            (Season::Summer, "close", Vec3::new(0., -0.025, 0.018)),
            (Season::Summer, "middle", Vec3::new(0., -0.20, 0.16)),
            (Season::Summer, "wide", Vec3::new(0., -0.85, 0.70)),
            (Season::Winter, "snow", Vec3::new(0., -0.20, 0.16)),
        ] {
            renderer.season = season;
            let png = renderer
                .report_screenshot(
                    &[&ground],
                    eye,
                    Vec3::ZERO,
                    0.,
                    hermes.minimap_view([0., 0.], 0.),
                )
                .unwrap();
            if let Some(dir) = std::env::var_os("GRASS_CAPTURE_DIR") {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(
                    std::path::Path::new(&dir).join(format!("grass-{name}.png")),
                    &png,
                )
                .unwrap();
            }
            captures.push(png);
        }
        assert_ne!(captures[1], captures[3], "grass must retain seasonal snow");
    }

    #[test]
    fn grass_mips_reach_one_texel_and_preserve_mean_light() {
        let levels = super::mip_chain();
        assert_eq!((levels[0].0, levels[0].1), (1600, 1600));
        let last = levels.last().unwrap();
        assert_eq!((last.0, last.1, last.2.len()), (1, 1, 4));
        for (w, h, pixels) in &levels {
            assert_eq!(pixels.len(), (w * h * 4) as usize);
        }
        for c in 0..3 {
            let mean = levels[0]
                .2
                .chunks_exact(4)
                .map(|p| super::srgb_to_linear(p[c]))
                .sum::<f32>()
                / (1600.0 * 1600.0);
            assert!((super::srgb_to_linear(last.2[c]) - mean).abs() < 0.01);
        }
    }
}
