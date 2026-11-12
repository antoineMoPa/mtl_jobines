use super::*;
fn screenshot() -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut e = png::Encoder::new(&mut bytes, 64, 48);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.write_header()
            .unwrap()
            .write_image_data(&[80, 90, 100, 255].repeat(64 * 48))
            .unwrap();
    }
    bytes
}
fn red(a: &Annotation) -> usize {
    a.pixels
        .chunks_exact(4)
        .filter(|p| p[0] > 220 && p[1] < 50)
        .count()
}
#[test]
fn strokes_survive_png_encoding_without_touching_the_capture_or_clear_pixels() {
    let original = screenshot();
    let mut a = Annotation::from_png(&original).unwrap();
    assert_eq!(a.screenshot(&original).unwrap(), original);
    a.stroke(egui::pos2(10., 10.), egui::pos2(40., 30.), 2.);
    assert!(red(&a) > 50);
    let saved = a.screenshot(&original).unwrap();
    let b = Annotation::from_png(&saved).unwrap();
    assert_eq!(a.pixels, b.pixels);
    assert_eq!(b.size, [64, 48]);
    assert_eq!(&b.pixels[..4], &[80, 90, 100, 255]);
    assert_eq!(red(&Annotation::from_png(&original).unwrap()), 0);
    a.stroke(egui::pos2(-100., -100.), egui::pos2(-10., -10.), 2.);
    a.stroke(egui::pos2(60., 40.), egui::pos2(300., 300.), 2.);
    assert_eq!(a.pixels.len(), 64 * 48 * 4);
}
fn frame(a: &mut Annotation, ctx: &egui::Context, pos: Pos2, pressed: Option<bool>, window: bool) {
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
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(128., 96.))),
            events,
            ..Default::default()
        },
        |_| {
            a.show(ctx);
            if window {
                egui::Window::new("test dialog")
                    .fixed_pos(egui::pos2(60., 10.))
                    .fixed_size(egui::vec2(55., 60.))
                    .show(ctx, |ui| {
                        ui.button("Save");
                    });
            }
        },
    );
    output.textures_delta.clear();
}
#[test]
fn pointer_events_draw_at_image_scale_and_release_does_not_join_separate_strokes() {
    let mut a = Annotation::from_png(&screenshot()).unwrap();
    let ctx = egui::Context::default();
    for _ in 0..3 {
        frame(&mut a, &ctx, egui::pos2(20., 20.), None, false);
    }
    frame(&mut a, &ctx, egui::pos2(20., 20.), Some(true), false);
    frame(&mut a, &ctx, egui::pos2(50., 40.), None, false);
    frame(&mut a, &ctx, egui::pos2(50., 40.), Some(false), false);
    assert!(red(&a) > 20);
    let original = a.pixels.clone();
    frame(&mut a, &ctx, egui::pos2(100., 80.), None, false);
    assert_eq!(a.pixels, original);
    frame(&mut a, &ctx, egui::pos2(100., 80.), Some(true), false);
    frame(&mut a, &ctx, egui::pos2(100., 80.), Some(false), false);
    let middle = (30 * 64 + 36) * 4;
    assert_eq!(&a.pixels[middle..middle + 4], &[80, 90, 100, 255]);
}

#[test]
fn dialog_pointer_input_does_not_mark_the_background_capture() {
    let mut a = Annotation::from_png(&screenshot()).unwrap();
    let ctx = egui::Context::default();
    let pos = egui::pos2(90., 50.);
    for _ in 0..3 {
        frame(&mut a, &ctx, pos, None, true);
    }
    frame(&mut a, &ctx, pos, Some(true), true);
    frame(&mut a, &ctx, pos + egui::vec2(2., 0.), None, true);
    frame(&mut a, &ctx, pos + egui::vec2(2., 0.), Some(false), true);
    assert_eq!(red(&a), 0);
}

#[test]
fn saved_report_png_contains_the_marks() {
    let original = screenshot();
    let mut a = Annotation::from_png(&original).unwrap();
    a.stroke(egui::pos2(8., 8.), egui::pos2(40., 30.), 2.);
    let city = opencity::runtime::City::from_program(&opencity::parse("").unwrap(), None).unwrap();
    let mut report = crate::engine::bug_report::Report::new(&city, [0.; 3], [0.; 3], [1.; 3]);
    report.description = "Marked contact edge".into();
    let root = std::env::temp_dir().join(format!("annotated-report-{}", std::process::id()));
    let folder = report
        .save(&root, &a.screenshot(&original).unwrap())
        .unwrap();
    let png = std::fs::read(folder.join("screenshot.png")).unwrap();
    assert_eq!(Annotation::from_png(&png).unwrap().pixels, a.pixels);
    std::fs::remove_dir_all(root).unwrap();
}
