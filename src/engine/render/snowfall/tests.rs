#[test]
fn world_snow_shader_validates() {
    for samples in [1, 4] {
        let module = naga::front::wgsl::parse_str(&crate::engine::render::depth_shader(
            include_str!("../snowfall.wgsl"),
            samples,
        ))
        .unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use crate::{dynamic_world::snow::Tracks, engine::render::Renderer};
    use glam::Vec3;
    use opencity::runtime::{Mesh, SurfaceKind};

    fn capture(r: &Renderer, eye: Vec3, look: Vec3, time: f32, depth: f32, name: &str) -> Vec<u8> {
        let texture = r.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("isolated snowfall QA"),
            size: wgpu::Extent3d {
                width: r.config.width,
                height: r.config.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: r.config.format.add_srgb_suffix(),
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        r.snowfall.prepare(
            &r.queue,
            r.view_projection(eye, look),
            eye,
            time,
            r.config.width,
            r.scene_height(),
            1.,
        );
        let mut encoder = r.device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("snow occluder depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &r.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(depth),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        {
            let view = texture.create_view(&Default::default());
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("isolated world snow"),
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
            pass.set_viewport(
                0.,
                0.,
                r.config.width as f32,
                r.scene_height() as f32,
                0.,
                1.,
            );
            pass.set_scissor_rect(0, 0, r.config.width, r.scene_height());
            r.snowfall.draw(&mut pass);
        }
        r.queue.submit(Some(encoder.finish()));
        let pixels = r.rgba_pixels(&texture).unwrap();
        if let Some(dir) = std::env::var_os("WINTER_CAPTURE") {
            std::fs::create_dir_all(&dir).unwrap();
            let mut png = png::Encoder::new(
                std::fs::File::create(std::path::Path::new(&dir).join(format!("{name}.png")))
                    .unwrap(),
                r.config.width,
                r.config.height,
            );
            png.set_color(png::ColorType::Rgba);
            png.set_depth(png::BitDepth::Eight);
            png.write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }
        pixels
    }
    fn bright(pixels: &[u8]) -> usize {
        pixels.chunks_exact(4).filter(|p| p[0] > 80).count()
    }
    fn changed(a: &[u8], b: &[u8]) -> usize {
        a.chunks_exact(4)
            .zip(b.chunks_exact(4))
            .filter(|(a, b)| a[0].abs_diff(b[0]) > 12)
            .count()
    }
    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn native_world_snow_parallax_cell_boundaries_occlusion_shelter_and_altitude() {
        let r = pollster::block_on(Renderer::new(None, 960, 640)).unwrap();
        let eye = Vec3::new(0., 0., 0.017);
        let look = eye + Vec3::Y;
        let first = capture(&r, eye, look, 12.5, 0., "snow-world-first");
        assert!(bright(&first) > 500, "world snowfall is too sparse");
        let offset = Vec3::new(0.04, 0., 0.);
        let moved = capture(
            &r,
            eye + offset,
            look + offset,
            12.5,
            0.,
            "snow-world-strafe",
        );
        assert!(
            changed(&first, &moved) > 500,
            "frozen flakes did not move with the camera"
        );
        let turned = capture(
            &r,
            eye,
            eye + Vec3::new(0.5, 0.866, 0.),
            12.5,
            0.,
            "snow-world-turn",
        );
        assert!(
            changed(&first, &turned) > 500,
            "turning did not change frozen snow"
        );
        assert_eq!(first, capture(&r, eye, look, 12.5, 0., "snow-world-return"));
        let tiny = Vec3::new(0.0000001, 0., 0.);
        let before = capture(&r, eye - tiny, look - tiny, 0., 0., "snow-cell-before");
        let after = capture(&r, eye + tiny, look + tiny, 0., 0., "snow-cell-after");
        assert!(
            changed(&before, &after) < 50,
            "crossing a signed cell boundary reseeded flakes"
        );
        for axis in [Vec3::Y, Vec3::Z] {
            let tiny = axis * 0.0000001;
            let before = capture(&r, -tiny, Vec3::Y - tiny, 0., 0., "snow-axis-before");
            let after = capture(&r, tiny, Vec3::Y + tiny, 0., 0., "snow-axis-after");
            assert!(
                changed(&before, &after) < 50,
                "grid rebasing popped flakes on axis {axis:?}"
            );
        }
        assert_eq!(
            bright(&capture(&r, eye, look, 12.5, 1., "snow-depth-occluded")),
            0,
            "flakes drew through scene depth"
        );
        for offset in [Vec3::new(-12.01, -8.02, 0.), Vec3::new(0., 0., 1.)] {
            assert!(
                bright(&capture(
                    &r,
                    eye + offset,
                    look + offset,
                    12.5,
                    0.,
                    "snow-world-travel"
                )) > 500,
                "snow vanished at a distant/elevated camera"
            );
        }
        let mut roof = Mesh::default();
        roof.quad(
            [
                [-0.5, -0.5, 0.5],
                [0.5, -0.5, 0.5],
                [0.5, 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
            [0.5; 3],
            SurfaceKind::Roof,
        );
        r.snow.set_geometry(&[&roof]);
        r.snow.update(&r.queue, [0., 0.], &Tracks::default());
        assert_eq!(
            bright(&capture(&r, eye, look, 12.5, 0., "snow-covered")),
            0,
            "snow appeared beneath a roof"
        );
        roof = Mesh::default();
        roof.quad(
            [
                [-0.5, -0.5, 0.5],
                [0., -0.5, 0.5],
                [0., 0.5, 0.5],
                [-0.5, 0.5, 0.5],
            ],
            [0.5; 3],
            SurfaceKind::Roof,
        );
        r.snow.set_geometry(&[&roof]);
        r.snow.update(&r.queue, [0., 0.], &Tracks::default());
        let partial = capture(&r, eye, look, 12.5, 0., "snow-canopy-edge");
        let half = |right: bool| {
            partial
                .chunks_exact(4)
                .enumerate()
                .filter(|(i, p)| {
                    let x = i % 960;
                    p[0] > 80 && if right { x > 500 } else { x < 460 }
                })
                .count()
        };
        assert_eq!(half(false), 0, "snow appeared inside the covered half");
        assert!(
            half(true) > 200,
            "a sheltered camera also hid snow outside the roof"
        );
        eprintln!("world snow QA: {} bright pixels; strafe {} changed; turn {} changed; boundary {} changed; covered/open {} / {}",
            bright(&first), changed(&first,&moved), changed(&first,&turned), changed(&before,&after), half(false), half(true));
    }

    #[test]
    #[ignore = "requires a native GPU adapter with timestamp queries"]
    fn native_world_snow_gpu_budget() {
        use crate::engine::{
            render::{depth, snowfall::Snowfall},
            snow::Snow,
        };
        use glam::Mat4;
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            eprintln!("snow GPU timing unavailable on this adapter");
            return;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .unwrap();
        let depth = depth(&device, 1280, 800, "snow timing depth", 1);
        let snow = Snow::new(&device, &queue);
        let field = Snowfall::new(
            &device,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            &depth,
            &snow,
            1,
        );
        let eye = Vec3::new(0., 0., 0.017);
        let matrix = Mat4::perspective_rh(58_f32.to_radians(), 1.6, 10., 0.003)
            * Mat4::look_at_rh(eye, eye + Vec3::Y, Vec3::Z);
        field.prepare(&queue, matrix, eye, 12.5, 1280, 800, 1.);
        let target = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("snow timing color"),
                size: wgpu::Extent3d {
                    width: 1280,
                    height: 800,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let query = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("snow timing"),
            ty: wgpu::QueryType::Timestamp,
            count: 18,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snow timing resolve"),
            size: 144,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snow timing readback"),
            size: 144,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("snow timing clear depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        // One untimed warmup, then nine batches of 64 identical open-sky draws.
        for visit in 0..10 {
            let timestamp_writes = (visit > 0).then(|| wgpu::RenderPassTimestampWrites {
                query_set: &query,
                beginning_of_pass_write_index: Some((visit - 1) * 2),
                end_of_pass_write_index: Some((visit - 1) * 2 + 1),
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("snow timing batch"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            for _ in 0..64 {
                field.draw(&mut pass);
            }
        }
        encoder.resolve_query_set(&query, 0..18, &resolve, 0);
        encoder.copy_buffer_to_buffer(&resolve, 0, &readback, 0, 144);
        queue.submit(Some(encoder.finish()));
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                tx.send(result).unwrap();
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let bytes = readback.slice(..).get_mapped_range().unwrap();
        let mut samples: Vec<_> = bytes
            .chunks_exact(16)
            .map(|pair| {
                let begin = u64::from_ne_bytes(pair[..8].try_into().unwrap());
                let end = u64::from_ne_bytes(pair[8..].try_into().unwrap());
                (end - begin) as f64 * f64::from(queue.get_timestamp_period()) / 1_000_000. / 64.
            })
            .collect();
        assert!(samples.iter().all(|v| v.is_finite() && *v > 0.));
        samples.sort_by(f64::total_cmp);
        eprintln!("world snow GPU 1280x800, 13,824 candidates, 55,296 unique vertex inputs, 9x64 draws: median {:.4} ms/draw; sorted samples {samples:?}",samples[4]);
    }
}
