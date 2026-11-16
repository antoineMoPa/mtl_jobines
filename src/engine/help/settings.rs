use super::{card_frame, shade, FONT, GREEN, INK, MUTED};
use egui::{Color32, FontFamily, FontId, RichText, Stroke};

fn text(label: &str, size: f32, color: Color32) -> RichText {
    RichText::new(label)
        .font(FontId::new(size, FontFamily::Name(FONT.into())))
        .color(color)
}

pub(in crate::engine) fn show(ctx: &egui::Context, open: &mut bool, antialiasing: &mut bool) {
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        *open = false;
        return;
    }
    shade(ctx);
    egui::Area::new(egui::Id::new("settings_card"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 84.).clamp(180., 414.);
            card_frame().show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 12.;
                let visuals = ui.visuals_mut();
                visuals.widgets.inactive.bg_fill = Color32::WHITE;
                visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
                visuals.widgets.inactive.bg_stroke = Stroke::new(2., INK);
                visuals.widgets.hovered.weak_bg_fill = GREEN;
                visuals.widgets.hovered.bg_stroke = Stroke::new(2., INK);
                visuals.widgets.active.weak_bg_fill = GREEN;
                visuals.widgets.active.bg_stroke = Stroke::new(2., INK);
                visuals.widgets.hovered.fg_stroke = Stroke::new(2., Color32::WHITE);
                visuals.widgets.active.fg_stroke = Stroke::new(2., Color32::WHITE);
                egui::ScrollArea::vertical()
                    .max_height((ctx.content_rect().height() - 148.).max(100.))
                    .show(ui, |ui| {
                        ui.label(text("PARAMÈTRES", 26., INK));
                        ui.add_space(4.);
                        ui.label(text("Graphismes", 18., GREEN));
                        ui.label(text("Anticrénelage 4× (MSAA)", 18., INK));
                        ui.label(text(
                            "Lisse les contours. Désactivé par défaut pour réduire la mémoire utilisée.",
                            15.,
                            MUTED,
                        ));
                        let label = if *antialiasing { "ACTIVÉ" } else { "DÉSACTIVÉ" };
                        let response = ui.add_sized(
                            [width, 52.],
                            egui::Button::new(text(label, 20., if *antialiasing { Color32::WHITE } else { INK }))
                                .selected(*antialiasing)
                                .fill(if *antialiasing { GREEN } else { Color32::WHITE })
                                .stroke(Stroke::new(2., INK))
                                .corner_radius(10),
                        ).on_hover_text("Activer ou désactiver l’anticrénelage");
                        if response.clicked() {
                            *antialiasing = !*antialiasing;
                        }
                        ui.label(text("Les changements s’appliquent immédiatement.", 15., MUTED));
                    });
                if ui.add_sized(
                    [width, 52.],
                    egui::Button::new(text("RETOUR", 22., Color32::WHITE))
                        .fill(GREEN)
                        .stroke(Stroke::new(2., INK))
                        .corner_radius(10),
                ).clicked() {
                    *open = false;
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_toggle_and_return_work_at_desktop_and_phone_sizes() {
        for size in [
            egui::vec2(1280., 800.),
            egui::vec2(390., 844.),
            egui::vec2(844., 390.),
        ] {
            let ctx = egui::Context::default();
            crate::engine::job_feed::install_font(&ctx);
            let mut open = true;
            let mut enabled = false;
            let mut frame = |events: Vec<egui::Event>| {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        events,
                        ..Default::default()
                    },
                    |ui| show(ui.ctx(), &mut open, &mut enabled),
                );
                output.textures_delta.clear();
                output
            };
            frame(vec![]);
            for label in ["DÉSACTIVÉ", "ACTIVÉ", "RETOUR"] {
                let output = frame(vec![]);
                let position = output
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            if text.galley.text() == label {
                                return Some(text.pos + text.galley.size() * 0.5);
                            }
                        }
                        None
                    })
                    .expect("settings button is visible");
                assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains(position));
                for pressed in [true, false] {
                    frame(vec![
                        egui::Event::PointerMoved(position),
                        egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]);
                }
            }
            assert!(!enabled);
            assert!(!open);
        }
    }
}
