use super::*;
use crate::hermes::Hermes;
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU adapter"]
fn expansion_fringe_gpu_capture() {
    let source = "road street: sidewalk carlane carlane sidewalk; end
        avenue=street(0,-1,0,0); land(-3,-3,3,-3,3,3,-3,3); player_start(0,-0.5);";
    let mut hermes = Hermes::test_map_from_source(source);
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    if let Some(pixels) = hermes.update_minimap([0., 0.]) {
        renderer.update_minimap(pixels);
    }
    let (world, details) = hermes.take_geometry();
    let world = renderer.upload(&world);
    let details = renderer.upload(&details);
    let navigation = hermes.minimap_view([0., 0.], 0.);
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("expansion fringe QA"),
        size: wgpu::Extent3d {
            width: 1280,
            height: 800,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&Default::default());
    for scale in [1_f32, 2.] {
        let mut painter = Painter::new(&renderer.device, renderer.config.format);
        let ctx = egui::Context::default();
        let mut picker = opencity::expansion_ui::Picker::default();
        picker.start();
        let mut program = opencity::parse(source).unwrap();
        renderer.reserved_bottom = (42. * scale) as u32;
        let eye = Vec3::new(0., -0.5, 0.18);
        let look = Vec3::new(0., 0.15, 0.01);
        let matrix = renderer.view_projection(eye, look);
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.) / scale);
        let viewport = picker.viewport_rect(bounds);
        let project = |p: [f32; 3]| {
            let v = matrix * glam::Vec4::new(p[0], p[1], p[2] + 0.003, 1.);
            if v.w <= 0. {
                return None;
            }
            let p = v.truncate() / v.w;
            Some(egui::pos2(
                viewport.center().x + p.x * viewport.width() / 2.,
                viewport.center().y - p.y * viewport.height() / 2.,
            ))
        };
        let endpoint = project([0.; 3]).unwrap();
        for frame in 0..4 {
            renderer.render(&target, &[&world, &details], eye, look, 0., navigation);
            let pixels = renderer.rgba_pixels(&texture).unwrap();
            let last = ((799 * 1280 + 900) * 4) as usize;
            assert_eq!(
                &pixels[last..last + 3],
                &[0, 0, 0],
                "world/HUD drew under the strip"
            );
            let events = if frame == 1 {
                let mut events = vec![egui::Event::PointerMoved(endpoint)];
                for pressed in [true, false] {
                    events.push(egui::Event::PointerButton {
                        pos: endpoint,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    });
                }
                events
            } else {
                vec![]
            };
            let mut input = egui::RawInput {
                screen_rect: Some(bounds),
                events,
                ..Default::default()
            };
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(scale);
            let output = ctx.run_ui(input, |ui| {
                picker.show(ui, &mut program, true);
                assert_eq!(ui.available_rect_before_wrap(), viewport);
                picker.world_overlay(ui.ctx(), &program, viewport, project);
            });
            painter.paint(
                &renderer,
                &target,
                ctx.tessellate(output.shapes, output.pixels_per_point),
                output.textures_delta,
                output.pixels_per_point,
            );
        }
        assert!(
            picker.selected().is_some(),
            "world end must be pickable at scale {scale}"
        );
        let pixels = renderer.rgba_pixels(&texture).unwrap();
        let index = ((799 * 1280 + 900) * 4) as usize;
        assert_eq!(&pixels[index..index + 3], &[27, 41, 34]);
        if let Some(directory) = std::env::var_os("ROAD_FRINGE_CAPTURE") {
            std::fs::create_dir_all(&directory).unwrap();
            let path = std::path::Path::new(&directory).join(format!("road-fringe-{scale}x.png"));
            let mut encoder = png::Encoder::new(std::fs::File::create(path).unwrap(), 1280, 800);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }
    }
}
