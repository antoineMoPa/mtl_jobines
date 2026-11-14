//! Read back production pavement sampling: tile repetition and both lattice seams.
use super::*;

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_pavement_sampling_breaks_repetition_and_keeps_seams_continuous() {
    let r = pollster::block_on(Renderer::new(None, 1024, 512)).unwrap();
    let textures = opencity::runtime::surface_materials::SurfaceTextures::new(&r.device, &r.queue);
    let layout = r
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pavement sampling regression"),
            entries: &opencity::runtime::surface_materials::SurfaceTextures::layout(0),
        });
    let group = r.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("pavement sampling regression"),
        layout: &layout,
        entries: &textures.entries(0),
    });
    let source = format!(
        "{}\n{}",
        include_str!("../../../opencity/src/runtime/material_lighting.wgsl"),
        r#"
@group(0) @binding(0) var pavement_color:texture_2d_array<f32>;
@group(0) @binding(1) var pavement_detail:texture_2d_array<f32>;
@group(0) @binding(2) var pavement_sampler:sampler;
@vertex fn probe_vs(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32> {
    return vec4<f32>(vec2<f32>(f32((i<<1u)&2u),f32(i&2u))*2.0-1.0,0.0,1.0);
}
@fragment fn probe_fs(@builtin(position) pixel:vec4<f32>)->@location(0) vec4<f32> {
    let px=u32(pixel.x);
    if pixel.y>=510.0 {
        let layer=i32(px&1u);let meters=select(3.0,2.0,layer==1);
        let cell=vec2<f32>(f32(px/16u)-32.0,f32((px/7u)%17u)-8.0);
        let t=(f32(px%16u)+0.5)/16.0;
        let epsilon=select(-0.00001,0.00001,pixel.y>=511.0);
        var grid=cell+vec2<f32>(t,1.0-t+epsilon);
        if (px&2u)!=0u {grid=cell+vec2<f32>(epsilon,t);}
        let u=grid.x/0.65;let v=(grid.y/(0.65*0.577350269)+u)*0.5;
        let material=scanned_pavement(vec3<f32>(u*meters,-v*meters,0.0),vec3<f32>(0.005,0.0,0.0),vec3<f32>(0.0,0.005,0.0),vec3<f32>(0.0,0.0,1.0),layer);
        var color=material.color;
        if (px&4u)!=0u {color=material.normal*0.5+0.5;}
        if (px&8u)!=0u {color=vec3<f32>(material.roughness);}
        return vec4<f32>(color,1.0);
    }
    let layer=i32(u32(pixel.y)/256u);let meters=select(3.0,2.0,layer==1);
    let point=vec2<f32>(pixel.x-floor(pixel.x/512.0)*512.0,pixel.y-floor(pixel.y/256.0)*256.0);
    let world=vec3<f32>(point*meters/32.0,0.0);
    let dx=vec3<f32>(meters/32.0,0.0,0.0);let dy=vec3<f32>(0.0,meters/32.0,0.0);
    if pixel.x<512.0 {
        let uv=vec2<f32>(world.x,-world.y)/meters;
        return vec4<f32>(pavement_scan_color(textureSampleGrad(pavement_color,pavement_sampler,uv,layer,vec2<f32>(1.0/32.0,0.0),vec2<f32>(0.0,-1.0/32.0)).rgb,layer),1.0);
    }
    return vec4<f32>(scanned_pavement(world,dx,dy,vec3<f32>(0.0,0.0,1.0),layer).color,1.0);
}
"#
    );
    let shader = r.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production pavement sampling probe"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = r
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
    let format = r.config.format.add_srgb_suffix();
    let pipeline = r
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("pavement repeat and seam probe"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("probe_vs"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("probe_fs"),
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
        });
    let texture = r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pavement regression readback"),
        size: wgpu::Extent3d {
            width: 1024,
            height: 512,
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
    let mut encoder = r.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("pavement regression"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    r.queue.submit([encoder.finish()]);
    let pixels = r.rgba_pixels(&texture).unwrap();
    for (name, first_row) in [("asphalt", 0), ("concrete", 256)] {
        let error = |left: usize| {
            let mut total = 0u64;
            let mut count = 0;
            for y in first_row..first_row + 252 {
                for x in 0..480 {
                    for c in 0..3 {
                        total += pixels[(y * 1024 + left + x) * 4 + c]
                            .abs_diff(pixels[(y * 1024 + left + x + 32) * 4 + c])
                            as u64;
                        count += 1;
                    }
                }
            }
            total as f64 / count as f64
        };
        let tiled = error(0);
        let blended = error(512);
        eprintln!(
            "{name}: one-tile-shift difference, repeat={tiled:.3}, stochastic={blended:.3} /255"
        );
        assert!(
            tiled < 0.2,
            "probe must reproduce a truly periodic baseline"
        );
        assert!(
            blended > tiled + 1.0,
            "material still repeats at its source tile period"
        );
    }
    let seam = (0..1024 * 4)
        .filter(|i| i % 4 != 3)
        .map(|i| pixels[510 * 1024 * 4 + i].abs_diff(pixels[511 * 1024 * 4 + i]))
        .max()
        .unwrap();
    eprintln!("maximum edge-pair color/normal/roughness difference: {seam}/255");
    assert!(seam <= 8, "stochastic patch edge is discontinuous");
    if let Some(directory) = std::env::var_os("PAVEMENT_TILING_CAPTURE") {
        std::fs::create_dir_all(&directory).unwrap();
        let mut png = png::Encoder::new(
            std::fs::File::create(Path::new(&directory).join("pavement-tiling.png")).unwrap(),
            1024,
            512,
        );
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
}
