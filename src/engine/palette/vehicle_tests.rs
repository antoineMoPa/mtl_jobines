use super::*;

#[test]
fn every_vehicle_can_be_searched_by_name_and_selected_with_enter() {
    let commands = commands(&[]);
    let definitions = crate::dynamic_world::catalog::definitions();
    assert_eq!(
        commands
            .iter()
            .filter(|c| c.id.starts_with("vehicle.switch."))
            .count(),
        definitions.len()
    );
    for (model, definition) in definitions.iter().enumerate() {
        let ctx = egui::Context::default();
        let mut menu = CommandPalette::default();
        menu.open();
        let mut selected = None;
        for events in [
            vec![],
            vec![egui::Event::Text(definition.config.name.clone())],
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        ] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if let Some(id) = menu.show(ui.ctx(), &commands) {
                        selected = PaletteAction::from_id(&id);
                    }
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(selected, Some(PaletteAction::SwitchVehicle(model)));
        assert!(!menu.is_open());
    }
    for id in [
        "vehicle.switch.",
        "vehicle.switch.unknown",
        "vehicle.switch.0",
    ] {
        assert_eq!(PaletteAction::from_id(id), None);
    }
}
