//! Touch input stays separate from physical keys; only the wasm shell can enable it.
use super::*;

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
#[derive(Clone, Copy, Debug)]
pub enum MobileAction {
    Forward,
    Backward,
    Left,
    Right,
    Run,
    Jump,
    Vehicle,
    Map,
    Phone,
    Menu,
    Door,
}
impl MobileAction {
    fn key(self) -> Option<KeyCode> {
        match self {
            Self::Forward => Some(KeyCode::ArrowUp),
            Self::Backward => Some(KeyCode::ArrowDown),
            Self::Left => Some(KeyCode::ArrowLeft),
            Self::Right => Some(KeyCode::ArrowRight),
            Self::Run => Some(KeyCode::ShiftLeft),
            Self::Jump => Some(KeyCode::Space),
            Self::Vehicle | Self::Map | Self::Phone | Self::Menu | Self::Door => None,
        }
    }
}
#[derive(Default)]
pub(super) struct Input {
    keys: HashSet<KeyCode>,
    #[cfg(target_arch = "wasm32")]
    view: Option<(bool, bool)>,
    #[cfg(target_arch = "wasm32")]
    scene_tap: Option<(u64, [f32; 2], Overlay)>,
}
impl Input {
    pub(super) fn clear(&mut self) {
        self.keys.clear();
        #[cfg(target_arch = "wasm32")]
        {
            self.scene_tap = None;
        }
    }
    fn hold(&mut self, action: MobileAction, pressed: bool, active: bool) {
        if let Some(key) = action.key() {
            if pressed && active {
                self.keys.insert(key);
            } else {
                self.keys.remove(&key);
            }
        }
    }
    pub(super) fn with_keyboard(&self, keyboard: &HashSet<KeyCode>) -> HashSet<KeyCode> {
        self.keys.union(keyboard).copied().collect()
    }
    #[cfg(target_arch = "wasm32")]
    pub(super) fn with_aircraft_keyboard(&self, keyboard: &HashSet<KeyCode>) -> HashSet<KeyCode> {
        let mut keys = keyboard.clone();
        for key in &self.keys {
            keys.extend(match key {
                KeyCode::ArrowUp => &[KeyCode::KeyW][..],
                KeyCode::ArrowDown => &[KeyCode::KeyS, KeyCode::ArrowUp],
                KeyCode::ArrowLeft => &[KeyCode::KeyA, KeyCode::ArrowLeft],
                KeyCode::ArrowRight => &[KeyCode::KeyD, KeyCode::ArrowRight],
                KeyCode::ShiftLeft => &[KeyCode::ArrowDown],
                KeyCode::Space => &[KeyCode::Space],
                _ => &[],
            });
        }
        keys
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use super::*;
    use std::cell::{Cell, RefCell};
    use wasm_bindgen::prelude::*;
    thread_local! {
        static PROXY: RefCell<Option<winit::event_loop::EventLoopProxy<GameEvent>>> = const { RefCell::new(None) };
        static ENABLED: Cell<bool> = const { Cell::new(false) };
    }
    pub(in super::super) enum Event {
        Layout(bool),
        Action(MobileAction, bool),
        Reset,
    }
    pub(in super::super) fn connect(proxy: winit::event_loop::EventLoopProxy<GameEvent>) {
        PROXY.with(|slot| *slot.borrow_mut() = Some(proxy));
    }
    pub(in super::super) fn enabled() -> bool {
        ENABLED.get()
    }
    fn send(event: Event) {
        PROXY.with(|slot| {
            if let Some(proxy) = slot.borrow().as_ref() {
                let _ = proxy.send_event(GameEvent::Mobile(event));
            }
        });
    }
    #[wasm_bindgen]
    pub fn mobile_set_enabled(enabled: bool) {
        send(Event::Layout(enabled));
    }
    #[wasm_bindgen]
    pub fn mobile_input(action: MobileAction, pressed: bool) {
        send(Event::Action(action, pressed));
    }
    #[wasm_bindgen]
    pub fn mobile_reset() {
        send(Event::Reset);
    }

    fn canvas_attribute(name: &str, value: &str) {
        if let Some(canvas) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("game"))
        {
            let _ = canvas.set_attribute(name, value);
        }
    }
    impl Game {
        pub(in super::super) fn sync_mobile_controls(&mut self) {
            let active = enabled()
                && !self.first_frame
                && self.auto_drive.is_none()
                && !self.simulation_paused()
                && self.world.arrest_progress().is_none();
            let driving = self.world.is_driving();
            if self.mobile.view != Some((active, driving)) {
                self.mobile.clear();
                self.mobile.view = Some((active, driving));
                canvas_attribute(
                    "data-mobile-mode",
                    if !active {
                        "hidden"
                    } else if driving {
                        "driving"
                    } else {
                        "walking"
                    },
                );
            }
        }
        pub(in super::super) fn mobile_event(&mut self, event: Event) {
            match event {
                Event::Layout(value) => {
                    ENABLED.set(value);
                    self.mobile.clear();
                }
                Event::Reset => self.mobile.clear(),
                Event::Action(action, pressed) => {
                    let active = enabled()
                        && !self.first_frame
                        && !self.simulation_paused()
                        && self.world.arrest_progress().is_none();
                    self.mobile.hold(action, pressed, active);
                    if pressed && active {
                        self.auto_drive = None;
                        self.audio.user_gesture();
                        match action {
                            MobileAction::Vehicle => {
                                self.mobile.clear();
                                if self.world.toggle_vehicle(&self.hermes) {
                                    self.reset_boarding_camera();
                                }
                            }
                            MobileAction::Door => {
                                self.world.toggle_door();
                            }
                            MobileAction::Map => self.set_overlay(Overlay::Map),
                            MobileAction::Menu => self.set_overlay(Overlay::Menu),
                            MobileAction::Phone => {
                                if let Some(palette) = &mut self.palette {
                                    palette.open_job_feed();
                                }
                                self.keys.clear();
                                self.mobile.clear();
                                self.stop_orbit_drag();
                                self.accumulator = 0.;
                                self.audio.silence();
                                self.show_cursor();
                            }
                            _ => {}
                        }
                    }
                }
            }
            self.sync_mobile_controls();
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }
        pub(in super::super) fn mobile_scene_touch(&mut self, touch: winit::event::Touch) {
            use winit::event::TouchPhase;
            if self.overlay != Overlay::Map && self.simulation_paused() {
                return;
            }
            let point = [touch.location.x as f32, touch.location.y as f32];
            match touch.phase {
                TouchPhase::Started if self.mobile.scene_tap.is_none() => {
                    self.mobile.scene_tap = Some((touch.id, point, self.overlay))
                }
                TouchPhase::Cancelled => self.mobile.scene_tap = None,
                TouchPhase::Ended => {
                    let Some((id, start, overlay)) = self.mobile.scene_tap.take() else {
                        return;
                    };
                    let Some(window) = &self.window else {
                        return;
                    };
                    let slop = 12. * window.scale_factor() as f32;
                    if id != touch.id
                        || overlay != self.overlay
                        || (point[0] - start[0]).abs() > slop
                        || (point[1] - start[1]).abs() > slop
                    {
                        return;
                    }
                    if self.overlay == Overlay::Driving {
                        if self.armed_aim() {
                            self.fire_weapon();
                        } else if self.loadout.equipped.is_none() {
                            self.world.start_punch();
                        }
                        return;
                    }
                    let size = window.inner_size();
                    let (w, h) = (size.width as f32, size.height as f32);
                    if hud::contains(hud::full_map_close_rect(w), point)
                        || !hud::contains([20., 20., w - 40., h - 40.], point)
                    {
                        self.start_playing();
                    } else {
                        self.select_map_point(w, h, point);
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                _ => {}
            }
        }
    }
}
#[cfg(target_arch = "wasm32")]
pub(super) use browser::{connect, enabled, Event};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_never_enables_touch_controls() {
        assert!(!super::super::mobile_enabled());
    }

    #[test]
    fn simultaneous_touch_actions_keep_keyboard_ownership() {
        let mut input = Input::default();
        input.hold(MobileAction::Forward, true, true);
        input.hold(MobileAction::Left, true, true);
        input.hold(MobileAction::Run, true, true);
        let keyboard = HashSet::from([KeyCode::ArrowUp]);
        let (car, walk) = movement_controls(&input.with_keyboard(&keyboard), 0.);
        assert_eq!(car.throttle, 1.);
        assert_eq!(car.steering, 1.);
        assert!(walk.run);
        input.hold(MobileAction::Forward, false, true);
        assert!(input.with_keyboard(&keyboard).contains(&KeyCode::ArrowUp));
        input.clear();
        assert_eq!(input.with_keyboard(&keyboard), keyboard);
        assert!(input.with_keyboard(&HashSet::new()).is_empty());
    }
    #[test]
    fn paused_inputs_are_rejected_and_releases_always_clear() {
        let mut input = Input::default();
        input.hold(MobileAction::Forward, true, false);
        assert!(input.keys.is_empty());
        input.hold(MobileAction::Jump, true, true);
        let (car, walk) = movement_controls(&input.keys, 10.);
        assert!(car.handbrake && walk.jump);
        input.hold(MobileAction::Jump, false, false);
        assert!(input.keys.is_empty());
        for action in [
            MobileAction::Vehicle,
            MobileAction::Door,
            MobileAction::Map,
            MobileAction::Phone,
            MobileAction::Menu,
        ] {
            input.hold(action, true, true);
        }
        assert!(input.keys.is_empty());
        input.hold(MobileAction::Backward, true, true);
        input.hold(MobileAction::Right, true, true);
        let (car, _) = movement_controls(&input.keys, 10.);
        assert!(car.brake);
        assert_eq!(car.steering, -1.);
    }
}
