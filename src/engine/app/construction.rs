use super::*;
impl Game {
    pub(super) fn open_construction(&mut self, action: PaletteAction) {
        let Some(watcher) = &self.live_reload else {
            return;
        };
        if let PaletteAction::PlaceProps(kind) = action {
            match super::super::props::Document::open(
                watcher.path(),
                self.world.player_position() / METERS_PER_UNIT,
                self.world.player_yaw(),
            ) {
                Ok(mut document) => {
                    document.choose_kind(kind);
                    if let Some(palette) = &mut self.palette {
                        palette.buildings = None;
                        palette.expansion = None;
                        palette.props = Some(document);
                    }
                }
                Err(error) => eprintln!("Could not open Props: {error}"),
            }
            self.keys.clear();
            self.stop_orbit_drag();
            self.scene_dirty = true;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
            return;
        }
        if action == PaletteAction::BuildBuildings {
            let player = self.world.player_position() / METERS_PER_UNIT;
            match super::super::buildings::Document::open(
                watcher.path(),
                player,
                self.world.player_yaw(),
            ) {
                Ok(document) => {
                    if let Some(palette) = &mut self.palette {
                        palette.expansion = None;
                        palette.props = None;
                        palette.buildings = Some(document);
                    }
                }
                Err(error) => eprintln!("Could not open Buildings: {error}"),
            }
            self.keys.clear();
            self.stop_orbit_drag();
            self.scene_dirty = true;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        } else {
            match super::super::expansion::Document::open(watcher.path()) {
                Ok(mut document) => {
                    document.enable_views(
                        self.world.player_position() / METERS_PER_UNIT,
                        self.world.player_yaw(),
                    );
                    if action == PaletteAction::GrowNetwork {
                        document.picker.start_growth();
                    }
                    if let Some(palette) = &mut self.palette {
                        palette.buildings = None;
                        palette.props = None;
                        palette.expansion = Some(document);
                    }
                }
                Err(error) => eprintln!("Could not open expansion picker: {error}"),
            }
        }
    }
}
