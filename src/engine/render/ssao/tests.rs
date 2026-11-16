#[test]
fn screen_space_shader_validates() {
    for samples in [1, 4] {
        let module = naga::front::wgsl::parse_str(&crate::engine::render::depth_shader(
            include_str!("../ssao.wgsl"),
            samples,
        ))
        .unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[path = "gpu_tests.rs"]
mod gpu_tests;
