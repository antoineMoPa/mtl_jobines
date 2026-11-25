//! Fixed device budget, selected before the world spawns its first NPCs.
pub fn startup_memory(stage: &str) {
    #[cfg(all(target_arch = "wasm32", feature = "memory-profile"))]
    web_sys::console::info_1(
        &format!(
            "[startup] {stage}: {} MiB WASM",
            core::arch::wasm32::memory_size::<0>() / 16
        )
        .into(),
    );
    #[cfg(not(all(target_arch = "wasm32", feature = "memory-profile")))]
    let _ = stage;
}
pub fn phone() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        thread_local! {
            static PHONE: bool = web_sys::window()
                .and_then(|w| w.match_media("(pointer: coarse) and (max-width: 1024px)").ok().flatten())
                .is_some_and(|query| query.matches());
        }
        PHONE.with(|phone| *phone)
    }
    #[cfg(not(target_arch = "wasm32"))]
    false
}
pub fn npc_budget(desktop: usize) -> usize {
    if phone() {
        desktop / 2
    } else {
        desktop
    }
}
pub fn draw_radius(desktop: f32) -> f32 {
    if phone() {
        desktop * 0.6
    } else {
        desktop
    }
}

/// Per-frame map loading budgets. A frame stops starting new work once its
/// share is spent, nearest work first, and the remainder carries to later frames.
/// Static scenery: vertices uploaded to the GPU for newly needed map draws.
pub fn scenery_upload_vertices() -> usize {
    npc_budget(120_000)
}
/// Props (and signals): new GPU meshes built from world geometry.
pub fn prop_uploads() -> usize {
    npc_budget(16)
}
/// Pedestrians placed by one population pass.
pub fn crowd_spawns() -> usize {
    npc_budget(2).max(1)
}
/// Vehicles placed by one population pass.
pub fn traffic_spawns() -> usize {
    npc_budget(3).max(1)
}
/// Delay before a population pass that stopped at its budget runs again: the very next step.
pub const SPAWN_CONTINUE: f32 = 0.;
