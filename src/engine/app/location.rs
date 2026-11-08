use super::*;

impl Game {
    pub(super) fn select_map_point(&mut self, width: f32, height: f32, point: [f32; 2]) {
        let Some(uv) = self.full_map.map_uv(width, height, point) else {
            return;
        };
        match self.full_map.selection {
            hud::MapSelection::Route => {
                let p = self.world.player_position() / METERS_PER_UNIT;
                self.hermes.select_destination(uv, p.to_array());
            }
            hud::MapSelection::Jump | hud::MapSelection::JumpBlocked => {
                let destination = self.hermes.map_world_point(uv);
                match self.world.respawn_at_point(&self.hermes, destination) {
                    Ok(()) => self.finish_location_change(),
                    Err(error) => {
                        self.full_map.selection = hud::MapSelection::JumpBlocked;
                        eprintln!("Cannot jump here: {error}");
                    }
                }
            }
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    pub(super) fn finish_location_change(&mut self) {
        self.orbit = 0.;
        self.zoom = 1.;
        self.camera_pitch = 0.;
        self.hood = false;
        let p = self.world.player_position() / METERS_PER_UNIT;
        let yaw = self.world.player_yaw();
        let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        self.eye = p - forward * 0.15 + Vec3::Z * 0.075;
        self.hermes.update_navigation(p.to_array());
        self.scene_dirty = true;
        self.set_overlay(Overlay::Driving);
    }
}
