use super::*;
use crate::engine::expansion::Document;
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU adapter"]
fn camera_follow_gpu_capture() {
    let source = "road street: sidewalk carlane carlane sidewalk; end
        avenue=street(0,-1,0,0); land(-3,-3,3,-3,3,3,-3,3);";
    let directory = std::env::temp_dir().join(format!("road-follow-gpu-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("test.road");
    std::fs::write(&path, source).unwrap();
    let mut document = Document::open(&path).unwrap();
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.reserved_bottom = 42;
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("camera follow QA"),
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
    let mut painter = Painter::new(&renderer.device, renderer.config.format);
    let ctx = egui::Context::default();
    let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.));
    let mut eyes = Vec::new();
    for frame in 0..10 {
        let mut hermes =
            crate::hermes::Hermes::test_map_from_source(&std::fs::read_to_string(&path).unwrap());
        if let Some(pixels) = hermes.update_minimap([0., 0.]) {
            renderer.update_minimap(pixels);
        }
        let (world, details) = hermes.take_geometry();
        let world = renderer.upload(&world);
        let details = renderer.upload(&details);
        let (eye, look, center) = document.camera().unwrap_or((
            Vec3::new(0., -0.5, 0.18),
            Vec3::new(0., 0.15, 0.01),
            [0., 0.],
        ));
        let matrix = renderer.view_projection(eye, look);
        let endpoint = matrix * glam::Vec4::new(0., 0., 0.003, 1.);
        let endpoint = egui::pos2(
            640. + endpoint.x / endpoint.w * 640.,
            379. - endpoint.y / endpoint.w * 379.,
        );
        renderer.render(
            &target,
            &[&world, &details],
            eye,
            look,
            0.,
            hermes.minimap_view(center, 0.),
        );
        let mut events = Vec::new();
        if frame == 1 || frame == 3 {
            let point = if frame == 1 {
                endpoint
            } else {
                egui::pos2(192., 779.)
            };
            events.push(egui::Event::PointerMoved(point));
            for pressed in [true, false] {
                events.push(egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                });
            }
        }
        if frame == 5 || frame == 8 {
            for pressed in [true, false] {
                events.push(egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                });
            }
        }
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(bounds),
                events,
                ..Default::default()
            },
            |ui| document.show(ui, matrix, [1280, 758]),
        );
        painter.paint(
            &renderer,
            &target,
            ctx.tessellate(output.shapes, output.pixels_per_point),
            output.textures_delta,
            output.pixels_per_point,
        );
        if frame == 4 {
            assert_eq!(
                document.picker.selected().unwrap().kind,
                opencity::expansion::Kind::CurveRight
            );
        }
        if frame == 6 || frame == 9 {
            let (_, _, center) = document.camera().unwrap();
            eyes.push(eye);
            let next = document.picker.selected().unwrap();
            assert_eq!(next.kind, opencity::expansion::Kind::Continue);
            assert!(
                (center[0] - next.end.point.x).abs() < 0.0001
                    && (center[1] - next.end.point.y).abs() < 0.0001
            );
        }
        if let Some(directory) =
            std::env::var_os("ROAD_FRINGE_CAPTURE").filter(|_| frame == 4 || frame == 9)
        {
            let name = if frame == 4 {
                "road-follow-before.png"
            } else {
                "road-follow-after.png"
            };
            let pixels = renderer.rgba_pixels(&texture).unwrap();
            let mut encoder = png::Encoder::new(
                std::fs::File::create(std::path::Path::new(&directory).join(name)).unwrap(),
                1280,
                800,
            );
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }
    }
    assert!(eyes[0].distance(eyes[1]) > 0.5);
    document.picker.open = false;
    assert!(document.camera().is_none());
    std::fs::remove_dir_all(directory).unwrap();
}
