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
    use std::{collections::HashMap, sync::OnceLock};
    use wasm_bindgen::prelude::*;

    static ASSETS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();
    const PATHS: &str = include_str!("../web/assets.list");

    #[wasm_bindgen]
    pub fn asset_paths() -> String {
        PATHS.to_owned()
    }

    #[wasm_bindgen]
    pub fn install_assets(values: js_sys::Array) -> Result<(), JsValue> {
        if values.length() as usize != PATHS.lines().count() {
            return Err(JsValue::from_str(
                "Web asset count does not match the build",
            ));
        }
        let mut assets = HashMap::with_capacity(values.length() as usize);
        let mut opencity_assets = HashMap::new();
        for (path, value) in PATHS.lines().zip(values.iter()) {
            let bytes = js_sys::Uint8Array::new(&value).to_vec();
            if path.starts_with("opencity/") {
                opencity_assets.insert(path.to_owned(), bytes);
            } else {
                assets.insert(path.to_owned(), bytes);
            }
        }
        opencity::web_assets::install(opencity_assets)
            .map_err(|error| JsValue::from_str(&error))?;
        ASSETS
            .set(assets)
            .map_err(|_| JsValue::from_str("Web assets were already installed"))
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
