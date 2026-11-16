use super::*;
use winit::{dpi::PhysicalPosition, window::CursorGrabMode};

pub(super) struct OrbitDrag {
    origin: [f32; 2],
    travel: [f32; 2],
    pub(super) captured: bool,
}

impl Game {
    pub(super) fn begin_orbit_drag(&mut self, origin: [f32; 2]) {
        if self.simulation_paused() {
            return;
        }
        let captured = self.window.as_ref().is_some_and(|window| {
            window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
                .is_ok()
        });
        self.orbit_drag = Some(OrbitDrag {
            origin,
            travel: [0.; 2],
            captured,
        });
        self.cursor_hide_at = None;
        if captured {
            self.cursor_hidden = true;
            if let Some(window) = &self.window {
                window.set_cursor_visible(false);
            }
        }
    }

    pub(super) fn stop_orbit_drag(&mut self) {
        self.dragging = false;
        if let Some(drag) = self.orbit_drag.take() {
            if drag.captured {
                if let Some(window) = &self.window {
                    let _ = window.set_cursor_grab(CursorGrabMode::None);
                    let _ = window
                        .set_cursor_position(PhysicalPosition::new(drag.origin[0], drag.origin[1]));
                }
                self.cursor = Some(drag.origin);
            }
            self.show_cursor();
        }
    }

    pub(super) fn orbit_motion(&mut self, delta: [f32; 2]) {
        if self.simulation_paused() {
            self.stop_orbit_drag();
            return;
        }
        let Some(drag) = &mut self.orbit_drag else {
            return;
        };
        if !self.dragging {
            drag.travel[0] += delta[0];
            drag.travel[1] += delta[1];
            let scale = self
                .window
                .as_ref()
                .map_or(1., |window| window.scale_factor() as f32);
            if drag.travel[0].hypot(drag.travel[1]) < 4. * scale {
                return;
            }
            self.dragging = true;
            // Consume the threshold without making the camera jump.
            return;
        }
        self.orbit -= delta[0] * CAMERA_ORBIT_SENSITIVITY;
        // Equivalent headings stay bounded without a hard stop at +/-180 degrees.
        self.orbit = (self.orbit + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        self.camera_pitch =
            (self.camera_pitch + delta[1] * CAMERA_PITCH_SENSITIVITY).clamp(-0.3, 1.);
    }

    pub(super) fn move_camera_with_cursor(&mut self, point: [f32; 2]) {
        if self.orbit_drag.as_ref().is_some_and(|drag| drag.captured) {
            return; // Captured motion comes exclusively from DeviceEvent::MouseMotion.
        }
        if let Some(previous) = self.cursor {
            self.orbit_motion([point[0] - previous[0], point[1] - previous[1]]);
        }
        self.cursor = Some(point);
    }
}
