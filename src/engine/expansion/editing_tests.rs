use super::*;
use opencity::{map, runtime::SurfaceKind};
use std::sync::atomic::{AtomicU64, Ordering};
const SOURCE: &str = include_str!("../../../tests/fixtures/street_edit.road");
static ID: AtomicU64 = AtomicU64::new(0);

fn frame(
    document: &mut Document,
    ctx: &egui::Context,
    mut events: Vec<egui::Event>,
    shift: bool,
    enabled: bool,
) {
    events.insert(
        0,
        egui::Event::ModifiersChanged(if shift {
            egui::Modifiers::SHIFT
        } else {
            Default::default()
        }),
    );
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 700.),
            )),
            events,
            ..Default::default()
        },
        |ui| document.show_with_input(ui, glam::Mat4::IDENTITY, [1000, 658], enabled),
    );
    output.textures_delta.clear();
}
fn click() -> Vec<egui::Event> {
    let pos = egui::pos2(500., 305.);
    let mut events = vec![egui::Event::PointerMoved(pos)];
    for pressed in [true, false] {
        events.push(egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::SHIFT,
        });
    }
    events
}
fn backspace() -> Vec<egui::Event> {
    vec![egui::Event::Key {
        key: egui::Key::Backspace,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }]
}
#[test]
fn shift_click_and_backspace_remove_only_the_selected_road_and_preserve_source() {
    for external in [false, true] {
        let directory = std::env::temp_dir().join(format!(
            "street-edit-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.road");
        std::fs::write(&path, SOURCE).unwrap();
        let mut document = Document::open(&path).unwrap();
        let ctx = egui::Context::default();
        frame(&mut document, &ctx, vec![], false, true);
        frame(&mut document, &ctx, click(), true, true);
        assert!(document.selected_road.is_some());
        frame(&mut document, &ctx, backspace(), false, false);
        assert_eq!(
            document.program,
            opencity::parse(SOURCE).unwrap(),
            "dialog input is isolated"
        );
        frame(&mut document, &ctx, vec![], false, true);
        let external_source = format!("{SOURCE}\n# External edit\n");
        if external {
            std::fs::write(&path, &external_source).unwrap();
        }
        frame(&mut document, &ctx, backspace(), false, true);
        let saved = std::fs::read_to_string(&path).unwrap();
        if external {
            assert_eq!(saved, external_source);
            assert!(document.picker.message.contains("changed on disk"));
        } else {
            assert!(!saved.contains("primary = street"));
            assert!(!saved.contains("grade(primary"));
            assert!(saved.contains("# Select and remove this road."));
            assert!(saved.contains(concat!(
                "# Keep this separate road and its exact formatting.\n",
                "secondary=street(-1,0.5,1,0.5);"
            )));
            assert_eq!(map::project(&document.program).segments.len(), 1);
            assert!(document.preview.waiting());
            document.map_reloaded(SOURCE.as_bytes());
            assert!(
                document.preview.waiting(),
                "an older reload must not clear the newer surface"
            );
            let current = document.source.clone();
            document.map_reloaded(current.as_bytes());
            assert!(!document.preview.waiting());
        }
        assert!(document.picker.open);
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn plain_surface_preview_contains_current_roads_without_building_bakes() {
    let mut program = opencity::parse(SOURCE).unwrap();
    let mesh = preview::surface(&program).unwrap();
    assert!(!mesh.vertices.is_empty());
    assert!(mesh
        .vertices
        .iter()
        .all(|v| v.surface[0] == SurfaceKind::Prop as u32 as f32));
    assert!(
        mesh.vertices.iter().all(|v| v.position[2] < 0.02),
        "no facade or terrain generation"
    );
    let source = map::project(&program).segments[0].source;
    map::remove(&mut program, source);
    let mesh = preview::surface(&program).unwrap();
    assert!(
        mesh.vertices.iter().all(|v| v.position[1] > 0.35),
        "removed road is absent immediately"
    );
    let canonical = opencity::parse(include_str!("../../../data/maps/mtl/map.road")).unwrap();
    let start = std::time::Instant::now();
    let mesh = preview::surface(&canonical).unwrap();
    eprintln!(
        "Canonical plain road surface: {:?}, {} vertices",
        start.elapsed(),
        mesh.vertices.len()
    );
    assert!(!mesh.vertices.is_empty());
}

#[test]
fn source_preserving_road_deletion_updates_an_implicit_junction() {
    let directory = std::env::temp_dir().join(format!(
        "street-junction-delete-{}",
        ID.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("map.road");
    let mut program = opencity::parse(include_str!(
        "../../../opencity/tests/fixtures/expansion_junctions.road"
    ))
    .unwrap();
    let end = expansion::ends(&program)
        .into_iter()
        .find(|e| e.at_end)
        .unwrap();
    expansion::possibilities(&program, &end, 1.)
        .into_iter()
        .find(|p| p.kind == expansion::Kind::FourWay)
        .unwrap()
        .apply(&mut program)
        .unwrap();
    std::fs::write(&path, program.to_road_source()).unwrap();
    let mut doc = Document::open(&path).unwrap();
    let ctx = egui::Context::default();
    let road = doc.projection.segments.last().unwrap().source;
    doc.selected_road = Some(road);
    frame(&mut doc, &ctx, backspace(), false, true);
    let junction = map::intersections(&doc.program, &doc.projection)
        .pop()
        .unwrap();
    assert_eq!(junction.kind, map::IntersectionKind::ThreeWay);
    opencity::runtime::City::from_program(
        &opencity::parse(&std::fs::read_to_string(&path).unwrap()).unwrap(),
        None,
    )
    .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
}
