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
    if let Some(pixels) = hermes.update_minimap([0., 0.]) {
        renderer.update_minimap(pixels);
    }
    let mut captures = Vec::new();
    for (season, name) in [(Season::Summer, "summer"), (Season::Winter, "winter")] {
        renderer.season = season;
        renderer.daylight = Some(1.);
        for (lod, parts) in [
            Kind::Tree.parts(),
            Kind::Tree.far_parts(),
            Kind::Tree.image_parts(),
        ]
        .into_iter()
        .enumerate()
        {
            let mesh = renderer.upload(&parts[0]);
            let capture = |renderer: &Renderer, meshes: &[_]| {
                renderer
                    .report_screenshot(
                        meshes,
                        Vec3::new(0., -0.23, 0.065),
                        Vec3::new(0., 0., 0.045),
                        0.,
                        hermes.minimap_view([0., 0.], 0.),
                    )
                    .unwrap()
            };
            let png = capture(&renderer, &[&mesh]);
            if lod == 2 {
                let background = capture(&renderer, &[]);
                renderer.daylight = Some(0.);
                let night = capture(&renderer, &[&mesh]);
                assert_billboard_dims(&png, &background, &night);
            }
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

fn assert_billboard_dims(day: &[u8], background: &[u8], night: &[u8]) {
    let pixels = |png: &[u8]| {
        let mut reader = png::Decoder::new(png).read_info().unwrap();
        let mut rgba = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut rgba).unwrap();
        rgba
    };
    let (day, background, night) = (pixels(day), pixels(background), pixels(night));
    let mut day_green = 0u64;
    let mut night_green = 0u64;
    for ((day, bg), night) in day
        .chunks_exact(4)
        .zip(background.chunks_exact(4))
        .zip(night.chunks_exact(4))
    {
        // Measure the same visible tree pixels in both captures.
        if day[1].abs_diff(bg[1]) > 30 {
            day_green += u64::from(day[1]);
            night_green += u64::from(night[1]);
        }
    }
    assert!(
        day_green > 0 && night_green * 2 < day_green,
        "billboard should darken at night ({night_green} vs {day_green})"
    );
}
