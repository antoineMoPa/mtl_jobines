use super::*;
use crate::{dynamic_world::DynamicWorld, hermes::Hermes};
use glam::Vec3;
#[test]
fn palette_exposes_each_prop_kind_without_changing_other_actions() {
    let choices = commands(&[]);
    for kind in crate::props::Kind::ALL {
        let id = format!("prop.place.{}", kind.keyword());
        assert!(choices.iter().any(|c| c.id == id));
        assert_eq!(
            PaletteAction::from_id(&id),
            Some(PaletteAction::PlaceProps(kind))
        );
    }
    assert!(PaletteAction::from_id("prop.place.unknown").is_none());
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_props_game_flow_renders_shared_models_and_the_game_toolbar() {
    let dir = std::env::temp_dir().join(format!("props-native-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("map.road");
    let source = include_str!("../../../tests/fixtures/props.road");
    std::fs::write(&path, source).unwrap();
    let mut hermes = Hermes::test_map_from_source(source);
    let world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    let scenery = super::super::scenery::Scenery::new(&renderer, &mut hermes);
    let mut props = super::super::prop_scenery::PropScenery::default();
    props.update(&renderer, &world);
    assert!(world.authored_prop_instances().len() > 5);
    let mut vehicles = super::super::vehicle_scenery::VehicleScenery::new(&renderer);
    vehicles.update(&renderer, &world);
    let mut moving = opencity::runtime::Mesh::default();
    world.write_moving_mesh(&mut moving);
    let dynamic = renderer.upload_dynamic(&moving);
    let mut palette = Palette::headless(&renderer);
    palette.props = Some(
        super::super::props::Document::open(
            &path,
            world.player_position() / crate::dynamic_world::METERS_PER_UNIT,
            world.player_yaw(),
        )
        .unwrap(),
    );
    let sound = &mut crate::audio::Settings::default();
    for frame in 0..4 {
        renderer.reserved_bottom = palette.reserved_bottom(1280, 800);
        renderer.construction_span = palette.orthographic_span();
        let (eye, look, center) = palette.build_camera().unwrap();
        let matrix = renderer.view_projection(eye, look);
        let ctx = palette.ctx.clone();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280., 800.),
                )),
                ..Default::default()
            },
            |ui| {
                palette.show_panels(ui, &mut renderer, sound, matrix, hermes.city());
            },
        );
        let text: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    Some(t.galley.text().to_owned())
                } else {
                    None
                }
            })
            .collect();
        for label in ["Street", "Iso", "Select", "Single", "Row", "Done"] {
            assert!(text.iter().any(|t| t == label), "missing {label}");
        }
        assert!(!text
            .iter()
            .any(|s| s.contains("Drag props") || s.contains("double-click")));
        let preview = palette
            .props
            .as_mut()
            .unwrap()
            .preview(&renderer, hermes.city())
            .unwrap();
        let mut meshes = scenery.visible(center);
        meshes.push(&dynamic);
        meshes.extend(props.visible_with_authored(false));
        meshes.extend(vehicles.visible());
        meshes.extend(preview.iter().map(|m| m.as_ref()));
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        let delta = std::mem::take(&mut output.textures_delta);
        let pixels = renderer
            .report_screenshot_with_overlay(
                &meshes,
                eye,
                look,
                0.,
                hermes.minimap_view(center, 0.),
                |target| {
                    palette.painter.paint(
                        &renderer,
                        target,
                        primitives.clone(),
                        delta,
                        output.pixels_per_point,
                    )
                },
            )
            .unwrap();
        if frame == 3 {
            let capture = std::env::var_os("PROPS_CAPTURE")
                .map(std::path::PathBuf::from)
                .unwrap_or(dir.clone());
            std::fs::create_dir_all(&capture).unwrap();
            std::fs::write(capture.join("game-props-iso.png"), pixels).unwrap();
        }
    }
    renderer.reserved_bottom = 0;
    renderer.construction_span = None;
    let mut meshes = scenery.visible([0., 0.]);
    meshes.push(&dynamic);
    meshes.extend(props.visible());
    meshes.extend(vehicles.visible());
    let pixels = renderer
        .report_screenshot(
            &meshes,
            Vec3::new(-0.045, -0.045, 0.025),
            Vec3::new(0., 0., 0.004),
            0.,
            hermes.minimap_view([0., 0.], 0.),
        )
        .unwrap();
    if let Some(dir) = std::env::var_os("PROPS_CAPTURE") {
        std::fs::write(
            std::path::Path::new(&dir).join("game-cone-close.png"),
            pixels,
        )
        .unwrap();
    }
    std::fs::remove_dir_all(dir).unwrap();
}
