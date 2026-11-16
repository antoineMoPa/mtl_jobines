//! Optional GPU preview against any loaded map; no window or audio device required.
use super::*;
use crate::engine::fabula::{self, Anchor, Playback};

#[test]
#[ignore = "requires a native GPU adapter"]
fn fabula_opening_preview() {
    let mut hermes = if let Some(path) = std::env::var_os("FABULA_MAP") {
        Hermes::load_with_game_data(std::path::Path::new(&path), None).unwrap()
    } else {
        Hermes::test_map()
    };
    let mut world = DynamicWorld::new(&hermes);
    if world.is_driving() {
        assert!(world.toggle_vehicle(&hermes));
    }
    let original = world.player_position();
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 720)).unwrap();
    renderer.cinematic = true;
    renderer.daylight = Some(1.);
    let scenery = Scenery::new(&renderer, &mut hermes);
    let mut actors = VehicleScenery::new(&renderer);
    let mut props = PropScenery::default();
    let ctx = egui::Context::default();
    let mut painter = super::super::palette::Painter::new(
        &renderer.device,
        renderer.config.format.add_srgb_suffix(),
    );
    let mut playback = Playback {
        scene: fabula::scene_for(fabula::Cue::GameIntro).unwrap(),
        anchor: Anchor {
            position: original,
            yaw: world.player_yaw(),
        },
        elapsed: 0.,
        presented: true,
    };
    for (name, time) in [("opening-wide", 1.2), ("opening-character", 3.4)] {
        playback.elapsed = time;
        actors.update_with_stage(&renderer, &world, Mat4::IDENTITY, Some(playback.stage()));
        props.update_with_stage(&renderer, &world, Some(playback.stage()));
        let center = (original / METERS_PER_UNIT).truncate().to_array();
        let mut meshes = scenery.visible(center);
        meshes.extend(actors.visible());
        meshes.extend(props.visible());
        let (eye, look) = playback.camera();
        let eye = world.camera_eye(&hermes, look, eye);
        // A second UI frame resolves Area anchoring before capture.
        let mut textures = egui::TexturesDelta::default();
        let mut output = None;
        for frame_index in 0..2 {
            let mut frame = ctx.run_ui(
                egui::RawInput {
                    time: Some(time as f64 + frame_index as f64 * 0.3),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 720.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    fabula::show(ui.ctx(), &playback, false);
                },
            );
            textures.append(std::mem::take(&mut frame.textures_delta));
            output = Some(frame);
        }
        let output = output.unwrap();
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let png = renderer
            .report_screenshot_with_overlay(
                &meshes,
                eye,
                look,
                0.,
                hermes.minimap_view(center, world.player_yaw()),
                |target| {
                    painter.paint(
                        &renderer,
                        target,
                        primitives,
                        textures,
                        output.pixels_per_point,
                    );
                },
            )
            .unwrap();
        if let Some(folder) = std::env::var_os("FABULA_CAPTURE") {
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                std::path::Path::new(&folder).join(format!("{name}.png")),
                png,
            )
            .unwrap();
        }
        assert_eq!(world.player_position(), original);
    }
}
