//! Compact paper labels shared by HUD instructions, actions and hover help.
use egui::{Color32, FontFamily, FontId, Frame, Margin, Response, RichText, Sense, Stroke, Ui};

fn frame() -> Frame {
    Frame::new()
        .fill(Color32::from_rgb(226, 226, 233))
        .stroke(Stroke::new(2., Color32::BLACK))
        .corner_radius(7)
        .inner_margin(Margin::symmetric(6, 3))
}
fn font(ctx: &egui::Context, size: f32) -> FontId {
    let family = FontFamily::Name("money_bold".into());
    if ctx.fonts(|fonts| fonts.definitions().families.contains_key(&family)) {
        FontId::new(size, family)
    } else {
        FontId::proportional(size)
    }
}
fn text(ui: &Ui, label: &str, size: f32) -> RichText {
    RichText::new(label)
        .color(Color32::BLACK)
        .font(font(ui.ctx(), size))
}
/// Draw non-interactive messages on their first frame, including offscreen captures.
pub(super) fn positioned(ctx: &egui::Context, pos: egui::Pos2, anchor: egui::Align2, label: &str) {
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        "game_message".into(),
    ));
    let galley = painter.layout(
        label.to_owned(),
        font(ctx, 14.),
        Color32::BLACK,
        (ctx.content_rect().width() - 36.).max(1.),
    );
    let panel = frame();
    let outer = anchor.anchor_size(pos, galley.size() + panel.total_margin().sum());
    let content = outer - panel.total_margin();
    painter.add(panel.paint(content));
    painter.galley(content.min, galley, Color32::BLACK);
}
pub(super) fn message(ui: &mut Ui, label: &str) {
    hint(ui, None, label);
}
pub(super) fn hint(ui: &mut Ui, key: Option<&str>, label: &str) -> Response {
    let Some(key) = key else {
        return frame()
            .show(ui, |ui| ui.label(text(ui, label, 14.)))
            .response;
    };
    let row = frame().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.;
            Frame::new()
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1., Color32::BLACK))
                .corner_radius(3)
                .inner_margin(Margin::symmetric(4, 1))
                .show(ui, |ui| ui.label(text(ui, key, 12.)));
            ui.label(text(ui, label, 14.));
        });
    });
    row.response
}
pub(super) fn action(ui: &mut Ui, key: Option<&str>, label: &str) -> bool {
    let mobile = super::app::mobile_enabled();
    if mobile {
        ui.add_space(10.);
    }
    let row = hint(ui, key, label);
    if mobile {
        ui.add_space(10.);
    }
    let rect = if mobile {
        row.rect
            .expand2(egui::vec2(0., ((44. - row.rect.height()) * 0.5).max(0.)))
    } else {
        row.rect
    };
    let response = ui
        .interact(rect, row.id.with("action"), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    if response.has_focus() {
        ui.painter().rect_stroke(
            row.rect.expand(1.),
            7,
            Stroke::new(2., Color32::from_rgb(0, 72, 254)),
            egui::StrokeKind::Outside,
        );
    }
    response.clicked()
}
pub(super) trait HoverHint {
    fn hover_hint(self, label: impl AsRef<str>) -> Response;
}
impl HoverHint for Response {
    fn hover_hint(self, label: impl AsRef<str>) -> Response {
        let mut tip = egui::Tooltip::for_enabled(&self);
        tip.popup = tip.popup.frame(frame());
        tip.show(|ui| ui.label(text(ui, label.as_ref(), 14.)));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cat_prompt_stays_compact_and_responds_to_a_click() {
        let ctx = egui::Context::default();
        super::super::job_feed::install_font(&ctx);
        let frame = |events| {
            let mut clicked = false;
            let mut rect = egui::Rect::NOTHING;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(320., 480.),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    rect = egui::Area::new("prompt_test".into())
                        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0., -30.))
                        .show(&ctx, |ui| {
                            clicked = action(ui, Some("Entrée"), "Attraper le chat")
                        })
                        .response
                        .rect;
                },
            );
            output.textures_delta.clear();
            (rect, clicked)
        };
        frame(vec![]);
        let (rect, _) = frame(vec![]);
        assert!(rect.width() < 170. && rect.height() <= 32., "{rect:?}");
        let pos = rect.center();
        let pointer = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![egui::Event::PointerMoved(pos), pointer(true)]);
        assert!(frame(vec![pointer(false)]).1);
    }
}
