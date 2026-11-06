//! Bounded procedural sound scene. Physics/world positions are meters, Z up.
//! Shared Rust synthesis/spatial mix with native and browser CPAL output.
use glam::Vec3;
use opencity::runtime::SurfaceKind;
mod vehicle;
pub(crate) use vehicle::VehicleSound;
pub(crate) use vehicle::{EngineKind, HornKind};

mod output;
mod scene;
#[cfg(any(target_arch = "wasm32", test))]
mod spatial_web;
#[cfg(test)]
mod tests;
pub(crate) const CAR_VOICES: usize = 10;
pub(crate) const IMPACT_VOICES: usize = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Material {
    #[default]
    Hard,
    Body,
    Fence,
    Lamp,
    Timber,
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Impact {
    pub serial: u64,
    pub position: Vec3,
    pub speed: f32,
    pub material: Material,
    pub broken: bool,
    pub age: f32,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Car {
    pub id: usize,
    pub position: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub throttle: f32,
    pub brake: bool,
    pub handbrake: bool,
    pub grounded: bool,
    pub surface: SurfaceKind,
    pub sound: VehicleSound,
    pub wheel_radius: f32,
    pub horn: bool,
    pub player: bool,
    pub engine_on: bool,
}
impl Default for Car {
    fn default() -> Self {
        Self {
            id: usize::MAX,
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            yaw: 0.,
            throttle: 0.,
            brake: false,
            handbrake: false,
            grounded: false,
            surface: SurfaceKind::Asphalt,
            sound: VehicleSound::default(),
            wheel_radius: 0.32,
            horn: false,
            player: false,
            engine_on: false,
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Frame {
    pub cars: [Car; CAR_VOICES],
    pub impacts: [Impact; IMPACT_VOICES],
    pub eye: Vec3,
    pub forward: Vec3,
    pub velocity: Vec3,
    pub foliage: f32,
    pub wind_speed: f32,
    pub settings: Settings,
}
impl Default for Frame {
    fn default() -> Self {
        Self {
            cars: [Car::default(); CAR_VOICES],
            impacts: [Impact::default(); IMPACT_VOICES],
            eye: Vec3::ZERO,
            forward: Vec3::X,
            velocity: Vec3::ZERO,
            foliage: 0.,
            wind_speed: 0.,
            settings: Settings::default(),
        }
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Settings {
    pub master: f32,
    pub engines: f32,
    pub tires: f32,
    pub ambience: f32,
    pub impacts: f32,
    pub horns: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            master: 0.8,
            engines: 1.,
            tires: 0.5,
            ambience: 0.6,
            impacts: 0.65,
            horns: 0.75,
        }
    }
}
impl Settings {
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) {
        egui::Window::new("Sound")
            .open(open)
            .resizable(false)
            .show(ctx, |ui| {
                for (label, value) in [
                    ("Master", &mut self.master),
                    ("Engines", &mut self.engines),
                    ("Horns", &mut self.horns),
                    ("Tires", &mut self.tires),
                    ("Wind and surroundings", &mut self.ambience),
                    ("Collisions and breakage", &mut self.impacts),
                ] {
                    ui.add(egui::Slider::new(value, 0.0..=1.0).text(label));
                }
            });
    }
}

/// Explicit world -> ambisonic axes: right, forward, up. Position and velocity
/// use the same basis; camera rotation itself must never create Doppler.
fn listener_vector(vector: Vec3, forward: Vec3) -> [f32; 3] {
    let length = forward.length();
    let forward = if length > 0.0001 {
        forward / length
    } else {
        Vec3::X
    };
    let horizontal = forward.x.hypot(forward.y).max(0.0001);
    let right = Vec3::new(forward.y / horizontal, -forward.x / horizontal, 0.);
    let up = right.cross(forward);
    [vector.dot(right), vector.dot(forward), vector.dot(up)]
}

#[derive(Default)]
pub(crate) struct Audio {
    backend: Option<output::Backend>,
    attempted: bool,
}
impl Audio {
    /// Discover the native device before the title is interactive. The stream
    /// remains silent until gameplay supplies its first active sound frame.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn prepare(&mut self) {
        self.ensure_backend(true, output::Backend::open);
    }

    fn ensure_backend(
        &mut self,
        active: bool,
        open: impl FnOnce() -> Result<output::Backend, String>,
    ) {
        if active && !self.attempted {
            self.attempted = true;
            match open() {
                Ok(backend) => self.backend = Some(backend),
                Err(error) => eprintln!("Sound unavailable: {error}"),
            }
        }
    }
    /// Browser output must first be unlocked synchronously by a user gesture.
    pub fn user_gesture(&mut self) {
        #[cfg(target_arch = "wasm32")]
        {
            self.ensure_backend(true, output::Backend::open);
            if let Some(backend) = &self.backend {
                backend.resume();
            }
        }
    }
    pub fn update(&mut self, frame: Frame, active: bool) {
        #[cfg(not(target_arch = "wasm32"))]
        self.ensure_backend(active, output::Backend::open);
        if let Some(backend) = &self.backend {
            backend.update(frame, active);
        }
    }
    pub fn silence(&self) {
        if let Some(backend) = &self.backend {
            backend.silence();
        }
    }
}
