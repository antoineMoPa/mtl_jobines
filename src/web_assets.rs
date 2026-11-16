//! Browser assets served as separate static files. Native builds keep embedded data.

#[macro_export]
macro_rules! web_asset {
    ($source:expr, $url:expr) => {{
        #[cfg(not(target_arch = "wasm32"))]
        {
            include_bytes!($source).as_slice()
        }
        #[cfg(target_arch = "wasm32")]
        {
            $crate::web_assets::get($url)
        }
    }};
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::{cell::RefCell, collections::HashMap, sync::OnceLock};
    use wasm_bindgen::prelude::*;

    static ASSETS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();
    const PATHS: &str = include_str!("../web/assets.list");

    #[wasm_bindgen]
    pub fn asset_paths() -> String {
        PATHS.to_owned()
    }

    thread_local! {
        static PENDING: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
    }

    /// Transfer each response before fetching the next, avoiding a JS copy of the whole catalog.
    #[wasm_bindgen]
    pub fn install_asset(path: String, value: js_sys::Uint8Array) -> Result<(), JsValue> {
        if ASSETS.get().is_some() || !PATHS.lines().any(|expected| expected == path) {
            return Err(JsValue::from_str("Unexpected web asset"));
        }
        PENDING.with(|pending| {
            let mut pending = pending.borrow_mut();
            if pending.contains_key(&path) {
                return Err(JsValue::from_str("Duplicate web asset"));
            }
            pending.insert(path, value.to_vec());
            Ok(())
        })
    }

    #[wasm_bindgen]
    pub fn discard_pending_assets() {
        PENDING.with(|pending| pending.borrow_mut().clear());
    }

    #[wasm_bindgen]
    pub fn install_assets() -> Result<(), JsValue> {
        PENDING.with(|pending| {
            let mut pending = pending.borrow_mut();
            if pending.len() != PATHS.lines().count() {
                return Err(JsValue::from_str(
                    "Web asset count does not match the build",
                ));
            }
            let mut assets = HashMap::new();
            let mut opencity_assets = HashMap::new();
            for (path, bytes) in pending.drain() {
                if path.starts_with("opencity/") {
                    opencity_assets.insert(path, bytes);
                } else {
                    assets.insert(path, bytes);
                }
            }
            opencity::web_assets::install(opencity_assets)
                .map_err(|error| JsValue::from_str(&error))?;
            ASSETS
                .set(assets)
                .map_err(|_| JsValue::from_str("Web assets were already installed"))
        })
    }

    pub fn get(path: &str) -> &'static [u8] {
        ASSETS
            .get()
            .expect("Web assets must be loaded before starting the game")
            .get(path)
            .unwrap_or_else(|| panic!("Missing web asset: {path}"))
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser::get;
