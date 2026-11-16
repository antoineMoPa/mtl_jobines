//! Half-resolution depth AO, followed by depth-aware upsampling before the HUD.
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Settings {
    inverse: [[f32; 4]; 4],
    eye_radius: [f32; 4],
    viewport: [f32; 4],
    tuning: [f32; 4],
    // Time, winter strength, daylight, and reserved.
    weather: [f32; 4],
}
pub(super) struct Ssao {
    pub scene: wgpu::TextureView,
    ao: wgpu::TextureView,
    uniform: wgpu::Buffer,
    ao_layout: wgpu::BindGroupLayout,
    composite_layout: wgpu::BindGroupLayout,
    ao_group: wgpu::BindGroup,
    composite_group: wgpu::BindGroup,
    ao_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
}
fn layout(device: &wgpu::Device, composite: bool, samples: u32) -> wgpu::BindGroupLayout {
    let mut entries = vec![
        wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
        wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Depth,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: samples > 1,
            },
            count: None,
        },
    ];
    if composite {
        for binding in [2, 3] {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("screen-space AO"),
        entries: &entries,
    })
}
fn texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("screen-space AO target"),
            size: wgpu::Extent3d {
                width: width.max(1),
                height: height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
impl Ssao {
    pub fn new(
        device: &wgpu::Device,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        depth: &wgpu::TextureView,
        samples: u32,
    ) -> Self {
        let ao_layout = layout(device, false, samples);
        let composite_layout = layout(device, true, samples);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("screen-space ambient occlusion"),
            source: wgpu::ShaderSource::Wgsl(
                super::depth_shader(include_str!("ssao.wgsl"), samples).into(),
            ),
        });
        let pipeline = |entry: &str, bind: &wgpu::BindGroupLayout, format| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("screen-space AO"),
                bind_group_layouts: &[Some(bind)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let ao_pipeline = pipeline("ao", &ao_layout, wgpu::TextureFormat::Rg16Float);
        let composite_pipeline = pipeline("composite", &composite_layout, format);
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen-space AO settings"),
            size: std::mem::size_of::<Settings>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let scene = texture(device, width, height, format);
        let ao = texture(
            device,
            width.div_ceil(2),
            height.div_ceil(2),
            wgpu::TextureFormat::Rg16Float,
        );
        let (ao_group, composite_group) = Self::groups(
            device,
            &uniform,
            &ao_layout,
            &composite_layout,
            depth,
            &scene,
            &ao,
        );
        Self {
            scene,
            ao,
            uniform,
            ao_layout,
            composite_layout,
            ao_group,
            composite_group,
            ao_pipeline,
            composite_pipeline,
            format,
        }
    }
    fn groups(
        device: &wgpu::Device,
        uniform: &wgpu::Buffer,
        ao_layout: &wgpu::BindGroupLayout,
        composite_layout: &wgpu::BindGroupLayout,
        depth: &wgpu::TextureView,
        scene: &wgpu::TextureView,
        ao: &wgpu::TextureView,
    ) -> (wgpu::BindGroup, wgpu::BindGroup) {
        let mut entries = vec![
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(depth),
            },
        ];
        let a = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("AO inputs"),
            layout: ao_layout,
            entries: &entries,
        });
        entries.extend([
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(scene),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(ao),
            },
        ]);
        let b = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("AO composite inputs"),
            layout: composite_layout,
            entries: &entries,
        });
        (a, b)
    }
    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        width: u32,
        height: u32,
        depth: &wgpu::TextureView,
    ) {
        self.scene = texture(device, width, height, self.format);
        self.ao = texture(
            device,
            width.div_ceil(2),
            height.div_ceil(2),
            wgpu::TextureFormat::Rg16Float,
        );
        (self.ao_group, self.composite_group) = Self::groups(
            device,
            &self.uniform,
            &self.ao_layout,
            &self.composite_layout,
            depth,
            &self.scene,
            &self.ao,
        );
    }
    pub fn draw(
        &self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        matrix: Mat4,
        eye: Vec3,
        width: u32,
        height: u32,
        enabled: bool,
        weather: [f32; 4],
        snowfall: Option<&super::snowfall::Snowfall>,
        timestamp: Option<wgpu::RenderPassTimestampWrites<'_>>,
    ) {
        let data = Settings {
            inverse: matrix.inverse().to_cols_array_2d(),
            eye_radius: [eye.x, eye.y, eye.z, 0.015],
            viewport: [width as f32, height as f32, 0., 0.],
            tuning: [0.00012, 1.8, f32::from(enabled), 0.],
            weather,
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&data));
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("half-resolution ambient occlusion"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.ao,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(
                0.,
                0.,
                width.div_ceil(2) as f32,
                height.div_ceil(2) as f32,
                0.,
                1.,
            );
            pass.set_scissor_rect(0, 0, width.div_ceil(2), height.div_ceil(2));
            pass.set_pipeline(&self.ao_pipeline);
            pass.set_bind_group(0, &self.ao_group, &[]);
            pass.draw(0..3, 0..1);
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("depth-aware AO composite"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: timestamp,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(0., 0., width as f32, height as f32, 0., 1.);
        pass.set_scissor_rect(0, 0, width, height);
        pass.set_pipeline(&self.composite_pipeline);
        pass.set_bind_group(0, &self.composite_group, &[]);
        pass.draw(0..3, 0..1);
        if let Some(snowfall) = snowfall {
            snowfall.draw(&mut pass);
        }
    }
}
#[cfg(test)]
mod tests;
