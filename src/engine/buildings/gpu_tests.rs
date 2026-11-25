use super::*;
use crate::engine::{palette::Painter, render::Renderer};
#[test]
#[ignore = "requires a native GPU adapter; optional FUEL_STATION_BEFORE supplies a comparison asset"]
fn native_fuel_station_asset_views() {
    let output = std::env::var_os("FUEL_STATION_CAPTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("fuel-station-captures"));
    std::fs::create_dir_all(&output).unwrap();
    let after = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/models/fuel-station.glb");
    let mut assets = Vec::new();
    if let Some(before) = std::env::var_os("FUEL_STATION_BEFORE") {
        assets.push(("before", PathBuf::from(before)));
    }
    assets.push(("after", after));
    let mut renderer = pollster::block_on(Renderer::new(None, 1200, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    renderer.season = crate::season::Season::Summer;
    renderer.winter_weather = false;
    renderer.ambient_occlusion = false;
    let mut ground = Mesh::default();
    ground.quad(
        [
            [-1., -1., -0.001],
            [1., -1., -0.001],
            [1., 1., -0.001],
            [-1., 1., -0.001],
        ],
        [0.2; 3],
        opencity::runtime::SurfaceKind::Asphalt,
    );
    let ground = renderer.upload(&ground);
    let nav = crate::hermes::MinimapView {
        texture_region: [0., 0., 1., 1.],
        uv_center: [0.5; 2],
        uv_span: 1.,
        heading: 0.,
        player_uv: [0.5; 2],
        destination_uv: [0.5; 2],
        route_status: crate::hermes::RouteStatus::Inactive,
    };
    let mut original_bounds: Option<(Vec3, Vec3)> = None;
    for (name, path) in assets {
        let bytes = std::fs::read(path).unwrap();
        let shared = crate::hermes::external_models::decode_shared(&bytes).unwrap();
        let mut mesh = shared.as_ref().clone();
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for v in &mesh.vertices {
            lo = lo.min(v.position.into());
            hi = hi.max(v.position.into());
        }
        if let Some((old_lo, old_hi)) = original_bounds {
            let drift: Vec3 = (lo - old_lo).abs().max((hi - old_hi).abs());
            assert!(
                drift.max_element() < 0.01,
                "asset bounds moved by {drift:?} metres"
            );
        } else {
            original_bounds = Some((lo, hi));
        }
        eprintln!(
            "{name}: {} triangles, {} used / {} allocated bytes, bounds {lo:?} .. {hi:?}",
            mesh.vertices.len() / 3,
            mesh.vertices.len() * 52,
            shared.vertices.capacity() * 52
        );
        let origin = Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, lo.z);
        for v in &mut mesh.vertices {
            v.position = ((Vec3::from(v.position) - origin) / 100.).to_array();
        }
        let gpu = renderer.upload(&mesh);
        for (view, eye, look) in [
            (
                "overview",
                Vec3::new(0.38, -0.48, 0.28),
                Vec3::new(0., 0., 0.04),
            ),
            (
                "forecourt",
                Vec3::new(0.10, -0.26, 0.045),
                Vec3::new(-0.02, 0.025, 0.035),
            ),
            (
                "reverse",
                Vec3::new(-0.38, 0.46, 0.24),
                Vec3::new(0., 0., 0.04),
            ),
        ] {
            let png = renderer
                .report_screenshot(&[&ground, &gpu], eye, look, 0., nav)
                .unwrap();
            std::fs::write(output.join(format!("{name}-{view}.png")), png).unwrap();
        }
    }
}
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
    ctx.set_theme(egui::ThemePreference::Light);
    let mut painter = Painter::new(&renderer.device, renderer.config.format);
    let mut time = 0.0;
    for (mode, name) in [
        (CameraMode::Build, "building-build.png"),
        (CameraMode::Street, "building-street.png"),
        (CameraMode::Build, "building-model.png"),
        (CameraMode::Build, "building-overview.png"),
        (CameraMode::Build, "building-landmark-picker.png"),
        (CameraMode::Build, "building-basilica.png"),
    ] {
        doc.mode = mode;
        if name == "building-overview.png" {
            doc.span = 12.;
        }
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
        if name == "building-landmark-picker.png" {
            doc.span = 1.2;
            doc.landmark_picker.open = true;
        }
        if name == "building-basilica.png" {
            doc.landmark_picker.open = false;
            doc.choose_landmark(hermes.city(), "study_05").unwrap();
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
        for _ in 0..5 {
            time += 0.2;
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
                    time: Some(time),
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
        if name != "building-landmark-picker.png" {
            assert_eq!(&pixels[index..index + 3], &[9, 14, 18]);
        }
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
