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

// The build packs square levels consecutively, largest first, into one lossless PNG.
fn mip_chain() -> (u32, Vec<u8>) {
    let mut reader = png::Decoder::new(crate::web_asset!(
        "../../assets/terrain/grass-mips.png",
        "assets/terrain/grass-mips.png"
    ))
    .read_info()
    .expect("baked grass mip chain");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("grass mip pixels");
    (info.width, pixels)
}

fn levels(width: u32, pixels: &[u8]) -> impl Iterator<Item = (u32, &[u8])> {
    let mut remaining = pixels;
    (0..width.ilog2() + 1).map(move |level| {
        let size = width >> level;
        let (data, rest) = remaining.split_at(size as usize * size as usize * 4);
        remaining = rest;
        (size, data)
    })
}

pub(super) fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Sampler) {
    let (width, pixels) = mip_chain();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("stochastic grass ground"),
        size: wgpu::Extent3d {
            width,
            height: width,
            depth_or_array_layers: 1,
        },
        mip_level_count: width.ilog2() + 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (size, pixels)) in levels(width, &pixels).enumerate() {
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
                bytes_per_row: Some(size * 4),
                rows_per_image: Some(size),
            },
            wgpu::Extent3d {
                width: size,
                height: size,
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
        let mut hermes =
            Hermes::test_map_from_source("yard=land(-1,-1,1,-1,1,1,-1,1); elevation(yard,0.02);");
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
        let mut grass_cache = Default::default();
        for (season, name, eye) in [
            (Season::Summer, "close", Vec3::new(0., -0.008, 0.0025)),
            (Season::Summer, "middle", Vec3::new(0., -0.20, 0.16)),
            (Season::Summer, "wide", Vec3::new(0., -0.85, 0.70)),
            (Season::Winter, "snow", Vec3::new(0., -0.20, 0.16)),
        ] {
            renderer.season = season;
            renderer.prepare_grass(&mut grass_cache, hermes.city(), eye, Vec3::ZERO);
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
            if name == "close" {
                let gust = renderer
                    .report_screenshot(
                        &[&ground],
                        eye,
                        Vec3::ZERO,
                        0.8,
                        hermes.minimap_view([0., 0.], 0.),
                    )
                    .unwrap();
                assert_ne!(png, gust, "wind moves the close canopy");
                renderer.construction_span = Some(1.);
                renderer.prepare_grass(&mut grass_cache, hermes.city(), eye, Vec3::ZERO);
                renderer.construction_span = None;
                let bare = renderer
                    .report_screenshot(
                        &[&ground],
                        eye,
                        Vec3::ZERO,
                        0.,
                        hermes.minimap_view([0., 0.], 0.),
                    )
                    .unwrap();
                assert_ne!(png, bare, "the ribbon pass must contribute visible pixels");
            }
            captures.push(png);
        }
        // Default car chase framing over turf, with identical lighting, time and HUD.
        renderer.season = Season::Summer;
        let eye = Vec3::new(0., -0.063, 0.0255);
        let look = Vec3::new(0., 0.06, 0.014);
        let frame = |renderer: &Renderer| {
            renderer
                .report_screenshot(&[&ground], eye, look, 0., hermes.minimap_view([0., 0.], 0.))
                .unwrap()
        };
        let ground_only = frame(&renderer);
        renderer.prepare_grass(&mut grass_cache, hermes.city(), eye, look);
        let with_blades = frame(&renderer);
        let pixels = |png: &[u8]| {
            let mut reader = png::Decoder::new(png).read_info().unwrap();
            let mut data = vec![0; reader.output_buffer_size()];
            reader.next_frame(&mut data).unwrap();
            data
        };
        let bare = pixels(&ground_only);
        let blades = pixels(&with_blades);
        // Foreground turf below the horizon, clear of the minimap.
        let mut changed = 0;
        for y in 630..870 {
            for x in 300..850 {
                let i = (y * 900 + x) * 4;
                if (0..3).any(|c| bare[i + c].abs_diff(blades[i + c]) >= 6) {
                    changed += 1;
                }
            }
        }
        eprintln!("Chase-camera grass changes {changed} of 132000 foreground pixels by at least six levels");
        if let Some(dir) = std::env::var_os("GRASS_CAPTURE_DIR") {
            std::fs::write(
                std::path::Path::new(&dir).join("grass-chase.png"),
                with_blades,
            )
            .unwrap();
        }
        assert!(
            changed > 6600,
            "blades must read in the gameplay foreground"
        );
        assert_ne!(captures[1], captures[3], "grass must retain seasonal snow");
    }

    fn srgb_to_linear(value: u8) -> f32 {
        let v = value as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }

    #[test]
    fn grass_mips_reach_one_texel_and_preserve_mean_light() {
        let (width, pixels) = super::mip_chain();
        let levels: Vec<_> = super::levels(width, &pixels).collect();
        assert_eq!(levels[0].0, 1600);
        let last = levels.last().unwrap();
        assert_eq!((last.0, last.1.len()), (1, 4));
        for (w, pixels) in &levels {
            assert_eq!(pixels.len(), (w * w * 4) as usize);
        }
        for c in 0..3 {
            let mean = levels[0]
                .1
                .chunks_exact(4)
                .map(|p| srgb_to_linear(p[c]))
                .sum::<f32>()
                / (1600.0 * 1600.0);
            assert!((srgb_to_linear(last.1[c]) - mean).abs() < 0.01);
        }
    }
}
