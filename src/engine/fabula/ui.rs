use super::Playback;
use egui::{Align2, Color32, FontId, RichText};

/// Returns true when Skip is clicked. The caller owns the next game action.
pub(in crate::engine) fn show(
    ctx: &egui::Context,
    playback: &Playback,
    waiting_for_gesture: bool,
) -> bool {
    let bounds = ctx.content_rect();
    let bar = letterbox(ctx);
    if waiting_for_gesture {
        egui::Area::new(egui::Id::new("cutscene_start"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                // The window input handler resumes audio during the gesture;
                // waiting until this UI frame would lose browser activation.
                ui.add(
                    egui::Button::new(
                        RichText::new("Touchez ou cliquez pour démarrer").color(Color32::WHITE),
                    )
                    .fill(Color32::from_black_alpha(220))
                    .min_size(egui::vec2(0., 48.)),
                );
            });
    }
    if let Some(text) = playback.subtitle() {
        egui::Area::new(egui::Id::new("cutscene_subtitle"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_BOTTOM, egui::vec2(0., -bar - 12.))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_black_alpha(220))
                    .inner_margin(egui::Margin::same(14))
                    .corner_radius(8)
                    .show(ui, |ui| {
                        ui.set_max_width((bounds.width() - 64.).clamp(100., 850.));
                        ui.label(
                            RichText::new(text)
                                .font(FontId::proportional(
                                    (bounds.height() / 30.).clamp(18., 28.),
                                ))
                                .color(Color32::WHITE),
                        );
                    });
            });
    }
    egui::Area::new(egui::Id::new("cutscene_skip"))
        .order(egui::Order::Foreground)
        .anchor(Align2::RIGHT_TOP, egui::vec2(-18., bar + 10.))
        .show(ctx, |ui| {
            ui.add(
                egui::Button::new(RichText::new("Passer (Échap)").color(Color32::WHITE))
                    .fill(Color32::from_black_alpha(210))
                    .min_size(egui::vec2(0., 40.)),
            )
            .clicked()
        })
        .inner
}

pub(in crate::engine) fn letterbox(ctx: &egui::Context) -> f32 {
    let bounds = ctx.content_rect();
    let bar = (bounds.height() * 0.075).min(64.);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("cutscene_bars"),
    ));
    for y in [bounds.top(), bounds.bottom() - bar] {
        painter.rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(bounds.left(), y),
                egui::vec2(bounds.width(), bar),
            ),
            0.,
            Color32::BLACK,
        );
    }
    bar
}
