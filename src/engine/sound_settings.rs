pub(super) fn show(sound: &mut crate::audio::Settings, ctx: &egui::Context, open: &mut bool) {
    egui::Window::new("Sound")
        .open(open)
        .resizable(false)
        .show(ctx, |ui| {
            for (label, value) in [
                ("Master", &mut sound.master),
                ("Engines", &mut sound.engines),
                ("Horns and sirens", &mut sound.horns),
                ("Tires", &mut sound.tires),
                ("Wind and surroundings", &mut sound.ambience),
                ("Collisions and breakage", &mut sound.impacts),
            ] {
                ui.add(egui::Slider::new(value, 0.0..=1.0).text(label));
            }
        });
}
