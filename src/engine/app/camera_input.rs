use super::*;
use winit::{dpi::PhysicalPosition, window::CursorGrabMode};

pub(super) fn chase_eye(
    world: &DynamicWorld,
    hermes: &Hermes,
    player: Vec3,
    mut eye: Vec3,
    check_ground: bool,
) -> Vec3 {
    if check_ground {
        let floor = hermes
            .city()
            .surface_at([eye.x, eye.y], eye.z + 0.005)
            .map_or(-1., |surface| surface.height + 0.015);
        eye.z = eye.z.max(floor);
    }
    world.camera_eye(hermes, player + Vec3::Z * 0.014, eye)
}

pub(super) struct OrbitDrag {
    origin: [f32; 2],
    travel: [f32; 2],
    pub(super) captured: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub(super) enum AimMouse {
    #[default]
    Free,
    Relative,
    Window,
}

impl AimMouse {
    pub(super) fn active(self) -> bool {
        self != Self::Free
    }

    pub(super) fn relative(self) -> bool {
        #[cfg(target_arch = "wasm32")]
        if !web_sys::window()
            .and_then(|w| w.document())
            .is_some_and(|d| d.pointer_lock_element().is_some())
        {
            return false;
        }
        self == Self::Relative
    }
}

impl Game {
    pub(super) fn scene_cursor(&self, armed: bool) -> egui::CursorIcon {
        if let Some(drag) = &self.orbit_drag {
            return if drag.captured {
                egui::CursorIcon::None
            } else {
                egui::CursorIcon::Default
            };
        }
        if armed && !self.simulation_paused() {
            egui::CursorIcon::Crosshair
        } else {
            egui::CursorIcon::Default
        }
    }

    pub(super) fn armed_aim(&self) -> bool {
        !self.simulation_paused()
            && self.world.can_equip_weapon()
            && self.loadout.equipped.is_some()
    }

    pub(super) fn sync_aim_mouse(&mut self) {
        if !self.armed_aim() || mobile_enabled() {
            self.release_aim_mouse();
            return;
        }
        if self.aim_mouse.active() || self.window.is_none() {
            return;
        }
        self.stop_orbit_drag();
        let window = self.window.as_ref().unwrap();
        self.aim_mouse = if window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined))
            .is_ok()
        {
            AimMouse::Relative
        } else {
            AimMouse::Window
        };
        self.cursor = None;
        self.cursor_hidden = true;
        self.cursor_hide_at = None;
        window.set_cursor_visible(false);
    }

    pub(super) fn release_aim_mouse(&mut self) {
        if !self.aim_mouse.active() {
            return;
        }
        if let Some(window) = &self.window {
            let _ = window.set_cursor_grab(CursorGrabMode::None);
        }
        self.aim_mouse = AimMouse::Free;
        self.cursor = None;
        self.show_cursor();
    }

    pub(super) fn aim_motion(&mut self, delta: [f32; 2]) {
        let yaw = self.orbit - delta[0] * AIM_LOOK_SENSITIVITY;
        self.orbit = yaw.sin().atan2(yaw.cos());
        self.camera_pitch =
            (self.camera_pitch + delta[1] * AIM_LOOK_SENSITIVITY).clamp(-1.25, 1.25);
    }

    pub(super) fn scroll_chase_camera(&mut self, steps: f32) {
        let closest = if self.world.is_driving() && self.world.player().is_aircraft() {
            MIN_AIRCRAFT_CHASE_ZOOM
        } else {
            DEFAULT_CHASE_ZOOM
        };
        self.zoom = (self.zoom * (-steps * 0.1).exp()).clamp(closest, 3.);
    }

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

    pub(super) fn release_orbit_drag(&mut self) -> bool {
        let click = self.orbit_drag.is_some() && !self.dragging;
        self.stop_orbit_drag();
        click
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
        if self.aim_mouse.relative() || self.orbit_drag.as_ref().is_some_and(|drag| drag.captured) {
            return; // Captured motion comes exclusively from DeviceEvent::MouseMotion.
        }
        if let Some(previous) = self.cursor {
            let delta = [point[0] - previous[0], point[1] - previous[1]];
            if self.aim_mouse.active() {
                self.aim_motion(delta);
            } else {
                self.orbit_motion(delta);
            }
        }
        self.cursor = Some(point);
    }
}
