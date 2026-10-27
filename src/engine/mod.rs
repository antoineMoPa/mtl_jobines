//! Native platform, frame coordination, cameras and GPU presentation.
mod app;
mod hud;
mod render;
mod scenery;
#[cfg(not(target_arch = "wasm32"))]
pub use app::run;
#[cfg(target_arch = "wasm32")]
pub use app::start_web;
