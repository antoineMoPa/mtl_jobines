#[test]
fn screen_space_shader_validates() {
    let module = naga::front::wgsl::parse_str(include_str!("../ssao.wgsl")).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[path = "gpu_tests.rs"]
mod gpu_tests;
