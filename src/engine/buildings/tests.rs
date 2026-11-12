use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static TEST_ID: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str = "# Leave this authored comment and formatting untouched.\n\
road local: sidewalk carlane carlane sidewalk; end\n\
avenue = local(-3,0,3,0);\n\
ground=land(-4,-4,4,-4,4,4,-4,4);\n";
struct Fixture {
    dir: PathBuf,
    path: PathBuf,
    document: Document,
    city: City,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "building-tool-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.road");
        std::fs::write(&path, SOURCE).unwrap();
        let city = City::from_program(&opencity::parse(SOURCE).unwrap(), None).unwrap();
        let document = Document::open(&path, Vec3::ZERO, 0.).unwrap();
        Self {
            dir,
            path,
            document,
            city,
        }
    }
    fn place(&mut self) {
        let feature = self.document.prototype(
            &self.city,
            geometry::rectangle(Vec2::new(0.4, 0.3), Vec2::new(0.56, 0.5)),
        );
        self.document.create(&self.city, feature).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn view(document: &Document) -> View {
    let (eye, look, _) = document.camera().unwrap();
    View {
        matrix: glam::Mat4::orthographic_rh(-0.9, 0.9, -0.6, 0.6, 0.003, 100.)
            * glam::Mat4::look_at_rh(eye, look, Vec3::Z),
        viewport: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900., 600.)),
    }
}
#[test]
fn saves_calls_preserves_source_and_reopens_appearance_and_position() {
    let mut f = Fixture::new();
    f.place();
    let original = std::fs::read_to_string(&f.path).unwrap();
    assert!(original.starts_with(SOURCE));
    let mut feature = geometry::translated(&f.document.items[0].feature, Vec2::new(0.23, 0.1));
    feature.building_style.as_mut().unwrap().architecture.family = BuildingFamily::Civic;
    f.document.edit(&f.city, 0, feature.clone()).unwrap();
    let saved = std::fs::read_to_string(&f.path).unwrap();
    assert!(saved.starts_with(SOURCE));
    let reopened = Document::open(&f.path, Vec3::ZERO, 0.).unwrap();
    let item = &reopened.items[0];
    assert_eq!(item.feature.name.as_deref(), Some("placed_building_1"));
    assert!((center(&item.feature) - center(&feature)).length() < 1e-6);
    assert_eq!(
        item.feature.building_style.unwrap().architecture.family,
        BuildingFamily::Civic
    );
    let city = City::from_program(&reopened.program, Some(&f.dir)).unwrap();
    assert!(city.landscape.iter().any(|b| b.name == item.feature.name));
}
#[test]
fn conflicting_external_edits_are_never_overwritten() {
    let mut f = Fixture::new();
    f.place();
    let changed = format!(
        "{}# External road edit\n",
        std::fs::read_to_string(&f.path).unwrap()
    );
    std::fs::write(&f.path, &changed).unwrap();
    let feature = geometry::translated(&f.document.items[0].feature, Vec2::new(0.1, 0.));
    let error = f.document.edit(&f.city, 0, feature).unwrap_err();
    assert!(error.contains("changed on disk"));
    assert_eq!(std::fs::read_to_string(&f.path).unwrap(), changed);
    assert!(std::fs::read_dir(&f.dir).unwrap().all(|e| !e
        .unwrap()
        .path()
        .to_string_lossy()
        .contains(".tmp")));
}
#[test]
fn validates_roads_other_buildings_and_street_range_but_not_build_range() {
    let mut f = Fixture::new();
    f.place();
    let road = f.document.prototype(
        &f.city,
        geometry::rectangle(Vec2::new(0.2, -0.04), Vec2::new(0.4, 0.08)),
    );
    assert!(f
        .document
        .validate(&f.city, &road, None)
        .unwrap_err()
        .contains("sidewalk"));
    assert!(f
        .document
        .validate(&f.city, &f.document.items[0].feature, None)
        .is_err());
    let far = f.document.prototype(
        &f.city,
        geometry::rectangle(Vec2::new(2., 1.), Vec2::new(2.2, 1.2)),
    );
    assert!(f.document.validate(&f.city, &far, None).is_ok());
    f.document.mode = CameraMode::Street;
    assert!(f
        .document
        .validate(&f.city, &far, None)
        .unwrap_err()
        .contains("50 m"));
}
#[test]
fn copies_glb_once_preserves_original_and_persists_rotation_in_road() {
    let mut f = Fixture::new();
    f.place();
    let original =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/models/fuel-station.glb");
    let before = std::fs::read(&original).unwrap();
    f.document.import_model(&f.city, &original).unwrap();
    let relative = f.document.items[0].feature.model.clone().unwrap();
    assert!(relative.starts_with("models/building-"));
    assert_eq!(std::fs::read(f.dir.join(&relative)).unwrap(), before);
    f.document.import_model(&f.city, &original).unwrap();
    assert_eq!(std::fs::read_dir(f.dir.join("models")).unwrap().count(), 1);
    let rotated = geometry::rotated(&f.document.items[0].feature, 37.);
    f.document.edit(&f.city, 0, rotated).unwrap();
    let reopened = Document::open(&f.path, Vec3::ZERO, 0.).unwrap();
    let feature = &reopened.items[0].feature;
    let edge = Vec2::from_array(feature.points[1]) - Vec2::from_array(feature.points[0]);
    assert!((edge.y.atan2(edge.x).to_degrees() - 37.).abs() < 0.001);
    let city = City::from_program(&reopened.program, Some(&f.dir)).unwrap();
    let mesh = crate::hermes::external_models::build(&city, &f.dir).unwrap();
    assert!(!mesh.vertices.is_empty());
    assert_eq!(mesh.vertices.len(), reopened.items[0].mesh.vertices.len());
    assert_eq!(std::fs::read(&original).unwrap(), before);
}
#[test]
fn invalid_glb_leaves_the_map_and_asset_folder_unchanged() {
    let mut f = Fixture::new();
    f.place();
    let before = std::fs::read(&f.path).unwrap();
    let bad = f.dir.join("invalid.glb");
    std::fs::write(&bad, b"not a GLB").unwrap();
    assert!(f.document.import_model(&f.city, &bad).is_err());
    assert_eq!(std::fs::read(&f.path).unwrap(), before);
    assert!(!f.dir.join("models").exists());
}
#[test]
fn surface_pick_and_four_pixel_click_threshold_do_not_write_a_move() {
    let mut f = Fixture::new();
    f.place();
    let matrix = view(&f.document);
    let feature = &f.document.items[0].feature;
    let point = matrix
        .project(center(feature).extend(feature.elevation + feature.height.unwrap()))
        .unwrap();
    assert_eq!(f.document.pick(&f.city, matrix, point), Some(0));
    let before = std::fs::read(&f.path).unwrap();
    let ctx = egui::Context::default();
    for (pos, pressed) in [(point, true), (point + egui::vec2(1., 1.), false)] {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(matrix.viewport),
                events: vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            |_| f.document.interaction(&ctx, &f.city, matrix, &[]),
        );
        output.textures_delta.clear();
    }
    assert_eq!(f.document.selected, Some(0));
    assert_eq!(std::fs::read(&f.path).unwrap(), before);
}
#[test]
fn nearby_defaults_use_the_surrounding_architectural_family() {
    let source = format!(
        "{SOURCE} neighbor=building(0.12,{{family:heritage}},0.4,0.3,0.6,0.3,0.6,0.5,0.4,0.5);"
    );
    let city = City::from_program(&opencity::parse(&source).unwrap(), None).unwrap();
    let mut f = Fixture::new();
    for _ in 0..8 {
        let feature = f.document.prototype(
            &city,
            geometry::rectangle(Vec2::new(0.7, 0.3), Vec2::new(0.9, 0.5)),
        );
        assert_eq!(
            feature.building_style.unwrap().architecture.family,
            BuildingFamily::Heritage
        );
        assert!((feature.height.unwrap() - 0.12).abs() <= 0.032);
    }
}

fn pointer_frame(
    document: &mut Document,
    city: &City,
    ctx: &egui::Context,
    view: View,
    pos: egui::Pos2,
    pressed: Option<bool>,
) {
    let mut events = vec![egui::Event::PointerMoved(pos)];
    if let Some(pressed) = pressed {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(view.viewport),
            events,
            ..Default::default()
        },
        |_| document.interaction(ctx, city, view, &[]),
    );
    output.textures_delta.clear();
}
#[test]
fn drawing_creates_once_on_release_and_dragging_saves_in_both_views() {
    let mut f = Fixture::new();
    let ctx = egui::Context::default();
    let matrix = view(&f.document);
    let a = matrix.project(Vec3::new(0.24, 0.14, 0.)).unwrap();
    let b = matrix.project(Vec3::new(0.36, 0.3, 0.)).unwrap();
    pointer_frame(&mut f.document, &f.city, &ctx, matrix, a, Some(true));
    pointer_frame(&mut f.document, &f.city, &ctx, matrix, b, None);
    assert!(f.document.items.is_empty());
    pointer_frame(&mut f.document, &f.city, &ctx, matrix, b, Some(false));
    assert_eq!(f.document.items.len(), 1);
    f.document.selected = None;
    let feature = f.document.items[0].feature.clone();
    let press = matrix
        .project(center(&feature).extend(feature.elevation + feature.height.unwrap()))
        .unwrap();
    let target = press + egui::vec2(15., -15.);
    pointer_frame(&mut f.document, &f.city, &ctx, matrix, press, Some(true));
    pointer_frame(&mut f.document, &f.city, &ctx, matrix, target, None);
    pointer_frame(&mut f.document, &f.city, &ctx, matrix, target, Some(false));
    assert!(center(&f.document.items[0].feature).distance(center(&feature)) > 0.02);
    f.document.mode = CameraMode::Street;
    f.document.selected = None;
    let street = View {
        matrix: glam::Mat4::perspective_rh(58_f32.to_radians(), 1.5, 0.003, 10.)
            * glam::Mat4::look_at_rh(
                Vec3::new(-0.15, 0., 0.075),
                Vec3::new(0.1, 0., 0.014),
                Vec3::Z,
            ),
        viewport: matrix.viewport,
    };
    let feature = f.document.items[0].feature.clone();
    let press = street
        .project(center(&feature).extend(feature.elevation + feature.height.unwrap() * 0.5))
        .unwrap();
    assert!(
        street.viewport.contains(press),
        "Street target outside viewport: {press:?}"
    );
    assert_eq!(f.document.pick(&f.city, street, press), Some(0));
    let target = press + egui::vec2(25., 0.);
    pointer_frame(&mut f.document, &f.city, &ctx, street, press, Some(true));
    assert!(matches!(f.document.gesture, Some(Gesture::Move { .. })));
    pointer_frame(&mut f.document, &f.city, &ctx, street, target, None);
    assert!(
        f.document.draft.is_some(),
        "Street drag produced no preview"
    );
    pointer_frame(&mut f.document, &f.city, &ctx, street, target, Some(false));
    assert!(f.document.error.is_empty(), "{}", f.document.error);
    let after = center(&f.document.items[0].feature)
        .extend(feature.elevation + feature.height.unwrap() * 0.5);
    assert!(street.project(after).unwrap().x > press.x);
    let reopened = Document::open(&f.path, Vec3::ZERO, 0.).unwrap();
    assert!((center(&reopened.items[0].feature) - after.truncate()).length() < 1e-6);
}

#[test]
fn build_ground_is_not_limited_to_the_players_elevation_and_cancel_discards_a_drag() {
    let mut f = Fixture::new();
    let source = format!("{SOURCE} high=terrain(1.2,0.8,0.8,1.8,0.8,1.8,1.8,0.8,1.8);");
    let city = City::from_program(&opencity::parse(&source).unwrap(), None).unwrap();
    let feature = f.document.prototype(
        &city,
        geometry::rectangle(Vec2::new(1., 1.), Vec2::new(1.2, 1.2)),
    );
    assert!(feature.elevation > 1.1);
    f.place();
    let old = std::fs::read(&f.path).unwrap();
    f.document.draft = Some(geometry::translated(
        &f.document.items[0].feature,
        Vec2::new(0.1, 0.1),
    ));
    f.document.gesture = Some(Gesture::Move {
        index: 0,
        before: f.document.items[0].feature.clone(),
        press: egui::Pos2::ZERO,
        active: true,
    });
    f.document.cancel_interaction();
    assert!(f.document.open);
    assert!(f.document.draft.is_none() && f.document.gesture.is_none());
    assert_eq!(std::fs::read(&f.path).unwrap(), old);
}

#[test]
fn proposed_lots_do_not_rotate_with_the_players_heading() {
    let mut f = Fixture::new();
    f.document.mode = CameraMode::Street;
    let before = f.document.candidates(&f.city);
    assert!(!before.is_empty());
    f.document.heading = 1.7;
    let after = f.document.candidates(&f.city);
    assert_eq!(
        before.iter().map(|f| &f.points).collect::<Vec<_>>(),
        after.iter().map(|f| &f.points).collect::<Vec<_>>()
    );
    for feature in after {
        let front = Vec2::from_array(feature.points[1]) - Vec2::from_array(feature.points[0]);
        assert!(front.y.abs() < 1e-6);
    }
}
