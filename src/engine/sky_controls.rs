//! Game sky controls live beside the existing command palette.
use super::lighting::SkySettings;

pub fn show(ctx: &egui::Context, open: &mut bool, settings: &mut SkySettings) {
    egui::Window::new("Sky, sun and stars")
        .open(open)
        .resizable(false)
        .vscroll(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (label, hour) in [("Noon", 12.), ("Sunset", 19.), ("Night", 0.)] {
                    if ui.button(label).clicked() {
                        settings.solar_hour = Some(hour);
                    }
                }
                if ui.button("Automatic").clicked() {
                    settings.solar_hour = None;
                }
            });
            let mut manual = settings.solar_hour.is_some();
            if ui.checkbox(&mut manual, "Hold solar time").changed() {
                settings.solar_hour = manual.then_some(12.);
            }
            if let Some(hour) = &mut settings.solar_hour {
                ui.add(egui::Slider::new(hour, 0.0..=24.0).text("Solar hour"));
            }
            ui.label("Automatic time follows the twenty-minute driving cycle.");
            ui.add(egui::Slider::new(&mut settings.latitude, -90.0..=90.0).text("Latitude °"));
            ui.add(egui::Slider::new(&mut settings.day_of_year, 1..=365).text("Day of year"));
            ui.add(
                egui::Slider::new(&mut settings.north_rotation, -180.0..=180.0)
                    .text("North rotation °"),
            );
            ui.separator();
            ui.add(egui::Slider::new(&mut settings.turbidity, 1.0..=10.0).text("Atmosphere haze"));
            ui.add(
                egui::Slider::new(&mut settings.sun_temperature, 2000.0..=10000.0)
                    .text("Sun temperature K"),
            );
            ui.add(egui::Slider::new(&mut settings.sun_intensity, 0.0..=4.0).text("Sun intensity"));
            ui.separator();
            ui.label("Stars appear as the sun drops below the horizon.");
            ui.add(egui::Slider::new(&mut settings.star_density, 0.0..=0.5).text("Star density"));
            ui.add(egui::Slider::new(&mut settings.star_size, 0.25..=2.5).text("Star size"));
            ui.add(
                egui::Slider::new(&mut settings.star_brightness, 0.0..=6.0).text("Star brightness"),
            );
            ui.add(
                egui::Slider::new(&mut settings.horizon_glow, 0.0..=3.0).text("Night horizon glow"),
            );
            ui.horizontal(|ui| {
                ui.label("Star color");
                ui.color_edit_button_rgb(&mut settings.star_color);
            });
            if ui.button("Restore Montréal defaults").clicked() {
                *settings = SkySettings::montreal();
            }
        });
}
