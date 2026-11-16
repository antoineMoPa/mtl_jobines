use super::*;
impl Game {
    pub(super) fn window_focus_lost(&mut self) {
        self.cutscene_focused = false;
        self.audio.silence();
        self.sound_eye = None;
        if self.fabula.active() {
            self.keys.clear();
            self.stop_orbit_drag();
            self.show_cursor();
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(document) = self
            .palette
            .as_mut()
            .and_then(|p| p.props.as_mut())
            .filter(|d| d.open)
        {
            document.cancel_interaction();
            self.keys.clear();
            self.stop_orbit_drag();
            self.show_cursor();
            return;
        }
        // Native file pickers and file drops transfer focus. Keep the building
        // document alive while discarding any unfinished pointer gesture.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(document) = self
            .palette
            .as_mut()
            .and_then(|p| p.buildings.as_mut())
            .filter(|d| d.open)
        {
            document.cancel_interaction();
            self.keys.clear();
            self.stop_orbit_drag();
            self.show_cursor();
            return;
        }
        if let Some(palette) = &mut self.palette {
            palette.dismiss();
        }
        if self.overlay != Overlay::Start {
            self.set_overlay(Overlay::Menu);
        }
        self.show_cursor();
    }
}
