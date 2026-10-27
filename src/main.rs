#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), String> {
    mtl_jobines::run()
}

#[cfg(target_arch = "wasm32")]
fn main() {}
