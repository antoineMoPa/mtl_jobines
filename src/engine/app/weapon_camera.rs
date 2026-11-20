use super::*;
#[cfg(test)]
use glam::Vec2;

const PIVOT_HEIGHT_M: f32 = 1.25;
const FOLLOW_DISTANCE_M: f32 = 4.2;

/// Aim over the player's head from directly behind the on-foot camera pivot.
pub(super) fn on_foot_view(player: Vec3, forward: Vec3, zoom: f32, pitch: f32) -> (Vec3, Vec3) {
    let direction = forward * pitch.cos() - Vec3::Z * pitch.sin();
    let pivot = player + Vec3::Z * (PIVOT_HEIGHT_M / METERS_PER_UNIT);
    let arm = FOLLOW_DISTANCE_M * zoom / DEFAULT_CHASE_ZOOM / METERS_PER_UNIT;
    let eye = pivot - direction * arm;
    (eye, eye + direction * 0.06)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn on_foot_camera_stays_centered_behind_the_player() {
        for heading in [0., 1.4, -2.7] {
            let forward = Vec2::from_angle(heading).extend(0.);
            for pitch in [-1.1, 0., 0.9] {
                let (eye, look) = on_foot_view(Vec3::ZERO, forward, DEFAULT_CHASE_ZOOM, pitch);
                assert!(eye.dot(forward) < 0.);
                assert!(eye.dot(forward.cross(Vec3::Z)).abs() < 1e-6);
                let pivot = Vec3::Z * (PIVOT_HEIGHT_M / METERS_PER_UNIT);
                assert!((pivot.distance(eye) * METERS_PER_UNIT - FOLLOW_DISTANCE_M).abs() < 1e-4);
                assert!((look - eye).cross(pivot - eye).length() < 1e-7);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn on_foot_view_is_the_same_before_during_and_after_firing() {
        let mut game = Game::new(Hermes::test_map());
        let walker = game.world.prepare_player_pedestrian_collision(&game.hermes);
        let target = walker + Vec3::Z * 1.25;
        game.set_overlay(Overlay::Driving);
        game.world.set_zombie_mode(game.hermes.city(), true);
        game.renderer = Some(pollster::block_on(Renderer::new(None, 960, 640)).unwrap());
        let p = game.world.player_position() / METERS_PER_UNIT;
        let delta = target - p * METERS_PER_UNIT;
        let distance = delta.truncate().length();
        let yaw = delta.y.atan2(delta.x);
        let pitch = -(delta.z - PIVOT_HEIGHT_M).atan2(distance);
        game.orbit = yaw - game.world.player_yaw();
        game.camera_pitch = pitch;
        let pose = |game: &Game| {
            on_foot_view(
                game.world.player_position() / METERS_PER_UNIT,
                Vec2::from_angle(game.world.player_yaw() + game.orbit).extend(0.),
                game.zoom,
                game.camera_pitch,
            )
        };
        let unarmed = pose(&game);
        game.loadout.equip(Some(weapons::Weapon::Rifle));
        let (eye, look) = pose(&game);
        assert_eq!(unarmed, (eye, look));
        let projection = game.renderer.as_ref().unwrap().view_projection(eye, look);
        let screen = projection.project_point3(target / METERS_PER_UNIT);
        assert!(screen.truncate().length() < 0.001);
        game.cursor = None;
        game.loadout.projection = Some(projection.inverse());
        game.world.aim_weapon(Some(target), true);
        game.fire_weapon();
        assert_eq!(pose(&game), (eye, look));
        game.simulate(DT);
        game.game_time += 10.;
        assert_eq!(pose(&game), (eye, look));
        let renderer = game.renderer.as_mut().unwrap();
        if let Some(path) = std::env::var_os("MTL_SHOULDER_QA_PNG") {
            renderer.zombie_mode = true;
            let scenery = Scenery::new(renderer, &mut game.hermes);
            let mut actors = VehicleScenery::new(renderer);
            actors.update(renderer, &game.world);
            let mut meshes = scenery.visible([p.x, p.y]);
            meshes.extend(actors.visible());
            game.loadout.effects.step(0.3);
            meshes.extend(game.loadout.visible(
                renderer,
                &game.world,
                target,
                Quat::IDENTITY,
                &mut Vec::new(),
            ));
            let image = renderer
                .capture_png(|view| {
                    renderer.render_scene(view, &meshes, eye, look, 10., None, None);
                    let ctx = egui::Context::default();
                    let output = ctx.run_ui(Default::default(), |ui| {
                        crate::engine::palette::weapon_reticle(ui.ctx(), renderer);
                    });
                    let mut painter = crate::engine::palette::Painter::new(
                        &renderer.device,
                        renderer.config.format.add_srgb_suffix(),
                    );
                    painter.paint(
                        renderer,
                        view,
                        ctx.tessellate(output.shapes, output.pixels_per_point),
                        output.textures_delta,
                        output.pixels_per_point,
                    );
                })
                .unwrap();
            std::fs::write(path, image).unwrap();
        }
        game.fire_weapon();
        assert_eq!(game.world.pedestrian_reactions()[1], 1);
        game.loadout.equip(None);
        assert_eq!(pose(&game), unarmed);
    }
}
