use super::*;
use crate::engine::{palette::Painter, render::Renderer};
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_building_tool_captures_both_cameras() {
    let dir = std::env::temp_dir().join(format!("building-gpu-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("map.road");
    let source = "road street: sidewalk carlane carlane sidewalk; end
        avenue=street(-3,0,3,0);ground=land(-4,-3,4,-3,4,3,-4,3);
        placed_building_1=building(0.15,{family:heritage},0.24,0.14,0.4,0.14,0.4,0.34,0.24,0.34);
        elevation(placed_building_1,-0.019);
        neighbor=building(0.3,{family:mixed},0.56,0.15,0.76,0.15,0.76,0.38,0.56,0.38);
        opposite=building(0.5,{family:podium},0.2,-0.42,0.45,-0.42,0.45,-0.16,0.2,-0.16);";
    std::fs::write(&path, source).unwrap();
    let mut hermes = crate::hermes::Hermes::test_map_from_source(source);
    let mut doc = Document::open(&path, Vec3::ZERO, 0.).unwrap();
    doc.selected = Some(0);
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.reserved_bottom = 42;
    if let Some(pixels) = hermes.update_minimap([0., 0.]) {
        renderer.update_minimap(pixels);
    }
    let (world, details) = hermes.take_geometry();
    let mut world = renderer.upload(&world);
    let mut details = renderer.upload(&details);
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native building tool QA"),
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
    let ctx = egui::Context::default();
    let mut painter = Painter::new(&renderer.device, renderer.config.format);
    for (mode, name) in [
        (CameraMode::Build, "building-build.png"),
        (CameraMode::Street, "building-street.png"),
        (CameraMode::Build, "building-model.png"),
    ] {
        doc.mode = mode;
        if name == "building-model.png" {
            let model =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/models/fuel-station.glb");
            doc.import_model(hermes.city(), &model).unwrap();
            let feature = geometry::rotated(&doc.items[0].feature, 37.);
            doc.edit(hermes.city(), 0, feature).unwrap();
            hermes = crate::hermes::Hermes::load_with_game_data(&path, None).unwrap();
            let geometry = hermes.take_geometry();
            world = renderer.upload(&geometry.0);
            details = renderer.upload(&geometry.1);
        }
        renderer.construction_span = doc.orthographic_span();
        let (eye, look, _) = doc.camera().unwrap_or((
            Vec3::new(-0.15, 0., 0.075),
            Vec3::new(0.1, 0., 0.014),
            [0., 0.],
        ));
        let matrix = renderer.view_projection(eye, look);
        for _ in 0..3 {
            renderer.render(
                &target,
                &[&world, &details],
                eye,
                look,
                0.,
                hermes.minimap_view([0., 0.], 0.),
            );
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 800.),
                    )),
                    ..Default::default()
                },
                |ui| doc.show(ui, matrix, [1280, 758], hermes.city()),
            );
            painter.paint(
                &renderer,
                &target,
                ctx.tessellate(output.shapes, output.pixels_per_point),
                output.textures_delta,
                output.pixels_per_point,
            );
        }
        let pixels = renderer.rgba_pixels(&texture).unwrap();
        let index = ((799 * 1280 + 1100) * 4) as usize;
        assert_eq!(&pixels[index..index + 3], &[9, 14, 18]);
        if let Some(directory) = std::env::var_os("BUILDING_TOOL_CAPTURE") {
            std::fs::create_dir_all(&directory).unwrap();
            let mut encoder = png::Encoder::new(
                std::fs::File::create(Path::new(&directory).join(name)).unwrap(),
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
    std::fs::remove_dir_all(dir).unwrap();
}
