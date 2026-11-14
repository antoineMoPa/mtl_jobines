//! Exercise the actual sky pipeline and its texture binding on a native GPU.
use crate::{engine::render::Renderer, hermes::Hermes};
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_cloud_highlights_day_twilight_night() {
    let mut map = Hermes::test_map_from_source("land(-1,-1,1,-1,1,1,-1,1);");
    let mut renderer = pollster::block_on(Renderer::new(None, 1000, 650)).unwrap();
    if let Some(pixels) = map.update_minimap([0., 0.]) {
        renderer.update_minimap(pixels);
    }
    let mut captures = Vec::new();
    for (name, hour) in [("day", 12.), ("twilight", 20.), ("night", 0.)] {
        renderer.sky_settings.solar_hour = Some(hour);
        let png = renderer
            .report_screenshot(
                &[],
                Vec3::new(0., 0., 0.1),
                Vec3::new(0., 1., 0.6),
                0.,
                map.minimap_view([0., 0.], 0.),
            )
            .unwrap();
        if let Some(dir) = std::env::var_os("SKY_CAPTURE_DIR") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("sky-{name}.png")),
                &png,
            )
            .unwrap();
        }
        captures.push(png);
    }
    assert_ne!(captures[0], captures[1]);
    assert_ne!(captures[1], captures[2]);
}
