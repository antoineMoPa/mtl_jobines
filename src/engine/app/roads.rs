use super::*;

impl Game {
    pub(super) fn record_road_movement(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.simulation_paused() && self.world.is_driving() {
            let ready = self.live_reload.as_ref().is_some_and(|r| !r.busy());
            self.road_tool.record(
                super::super::road_drawing::Pose {
                    position: self.world.player_position() / METERS_PER_UNIT,
                    yaw: self.world.player_yaw(),
                },
                ready,
            );
        }
    }
    pub(super) fn select_road_end(&mut self, size: [f32; 2], point: [f32; 2]) {
        #[cfg(not(target_arch = "wasm32"))]
        self.road_tool.select(
            self.hermes.city(),
            self.live_reload.as_ref().map(|r| r.path()),
            size,
            point,
            super::super::road_drawing::Pose {
                position: self.world.player_position() / METERS_PER_UNIT,
                yaw: self.world.player_yaw(),
            },
        );
        #[cfg(target_arch = "wasm32")]
        self.road_tool.select(size, point);
    }
}
