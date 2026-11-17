mod engine;

use mtl_world::web_asset;
use mtl_world::{audio, dynamic_world, hermes, kairos, performance, props, season};

#[cfg(not(target_arch = "wasm32"))]
pub use engine::run;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    engine::start_web().map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
