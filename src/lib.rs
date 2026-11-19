mod engine;

use mtl_world::web_asset;
use mtl_world::{audio, dynamic_world, hermes, kairos, performance, props, season};

#[cfg(not(target_arch = "wasm32"))]
pub use engine::run;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    start_with_client(None)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start_with_client(rule: Option<js_sys::Function>) -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    engine::start_web(rule).map_err(|error| wasm_bindgen::JsValue::from_str(&error))
}

#[cfg(not(target_arch = "wasm32"))]
type VehicleReturnFn = extern "C" fn(u32, f32, f32) -> f32;

#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn mtl_run_v1(rule: Option<VehicleReturnFn>) -> i32 {
    match engine::run_with_client(rule) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("{error}");
            1
        }
    }
}
