//! The game-native job feed, money display, and phone entry point.
use super::hud::Overlay;
use crate::engine::tooltip::HoverHint;
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
            MissionId::Baggage | MissionId::Cat | MissionId::Taxi | MissionId::Zombie => &self.taxi,
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

#[derive(Clone, Copy)]
pub(super) struct Vitals {
    health: u8,
    stars: u8,
    searching: bool,
}

impl Vitals {
    pub(super) fn from_world(world: &crate::dynamic_world::DynamicWorld) -> Self {
        Self {
            health: world.player_health(),
            stars: world.wanted_stars(),
            searching: world.chase_status() == crate::dynamic_world::ChaseStatus::Searching,
        }
    }
}

pub(super) fn show(
    ctx: &egui::Context,
    open: &mut bool,
    kairos: &Kairos,
    overlay: Overlay,
    vitals: Option<Vitals>,
    phone_anchor: Option<egui::Vec2>,
    portraits: Option<&Portraits>,
) -> Option<super::palette::PaletteAction> {
    if overlay != Overlay::Driving {
        return None;
    }
    let money_top = 8.;
    let mobile = super::app::mobile_enabled();
    let status = egui::Area::new(egui::Id::new("job_money"))
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-20., money_top))
        .show(ctx, |ui| {
            let amount = format!("{}$", kairos.money_cents() / 100);
            let galley = ui.painter().layout_no_wrap(
                amount,
                egui::FontId::new(
                    if mobile { 28. } else { 48. },
                    egui::FontFamily::Name(MONEY_FONT.into()),
                ),
                Color32::PLACEHOLDER,
            );
            let cash_size = galley.size() + egui::vec2(4., 4.);
            let hearts = vitals.map(|v| {
                let heart = ui.painter().layout_no_wrap(
                    "♥".into(),
                    egui::FontId::proportional(if mobile { 18. } else { 25. }),
                    Color32::PLACEHOLDER,
                );
                (v.health, heart)
            });
            let mut size = cash_size;
            if let Some((_, heart)) = &hearts {
                size.x = size.x.max(4. * (heart.size().x + 2.));
                size.y += heart.size().y + 2.;
            }
            let stars = vitals.map_or(0, |v| v.stars);
            let stride = if mobile { 16. } else { 22. };
            if stars > 0 {
                size.x = size.x.max(5. * stride);
                size.y += stride;
            }
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let painter = ui.painter();
            if stars > 0 {
                for slot in 0..5 {
                    let center = rect.right_bottom()
                        - egui::vec2((4.5 - slot as f32) * stride, stride * 0.5);
                    let points: [egui::Pos2; 10] = std::array::from_fn(|i| {
                        let angle = std::f32::consts::PI * (i as f32 / 5. - 0.5);
                        center
                            + egui::Vec2::angled(angle)
                                * stride
                                * if i % 2 == 0 { 0.47 } else { 0.22 }
                    });
                    let fill = if slot >= stars {
                        Color32::from_rgb(102, 107, 114)
                    } else if vitals.is_some_and(|v| v.searching) {
                        Color32::from_rgb(178, 148, 84)
                    } else {
                        Color32::from_rgb(255, 204, 66)
                    };
                    let mut mesh = egui::Mesh::default();
                    mesh.colored_vertex(center, fill);
                    for point in points {
                        mesh.colored_vertex(point, fill);
                    }
                    for i in 0..10 {
                        mesh.add_triangle(0, i + 1, (i + 1) % 10 + 1);
                    }
                    painter.add(mesh);
                    painter.add(egui::Shape::closed_line(
                        points.to_vec(),
                        egui::Stroke::new(1.3, INK),
                    ));
                }
            }
            let cash = rect.right_top() - egui::vec2(cash_size.x, 0.);
            for offset in [
                egui::vec2(0., 0.),
                egui::vec2(0., 4.),
                egui::vec2(4., 0.),
                egui::vec2(4., 4.),
            ] {
                painter.galley(cash + offset, galley.clone(), INK);
            }
            painter.galley(cash + egui::vec2(2., 2.), galley, MONEY_GREEN);
            if let Some((hp, heart)) = hearts {
                let stride = heart.size().x + 2.;
                for slot in 0..4 {
                    let pos = egui::pos2(
                        rect.right() - (4 - slot) as f32 * stride + 1.,
                        rect.top() + cash_size.y + 1.,
                    );
                    for offset in [(-1., -1.), (-1., 1.), (1., -1.), (1., 1.)] {
                        painter.galley(pos + egui::vec2(offset.0, offset.1), heart.clone(), INK);
                    }
                    let color = if hp > slot * 25 {
                        Color32::from_rgb(241, 93, 96)
                    } else {
                        HUD_PAPER
                    };
                    painter.galley(pos, heart.clone(), color);
                }
            }
        });
    if let Some(objective) = kairos.objective() {
        egui::Area::new(egui::Id::new("job_objective"))
            .anchor(
                egui::Align2::RIGHT_TOP,
                egui::vec2(
                    -20.,
                    money_top
                        + (status.response.rect.height() + 6.).max(if mobile { 48. } else { 76. }),
                ),
            )
            .show(ctx, |ui| {
                ui.set_max_width(if mobile {
                    (ctx.content_rect().width() - 110.).clamp(100., 190.)
                } else {
                    320.
                });
                let mut label = match kairos.vehicle_return_seconds() {
                    Some(seconds) => {
                        format!("Retourne à ton véhicule avant la fin de la jobine ({seconds} s)")
                    }
                    None => objective.label().to_owned(),
                };
                if let Some(MissionProgress::ClearedDistanceKm(km)) = kairos.progress() {
                    label.push_str(&format!(" · {km:.2} km"));
                }
                if let Some(MissionProgress::Bags { loaded, delivered }) = kairos.progress() {
                    label.push_str(&format!(" · {loaded} à bord · {delivered}/4 livrés"));
                }
                super::tooltip::message(ui, &label);
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
                response.hover_hint("Ouvrir les jobines");
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
        .default_height(650.)
        .max_height((ctx.content_rect().height() - 48.).max(120.))
        .vscroll(true)
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
            if mobile {
                ui.spacing_mut().interact_size.y = 44.;
            }
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
                                        .fill(GREEN)
                                        .min_size(if mobile {
                                            egui::vec2(44., 44.)
                                        } else {
                                            egui::Vec2::ZERO
                                        }),
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
                (MissionId::Baggage, "Répartition YUL", "@yul  service au sol", "Première rotation : 4 bagages du terminal à l’Estuaire. Tracteur blanc au hangar · 20 $ par vol.", "✈"),
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
                (
                    MissionId::Zombie,
                    "Cynthia Tremblay",
                    "@cynthia_tremblay  construction civile",
                    "Y’a des zombies dans les rues! Tes munitions sont illimitées.",
                    "⚠",
                ),
                (
                    MissionId::Cat,
                    "Cynthia Tremblay",
                    "@cynthia_tremblay  construction civile",
                    "Mon chat blanc s’est sauvé ! J’ai marqué le coin sur ta carte. Peux-tu me le ramener au duplex ?",
                    "🐈",
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
    fn outcome_titles_follow_death_and_confirmed_arrest() {
        use crate::dynamic_world::ChaseStatus::*;
        let ctx = egui::Context::default();
        install_font(&ctx);
        let size = egui::vec2(360., 640.);
        for (health, chase, expected) in [
            (0, Clear, Some("Mangé")),
            (100, Arrested, Some("Busted")),
            (0, Arrested, Some("Busted")),
            (100, Reported(crate::dynamic_world::Crime::Gunshot), None),
            (100, Pursued, None),
            (100, Searching, None),
            (100, Surrendering, None),
            (100, Clear, None),
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ui| player_status(ui.ctx(), size, health, chase),
            );
            output.textures_delta.clear();
            let title = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) => Some(t.galley.text()),
                _ => None,
            });
            assert_eq!(title, expected);
        }
    }

    #[test]
    fn phone_button_opens_feed_and_zombie_card_selects_mission() {
        let ctx = egui::Context::default();
        install_font(&ctx);
        let kairos = Kairos::test_session();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 900.));
        let mut open = false;
        let mut selected = None;
        let mut frame = |events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |_| {
                    if let Some(action) = show(
                        &ctx,
                        &mut open,
                        &kairos,
                        Overlay::Driving,
                        None,
                        Some(egui::vec2(-20., -20.)),
                        None,
                    ) {
                        selected = Some(action);
                    }
                },
            );
            output.textures_delta.clear();
            output.shapes
        };
        let click = |pos| {
            let mut events = vec![egui::Event::PointerMoved(pos)];
            for pressed in [true, false] {
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            events
        };
        frame(vec![]);
        frame(vec![]);
        frame(click(screen.max - egui::vec2(42., 42.)));
        frame(vec![]);
        let shapes = frame(vec![]);
        let mut labels = shapes.iter().filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text),
            _ => None,
        });
        labels
            .find(|text| text.galley.text().contains(MissionId::Zombie.title()))
            .expect("zombie job is listed");
        let choose = labels
            .find(|text| text.galley.text() == "Choisir")
            .expect("zombie job has a selection button");
        frame(click(choose.visual_bounding_rect().center()));
        assert!(matches!(
            selected,
            Some(super::super::palette::PaletteAction::SelectMission(
                MissionId::Zombie
            ))
        ));
        assert!(!open);
    }
}

pub(super) fn player_status(
    ctx: &egui::Context,
    scene_size: egui::Vec2,
    health: u8,
    status: crate::dynamic_world::ChaseStatus,
) {
    if status == crate::dynamic_world::ChaseStatus::Arrested {
        outcome_banner(ctx, scene_size, "Busted");
    } else if health == 0 {
        outcome_banner(ctx, scene_size, "Mangé");
    }
}

fn outcome_banner(ctx: &egui::Context, scene_size: egui::Vec2, title: &str) {
    super::tooltip::positioned(
        ctx,
        (scene_size * 0.5).to_pos2(),
        egui::Align2::CENTER_CENTER,
        title,
    );
}
