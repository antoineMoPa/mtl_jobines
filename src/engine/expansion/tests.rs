use super::*;

fn frame(document: &mut Document, ctx: &egui::Context, events: Vec<egui::Event>) {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 700.),
            )),
            events,
            ..Default::default()
        },
        |ui| document.show(ui, glam::Mat4::IDENTITY, [1000, 658]),
    );
    output.textures_delta.clear();
}

#[test]
fn fringe_accept_saves_source_and_refuses_an_external_edit() {
    let directory = std::env::temp_dir().join(format!("expansion-fringe-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("test.road");
    let source = "# Keep this comment\nroad street: carlane carlane; end street(0,0,1,0);\n";
    for external_edit in [false, true] {
        std::fs::write(&path, source).unwrap();
        let mut document = Document::open(&path).unwrap();
        let ctx = egui::Context::default();
        frame(&mut document, &ctx, vec![]);
        let p = egui::pos2(500., 329.);
        let mut events = vec![egui::Event::PointerMoved(p)];
        for pressed in [true, false] {
            events.push(egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        frame(&mut document, &ctx, events);
        assert!(document.picker.selected().is_some());
        let changed = format!("{source}# External edit\n");
        if external_edit {
            std::fs::write(&path, &changed).unwrap();
        }
        let events = [true, false]
            .map(|pressed| egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            })
            .into();
        frame(&mut document, &ctx, events);
        let saved = std::fs::read_to_string(&path).unwrap();
        if external_edit {
            assert_eq!(saved, changed);
            assert!(document.picker.message.contains("changed on disk"));
            assert_eq!(opencity::map::project(&document.program).segments.len(), 1);
            assert!(document.camera().is_none());
            assert!(document.picker.take_follow_target().is_none());
        } else {
            assert!(saved.starts_with("# Keep this comment\n"));
            assert_eq!(
                opencity::map::project(&opencity::parse(&saved).unwrap())
                    .segments
                    .len(),
                2
            );
            assert!(document.picker.message.starts_with("Saved"));
            let (eye, look, center) = document.camera().unwrap();
            assert_eq!(center, [-1., 0.]);
            assert!(eye.is_finite() && look.is_finite());
            assert!(eye.x > center[0] && look.x < center[0]);
            assert!(eye.z > look.z);
            assert!(document.picker.selected().is_some());
            let enter = [true, false]
                .map(|pressed| egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                })
                .into();
            frame(&mut document, &ctx, enter);
            let chained = std::fs::read_to_string(&path).unwrap();
            assert_eq!(
                opencity::map::project(&opencity::parse(&chained).unwrap())
                    .segments
                    .len(),
                3
            );
            assert_eq!(document.camera().unwrap().2, [-2., 0.]);
            document.picker.open = false;
            assert!(
                document.camera().is_none(),
                "closing restores the driving view"
            );
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn build_camera_frames_curves_and_grades_with_room_for_the_next_preview() {
    use opencity::expansion_ui::FollowTarget;
    for heading in [0., std::f32::consts::FRAC_PI_2, std::f32::consts::PI, -2.] {
        for height in [-0.2, 0., 0.6] {
            for length in [0.05, 1., 8.] {
                let target = FollowTarget {
                    position: [4., -3., height],
                    heading,
                    length,
                };
                let camera = BuildCamera::at(target);
                let forward = glam::Vec3::new(heading.cos(), heading.sin(), 0.);
                assert!((camera.look - camera.eye).dot(forward) > 0.);
                assert!(camera.eye.z > height);
                let matrix = glam::Mat4::perspective_rh(58_f32.to_radians(), 1.6, 0.003, 10.)
                    * glam::Mat4::look_at_rh(camera.eye, camera.look, glam::Vec3::Z);
                let p = matrix * glam::Vec3::from_array(target.position).extend(1.);
                let ndc = p.truncate() / p.w;
                assert!(p.w > 0. && ndc.x.abs() < 0.01 && ndc.y.abs() < 0.9);
                assert!(ndc.z >= 0. && ndc.z <= 1.);
            }
        }
    }
}

fn click_label(document: &mut Document, ctx: &egui::Context, label: &str) {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 700.),
            )),
            ..Default::default()
        },
        |ui| document.show(ui, glam::Mat4::IDENTITY, [1000, 658]),
    );
    let position = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == label => {
                Some(t.visual_bounding_rect().center())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing {label} button"));
    for pressed in [true, false] {
        frame(
            document,
            ctx,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}

#[test]
fn road_view_toggle_restores_street_camera_and_preserves_authored_map() {
    let directory = std::env::temp_dir().join(format!("road-view-toggle-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("test.road");
    let source = "road street: sidewalk carlane carlane sidewalk; end street(0,0,1,0);";
    std::fs::write(&path, source).unwrap();
    for growth in [false, true] {
        let mut document = Document::open(&path).unwrap();
        document.enable_views(glam::Vec3::ZERO, 0.);
        if growth {
            document.picker.start_growth();
        }
        let ctx = egui::Context::default();
        frame(&mut document, &ctx, vec![]);
        let iso = document.camera().unwrap();
        assert_eq!(document.orthographic_span(), Some(1.2));
        click_label(&mut document, &ctx, "Street");
        assert!(document.camera().is_none());
        assert!(document.orthographic_span().is_none());
        click_label(&mut document, &ctx, "Iso");
        assert_eq!(document.camera().unwrap(), iso);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        document.picker.open = false;
        assert!(document.camera().is_none() && document.orthographic_span().is_none());
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn iso_follows_the_selected_end_and_report_input_cannot_accept_or_close_it() {
    let directory = std::env::temp_dir().join(format!("road-iso-follow-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("test.road");
    let source = "road street: sidewalk carlane carlane sidewalk; end street(0,0,1,0);";
    std::fs::write(&path, source).unwrap();
    let mut document = Document::open(&path).unwrap();
    document.enable_views(glam::Vec3::ZERO, 0.);
    let ctx = egui::Context::default();
    frame(&mut document, &ctx, vec![]);
    let pos = egui::pos2(500., 329.);
    let mut events = vec![egui::Event::PointerMoved(pos)];
    for pressed in [true, false] {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        });
    }
    frame(&mut document, &ctx, events);
    assert!(document.picker.selected().is_some());
    let before = document.camera().unwrap();
    let selected = document.picker.selected().unwrap();
    let ghost = selected.ghosts.last().unwrap();
    let expected = [
        (selected.end.point.x + ghost.end.x) / 2.,
        (selected.end.point.y + ghost.end.y) / 2.,
    ];
    assert_eq!(before.2, expected, "Iso did not frame the chosen extension");
    let mut output = ctx.run_ui(
        egui::RawInput {
            events: [egui::Key::Enter, egui::Key::Escape]
                .map(|key| egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                })
                .into(),
            ..Default::default()
        },
        |ui| document.show_with_input(ui, glam::Mat4::IDENTITY, [1000, 658], false),
    );
    output.textures_delta.clear();
    assert!(document.picker.open);
    assert_eq!(document.camera().unwrap(), before);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    frame(
        &mut document,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    frame(
        &mut document,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    let after = document.camera().unwrap().2;
    assert!(glam::Vec2::from_array(after).distance(glam::Vec2::from_array(before.2)) > 0.5);
    assert_eq!(opencity::map::project(&document.program).segments.len(), 2);
    std::fs::remove_dir_all(directory).unwrap();
}
