//! First-play controls and the in-game help key.
use super::hud::{MenuItem, Overlay};
use crate::engine::tooltip::HoverHint;
use egui::{Color32, RichText, Stroke};

mod settings;
pub(super) use settings::show as settings;

const INK: Color32 = Color32::from_rgb(38, 62, 61);
const GREEN: Color32 = Color32::from_rgb(49, 125, 109);
const MUTED: Color32 = Color32::from_rgb(101, 126, 121);
const PAPER: Color32 = Color32::from_rgb(246, 249, 247);
const FONT: &str = "money_bold";
const PHONE_BOTTOM: f32 = 20.;
const CONTROL_KEY_COLUMN: f32 = 166.;
const CONTROL_COLUMN_GAP: f32 = 18.;

/// Positions corner actions from the bottom up, keeping each icon centered in one column.
pub(super) struct CornerIcons {
    right: f32,
    bottom: f32,
    column_width: f32,
    gap: f32,
}

impl CornerIcons {
    pub(super) fn new(walking: bool) -> Self {
        Self {
            right: if walking { 20. } else { 16. },
            bottom: PHONE_BOTTOM,
            column_width: 88.,
            gap: 12.,
        }
    }

    pub(super) fn place(&mut self, size: f32) -> egui::Vec2 {
        let offset = egui::vec2(-self.right - (self.column_width - size) * 0.5, -self.bottom);
        self.bottom += size + self.gap;
        offset
    }
}

pub(super) fn load_icon(ctx: &egui::Context) -> egui::TextureHandle {
    let mut reader =
        png::Decoder::new(include_bytes!("../../assets/jobines/help-button.png").as_slice())
            .read_info()
            .expect("help icon PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("help icon pixels");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    ctx.load_texture(
        "help key icon",
        egui::ColorImage::from_rgba_unmultiplied(
            [info.width as usize, info.height as usize],
            &pixels[..info.buffer_size()],
        ),
        egui::TextureOptions::LINEAR,
    )
}

#[derive(Clone, Copy)]
enum Arrow {
    Up,
    Left,
    Down,
    Right,
}

fn arrow_key(painter: &egui::Painter, center: egui::Pos2, arrow: Arrow) {
    let rect = egui::Rect::from_center_size(center, egui::vec2(36., 36.));
    painter.rect_filled(rect, 7., Color32::WHITE);
    painter.rect_stroke(rect, 7., Stroke::new(2., INK), egui::StrokeKind::Inside);
    let (shaft, head_a, head_b) = match arrow {
        Arrow::Up => (
            [egui::vec2(0., 8.), egui::vec2(0., -8.)],
            [egui::vec2(-6., -2.), egui::vec2(0., -8.)],
            [egui::vec2(0., -8.), egui::vec2(6., -2.)],
        ),
        Arrow::Left => (
            [egui::vec2(8., 0.), egui::vec2(-8., 0.)],
            [egui::vec2(-2., -6.), egui::vec2(-8., 0.)],
            [egui::vec2(-8., 0.), egui::vec2(-2., 6.)],
        ),
        Arrow::Down => (
            [egui::vec2(0., -8.), egui::vec2(0., 8.)],
            [egui::vec2(-6., 2.), egui::vec2(0., 8.)],
            [egui::vec2(0., 8.), egui::vec2(6., 2.)],
        ),
        Arrow::Right => (
            [egui::vec2(-8., 0.), egui::vec2(8., 0.)],
            [egui::vec2(2., -6.), egui::vec2(8., 0.)],
            [egui::vec2(8., 0.), egui::vec2(2., 6.)],
        ),
    };
    for [from, to] in [shaft, head_a, head_b] {
        painter.line_segment([center + from, center + to], Stroke::new(2.5, INK));
    }
}

fn arrows(ui: &mut egui::Ui) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(CONTROL_KEY_COLUMN, 80.), egui::Sense::hover());
    let x = rect.center().x;
    let y = rect.top() + 19.;
    let painter = ui.painter();
    arrow_key(painter, egui::pos2(x, y), Arrow::Up);
    arrow_key(painter, egui::pos2(x - 40., y + 40.), Arrow::Left);
    arrow_key(painter, egui::pos2(x, y + 40.), Arrow::Down);
    arrow_key(painter, egui::pos2(x + 40., y + 40.), Arrow::Right);
}

fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PAPER)
        .stroke(Stroke::new(4., INK))
        .corner_radius(18)
        .inner_margin(egui::Margin::same(22))
}

fn shade(ctx: &egui::Context) {
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("help_shade"),
    ))
    .rect_filled(
        ctx.content_rect(),
        0.,
        Color32::from_rgba_unmultiplied(23, 45, 54, 187),
    );
}

pub(super) fn show(ctx: &egui::Context, open: &mut bool, first_play: bool) {
    let bounds = ctx.content_rect();
    shade(ctx);
    egui::Area::new(egui::Id::new("help_card"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let width = (bounds.width() - 32.).min(510.).max(250.);
            card_frame().show(ui, |ui| {
                ui.set_width(width - 44.);
                if super::app::mobile_enabled() {
                    ui.label(
                        RichText::new("À vous la ville !")
                            .color(INK)
                            .font(egui::FontId::new(24., egui::FontFamily::Name(FONT.into()))),
                    );
                    ui.add_space(12.);
                    for text in [
                        "À gauche : maintenez les flèches pour vous déplacer.",
                        "À droite : courir, sauter ou conduire.",
                        "En haut : carte, téléphone, menu et portière.",
                    ] {
                        super::tooltip::message(ui, text);
                        ui.add_space(10.);
                    }
                } else {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = CONTROL_COLUMN_GAP;
                        arrows(ui);
                        super::tooltip::message(ui, "Se déplacer");
                    });
                    ui.separator();
                    ui.add_space(8.);
                    super::tooltip::hint(
                        ui,
                        Some("F"),
                        "Chausser les skis, entrer ou sortir du véhicule",
                    );
                    super::tooltip::message(ui, "À skis : W glisser, S freiner puis reculer, E bâtons, A/D tourner, Espace freiner, Maj se ramasser. Espace + A/D : arrêt hockey.");
                    super::tooltip::message(ui, "Télésiège : attendez dans la voie bleue, le siège vous embarque automatiquement.");
                    super::tooltip::hint(ui, Some("O"), "Ouvrir ou fermer la portière");
                    ui.add_space(8.);
                    ui.separator();
                    ui.add_space(8.);
                    super::tooltip::message(ui, "À pied : 1 fusil, 2 pistolet, 0 ranger");
                    super::tooltip::message(ui, "Souris pour viser, A/D pour se décaler");
                    super::tooltip::message(ui, "Clic gauche à mains nues pour frapper");
                    super::tooltip::message(ui, "Arme équipée : clic pour tirer, R pour recharger");
                    ui.add_space(8.);
                    super::tooltip::hint(ui, Some("P"), "Ouvrir le téléphone");
                    ui.add_space(8.);
                    ui.separator();
                    ui.add_space(16.);
                    super::tooltip::hint(ui, Some("?"), "Retrouvez cette aide à tout moment.");
                }
                ui.add_space(18.);
                let label = if first_play { "JOUER" } else { "REPRENDRE" };
                let (button, response) =
                    ui.allocate_exact_size(egui::vec2(width - 44., 56.), egui::Sense::click());
                ui.painter().rect_filled(
                    button,
                    10.,
                    if response.hovered() {
                        Color32::from_rgb(62, 147, 129)
                    } else {
                        GREEN
                    },
                );
                ui.painter().text(
                    button.center(),
                    egui::Align2::CENTER_CENTER,
                    label,
                    egui::FontId::new(22., egui::FontFamily::Name(FONT.into())),
                    Color32::WHITE,
                );
                if response.clicked() {
                    *open = false;
                }
            });
        });
}

pub(super) fn menu(
    ctx: &egui::Context,
    overlay: Overlay,
    selection: &mut MenuItem,
) -> Option<MenuItem> {
    let bounds = ctx.content_rect();
    shade(ctx);
    let mut clicked = None;
    egui::Area::new(egui::Id::new("game_menu_card"))
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            let width = (bounds.width() - 32.).min(360.).max(250.);
            card_frame().show(ui, |ui| {
                ui.set_width(width - 44.);
                for (item, label) in [
                    (
                        MenuItem::Resume,
                        if overlay == Overlay::Start {
                            "JOUER"
                        } else {
                            "REPRENDRE"
                        },
                    ),
                    (MenuItem::Map, "CARTE"),
                    (MenuItem::Settings, "PARAMÈTRES"),
                    (MenuItem::Exit, "QUITTER"),
                ] {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(width - 44., 52.), egui::Sense::click());
                    if response.hovered() {
                        *selection = item;
                    }
                    let active = *selection == item || response.hovered();
                    ui.painter().rect_filled(
                        rect,
                        10.,
                        if active { GREEN } else { Color32::WHITE },
                    );
                    ui.painter().rect_stroke(
                        rect,
                        10.,
                        Stroke::new(2., INK),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::new(22., egui::FontFamily::Name(FONT.into())),
                        if active { Color32::WHITE } else { INK },
                    );
                    if response.clicked() {
                        clicked = Some(item);
                    }
                    ui.add_space(10.);
                }
            });
        });
    clicked
}

pub(super) fn indicator(
    ctx: &egui::Context,
    icon: &egui::TextureHandle,
    open: &mut bool,
    offset: egui::Vec2,
) {
    egui::Area::new(egui::Id::new("help_key"))
        .anchor(egui::Align2::RIGHT_BOTTOM, offset)
        .show(ctx, |ui| {
            let (rect, response) =
                ui.allocate_exact_size(egui::vec2(56., 56.), egui::Sense::click());
            ui.painter().image(
                icon.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
            if response.hover_hint("Afficher les commandes (?)").clicked() {
                *open = true;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_menu_map_stays_clickable_after_clicking_the_dimmed_background() {
        let ctx = egui::Context::default();
        super::super::job_feed::install_font(&ctx);
        let mut selection = MenuItem::Resume;
        let mut frame = |events: Vec<egui::Event>| {
            let mut clicked = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| clicked = menu(ui.ctx(), Overlay::Menu, &mut selection),
            );
            output.textures_delta.clear();
            clicked
        };
        frame(vec![]);
        let background = egui::pos2(10., 10.);
        let map_button = egui::pos2(640., 365.);
        for position in [background, map_button] {
            frame(vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            let clicked = frame(vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            if position == map_button {
                assert_eq!(clicked, Some(MenuItem::Map));
            }
        }
    }
}
