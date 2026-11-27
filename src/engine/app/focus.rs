use super::*;
impl Game {
    pub(super) fn window_active(&self) -> bool {
        self.focused && !self.hidden && !self.minimized && !self.suspended
    }

    pub(super) fn can_present_frame(&self) -> bool {
        if cfg!(target_os = "macos") {
            // A Space can reveal a window before it becomes key, or restore
            // focus before winit reports visibility. Metal checks live occlusion
            // on acquisition, so either signal can restart presentation.
            return (self.focused || !self.hidden) && !self.minimized && !self.suspended;
        }
        // The web loader covers the canvas until presentation. A visible tab
        // can finish that first frame before the canvas receives keyboard focus.
        let first_web_frame = cfg!(target_arch = "wasm32") && self.first_frame;
        (self.focused || first_web_frame) && !self.hidden && !self.minimized && !self.suspended
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
        if !self.window_active() {
            self.audio.silence();
            self.keys.clear();
            self.loadout.projection = None;
            self.release_aim_mouse();
            self.stop_orbit_drag();
            self.show_cursor();
            #[cfg(target_arch = "wasm32")]
            self.mobile.clear();
        }
        if self.can_present_frame() {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
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
                } else if self.can_present_frame() {
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::Focused(false) => {
                // winit watches the canvas, while the web game also owns DOM
                // touch controls. A canvas blur inside the focused page is
                // not the application losing focus (notably on Safari).
                #[cfg(target_arch = "wasm32")]
                if web_sys::window()
                    .and_then(|window| window.document())
                    .is_some_and(|document| {
                        !document.hidden() && document.has_focus().unwrap_or(false)
                    })
                {
                    self.keys.clear();
                    self.mobile.clear();
                    self.stop_orbit_drag();
                    self.reset_frame_timing();
                    return true;
                }
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
