//! The game-native job feed, money display, and phone entry point.
use super::hud::Overlay;
use crate::kairos::{Kairos, MissionId, MissionProgress};
use egui::{Color32, RichText};

const INK: Color32 = Color32::from_rgb(38, 62, 61);
const MUTED: Color32 = Color32::from_rgb(101, 126, 121);
const GREEN: Color32 = Color32::from_rgb(49, 125, 109);
const MONEY_GREEN: Color32 = Color32::from_rgb(102, 170, 148);
const HUD_PAPER: Color32 = Color32::from_rgb(226, 226, 233);
const MONEY_FONT: &str = "money_bold";

pub(super) fn install_font(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        MONEY_FONT.to_owned(),
        std::sync::Arc::new(egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/pt-sans-bold.ttf"
        ))),
    );
    fonts.families.insert(
        egui::FontFamily::Name(MONEY_FONT.into()),
        vec![MONEY_FONT.to_owned()],
    );
    ctx.set_fonts(fonts);
}

pub(super) struct Portraits {
    taxi: egui::TextureHandle,
    bus: egui::TextureHandle,
    snowplow: egui::TextureHandle,
    phone: egui::TextureHandle,
}

impl Portraits {
    fn for_mission(&self, id: MissionId) -> &egui::TextureHandle {
        match id {
            MissionId::Taxi => &self.taxi,
            MissionId::Bus => &self.bus,
            MissionId::Snowplow => &self.snowplow,
        }
    }
}

fn load_portrait(ctx: &egui::Context, label: &str, bytes: &[u8]) -> egui::TextureHandle {
    let mut reader = png::Decoder::new(bytes)
        .read_info()
        .expect("job portrait PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("job portrait pixels");
    assert_eq!(info.color_type, png::ColorType::Rgb);
    ctx.load_texture(
        label,
        egui::ColorImage::from_rgb(
            [info.width as usize, info.height as usize],
            &pixels[..info.buffer_size()],
        ),
        egui::TextureOptions::LINEAR,
    )
}

fn load_phone_icon(ctx: &egui::Context) -> egui::TextureHandle {
    let mut reader =
        png::Decoder::new(include_bytes!("../../assets/jobines/phone-button.png").as_slice())
            .read_info()
            .expect("phone icon PNG");
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).expect("phone icon pixels");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    ctx.load_texture(
        "job phone icon",
        egui::ColorImage::from_rgba_unmultiplied(
            [info.width as usize, info.height as usize],
            &pixels[..info.buffer_size()],
        ),
        egui::TextureOptions::LINEAR,
    )
}

pub(super) fn load_portraits(ctx: &egui::Context) -> Portraits {
    Portraits {
        taxi: load_portrait(
            ctx,
            "job portrait taxi",
            crate::web_asset!(
                "../../assets/jobines/worker-portrait-01.png",
                "assets/jobines/worker-portrait-01.png"
            ),
        ),
        bus: load_portrait(
            ctx,
            "job portrait bus",
            crate::web_asset!(
                "../../assets/jobines/driver-portrait-01.png",
                "assets/jobines/driver-portrait-01.png"
            ),
        ),
        snowplow: load_portrait(
            ctx,
            "job portrait snowplow",
            crate::web_asset!(
                "../../assets/jobines/plow-portrait-01.png",
                "assets/jobines/plow-portrait-01.png"
            ),
        ),
        phone: load_phone_icon(ctx),
    }
}

pub(super) fn show(
    ctx: &egui::Context,
    open: &mut bool,
    kairos: &Kairos,
    overlay: Overlay,
    phone_anchor: Option<egui::Vec2>,
    portraits: Option<&Portraits>,
) -> Option<super::palette::PaletteAction> {
    if overlay != Overlay::Driving {
        return None;
    }
    let money_top = 8.;
    egui::Area::new(egui::Id::new("job_money"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-20., money_top))
        .show(ctx, |ui| {
            let amount = format!("{}$", kairos.money_cents() / 100);
            let galley = ui.painter().layout_no_wrap(
                amount,
                egui::FontId::new(48., egui::FontFamily::Name(MONEY_FONT.into())),
                Color32::PLACEHOLDER,
            );
            let (rect, _) =
                ui.allocate_exact_size(galley.size() + egui::vec2(4., 4.), egui::Sense::hover());
            let painter = ui.painter();
            for offset in [
                egui::vec2(0., 0.),
                egui::vec2(0., 4.),
                egui::vec2(4., 0.),
                egui::vec2(4., 4.),
            ] {
                painter.galley(rect.min + offset, galley.clone(), INK);
            }
            painter.galley(rect.min + egui::vec2(2., 2.), galley, MONEY_GREEN);
        });
    if let Some(objective) = kairos.objective() {
        egui::Area::new(egui::Id::new("job_objective"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-20., money_top + 76.))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(HUD_PAPER)
                    .stroke(egui::Stroke::new(7., Color32::BLACK))
                    .corner_radius(24)
                    .inner_margin(egui::Margin::symmetric(18, 14))
                    .show(ui, |ui| {
                        ui.set_max_width((ctx.content_rect().width() - 72.).clamp(120., 360.));
                        ui.label(RichText::new(objective.label()).color(Color32::BLACK).font(
                            egui::FontId::new(24., egui::FontFamily::Name(MONEY_FONT.into())),
                        ));
                        if let Some(MissionProgress::ClearedDistanceKm(km)) = kairos.progress() {
                            ui.add_space(10.);
                            ui.label(RichText::new(format!("{km:.2} km")).color(GREEN).font(
                                egui::FontId::new(16., egui::FontFamily::Name(MONEY_FONT.into())),
                            ));
                        }
                    });
            });
    }
    if let Some(phone_anchor) = phone_anchor.filter(|_| !*open) {
        egui::Area::new(egui::Id::new("job_phone"))
            .anchor(egui::Align2::RIGHT_BOTTOM, phone_anchor)
            .show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(88., 88.), egui::Sense::click());
                if let Some(images) = portraits {
                    ui.painter().image(
                        images.phone.id(),
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0., 0.), egui::pos2(1., 1.)),
                        Color32::WHITE,
                    );
                }
                if response.clicked() {
                    *open = true;
                }
                response.on_hover_text("Ouvrir les jobines");
            });
    }
    if !*open {
        return None;
    }
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        *open = false;
        return None;
    }
    let mut selected = None;
    let mut window_open = *open;
    let mut close_requested = false;
    egui::Window::new("Les jobines du coin")
        .open(&mut window_open)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width((ctx.content_rect().width() - 36.).min(500.))
        .frame(
            egui::Frame::new()
                .fill(Color32::from_rgb(246, 249, 247))
                .corner_radius(18)
                .inner_margin(egui::Margin::same(18))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(55),
                }),
        )
        .show(ctx, |ui| {
            *ui.visuals_mut() = egui::Visuals::light();
            egui::Frame::new()
                .fill(GREEN)
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Les jobines du coin")
                                .color(Color32::WHITE)
                                .strong()
                                .size(16.),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new("×").color(Color32::WHITE))
                                        .fill(GREEN),
                                )
                                .clicked()
                            {
                                close_requested = true;
                            }
                        });
                    });
                });
            ui.add_space(12.);
            for (id, author, handle, message, icon) in [
                (
                    MissionId::Taxi,
                    "Cynthia Tremblay",
                    "@cynthia_tremblay  construction civile",
                    "Mon ami cherche quelqu'un pour conduire son taxi aujourd'hui. Ça te dit?",
                    "🚕",
                ),
                (
                    MissionId::Bus,
                    "Mike Vachon",
                    "@mike_vachon  chauffeur d'autobus",
                    "On cherche du monde pour conduire l'autobus du quartier. Tu embarques?",
                    "🚌",
                ),
                (
                    MissionId::Snowplow,
                    "Yvan Tansiarge",
                    "@yvan_tansiarge  déneigeur",
                    "Une déneigeuse t'attend.",
                    "❄",
                ),
            ] {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .corner_radius(14)
                    .inner_margin(egui::Margin::same(13))
                    .shadow(egui::Shadow {
                        offset: [0, 2],
                        blur: 7,
                        spread: 0,
                        color: Color32::from_black_alpha(18),
                    })
                    .show(ui, |ui| {
                        ui.set_min_width((ctx.content_rect().width() - 84.).clamp(230., 450.));
                        ui.horizontal(|ui| {
                            if let Some(texture) =
                                portraits.map(|portraits| portraits.for_mission(id))
                            {
                                ui.add(
                                    egui::Image::new((texture.id(), egui::vec2(40., 40.)))
                                        .corner_radius(20),
                                );
                            } else {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(40., 40.),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(
                                    rect.center(),
                                    20.,
                                    Color32::from_rgb(220, 234, 226),
                                );
                                ui.painter().circle_filled(
                                    rect.center() + egui::vec2(0., -5.),
                                    6.,
                                    MUTED,
                                );
                                ui.painter().circle_filled(
                                    rect.center() + egui::vec2(0., 10.),
                                    11.,
                                    MUTED,
                                );
                            }
                            ui.vertical(|ui| {
                                ui.label(RichText::new(author).color(INK).strong().size(12.));
                                ui.label(RichText::new(handle).color(MUTED).size(10.));
                            });
                        });
                        ui.add_space(7.);
                        ui.label(RichText::new(message).color(INK).size(13.));
                        ui.add_space(8.);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{icon}  {}", id.title()))
                                    .color(INK)
                                    .strong(),
                            );
                            ui.add_space(8.);
                            if kairos.completed(id) > 0 {
                                ui.label(
                                    RichText::new(format!("✓ {}", kairos.completed(id)))
                                        .color(MUTED)
                                        .size(11.),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let active =
                                        kairos.active_id() == Some(id) && !kairos.is_complete();
                                    if active {
                                        ui.label(RichText::new("En cours").color(GREEN).strong());
                                    } else if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("Choisir").color(Color32::WHITE),
                                            )
                                            .fill(GREEN),
                                        )
                                        .clicked()
                                    {
                                        selected =
                                            Some(super::palette::PaletteAction::SelectMission(id));
                                    }
                                },
                            );
                        });
                    });
                ui.add_space(10.);
            }
            if kairos.active_id().is_some() && ui.button("Abandonner la jobine").clicked() {
                selected = Some(super::palette::PaletteAction::AbandonMission);
            }
        });
    *open = window_open && !close_requested && selected.is_none();
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_button_opens_job_feed() {
        let ctx = egui::Context::default();
        install_font(&ctx);
        let kairos = Kairos::test_session();
        let mut open = false;
        let point = egui::pos2(758., 600. - 20. - 22.);
        for events in [
            vec![],
            vec![],
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    events,
                    ..Default::default()
                },
                |_| {
                    let _ = show(
                        &ctx,
                        &mut open,
                        &kairos,
                        Overlay::Driving,
                        Some(egui::vec2(-20., -20.)),
                        None,
                    );
                },
            );
            output.textures_delta.clear();
        }
        assert!(open);
    }
}
