mod dynamic_world;
mod engine;
mod hermes;

#[cfg(not(target_arch = "wasm32"))]
pub use engine::run;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    engine::start_web().map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}
