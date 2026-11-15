//! Text and close affordance for the game map. The map texture stays in the HUD renderer.
use super::hud::MapSelection;
use egui::{Align2, Color32, FontFamily, FontId, Stroke};

const INK: Color32 = Color32::from_rgb(38, 62, 61);
const MUTED: Color32 = Color32::from_rgb(101, 126, 121);
const PAPER: Color32 = Color32::from_rgb(246, 249, 247);
const FONT: &str = "money_bold";

pub(super) fn show(ctx: &egui::Context, selection: MapSelection) {
    let scale = ctx.pixels_per_point();
    let width = ctx.content_rect().width() * scale;
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("game_map_chrome"),
    ));
    let font = FontFamily::Name(FONT.into());
    let center_y = 53. / scale;
    painter.text(
        egui::pos2(44. / scale, center_y),
        Align2::LEFT_CENTER,
        "MTL",
        FontId::new(29. / scale, font.clone()),
        INK,
    );

    if width >= 390. {
        let label = match selection {
            MapSelection::Route => {
                if width >= 570. {
                    "Cliquez pour tracer un trajet."
                } else {
                    "Cliquez : trajet"
                }
            }
            MapSelection::Jump => {
                if width >= 570. {
                    "Cliquez pour vous déplacer."
                } else {
                    "Cliquez : déplacement"
                }
            }
            MapSelection::JumpBlocked => {
                if width >= 570. {
                    "Choisissez un terrain dégagé."
                } else {
                    "Terrain dégagé requis"
                }
            }
        };
        painter.text(
            egui::pos2(140. / scale, center_y),
            Align2::LEFT_CENTER,
            label,
            FontId::new(15. / scale, font.clone()),
            MUTED,
        );
    }

    let close = super::hud::full_map_close_rect(width);
    let rect = egui::Rect::from_min_size(
        egui::pos2(close[0] / scale, close[1] / scale),
        egui::vec2(close[2] / scale, close[3] / scale),
    );
    painter.rect_filled(rect, 9. / scale, PAPER);
    painter.rect_stroke(
        rect,
        9. / scale,
        Stroke::new(2. / scale, INK),
        egui::StrokeKind::Inside,
    );
    painter.text(
        rect.center() + egui::vec2(0., -1. / scale),
        Align2::CENTER_CENTER,
        "×",
        FontId::new(25. / scale, font),
        INK,
    );
}
