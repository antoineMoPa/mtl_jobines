//! Text and close affordance for the game map. The map texture stays in the HUD renderer.
use super::hud::{FullMapView, MapSelection};
use crate::dynamic_world::DriveAvailability;
use egui::{Align2, Color32, FontFamily, FontId, Stroke};

const INK: Color32 = Color32::from_rgb(38, 62, 61);
const MUTED: Color32 = Color32::from_rgb(101, 126, 121);
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

    if map.selection.is_route() {
        let [x, y, w, h] = super::hud::full_map_drive_rect(width);
        let rect = egui::Rect::from_min_size(
            egui::pos2(x / scale, y / scale),
            egui::vec2(w / scale, h / scale),
        );
        let ready = map.drive == DriveAvailability::Ready;
        painter.rect_filled(rect, 8. / scale, if ready { INK } else { PAPER });
        let caption = match map.drive {
            DriveAvailability::SelectDestination => "Choisir un trajet",
            DriveAvailability::NeedVehicle => "Véhicule requis",
            DriveAvailability::Unreachable => "Trajet inaccessible",
            DriveAvailability::Arrived => "Déjà sur place",
            DriveAvailability::Ready if map.selection == MapSelection::DriveBlocked => {
                "Passage bloqué · Réessayer"
            }
            DriveAvailability::Ready => "",
        };
        let title_y = if caption.is_empty() {
            rect.center().y
        } else {
            rect.center().y - 7. / scale
        };
        painter.text(
            egui::pos2(rect.center().x, title_y),
            Align2::CENTER_CENTER,
            "Conduite cinéma",
            FontId::new(14. / scale, font.clone()),
            if ready { PAPER } else { MUTED },
        );
        if !caption.is_empty() {
            painter.text(
                rect.center() + egui::vec2(0., 9. / scale),
                Align2::CENTER_CENTER,
                caption,
                FontId::new(11. / scale, font.clone()),
                if ready { PAPER } else { INK },
            );
        }
    }

    if width >= 390. && (!map.selection.is_route() || width >= 610.) {
        let label = match map.selection {
            MapSelection::Route | MapSelection::DriveBlocked => "Cliquez pour tracer un trajet.",
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

pub(super) fn resume_driving(ctx: &egui::Context) -> bool {
    let bar = super::fabula::letterbox(ctx);
    egui::Area::new("cinematic_drive_controls".into())
        .anchor(Align2::RIGHT_TOP, egui::vec2(-18., bar + 10.))
        .show(ctx, |ui| {
            ui.add(
                egui::Button::new(
                    egui::RichText::new("Reprendre les commandes").color(Color32::WHITE),
                )
                .fill(Color32::from_black_alpha(210))
                .min_size(egui::vec2(0., 40.)),
            )
            .clicked()
        })
        .inner
}
