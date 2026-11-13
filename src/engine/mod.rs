//! Native platform, frame coordination, cameras and GPU presentation.
mod app;
#[cfg(not(target_arch = "wasm32"))]
mod bug_report;
#[cfg(not(target_arch = "wasm32"))]
mod buildings;
mod commerce;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod contact_tests;
mod contact_texture;
#[cfg(not(target_arch = "wasm32"))]
mod expansion;
mod frame_rate;
mod gpu_profile;
mod graffiti;
mod hud;
mod lighting;
mod local_lights;
mod palette;
mod prop_scenery;
mod render;
#[cfg(not(target_arch = "wasm32"))]
mod report_replay;
mod scenery;
#[cfg(not(target_arch = "wasm32"))]
mod screen_capture;
mod sky;
mod sky_controls;
#[cfg(not(target_arch = "wasm32"))]
mod sky_probe;
#[cfg(not(target_arch = "wasm32"))]
mod smoke;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod surface_tests;
mod vehicle_scenery;
#[cfg(not(target_arch = "wasm32"))]
pub use app::run;
#[cfg(target_arch = "wasm32")]
pub use app::start_web;
