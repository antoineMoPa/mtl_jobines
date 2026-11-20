use super::*;
impl Game {
    pub(super) fn window_active(&self) -> bool {
        self.focused && !self.hidden && !self.minimized && !self.suspended
    }

    pub(super) fn reset_frame_timing(&mut self) {
        self.last = None;
        self.accumulator = 0.;
        self.frame_rate = FrameRate::default();
        self.sound_eye = None;
    }

    pub(super) fn window_activity_changed(&mut self) {
        self.reset_frame_timing();
        self.surface_retry = None;
        if self.window_active() {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        } else {
            self.audio.silence();
            self.keys.clear();
            self.loadout.projection = None;
            self.release_aim_mouse();
            self.stop_orbit_drag();
            self.show_cursor();
            #[cfg(target_arch = "wasm32")]
            self.mobile.clear();
        }
    }

    pub(super) fn handle_window_activity(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::Resized(size) => {
                let minimized = size.width == 0 || size.height == 0;
                self.pending_resize = Some(*size);
                if self.minimized != minimized {
                    self.minimized = minimized;
                    self.window_activity_changed();
                } else if self.window_active() {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Focused(false) => {
                self.kairos.save_now();
                self.window_focus_lost();
            }
            WindowEvent::Occluded(occluded) => {
                if self.hidden != *occluded {
                    self.hidden = *occluded;
                    self.window_activity_changed();
                }
            }
            WindowEvent::Focused(true) => {
                self.focused = true;
                self.window_activity_changed();
                if self.cursor.is_some() {
                    self.cursor_activity(Instant::now());
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn window_focus_lost(&mut self) {
        self.focused = false;
        self.window_activity_changed();
        if self.fabula.active() {
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
