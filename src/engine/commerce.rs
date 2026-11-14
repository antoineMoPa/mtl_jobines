//! Device-owned approved commerce snapshots and appended shader entry points.
#[cfg(test)]
use crate::hermes::commerce_mesh::BRANDS;
pub(super) struct Commerce {
    pub layout: wgpu::BindGroupLayout,
    pub group: wgpu::BindGroup,
    pub graffiti: super::graffiti::PaintTexture,
}
const ART: [&[u8]; 10] = [
    include_bytes!("../../assets/commerce/brands/jean-couteau-light.png"),
    include_bytes!("../../assets/commerce/brands/st-hurtubise-panel.png"),
    include_bytes!("../../assets/commerce/brands/couche-tot-light.png"),
    include_bytes!("../../assets/commerce/brands/mini-panel.png"),
    include_bytes!("../../assets/commerce/brands/ici-light.png"),
    include_bytes!("../../assets/commerce/brands/piassorama-panel.png"),
    include_bytes!("../../assets/commerce/brands/tom-houblon-light.png"),
    include_bytes!("../../assets/commerce/brands/belle-patente-panel.png"),
    include_bytes!("../../assets/commerce/brands/simonac-light.png"),
    include_bytes!("../../assets/commerce/brands/cineplouf-panel.png"),
];
fn decode(bytes: &[u8]) -> (Vec<u8>, u32, u32) {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .expect("approved PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("approved RGBA");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    pixels.truncate(info.buffer_size());
    (pixels, info.width, info.height)
}
pub(super) fn shader() -> String {
    format!(
        "{}\n{}\n{}\n{}\n{}",
        opencity::runtime::GAME_SHADER,
        include_str!("snow.wgsl"),
        include_str!("local_lights.wgsl"),
        include_str!("commerce.wgsl"),
        include_str!("graffiti.wgsl")
    )
}
impl Commerce {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = |label, width, height, layers| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let signs = texture("approved commerce signs", 512, 128, 10);
        for (layer, art) in ART.iter().enumerate() {
            let (source, w, h) = decode(art);
            let mut scaled = vec![0; 512 * 128 * 4];
            for y in 0..128usize {
                for x in 0..512usize {
                    let from = ((y * h as usize / 128) * w as usize + x * w as usize / 512) * 4;
                    scaled[(y * 512 + x) * 4..(y * 512 + x + 1) * 4]
                        .copy_from_slice(&source[from..from + 4]);
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &signs,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: 0,
                        y: 0,
                        z: layer as u32,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &scaled,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(512 * 4),
                    rows_per_image: Some(128),
                },
                wgpu::Extent3d {
                    width: 512,
                    height: 128,
                    depth_or_array_layers: 1,
                },
            );
        }
        let (pixels, w, h) = decode(include_bytes!("../../assets/commerce/interiors.png"));
        let rooms = texture("two virtual commerce layers", w, h, 1);
        queue.write_texture(
            rooms.as_image_copy(),
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
        let entries = [
            wgpu::TextureViewDimension::D2Array,
            wgpu::TextureViewDimension::D2,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, dimension)| wgpu::BindGroupLayoutEntry {
            binding: i as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: dimension,
                multisampled: false,
            },
            count: None,
        });
        let mut entries: Vec<_> = entries.collect();
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        entries.extend([
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 4,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 5,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(32),
                },
                count: None,
            },
        ]);
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 6,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2Array,
                multisampled: false,
            },
            count: None,
        });
        entries.push(wgpu::BindGroupLayoutEntry {
            binding: 7,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
        entries.extend(opencity::runtime::tree_textures::TreeTextures::layout(8));
        entries.extend(opencity::runtime::tree_textures::TreeImpostors::layout(10));
        let (vehicle_albedo, vehicle_sampler) = super::vehicle_textures::upload(device, queue);
        let trees = opencity::runtime::tree_textures::TreeTextures::new(device, queue);
        let impostors = opencity::runtime::tree_textures::TreeImpostors::new(device, queue);
        let graffiti = super::graffiti::PaintTexture::new(device, queue);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("game commerce"),
            entries: &entries,
        });
        let signs = signs.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let rooms = rooms.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("game commerce"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&signs),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&rooms),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&graffiti.view),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::Sampler(&graffiti.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: graffiti.settings.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&vehicle_albedo),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&vehicle_sampler),
                },
            ]
            .into_iter()
            .chain(trees.entries(8))
            .chain(impostors.entries(10))
            .collect::<Vec<_>>(),
        });
        Self {
            layout,
            group,
            graffiti,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approved_alpha_and_aspect_are_preserved() {
        for (i, bytes) in ART.iter().enumerate() {
            let (p, w, h) = decode(bytes);
            assert!((w as f32 / h as f32 - BRANDS[i].1).abs() < 0.001);
            if BRANDS[i].2 {
                assert!(p.chunks_exact(4).any(|p| p[3] == 0));
                assert!(p.chunks_exact(4).any(|p| p[3] > 240));
            }
        }
        let (p, w, h) = decode(include_bytes!("../../assets/commerce/interiors.png"));
        assert_eq!((w, h), (1024, 1024));
        assert!(p.chunks_exact(4).any(|p| p[3] == 0));
    }
    #[test]
    fn appended_shader_validates() {
        let module = naga::front::wgsl::parse_str(&shader()).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
