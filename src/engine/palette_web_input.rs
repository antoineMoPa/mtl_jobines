//! Browser input bridge: egui-winit 0.36's native dropped-file adapter does not
//! compile on wasm. Keep this bridge limited to the palette's keyboard/pointer UI.
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use winit::{
    event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent},
    keyboard::{Key as WinitKey, NamedKey},
    window::Window,
};
pub struct Response {
    pub repaint: bool,
}
pub struct Input {
    raw: egui::RawInput,
    pointer: Pos2,
    modifiers: Modifiers,
    started: web_time::Instant,
}
impl Input {
    pub fn new(
        _: egui::Context,
        _: egui::ViewportId,
        _: &Window,
        _: Option<f32>,
        _: Option<winit::window::Theme>,
        _: Option<usize>,
    ) -> Self {
        Self {
            raw: Default::default(),
            pointer: Pos2::ZERO,
            modifiers: Modifiers::default(),
            started: web_time::Instant::now(),
        }
    }
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> Response {
        let scale = window.scale_factor() as f32;
        let modifiers = self.modifiers;
        let mut repaint = true;
        match event {
            WindowEvent::ModifiersChanged(value) => {
                let state = value.state();
                self.modifiers = Modifiers {
                    alt: state.alt_key(),
                    ctrl: state.control_key(),
                    shift: state.shift_key(),
                    mac_cmd: state.super_key(),
                    command: state.control_key() || state.super_key(),
                };
                self.raw
                    .events
                    .push(Event::ModifiersChanged(self.modifiers));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let key = match &event.logical_key {
                    WinitKey::Named(named) => match named {
                        NamedKey::ArrowUp => Some(Key::ArrowUp),
                        NamedKey::ArrowDown => Some(Key::ArrowDown),
                        NamedKey::ArrowLeft => Some(Key::ArrowLeft),
                        NamedKey::ArrowRight => Some(Key::ArrowRight),
                        NamedKey::Enter => Some(Key::Enter),
                        NamedKey::Escape => Some(Key::Escape),
                        NamedKey::Backspace => Some(Key::Backspace),
                        NamedKey::Delete => Some(Key::Delete),
                        NamedKey::Home => Some(Key::Home),
                        NamedKey::End => Some(Key::End),
                        NamedKey::Tab => Some(Key::Tab),
                        _ => None,
                    },
                    WinitKey::Character(text) => Key::from_name(text.as_str()),
                    _ => None,
                };
                if let Some(key) = key {
                    self.raw.events.push(Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: event.repeat,
                        modifiers,
                    });
                }
                if pressed && !modifiers.command && !modifiers.ctrl {
                    if let Some(text) = &event.text {
                        if !text.chars().any(char::is_control) {
                            self.raw.events.push(Event::Text(text.to_string()));
                        }
                    }
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => self.raw.events.push(Event::Text(text.clone())),
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = egui::pos2(position.x as f32 / scale, position.y as f32 / scale);
                self.raw.events.push(Event::PointerMoved(self.pointer));
            }
            WindowEvent::CursorLeft { .. } => self.raw.events.push(Event::PointerGone),
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => Some(PointerButton::Primary),
                    MouseButton::Right => Some(PointerButton::Secondary),
                    MouseButton::Middle => Some(PointerButton::Middle),
                    _ => None,
                };
                if let Some(button) = button {
                    self.raw.events.push(Event::PointerButton {
                        pos: self.pointer,
                        button,
                        pressed: *state == ElementState::Pressed,
                        modifiers,
                    });
                }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let (unit, delta) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (egui::MouseWheelUnit::Line, egui::vec2(*x, *y))
                    }
                    MouseScrollDelta::PixelDelta(p) => (
                        egui::MouseWheelUnit::Point,
                        egui::vec2(p.x as f32 / scale, p.y as f32 / scale),
                    ),
                };
                self.raw.events.push(Event::MouseWheel {
                    unit,
                    phase: match phase {
                        winit::event::TouchPhase::Started => egui::TouchPhase::Start,
                        winit::event::TouchPhase::Moved => egui::TouchPhase::Move,
                        winit::event::TouchPhase::Ended => egui::TouchPhase::End,
                        winit::event::TouchPhase::Cancelled => egui::TouchPhase::Cancel,
                    },
                    delta,
                    modifiers,
                });
            }
            WindowEvent::Focused(focused) => {
                if !focused {
                    self.modifiers = Default::default();
                    self.raw
                        .events
                        .push(Event::ModifiersChanged(self.modifiers));
                }
                self.raw.focused = *focused;
                self.raw.events.push(Event::WindowFocused(*focused));
            }
            _ => repaint = false,
        }
        Response { repaint }
    }
    pub fn take_egui_input(&mut self, window: &Window) -> egui::RawInput {
        let scale = window.scale_factor() as f32;
        let size = window.inner_size();
        self.raw.screen_rect = Some(egui::Rect::from_min_size(
            Pos2::ZERO,
            egui::vec2(size.width as f32 / scale, size.height as f32 / scale),
        ));
        self.raw
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(scale);
        self.raw.time = Some(self.started.elapsed().as_secs_f64());
        self.raw.take()
    }
    pub fn handle_platform_output(&mut self, window: &Window, output: egui::PlatformOutput) {
        window.set_ime_allowed(output.ime.is_some());
    }
}
