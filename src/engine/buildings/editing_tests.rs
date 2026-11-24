use super::{
    tests::{pointer_frame, view, Fixture},
    *,
};

fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}

#[test]
fn typing_floor_count_preserves_footprint_and_survives_reopen() {
    let mut f = Fixture::new();
    f.place();
    let before = f.document.items[0].feature.clone();
    let ctx = egui::Context::default();
    let matrix = view(&f.document);
    let mut frame = |events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(matrix.viewport),
                events,
                ..Default::default()
            },
            |ui| f.document.show(ui, matrix.matrix, [900, 486], &f.city),
        );
        output.textures_delta.clear();
        output
    };
    frame(vec![]);
    let output = frame(vec![]);
    fn collect_text<'a>(shape: &'a egui::Shape, text: &mut Vec<&'a egui::epaint::TextShape>) {
        match shape {
            egui::Shape::Text(t) => text.push(t),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_text(shape, text);
                }
            }
            _ => {}
        }
    }
    let mut text = Vec::new();
    for shape in &output.shapes {
        collect_text(&shape.shape, &mut text);
    }
    let label = text
        .iter()
        .find(|t| t.galley.text() == "Floors")
        .unwrap()
        .visual_bounding_rect();
    let value = text
        .iter()
        .filter(|t| {
            let rect = t.visual_bounding_rect();
            rect.left() > label.right() && (rect.center().y - label.center().y).abs() < 5.
        })
        .min_by(|a, b| {
            a.visual_bounding_rect()
                .left()
                .total_cmp(&b.visual_bounding_rect().left())
        })
        .unwrap();
    let pos = value.visual_bounding_rect().center();
    assert_eq!(
        value.galley.text(),
        format!(
            "{:.0}",
            before.height.unwrap() / before.building_style.unwrap().floor_height
        )
    );
    frame(vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]);
    }
    frame(vec![]);
    assert!(
        ctx.egui_wants_keyboard_input(),
        "floor input did not gain focus at {pos:?}"
    );
    frame(vec![egui::Event::Text("7".into())]);
    frame(vec![key(egui::Key::Enter)]);
    frame(vec![]);
    drop(frame);
    assert!(f.document.error.is_empty(), "{}", f.document.error);
    let reopened = Document::open(&f.path, Vec3::ZERO, 0.).unwrap();
    let after = &reopened.items[0].feature;
    assert_eq!(after.points, before.points);
    assert_eq!(after.elevation, before.elevation);
    assert!(
        (after.height.unwrap() - 7. * before.building_style.unwrap().floor_height).abs() < 1e-6,
        "expected seven floors, got height {:?}",
        after.height
    );
}

#[test]
fn unbaked_picking_mesh_matches_render_geometry_for_all_building_families() {
    let mut f = Fixture::new();
    f.place();
    let mut feature = f.document.items[0].feature.clone();
    let mut shaded_vertices = 0;
    for family in BuildingFamily::ALL {
        feature.building_style.as_mut().unwrap().architecture.family = family;
        let Some(recipe) = BuildingRecipe::from_feature(&feature) else {
            continue;
        };
        let picking = recipe.picking_mesh();
        let rendered = recipe.mesh();
        assert_eq!(picking.vertices.len(), rendered.vertices.len());
        for (a, b) in picking.vertices.iter().zip(&rendered.vertices) {
            assert_eq!(a.position, b.position);
            assert_eq!(a.normal, b.normal);
            assert_eq!(a.surface, b.surface);
            shaded_vertices += usize::from(a.color != b.color);
        }
    }
    assert!(
        shaded_vertices > 0,
        "rendered meshes must retain their AO bake"
    );
}
fn input(
    doc: &mut Document,
    city: &City,
    ctx: &egui::Context,
    matrix: View,
    events: Vec<egui::Event>,
) {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(matrix.viewport),
            events,
            ..Default::default()
        },
        |_| doc.interaction(ctx, city, matrix, &[]),
    );
    output.textures_delta.clear();
}
#[test]
fn saved_buildings_can_be_selected_after_toolbar_focus_and_deleted() {
    let mut f = Fixture::new();
    f.place();
    f.city = City::from_program(&f.document.program, Some(&f.dir)).unwrap();
    f.document = Document::open(&f.path, Vec3::ZERO, 0.).unwrap();
    let matrix = view(&f.document);
    let feature = &f.document.items[0].feature;
    let point = matrix
        .project(center(feature).extend(feature.elevation + feature.height.unwrap()))
        .unwrap();
    let ctx = egui::Context::default();
    let mut output = ctx.run_ui(Default::default(), |ui| {
        ui.button("Build").request_focus();
    });
    output.textures_delta.clear();
    assert!(ctx.egui_wants_keyboard_input());
    pointer_frame(&mut f.document, &f.city, &ctx, matrix, point, Some(true));
    pointer_frame(&mut f.document, &f.city, &ctx, matrix, point, Some(false));
    assert_eq!(f.document.selected, Some(0));
    assert!(!ctx.egui_wants_keyboard_input());
    input(
        &mut f.document,
        &f.city,
        &ctx,
        matrix,
        vec![key(egui::Key::Backspace)],
    );
    assert!(f.document.items.is_empty(), "{}", f.document.error);
    let saved = std::fs::read_to_string(&f.path).unwrap();
    assert!(saved.starts_with("# Leave this authored comment and formatting untouched."));
    assert!(!saved.contains("placed_building_1"));
    assert!(Document::open(&f.path, Vec3::ZERO, 0.)
        .unwrap()
        .items
        .is_empty());
}
#[test]
fn delete_shortcuts_work_while_a_toolbar_button_has_focus() {
    for shortcut in [egui::Key::Backspace, egui::Key::Delete] {
        let mut f = Fixture::new();
        f.place();
        let ctx = egui::Context::default();
        let matrix = view(&f.document);
        for events in [vec![], vec![key(shortcut)]] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(matrix.viewport),
                    events,
                    ..Default::default()
                },
                |ui| {
                    ui.button("Build").request_focus();
                    assert!(ctx.egui_wants_keyboard_input());
                    f.document.interaction(&ctx, &f.city, matrix, &[]);
                },
            );
            output.textures_delta.clear();
        }
        assert!(f.document.items.is_empty(), "{}", f.document.error);
        assert!(f.document.selected.is_none());
        assert!(Document::open(&f.path, Vec3::ZERO, 0.)
            .unwrap()
            .items
            .is_empty());
    }
}

#[test]
fn backspace_during_text_entry_keeps_the_selected_building() {
    let mut f = Fixture::new();
    f.place();
    let before = std::fs::read(&f.path).unwrap();
    let matrix = view(&f.document);
    let ctx = egui::Context::default();
    let mut text = String::from("37");
    for events in [vec![], vec![key(egui::Key::Backspace)]] {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(matrix.viewport),
                events,
                ..Default::default()
            },
            |ui| {
                ui.text_edit_singleline(&mut text).request_focus();
                f.document.interaction(&ctx, &f.city, matrix, &[]);
            },
        );
        output.textures_delta.clear();
    }
    assert_eq!(f.document.items.len(), 1);
    assert_eq!(std::fs::read(&f.path).unwrap(), before);
}
#[test]
fn deletion_preserves_comments_other_buildings_and_shared_model_assets() {
    let mut f = Fixture::new();
    f.place();
    let original =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/models/fuel-station.glb");
    f.document.import_model(&f.city, &original).unwrap();
    let asset = f
        .dir
        .join(f.document.items[0].feature.model.as_ref().unwrap());
    let next = geometry::translated(&f.document.items[0].feature, Vec2::new(0.6, 0.));
    let mut next = next;
    next.name = None;
    next.model = None;
    f.document.create(&f.city, next).unwrap();
    let with_comments = std::fs::read_to_string(&f.path).unwrap().replace(
        "elevation(placed_building_1",
        "# keep before elevation\nelevation(placed_building_1",
    );
    std::fs::write(&f.path, &with_comments).unwrap();
    f.document = Document::open(&f.path, Vec3::ZERO, 0.).unwrap();
    f.document.delete(0).unwrap();
    let survivor = geometry::translated(&f.document.items[0].feature, Vec2::new(0., 0.1));
    f.document.edit(&f.city, 0, survivor.clone()).unwrap();
    let saved = std::fs::read_to_string(&f.path).unwrap();
    assert!(saved.contains("# keep before elevation"));
    assert!(!saved.contains("placed_building_1"));
    assert!(saved.contains("placed_building_2"));
    assert!(asset.is_file());
    let reopened = Document::open(&f.path, Vec3::ZERO, 0.).unwrap();
    assert_eq!(reopened.items.len(), 1);
    assert!((center(&reopened.items[0].feature) - center(&survivor)).length() < 1e-6);
}
#[test]
fn model_scale_and_elevation_save_and_reopen_with_matching_mesh_bounds() {
    let mut f = Fixture::new();
    f.place();
    let asset = Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/models/fuel-station.glb");
    f.document.import_model(&f.city, &asset).unwrap();
    let before = f.document.items[0].clone();
    let mut changed = geometry::scaled(&before.feature, 2.);
    changed.elevation -= 0.03;
    f.document.edit(&f.city, 0, changed.clone()).unwrap();
    let reopened = Document::open(&f.path, Vec3::ZERO, 0.).unwrap();
    let after = &reopened.items[0];
    assert!((after.feature.height.unwrap() - before.feature.height.unwrap() * 2.).abs() < 1e-6);
    assert!((after.feature.elevation - changed.elevation).abs() < 1e-6);
    assert!((center(&after.feature) - center(&before.feature)).length() < 1e-6);
    let before_bounds = crate::hermes::external_models::bounds(&before.mesh);
    let after_bounds = crate::hermes::external_models::bounds(&after.mesh);
    let before_size = before_bounds.1 - before_bounds.0;
    let after_size = after_bounds.1 - after_bounds.0;
    assert!((after_size - before_size * 2.).length() < 1e-5);
    assert!((after_bounds.0.z - (before_bounds.0.z - 0.03)).abs() < 1e-6);
}
#[test]
fn delete_respects_external_changes_without_mutating_the_document() {
    let mut f = Fixture::new();
    f.place();
    let changed = format!("{}# road edit\n", std::fs::read_to_string(&f.path).unwrap());
    std::fs::write(&f.path, &changed).unwrap();
    assert!(f
        .document
        .delete(0)
        .unwrap_err()
        .contains("changed on disk"));
    assert_eq!(f.document.items.len(), 1);
    assert_eq!(std::fs::read_to_string(&f.path).unwrap(), changed);
}
#[test]
fn cached_candidates_refresh_after_edits_and_map_reload() {
    let mut f = Fixture::new();
    let before = f.document.candidates(&f.city);
    assert!(!before.is_empty());
    let feature = f.document.prototype(&f.city, before[0].points.clone());
    f.document.create(&f.city, feature).unwrap();
    let after = f.document.candidates(&f.city);
    assert!(after
        .iter()
        .all(|c| !geometry::overlap(&c.points, &before[0].points)));
    let mut program = f.document.program.clone();
    let mut obstruction = after[0].clone();
    obstruction.name = Some("obstruction".into());
    program
        .statements
        .push(Statement::Call(storage::call(&obstruction).unwrap()));
    let city = City::from_program(&program, Some(&f.dir)).unwrap();
    f.document.map_reloaded();
    let reloaded = f.document.candidates(&city);
    assert!(reloaded
        .iter()
        .all(|c| !geometry::overlap(&c.points, &obstruction.points)));
}
fn iso_view(doc: &Document) -> View {
    let (eye, look, _) = doc.camera().unwrap();
    View {
        matrix: glam::Mat4::orthographic_rh(
            -doc.span * 0.75,
            doc.span * 0.75,
            -doc.span / 2.,
            doc.span / 2.,
            (doc.span * 4.).max(100.),
            0.003,
        ) * glam::Mat4::look_at_rh(eye, look, Vec3::Z),
        viewport: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900., 600.)),
    }
}
#[test]
fn iso_zoom_stays_at_the_cursor_and_middle_drag_pans_without_editing() {
    let mut f = Fixture::new();
    f.place();
    let before = std::fs::read(&f.path).unwrap();
    let ctx = egui::Context::default();
    let pos = egui::pos2(670., 200.);
    let matrix = iso_view(&f.document);
    let anchor = matrix.ground(pos, f.document.center.z).unwrap();
    input(
        &mut f.document,
        &f.city,
        &ctx,
        matrix,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0., -120.),
                modifiers: Default::default(),
            },
        ],
    );
    assert!(f.document.span > 1.2);
    let after = iso_view(&f.document)
        .ground(pos, f.document.center.z)
        .unwrap();
    assert!(
        (after - anchor).length() < 0.0001,
        "{after:?} != {anchor:?}"
    );
    let center = f.document.center;
    // Use a fresh input context to finish the wheel gesture before dragging.
    let ctx = egui::Context::default();
    for (pos, pressed) in [
        (pos, Some(true)),
        (pos + egui::vec2(80., 30.), None),
        (pos + egui::vec2(80., 30.), Some(false)),
    ] {
        let mut events = vec![egui::Event::PointerMoved(pos)];
        if let Some(pressed) = pressed {
            events.push(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Middle,
                pressed,
                modifiers: Default::default(),
            });
        }
        let matrix = iso_view(&f.document);
        input(&mut f.document, &f.city, &ctx, matrix, events);
    }
    assert!(f.document.center.distance(center) > 0.01);
    assert!(f.document.gesture.is_none());
    assert!(f.document.draft.is_none());
    assert_eq!(std::fs::read(&f.path).unwrap(), before);
    f.document.span = 64.;
    assert!(iso_view(&f.document).project(f.document.center).is_some());
}
