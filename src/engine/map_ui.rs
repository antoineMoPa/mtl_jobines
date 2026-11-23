//! Text and close affordance for the game map. The map texture stays in the HUD renderer.
use super::hud::{FullMapView, MapSelection};
use crate::dynamic_world::DriveAvailability;
use egui::{Align2, Color32, FontFamily, FontId, Stroke};

const INK: Color32 = Color32::from_rgb(38, 62, 61);
const PAPER: Color32 = Color32::from_rgb(246, 249, 247);
const FONT: &str = "money_bold";

pub(super) fn show(ctx: &egui::Context, map: FullMapView) {
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

    if map.selection.is_route() && map.drive == DriveAvailability::Ready {
        let [x, y, w, h] = super::hud::full_map_drive_rect(width);
        let rect = egui::Rect::from_min_size(
            egui::pos2(x / scale, y / scale),
            egui::vec2(w / scale, h / scale),
        );
        painter.rect_filled(rect, 8. / scale, INK);
        let blocked = map.selection == MapSelection::DriveBlocked;
        let title_y = rect.center().y;
        painter.text(
            egui::pos2(rect.center().x, title_y),
            Align2::CENTER_CENTER,
            "Conduite cinéma",
            FontId::new(14. / scale, font.clone()),
            PAPER,
        );
        if blocked {
            egui::Area::new("map_drive_hint".into())
                .anchor(Align2::CENTER_TOP, egui::vec2(0., rect.bottom() + 4.))
                .interactable(false)
                .show(ctx, |ui| {
                    super::tooltip::message(ui, "Passage bloqué. Réessayer")
                });
        }
    }

    let jump_hint = match map.selection {
        MapSelection::Jump if width >= 570. => Some("Cliquez pour vous déplacer."),
        MapSelection::Jump if width >= 390. => Some("Cliquez : déplacement"),
        MapSelection::JumpBlocked if width >= 570. => Some("Choisissez un terrain dégagé."),
        MapSelection::JumpBlocked if width >= 390. => Some("Terrain dégagé requis"),
        _ => None,
    };
    if let Some(label) = jump_hint {
        egui::Area::new("map_jump_hint".into())
            .fixed_pos(egui::pos2(112. / scale, center_y - 12.))
            .interactable(false)
            .show(ctx, |ui| super::tooltip::message(ui, label));
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

pub(super) fn resume_driving(ctx: &egui::Context) -> bool {
    let bar = super::fabula::letterbox(ctx);
    egui::Area::new("cinematic_drive_controls".into())
        .anchor(Align2::RIGHT_TOP, egui::vec2(-18., bar + 10.))
        .show(ctx, |ui| {
            super::tooltip::action(ui, None, "Reprendre les commandes")
        })
        .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_header_only_offers_a_ready_drive() {
        let ctx = egui::Context::default();
        crate::engine::job_feed::install_font(&ctx);
        let mut map = FullMapView::default();
        for drive in [
            DriveAvailability::SelectDestination,
            DriveAvailability::NeedVehicle,
            DriveAvailability::Unreachable,
            DriveAvailability::Arrived,
            DriveAvailability::Ready,
        ] {
            map.drive = drive;
            let mut output = ctx.run_ui(Default::default(), |ui| show(ui.ctx(), map));
            output.textures_delta.clear();
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.text()),
                    _ => None,
                })
                .collect();
            if drive == DriveAvailability::Ready {
                assert_eq!(labels, ["MTL", "Conduite cinéma", "×"]);
            } else {
                assert_eq!(labels, ["MTL", "×"]);
            }
        }
    }
}
