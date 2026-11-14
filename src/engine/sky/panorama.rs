//! Linear cloud masks: red is daytime, green is nighttime; horizontal repeat.
pub(super) fn create(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::BindGroupLayout, wgpu::BindGroup) {
    let mut reader =
        png::Decoder::new(include_bytes!("../../../assets/sky/cloud-highlights.png").as_slice())
            .read_info()
            .expect("cloud highlight PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut pixels)
        .expect("cloud highlight pixels");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    let size = wgpu::Extent3d {
        width: info.width,
        height: info.height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("day and night cloud highlights"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &pixels[..info.buffer_size()],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(info.width * 4),
            rows_per_image: Some(info.height),
        },
        size,
    );
    let view = texture.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("sky panorama repeat"),
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sky panorama"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sky panorama"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    (layout, group)
}
