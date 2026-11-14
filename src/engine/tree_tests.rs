//! Native material and seasonal replacement smoke test.
use super::render::Renderer;
use crate::{hermes::Hermes, season::Season};
use glam::Vec3;
use opencity::props::Kind;

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_replacement_tree_seasons_and_lods() {
    let mut hermes = Hermes::test_map_from_source("land(-1,-1,1,-1,1,1,-1,1);");
    let mut renderer = pollster::block_on(Renderer::new(None, 900, 900)).unwrap();
    renderer.daylight = Some(1.);
    if let Some(pixels) = hermes.update_minimap([0., 0.]) {
        renderer.update_minimap(pixels);
    }
    let mut captures = Vec::new();
    for (season, name) in [(Season::Summer, "summer"), (Season::Winter, "winter")] {
        renderer.season = season;
        for (lod, parts) in [
            Kind::Tree.parts(),
            Kind::Tree.far_parts(),
            Kind::Tree.image_parts(),
        ]
        .into_iter()
        .enumerate()
        {
            let mesh = renderer.upload(&parts[0]);
            let png = renderer
                .report_screenshot(
                    &[&mesh],
                    Vec3::new(0., -0.23, 0.065),
                    Vec3::new(0., 0., 0.045),
                    0.,
                    hermes.minimap_view([0., 0.], 0.),
                )
                .unwrap();
            if let Some(dir) = std::env::var_os("TREE_CAPTURE_DIR") {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(
                    std::path::Path::new(&dir).join(format!("{name}-lod{lod}.png")),
                    &png,
                )
                .unwrap();
            }
            captures.push(png);
        }
    }
    assert_ne!(
        captures[0], captures[3],
        "winter must change tree rendering"
    );
    assert_ne!(
        captures[2], captures[5],
        "billboard must use its seasonal bake"
    );
}
