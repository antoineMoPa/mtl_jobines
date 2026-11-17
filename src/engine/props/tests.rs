use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static ID: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    hermes: crate::hermes::Hermes,
}
impl Fixture {
    fn new(source: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "props-edit-{}-{}.road",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, source).unwrap();
        Self {
            path,
            hermes: crate::hermes::Hermes::test_map_from_source(source),
        }
    }
    fn doc(&self) -> Document {
        Document::open(&self.path, Vec3::ZERO, 0.).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
const SOURCE: &str = include_str!("../../../tests/fixtures/props.road");
#[test]
fn road_sign_configuration_survives_edit_duplicate_undo_and_reopen() {
    let f = Fixture::new(include_str!("../../../tests/fixtures/road_signs.road"));
    let mut d = f.doc();
    d.rebuild(f.hermes.city());
    let index = d
        .entries
        .iter()
        .position(|e| e.kind() == Kind::HighwaySign)
        .unwrap();
    let before = d.entries.clone();
    if let Some(opencity::props::SignDesign::Highway {
        destination, exit, ..
    }) = d.entries[index].sign_mut()
    {
        *destination = "Trois-Rivières".into();
        *exit = "22".into();
    } else {
        panic!("highway design");
    }
    d.accept(f.hermes.city(), before.clone());
    assert!(d.error.is_empty(), "{}", d.error);
    assert_eq!(f.doc().entries, d.entries);
    d.selected = vec![Member {
        entry: index,
        index: 0,
    }];
    d.duplicate(f.hermes.city());
    assert_eq!(d.entries.len(), before.len() + 1);
    assert_eq!(
        d.entries.last().unwrap().clone().sign_mut(),
        d.entries[index].clone().sign_mut()
    );
    d.history(f.hermes.city(), false);
    d.history(f.hermes.city(), false);
    assert_eq!(d.entries, before);
    assert_eq!(f.doc().entries, before);
}
#[test]
fn edits_save_reload_undo_and_preserve_non_prop_source() {
    let f = Fixture::new(SOURCE);
    let mut d = f.doc();
    d.rebuild(f.hermes.city());
    let original = d.entries.clone();
    let before = d.entries.clone();
    d.entries[0].translate(Vec2::new(0.01, 0.012));
    d.accept(f.hermes.city(), before);
    assert!(d.error.is_empty(), "{}", d.error);
    assert_eq!(f.doc().entries, d.entries);
    let saved = std::fs::read_to_string(&f.path).unwrap();
    assert!(
        saved.contains("road street: sidewalk carlane carlane sidewalk; end\nstreet(-1,0,1,0);")
    );
    d.history(f.hermes.city(), false);
    assert_eq!(d.entries, original);
    d.history(f.hermes.city(), true);
    assert_ne!(d.entries, original);
    d.selected = vec![Member { entry: 0, index: 0 }];
    d.delete(f.hermes.city());
    assert_eq!(d.entries.len(), original.len() - 1);
    assert_eq!(f.doc().entries.len(), d.entries.len());
    d.history(f.hermes.city(), false);
    assert_eq!(d.entries.len(), original.len());
}
#[test]
fn group_and_individual_row_edits_survive_reopen() {
    let f = Fixture::new(SOURCE);
    let mut d = f.doc();
    d.rebuild(f.hermes.city());
    let row = d
        .entries
        .iter()
        .position(|e| matches!(e.shape, Shape::Row { .. }))
        .unwrap();
    let members: Vec<_> = d
        .samples
        .iter()
        .filter(|s| s.member.entry == row)
        .take(2)
        .map(|s| s.member)
        .collect();
    let before = d.entries.clone();
    d.move_members(f.hermes.city(), &members, Vec2::new(0.003, 0.007));
    d.accept(f.hermes.city(), before);
    assert!(d.error.is_empty(), "{}", d.error);
    assert_eq!(f.doc().entries, d.entries);
    let before = d.entries.clone();
    if let Shape::Row { row, .. } = &mut d.entries[row].shape {
        row.control += Vec2::new(0.014, -0.023);
        row.a += Vec2::new(-0.008, 0.011);
    }
    d.accept(f.hermes.city(), before);
    assert_eq!(f.doc().entries, d.entries);
}
#[test]
fn conflicting_external_edit_is_never_overwritten() {
    let f = Fixture::new(SOURCE);
    let mut d = f.doc();
    let foreign = format!("{SOURCE}\n\n");
    std::fs::write(&f.path, &foreign).unwrap();
    let before = d.entries.clone();
    d.entries[0].translate(Vec2::X * 0.01);
    d.accept(f.hermes.city(), before.clone());
    assert_eq!(d.entries, before);
    assert!(d.error.contains("changed on disk"));
    assert_eq!(std::fs::read_to_string(&f.path).unwrap(), foreign);
    d.map_reloaded();
    assert_eq!(d.source, foreign);
}
fn frame(
    d: &mut Document,
    ctx: &egui::Context,
    city: &City,
    events: Vec<egui::Event>,
    enabled: bool,
) {
    let matrix = glam::Mat4::orthographic_rh(-0.25, 0.25, -0.16, 0.16, 0.01, 5.)
        * glam::Mat4::look_at_rh(Vec3::Z, Vec3::ZERO, Vec3::Y);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 800.),
            )),
            events,
            ..Default::default()
        },
        |ui| d.show_with_input(ui, matrix, [1280, 758], city, enabled),
    );
    output.textures_delta.clear();
}

fn pointer(p: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(p),
        egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        },
    ]
}
#[test]
fn pointer_row_creation_is_free_angle_and_stationary_frames_do_not_rebuild() {
    let f = Fixture::new("road r: carlane carlane; end r(-1,0,1,0); land(-2,-2,2,-2,2,2,-2,2);");
    let mut d = f.doc();
    d.tool = Tool::Row;
    let ctx = egui::Context::default();
    frame(&mut d, &ctx, f.hermes.city(), vec![], true);
    let a = egui::pos2(490., 330.);
    let b = egui::pos2(710., 395.);
    frame(&mut d, &ctx, f.hermes.city(), pointer(a, true), true);
    frame(
        &mut d,
        &ctx,
        f.hermes.city(),
        vec![egui::Event::PointerMoved(b)],
        true,
    );
    frame(&mut d, &ctx, f.hermes.city(), pointer(b, false), true);
    assert!(d.error.is_empty(), "{}", d.error);
    assert_eq!(d.entries.len(), 1);
    let Shape::Row { row, .. } = &d.entries[0].shape else {
        panic!()
    };
    assert!((row.a.x - row.b.x).abs() > 0.01 && (row.a.y - row.b.y).abs() > 0.01);
    let revision = d.revision;
    for _ in 0..10 {
        frame(&mut d, &ctx, f.hermes.city(), vec![], true);
    }
    assert_eq!(d.revision, revision);
}
#[test]
fn disabled_overlay_cannot_place_or_move_props() {
    let f = Fixture::new(SOURCE);
    let mut d = f.doc();
    let ctx = egui::Context::default();
    let original = std::fs::read(&f.path).unwrap();
    frame(
        &mut d,
        &ctx,
        f.hermes.city(),
        pointer(egui::pos2(750., 350.), true),
        false,
    );
    frame(
        &mut d,
        &ctx,
        f.hermes.city(),
        pointer(egui::pos2(800., 400.), false),
        false,
    );
    assert_eq!(std::fs::read(&f.path).unwrap(), original);
}
