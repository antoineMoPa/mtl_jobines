use super::*;
#[cfg(not(target_arch = "wasm32"))]
use crate::engine::tooltip::HoverHint;
impl Palette {
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn remember_report_overlay(
        &mut self,
        selected: &Option<PaletteAction>,
        primitives: &[egui::ClippedPrimitive],
        scale: f32,
    ) {
        self.report_overlay = (*selected == Some(PaletteAction::BugReport)
            && (self.buildings.as_ref().is_some_and(|d| d.open)
                || self.expansion.as_ref().is_some_and(|d| d.picker.open)
                || self.props.as_ref().is_some_and(|d| d.open)))
        .then(|| (primitives.to_vec(), scale));
    }
    pub(super) fn show_panels(
        &mut self,
        ui: &mut egui::Ui,
        renderer: &mut Renderer,
        sound: &mut crate::audio::Settings,
        view_projection: glam::Mat4,
        city: &opencity::runtime::City,
        kairos: &Kairos,
        walking: bool,
        vitals: Option<super::super::job_feed::Vitals>,
        help_visible: &mut bool,
        game_overlay: Overlay,
        menu_selection: &mut MenuItem,
    ) -> Option<PaletteAction> {
        let ctx = ui.ctx().clone();
        if self.settings_open {
            let mut enabled = renderer.antialiasing();
            super::super::help::settings(&ctx, &mut self.settings_open, &mut enabled);
            renderer.set_antialiasing(enabled);
            return None;
        }
        if *help_visible {
            super::super::help::show(&ctx, help_visible, game_overlay == Overlay::Start);
            return None;
        }
        if matches!(game_overlay, Overlay::Start | Overlay::Menu) {
            self.menu_clicked = super::super::help::menu(&ctx, game_overlay, menu_selection);
            return None;
        }
        if game_overlay == Overlay::Map {
            super::super::map_ui::show(&ctx, renderer.full_map);
            return None;
        }
        let mut icons = super::super::help::CornerIcons::new(walking);
        let mobile = super::super::app::mobile_enabled();
        let phone_anchor = (!mobile).then(|| icons.place(88.));
        let help_anchor = icons.place(56.);
        if game_overlay == Overlay::Driving && !self.is_open() && !mobile {
            super::super::help::indicator(&ctx, &self.help_icon, help_visible, help_anchor);
        }
        let mut selected = None;
        #[cfg(not(target_arch = "wasm32"))]
        let reporting = self.bug_report.is_open();
        #[cfg(not(target_arch = "wasm32"))]
        let report_link = {
            let enabled = !self.menu.is_open() && !self.sky_open && !self.sound_open && !reporting;
            let pixels_per_point = ctx.pixels_per_point();
            let screen_height = ctx.content_rect().height() * pixels_per_point;
            let fps_scale = (screen_height / 800.).clamp(1., 2.);
            let response = egui::Area::new(egui::Id::new("bug_report_link"))
                .anchor(
                    egui::Align2::LEFT_TOP,
                    egui::vec2(
                        16. * fps_scale / pixels_per_point,
                        48. * fps_scale / pixels_per_point + 8.,
                    ),
                )
                .show(&ctx, |ui| {
                    ui.add_enabled(enabled, egui::Link::new("[bug]"))
                        .hover_hint("Report a bug")
                })
                .inner;
            if response.clicked() {
                selected = Some(PaletteAction::BugReport);
            }
            response.rect
        };
        if let Some(id) = self.menu.show(&ctx, &self.commands) {
            if id == "sound.settings" {
                self.sound_open = true;
            } else if id == "sky.settings" {
                self.sky_open = true;
                if renderer.daylight.is_some() {
                    renderer.sky_settings.solar_hour =
                        Some(renderer.sky_settings.hour(0., renderer.daylight));
                }
                renderer.daylight = None;
            } else {
                selected = PaletteAction::from_id(&id);
            }
        }
        if self.sky_open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.sky_open = false;
        }
        super::super::sky_controls::show(&ctx, &mut self.sky_open, &mut renderer.sky_settings);
        if self.sound_open && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.sound_open = false;
        }
        super::super::sound_settings::show(sound, &ctx, &mut self.sound_open);
        let vitals = vitals.filter(|_| !self.is_open() && !*help_visible);
        if let Some(action) = super::super::job_feed::show(
            &ctx,
            &mut self.job_feed_open,
            kairos,
            renderer.overlay,
            vitals,
            phone_anchor,
            Some(&self.job_portraits),
        ) {
            selected = Some(action);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let report_hovered = ctx
                .pointer_hover_pos()
                .is_some_and(|p| report_link.contains(p));
            let input_enabled =
                !reporting && !report_hovered && selected != Some(PaletteAction::BugReport);
            if let Some(document) = &mut self.expansion {
                document.show_with_input(
                    ui,
                    view_projection,
                    [renderer.config.width, renderer.scene_height()],
                    input_enabled && !self.menu.is_open() && !self.sky_open && !self.sound_open,
                );
            }
            if let Some(document) = &mut self.buildings {
                if reporting || selected == Some(PaletteAction::BugReport) {
                    document.cancel_interaction();
                }
                document.show_with_input(
                    ui,
                    view_projection,
                    [renderer.config.width, renderer.scene_height()],
                    city,
                    input_enabled,
                );
            }
            if let Some(document) = &mut self.props {
                let enabled =
                    input_enabled && !self.menu.is_open() && !self.sky_open && !self.sound_open;
                if !enabled {
                    document.cancel_interaction();
                }
                document.show_with_input(
                    ui,
                    view_projection,
                    [renderer.config.width, renderer.scene_height()],
                    city,
                    enabled,
                );
            }
            self.bug_report.show(&ctx);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (view_projection, city);
        selected
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::engine) fn paint_report_overlay(
        &mut self,
        renderer: &Renderer,
        target: &wgpu::TextureView,
    ) {
        if let Some((primitives, scale)) = self.report_overlay.take() {
            self.painter
                .paint(renderer, target, primitives, Default::default(), scale);
        }
    }
}
