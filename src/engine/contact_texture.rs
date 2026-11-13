//! Host bindings for OpenCity's shared, once-baked contact masks.
use opencity::runtime::contact_debris;
use wgpu::util::DeviceExt;

pub(super) struct ContactTexture {
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub settings: wgpu::Buffer,
    enabled: std::sync::atomic::AtomicBool,
}
impl ContactTexture {
    pub fn layout(first: u32) -> [wgpu::BindGroupLayoutEntry; 3] {
        [
            wgpu::BindGroupLayoutEntry {
                binding: first,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: first + 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: first + 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(16),
                },
                count: None,
            },
        ]
    }
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let mips = contact_debris::atlas();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("contact masks: dirt gravel leaves paper"),
            size: wgpu::Extent3d {
                width: contact_debris::WIDTH,
                height: contact_debris::HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: mips.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (level, mip) in mips.iter().enumerate() {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    mip_level: level as u32,
                    ..texture.as_image_copy()
                },
                &mip.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(mip.width * 4),
                    rows_per_image: Some(mip.height),
                },
                wgpu::Extent3d {
                    width: mip.width,
                    height: mip.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("filtered contact tile"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let settings = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("approved contact settings"),
            contents: bytemuck::cast_slice(&contact_debris::Settings::default().uniform()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        Self {
            view: texture.create_view(&Default::default()),
            sampler,
            settings,
            enabled: std::sync::atomic::AtomicBool::new(true),
        }
    }
    pub fn entries(&self, first: u32) -> [wgpu::BindGroupEntry<'_>; 3] {
        [
            wgpu::BindGroupEntry {
                binding: first,
                resource: wgpu::BindingResource::TextureView(&self.view),
            },
            wgpu::BindGroupEntry {
                binding: first + 1,
                resource: wgpu::BindingResource::Sampler(&self.sampler),
            },
            wgpu::BindGroupEntry {
                binding: first + 2,
                resource: self.settings.as_entire_binding(),
            },
        ]
    }
    pub fn set_enabled(&self, queue: &wgpu::Queue, enabled: bool) {
        if self
            .enabled
            .swap(enabled, std::sync::atomic::Ordering::Relaxed)
            == enabled
        {
            return;
        }
        queue.write_buffer(
            &self.settings,
            0,
            bytemuck::cast_slice(
                &contact_debris::Settings {
                    enabled,
                    ..Default::default()
                }
                .uniform(),
            ),
        );
    }
}
