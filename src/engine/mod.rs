//! Native platform, frame coordination, cameras and GPU presentation.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod aircraft_tests;
mod airport_lights;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod alpine_gpu_tests;
mod app;
#[cfg(not(target_arch = "wasm32"))]
mod boot;
#[cfg(not(target_arch = "wasm32"))]
mod bug_report;
#[cfg(not(target_arch = "wasm32"))]
mod buildings;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod chairlift_gpu_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod collecteur_tests;
mod commerce;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod contact_tests;
mod contact_texture;
mod crash;
#[cfg(not(target_arch = "wasm32"))]
mod expansion;
mod fabula;
mod foot_motion;
mod frame_rate;
mod gpu_profile;
mod gpu_trace;
mod graffiti;
mod grass;
mod help;
mod hud;
mod impact;
mod job_feed;
mod lighting;
mod local_lights;
mod map_ui;
mod model_textures;
mod muzzle;
mod owe;
mod palette;
mod pedestrian_scenery;
mod prop_scenery;
#[cfg(not(target_arch = "wasm32"))]
mod props;
mod render;
#[cfg(not(target_arch = "wasm32"))]
mod report_replay;
mod scenery;
#[cfg(not(target_arch = "wasm32"))]
mod screen_capture;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod ski_set_gpu_tests;
mod sky;
mod sky_controls;
#[cfg(not(target_arch = "wasm32"))]
mod sky_probe;
#[cfg(not(target_arch = "wasm32"))]
mod smoke;
mod snow;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod snowmobile_gpu_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod snowplow_tests;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod surface_tests;
mod tooltip;
mod vehicle_scenery;
mod vehicle_textures;
#[cfg(not(target_arch = "wasm32"))]
pub use app::run;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use app::run_with_client;
#[cfg(target_arch = "wasm32")]
pub use app::start_web;

#[cfg(test)]
mod tree_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod bicycle_tests;

mod sound_settings;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod metro_tests;

mod wildlife;
