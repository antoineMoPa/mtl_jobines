//! Fixed device budget, selected before the world spawns its first NPCs.
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
