//! Matched native captures for pavement and infrastructure material changes.
use super::render::Renderer;
use glam::Vec3;
use std::path::Path;

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_street_surface_capture() {
    let dir = std::env::temp_dir().join(format!("street-surfaces-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut encoder = png::Encoder::new(
        std::fs::File::create(dir.join("height.png")).unwrap(),
        1024,
        1024,
    );
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&vec![255; 1024 * 1024])
        .unwrap();
    let path = dir.join("map.road");
    std::fs::write(
        &path,
        "elevation_map(\"height.png\",-2,-2,4,4,0,0.04);
        terrain_cut(0,-0.075,-1,0.075,-1,0.075,1,-0.075,1);
        road avenue: sidewalk carlane middle carlane sidewalk; end
        main=avenue(0,-1.5,0,1.5);elevation(main,0.002);
        road bridge: sidewalk carlane carlane sidewalk; end
        crossing=bridge(-1,0.35,1,0.35);elevation(crossing,0.10);
        airport_apron=concrete(0.1,-0.7,0.5,-0.7,0.5,0.1,0.1,0.1);elevation(airport_apron,0.061);
        airport_west_runway=concrete(-1.65,-1.5,-1.0,-1.5,-1.0,1.5,-1.65,1.5);
        elevation(airport_west_runway,0.06015);",
    )
    .unwrap();
    let mut hermes = crate::hermes::Hermes::load_with_game_data(&path, None).unwrap();
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    if let Some(pixels) = hermes.update_minimap([0., 0.]) {
        renderer.update_minimap(pixels);
    }
    let geometry = hermes.take_geometry();
    let world = renderer.upload(&geometry.0);
    let details = renderer.upload(&geometry.1);
    let mut grass_cache = Default::default();
    for (name, span, eye, look) in [
        (
            "street",
            None,
            Vec3::new(0., -0.46, 0.028),
            Vec3::new(0., 0.3, 0.025),
        ),
        (
            "apron-edge",
            None,
            Vec3::new(0.08, -0.25, 0.049),
            Vec3::new(0.103, -0.20, 0.041),
        ),
        (
            "runway-threshold",
            None,
            Vec3::new(-1.675, -1.53, 0.078),
            Vec3::new(-1.46, -1.34, 0.042),
        ),
        (
            "runway-edge",
            None,
            Vec3::new(-1.658, -1.43, 0.0408),
            Vec3::new(-1.65, -1.418, 0.040075),
        ),
        (
            "runway-flyover",
            None,
            Vec3::new(-0.82, -2.6, 1.25),
            Vec3::new(-1.325, 0., 0.04),
        ),
        (
            "iso",
            Some(0.9),
            Vec3::new(-0.65, -0.65, 0.8),
            Vec3::new(0., 0.05, 0.015),
        ),
    ] {
        renderer.construction_span = span;
        renderer.prepare_grass(&mut grass_cache, hermes.city(), eye, look);
        let png = renderer
            .report_screenshot(
                &[&world, &details],
                eye,
                look,
                0.,
                hermes.minimap_view([0., 0.], 0.),
            )
            .unwrap();
        let mut reader = png::Decoder::new(png.as_slice()).read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut pixels).unwrap();
        assert!(pixels.chunks_exact(4).all(|p| p[3] == 255));
        if let Some(directory) = std::env::var_os("STREET_SURFACE_CAPTURE") {
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(
                Path::new(&directory).join(format!("pavement-{name}.png")),
                png,
            )
            .unwrap();
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

mod tiling;
