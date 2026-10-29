//! Native smoke oracle: real window reflections must match the shared sky.
pub fn validate(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layouts: [&wgpu::BindGroupLayout; 3],
    groups: [&wgpu::BindGroup; 3],
) -> Result<(), String> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("window reflection oracle"),
        source: wgpu::ShaderSource::Wgsl(source().into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("window reflection oracle"),
        bind_group_layouts: &layouts,
        push_constant_ranges: &[],
    });
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("window reflection oracle"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("probe_vertex"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("probe_fragment"),
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
        multiview: None,
        cache: None,
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("eight reflection comparisons"),
        size: wgpu::Extent3d {
            width: 18,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("reflection comparison readback"),
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("reflection oracle"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        for (index, group) in groups.into_iter().enumerate() {
            pass.set_bind_group(index as u32, group, &[]);
        }
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 18,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(Some(encoder.finish()));
    let (tx, rx) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    device.poll(wgpu::Maintain::Wait);
    rx.recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let bytes = readback.slice(..).get_mapped_range();
    let difference = bytes[..72]
        .chunks_exact(8)
        .flat_map(|pair| (0..3).map(move |channel| pair[channel].abs_diff(pair[channel + 4])))
        .max()
        .unwrap();
    let varied = (0..3).any(|channel| {
        let reference: Vec<_> = bytes[..72]
            .chunks_exact(8)
            .map(|pair| pair[channel])
            .collect();
        reference.iter().max().unwrap() - reference.iter().min().unwrap() > 1
    });
    let star_proof = bytes[67];
    drop(bytes);
    readback.unmap();
    if star_proof < 8 {
        return Err("reflection oracle failed to exercise a visible star core".into());
    }
    if difference > 1 {
        return Err(format!("window/sky reflection mismatch: {difference}/255"));
    }
    if !varied {
        return Err("reflection oracle expected a directional sky, got uniform samples".into());
    }
    eprintln!("Window reflection oracle: nine directions match sky within {difference}/255");
    Ok(())
}
fn source() -> String {
    format!(
        "{}\n{}",
        opencity::runtime::GAME_SHADER,
        include_str!("sky_probe.wgsl")
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn reflection_oracle_uses_valid_world_shader() {
        let module = naga::front::wgsl::parse_str(&super::source()).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}
