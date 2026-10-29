//! Native platform, frame coordination, cameras and GPU presentation.
mod app;
mod commerce;
mod frame_rate;
mod gpu_profile;
mod hud;
mod lighting;
mod palette;
mod prop_scenery;
mod render;
mod scenery;
mod sky;
mod sky_controls;
#[cfg(not(target_arch = "wasm32"))]
mod sky_probe;
#[cfg(not(target_arch = "wasm32"))]
mod smoke;
mod vehicle_scenery;
#[cfg(not(target_arch = "wasm32"))]
pub use app::run;
#[cfg(target_arch = "wasm32")]
pub use app::start_web;
