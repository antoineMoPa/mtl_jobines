//! Native, opt-in OpenTelemetry capture shared by the game and world simulation.
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
pub struct InactiveSpan;

#[cfg(target_arch = "wasm32")]
#[inline]
pub fn span(_: &'static str) -> InactiveSpan {
    InactiveSpan
}

#[cfg(target_arch = "wasm32")]
#[inline]
pub fn frame() {}
