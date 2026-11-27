use super::*;

#[test]
fn busted_camera_slides_from_the_playing_view_and_finishes_on_the_arrest() {
    let focus = Vec3::new(1.02, 1., 0.015);
    let start = (Vec3::new(0.94, 1., 0.03), Vec3::new(1.05, 1., 0.015));
    assert_eq!(arrest_view(start, focus, Vec3::X, 0.), start);
    let (middle, _) = arrest_view(start, focus, Vec3::X, 0.5);
    let (end, aim) = arrest_view(start, focus, Vec3::X, 1.);
    assert!(middle.distance(start.0.lerp(end, 0.5)) < 0.00001);
    assert!((end.y - start.0.y).abs() * METERS_PER_UNIT > 3.);
    assert!(aim.distance(focus) < 0.00001);
}

#[test]
fn taxi_aim_eases_across_the_yaw_wrap_at_any_frame_rate() {
    let start = 179_f32.to_radians();
    let nose = (-171_f32).to_radians();
    let first = taxi_aim_yaw(start, nose, 1. / 30.);
    assert!(first > start && first < start + 2_f32.to_radians());
    let after_one_second =
        |fps| (0..fps).fold(start, |yaw, _| taxi_aim_yaw(yaw, nose, 1. / fps as f32));
    let yaw = after_one_second(30);
    assert!((yaw - after_one_second(120)).abs() < 1e-5);
    assert!((yaw - 189_f32.to_radians()).abs() < 0.2_f32.to_radians());
}

#[test]
fn aircraft_default_chase_frames_the_airframe_and_the_horizon() {
    let mut game = Game::new(Hermes::test_map());
    for (id, def) in crate::dynamic_world::catalog::definitions()
        .iter()
        .enumerate()
    {
        if def.config.category != "Aircraft" {
            continue;
        }
        game.world.switch_vehicle(id);
        game.finish_location_change();
        let plane = game.world.player();
        let forward = Vec3::new(plane.yaw.cos(), plane.yaw.sin(), 0.);
        let offset = (game.eye - plane.position / METERS_PER_UNIT) * METERS_PER_UNIT;
        let length = plane.length();
        assert!((1.35..1.5).contains(&(-offset.dot(forward) / length)));
        let (_, aim) = aircraft_chase(plane.length(), forward, game.zoom, 0.);
        assert!(aim.dot(forward) * METERS_PER_UNIT > plane.length() * 2.);
        let sight = aim * METERS_PER_UNIT - offset;
        assert!(sight.z.atan2(sight.truncate().length()).abs() < 0.055);
        game.scroll_chase_camera(1.);
        assert!(game.zoom < AIRCRAFT_CHASE_ZOOM);
        game.scroll_chase_camera(100.);
        let (closest, _) = aircraft_chase(length, forward, game.zoom, 0.);
        assert!((0.55..0.60).contains(&(-closest.dot(forward) * METERS_PER_UNIT / length)));
        game.scroll_chase_camera(-1.);
        assert!(game.zoom > MIN_AIRCRAFT_CHASE_ZOOM);
    }
}

#[test]
fn large_vehicle_chase_is_farther_in_free_driving_and_preserves_zoom() {
    let mut game = Game::new(Hermes::test_map());
    for research_id in ["vehicle-j02", "vehicle-j03"] {
        let model = crate::dynamic_world::catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == research_id)
            .unwrap();
        assert!(game.world.switch_vehicle(model));
        game.finish_location_change();
        assert!(game.kairos.active_id().is_none());
        let p = game.world.player_position() / METERS_PER_UNIT;
        assert!(game.eye.truncate().distance(p.truncate()) * METERS_PER_UNIT > 10.);
        assert_eq!(game.zoom, DEFAULT_CHASE_ZOOM);
        let old_distance = game.eye.distance(p);
        game.scroll_chase_camera(-2.);
        let car = game.world.player();
        let forward = Vec3::new(car.yaw.cos(), car.yaw.sin(), 0.);
        let (eye, _) = vehicle_chase(car, p, forward, forward, game.zoom, 0.);
        assert!(eye.distance(p) > old_distance);
    }
}

#[test]
fn baggage_chase_frames_the_train_without_a_mission_and_shrinks_when_detached() {
    let h = Hermes::test_map_from_source("pad=concrete(-3,-3,3,-3,3,3,-3,3); elevation(pad,0.122001); player_start(-1,0,0); parked_vehicle(\"vehicle-airport-baggage-tractor\",0,0,90); parked_vehicle(\"vehicle-air-estuaire\",2,0,-90); mission_baggage_dispatch=map_point(0,1); mission_baggage_aircraft=map_point(0,-1);");
    let mut game = Game::new(h);
    game.set_overlay(Overlay::Driving);
    game.world.begin_baggage_shift(&game.hermes).unwrap();
    assert!(game.world.approach_baggage_tug(&game.hermes));
    game.world.end_baggage_shift();
    assert!(game.world.toggle_vehicle(&game.hermes));
    for _ in 0..180 {
        game.simulate(1. / 60.);
    }
    assert!(game.kairos.active_id().is_none());
    game.finish_location_change();
    let p = game.world.player_position() / METERS_PER_UNIT;
    let full_distance = game.eye.truncate().distance(p.truncate()) * METERS_PER_UNIT;
    assert!(full_distance > 20.);
    while game.world.detach_baggage_cart() {}
    assert_eq!(game.world.player().towing_length(), 0.);
    game.finish_location_change();
    assert!(game.eye.truncate().distance(p.truncate()) * METERS_PER_UNIT < 10.);
}

#[test]
fn fast_car_render_pose_advances_between_physics_steps() {
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    let current = game.world.player().position;
    game.previous_car_position = current - Vec3::X * 0.4;
    game.accumulator = DT * 0.5;
    let (position, orientation, yaw) = game.player_render_pose();
    assert!((position.x - (current.x - 0.2)).abs() < 1e-5);
    assert_eq!(position.y, current.y);
    assert_eq!(orientation, game.world.player().orientation());
    assert_eq!(yaw, game.world.player().yaw);
}

#[test]
fn reported_close_chase_view_and_pointer_orbit() {
    let mut game = Game::new(Hermes::test_map());
    let player = game.world.player_position() / METERS_PER_UNIT;
    let distance = game.eye.truncate().distance(player.truncate());
    assert!((distance - 0.063).abs() < 0.0001);
    assert!((game.eye.z - player.z - 0.0255).abs() < 0.0001);
    assert_eq!(game.zoom, DEFAULT_CHASE_ZOOM);

    game.set_overlay(Overlay::Driving);
    game.move_camera_with_cursor([100., 100.]);
    assert_eq!(game.orbit, 0.);
    game.move_camera_with_cursor([110., 100.]);
    assert_eq!(game.orbit, 0., "hovering must not orbit");
    game.begin_orbit_drag([110., 100.]);
    game.move_camera_with_cursor([112., 100.]);
    assert!(!game.dragging, "a click has a small movement tolerance");
    assert_eq!(game.orbit, 0.);
    game.move_camera_with_cursor([120., 100.]);
    assert!(game.dragging);
    game.move_camera_with_cursor([160., 140.]);
    assert!((game.orbit + 0.2).abs() < 0.0001);
    assert!((game.camera_pitch - 0.12).abs() < 0.0001);

    game.set_overlay(Overlay::Map);
    game.move_camera_with_cursor([200., 200.]);
    assert!((game.orbit + 0.2).abs() < 0.0001);
    assert!((game.camera_pitch - 0.12).abs() < 0.0001);
    game.set_overlay(Overlay::Driving);
    game.move_camera_with_cursor([210., 200.]);
    assert!((game.orbit + 0.2).abs() < 0.0001);
    assert!(
        game.orbit_drag.is_none(),
        "opening the map cancels the drag"
    );

    game.begin_orbit_drag([210., 200.]);
    game.orbit_motion([10., 0.]);
    game.orbit_motion([10., 0.]);
    assert!((game.orbit + 0.25).abs() < 0.0001);
    game.stop_orbit_drag();
    game.move_camera_with_cursor([300., 300.]);
    assert!((game.orbit + 0.25).abs() < 0.0001);

    game.begin_orbit_drag([300., 300.]);
    game.orbit_motion([10., 0.]);
    game.orbit_motion([2000., 0.]);
    let expected = (-0.25_f32 - 10. + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    assert!((game.orbit - expected).abs() < 0.0001);
    game.window_focus_lost();
    assert!(game.orbit_drag.is_none());
    assert!(!game.dragging);
}

#[test]
fn unarmed_click_is_distinct_from_drag_and_focus_cancellation() {
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    game.begin_orbit_drag([100., 100.]);
    game.orbit_motion([1., 1.]);
    assert!(game.release_orbit_drag());
    assert!(!game.release_orbit_drag());
    game.begin_orbit_drag([100., 100.]);
    game.orbit_motion([8., 0.]);
    assert!(!game.release_orbit_drag());
    game.begin_orbit_drag([100., 100.]);
    game.window_focus_lost();
    assert!(!game.release_orbit_drag());
}

#[test]
fn captured_orbit_ignores_absolute_cursor_events_and_restores_click_position() {
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    game.move_camera_with_cursor([100., 100.]);
    game.begin_orbit_drag([100., 100.]);
    // Exercise captured event routing without requiring a native window in CI.
    game.orbit_drag.as_mut().unwrap().captured = true;
    game.cursor_hidden = true;
    game.orbit_motion([10., 0.]);
    game.orbit_motion([40., 40.]);
    game.move_camera_with_cursor([900., 900.]);
    assert!((game.orbit + 0.2).abs() < 0.0001);
    assert!((game.camera_pitch - 0.12).abs() < 0.0001);
    game.cursor_activity(Instant::now());
    assert!(game.cursor_hidden);
    game.stop_orbit_drag();
    assert_eq!(game.cursor, Some([100., 100.]));
    assert!(!game.cursor_hidden);
    game.orbit_motion([40., 40.]);
    assert!((game.orbit + 0.2).abs() < 0.0001);
}

#[test]
fn vehicle_switch_command_resumes_driving_with_fresh_input_and_camera() {
    let mut game = Game::new(Hermes::test_map());
    assert!(game.world.toggle_vehicle(&game.hermes));
    game.set_overlay(Overlay::Map);
    game.keys.insert(KeyCode::ArrowUp);
    game.orbit = 1.;
    game.zoom = 3.;
    game.hood = true;
    game.sound_eye = Some(Vec3::splat(100.));
    let position = game.world.player().position.truncate();
    let definition = &crate::dynamic_world::catalog::definitions()[1];
    game.apply_palette_action(PaletteAction::SwitchVehicle(1));
    assert!(game.world.is_driving());
    assert_eq!(game.world.player().name(), definition.config.name);
    assert_eq!(game.world.player_position().truncate(), position);
    assert_eq!(game.overlay, Overlay::Driving);
    assert!(game.keys.is_empty());
    assert!(game.sound_eye.is_none());
    assert_eq!(game.orbit, 0.);
    assert_eq!(game.zoom, DEFAULT_CHASE_ZOOM);
    assert!(!game.hood);
    assert!(game.scene_dirty);
    assert!(
        game.eye
            .distance(game.world.player_position() / METERS_PER_UNIT)
            < 1.
    );
}

#[test]
fn jump_command_maps_zoomed_click_to_world_and_resumes_with_current_car() {
    let mut game = Game::new(Hermes::test_map_from_source(
        "road street: carlane carlane; end street(-2,0,2,0);
         land(-3,-1,3,-1,3,1,-3,1); player_start(-1,0);",
    ));
    assert!(game.world.toggle_vehicle(&game.hermes));
    game.apply_palette_action(PaletteAction::JumpToLocation);
    assert_eq!(game.overlay, Overlay::Map);
    assert!(game.simulation_paused());
    assert_eq!(game.full_map.selection, hud::MapSelection::Jump);
    let (w, h) = (1280., 800.);
    let center = [w * 0.5, h * 0.5 + 30.];
    game.full_map.zoom_at(w, h, center, 4.);
    let click = [center[0] - 60., center[1] + 60.];
    let uv = game.full_map.map_uv(w, h, click).unwrap();
    let expected = game.hermes.map_world_point(uv);
    let route_revision = game.hermes.route_revision();
    game.keys.insert(KeyCode::ArrowUp);
    game.orbit = 1.;
    game.camera_pitch = 0.4;
    game.zoom = 3.;
    game.hood = true;
    game.sound_eye = Some(Vec3::splat(100.));
    game.select_map_point(w, h, click);
    let position = game.world.player_position();
    assert!((position.x - expected[0] * METERS_PER_UNIT).abs() < 0.001);
    assert!((position.y - expected[1] * METERS_PER_UNIT).abs() < 0.001);
    assert_eq!(game.overlay, Overlay::Driving);
    assert_eq!(game.full_map.selection, hud::MapSelection::Route);
    assert!(game.world.is_driving());
    assert_eq!(game.world.player().velocity, Vec3::ZERO);
    assert_eq!(game.orbit, 0.);
    assert_eq!(game.camera_pitch, 0.);
    assert_eq!(game.zoom, DEFAULT_CHASE_ZOOM);
    assert!(!game.hood);
    assert!(game.keys.is_empty());
    assert!(game.sound_eye.is_none());
    assert!(game.scene_dirty);
    assert!(game.eye.distance(position / METERS_PER_UNIT) < 1.);
    assert_eq!(game.hermes.route_revision(), route_revision);
    game.world.reset_player(&game.hermes);
    assert!(game.world.player_position().distance(position) < 0.001);
}

#[test]
fn jump_ignores_padding_retries_blocked_points_and_cancels_without_moving() {
    let mut game = Game::new(Hermes::test_map_from_source(
        "road street: carlane carlane; end street(-2,0,2,0);
         land(-3,-1,3,-1,3,1,-3,1); player_start(-1,0);
         building(0.2,-0.1,-0.1,0.1,-0.1,0.1,0.1,-0.1,0.1);",
    ));
    let before = game.world.player_position();
    game.apply_palette_action(PaletteAction::JumpToLocation);
    // Header, letterboxing and outside the panel cannot select a point.
    for click in [[50., 50.], [23., 430.], [0., 0.]] {
        game.select_map_point(1280., 800., click);
        assert_eq!(game.full_map.selection, hud::MapSelection::Jump);
        assert_eq!(game.world.player_position(), before);
    }
    game.select_map_point(1280., 800., [640., 430.]);
    assert_eq!(game.full_map.selection, hud::MapSelection::JumpBlocked);
    assert_eq!(game.overlay, Overlay::Map);
    assert_eq!(game.world.player_position(), before);
    game.full_map.zoom_at(1280., 800., [640., 430.], 4.);
    game.full_map.reset_zoom();
    assert_eq!(game.full_map.selection, hud::MapSelection::JumpBlocked);
    // A second click on clear land succeeds, even away from a road.
    game.select_map_point(1280., 800., [640. - 50., 430. + 80.]);
    assert_eq!(game.overlay, Overlay::Driving);
    assert!(game.world.player_position().distance(before) > 50.);
    let visited = game.world.player_position();
    game.apply_palette_action(PaletteAction::JumpToLocation);
    game.set_overlay(game.overlay.escape());
    assert_eq!(game.overlay, Overlay::Driving);
    assert_eq!(game.world.player_position(), visited);
    game.set_overlay(Overlay::Map);
    assert_eq!(game.full_map.selection, hud::MapSelection::Route);
    game.select_map_point(1280., 800., [640., 430. + 140.]);
    assert_eq!(game.overlay, Overlay::Map);
    assert_eq!(game.world.player_position(), visited);
    assert_ne!(game.hermes.route_revision(), 0);
}

#[test]
fn landmark_command_resumes_driving_and_moves_camera_without_stale_input() {
    let mut game = Game::new(Hermes::test_map_from_source(
        "road street: sidewalk carlane carlane sidewalk; end
         street(-1,0,1,0); land(-2,-2,2,-2,2,2,-2,2);
         player_start(-0.8,0);
         site_model_01 = building(0.1, \"study_01\", 0.4,0.2,0.6,0.2,0.6,0.4,0.4,0.4);",
    ));
    game.set_overlay(Overlay::Map);
    game.keys.insert(KeyCode::ArrowUp);
    game.orbit = 1.;
    game.zoom = 3.;
    game.hood = true;
    game.sound_eye = Some(Vec3::splat(100.));
    let before = game.world.player_position();
    game.apply_palette_action(PaletteAction::Respawn("site_model_01".into()));
    let position = game.world.player_position();
    assert!(position.distance(before) > 50.);
    assert_eq!(game.overlay, Overlay::Driving);
    assert!(game.world.is_driving());
    assert_eq!(game.orbit, 0.);
    assert_eq!(game.zoom, DEFAULT_CHASE_ZOOM);
    assert!(!game.hood);
    assert!(game.keys.is_empty());
    assert!(game.sound_eye.is_none());
    assert!(game.scene_dirty);
    assert!(game.eye.distance(position / METERS_PER_UNIT) < 1.);
}

#[test]
fn direction_keys_brake_opposing_motion_then_accelerate_either_way() {
    for (key, direction) in [
        (KeyCode::ArrowUp, 1.),
        (KeyCode::KeyW, 1.),
        (KeyCode::ArrowDown, -1.),
        (KeyCode::KeyS, -1.),
    ] {
        let keys = HashSet::from([key]);
        let (opposing, _) = movement_controls(&keys, -10. * direction);
        assert!(opposing.brake);
        assert_eq!(opposing.throttle, 0.);
        for speed in [-0.1, 0., 0.1, 10. * direction] {
            let (driving, _) = movement_controls(&keys, speed);
            assert!(!driving.brake);
            assert_eq!(driving.throttle, direction);
        }
    }
    let (driving, _) =
        movement_controls(&HashSet::from([KeyCode::ArrowUp, KeyCode::ArrowDown]), 10.);
    assert_eq!(driving.throttle, 0.);
    assert!(!driving.brake);
}

#[test]
fn space_handbrake_and_shift_sprint_are_independent() {
    let (driving, walking) = movement_controls(&HashSet::from([KeyCode::Space]), 10.);
    assert!(!driving.boost);
    assert!(driving.handbrake);
    assert!(!driving.brake);
    assert!(!walking.run);
    assert!(walking.jump);
    for key in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
        let (driving, walking) = movement_controls(&HashSet::from([key]), 10.);
        assert!(!driving.handbrake);
        assert!(!driving.brake);
        assert!(driving.boost);
        assert!(walking.run);
        assert!(!walking.jump);
    }
}

#[test]
fn reticle_stays_centered_over_hud_coordinates_and_yields_to_menus() {
    use egui::CursorIcon::{Crosshair, Default};
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    assert_eq!(game.scene_cursor(false), Default);
    for cursor in [None, Some([940., 620.]), Some([10., 10.])] {
        game.cursor = cursor;
        assert_eq!(game.scene_cursor(true), Crosshair);
    }
    game.window_focus_lost();
    assert_eq!(game.scene_cursor(true), Default);
}

#[test]
fn cursor_idle_deadline_resets_on_pointer_activity() {
    let mut game = Game::new(Hermes::test_map());
    let now = Instant::now();
    assert_eq!(game.cursor_wait(now), ControlFlow::Wait);
    assert!(!game.cursor_hidden);

    for overlay in [
        Overlay::Start,
        Overlay::Driving,
        Overlay::Menu,
        Overlay::Map,
    ] {
        game.set_overlay(overlay);
        game.cursor_activity(now);
        #[cfg(not(target_arch = "wasm32"))]
        if overlay.paused() {
            assert_eq!(game.cursor_wait(now + CURSOR_IDLE_DELAY), ControlFlow::Wait);
            assert!(!game.cursor_hidden);
            continue;
        }
        let moved = now + Duration::from_secs(1);
        game.cursor_activity(moved);
        let deadline = moved + CURSOR_IDLE_DELAY;
        assert_eq!(
            game.cursor_wait(now + CURSOR_IDLE_DELAY),
            ControlFlow::WaitUntil(deadline)
        );
        assert!(!game.cursor_hidden);
        assert_eq!(game.cursor_wait(deadline), ControlFlow::Wait);
        assert!(game.cursor_hidden);
        assert_eq!(game.cursor_hide_at, None);

        game.cursor_activity(deadline);
        assert!(!game.cursor_hidden);
        assert_eq!(
            game.cursor_wait(deadline),
            ControlFlow::WaitUntil(deadline + CURSOR_IDLE_DELAY)
        );
        // Leaving the window or losing focus cancels hiding.
        game.show_cursor();
        assert_eq!(
            game.cursor_wait(deadline + CURSOR_IDLE_DELAY),
            ControlFlow::Wait
        );
        assert!(!game.cursor_hidden);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn opening_native_ui_reveals_the_pointer_and_cancels_pending_hiding() {
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    let now = Instant::now();
    let deadline = now + CURSOR_IDLE_DELAY;
    game.cursor_activity(now);
    game.cursor_wait(deadline);
    assert!(game.cursor_hidden);
    game.help_visible = true;
    game.cursor_wait(deadline);
    assert!(!game.cursor_hidden);
    game.help_visible = false;
    game.cursor_activity(now);
    game.window_focus_lost();
    assert_eq!(game.cursor_wait(deadline), ControlFlow::Wait);
    assert!(!game.cursor_hidden);
}

#[test]
fn menu_arrows_select_and_enter_activates_without_leaking_controls() {
    let mut game = Game::new(Hermes::test_map());
    for overlay in [Overlay::Start, Overlay::Menu] {
        game.set_overlay(overlay);
        assert_eq!(game.menu_selection, MenuItem::Resume);
        assert_eq!(
            game.menu_key(KeyCode::Enter, ElementState::Pressed, false),
            Some(MenuItem::Resume)
        );
        for key in [
            KeyCode::ArrowDown,
            KeyCode::ArrowDown,
            KeyCode::ArrowUp,
            KeyCode::ArrowUp,
        ] {
            let expected = if key == KeyCode::ArrowUp {
                game.menu_selection.previous()
            } else {
                game.menu_selection.other()
            };
            assert_eq!(game.menu_key(key, ElementState::Pressed, false), None);
            assert_eq!(game.menu_selection, expected);
            assert_eq!(
                game.menu_key(KeyCode::Enter, ElementState::Pressed, false),
                Some(expected)
            );
            game.menu_key(key, ElementState::Pressed, true);
            game.menu_key(key, ElementState::Released, false);
            assert_eq!(game.menu_selection, expected);
        }
        assert_eq!(
            game.menu_key(KeyCode::Enter, ElementState::Pressed, true),
            None
        );
        assert_eq!(
            game.menu_key(KeyCode::Enter, ElementState::Released, false),
            None
        );
        assert_eq!(
            game.menu_key(KeyCode::KeyQ, ElementState::Pressed, false),
            Some(MenuItem::Exit)
        );
        assert!(game.keys.is_empty());
        game.menu_key(KeyCode::ArrowDown, ElementState::Pressed, false);
        game.set_overlay(Overlay::Driving);
        assert_eq!(game.menu_selection, MenuItem::Resume);
    }
    for overlay in [Overlay::Driving, Overlay::Map] {
        game.set_overlay(overlay);
        for key in [
            KeyCode::ArrowDown,
            KeyCode::ArrowUp,
            KeyCode::Enter,
            KeyCode::KeyQ,
        ] {
            assert_eq!(game.menu_key(key, ElementState::Pressed, false), None);
            assert_eq!(game.menu_selection, MenuItem::Resume);
        }
    }
}

#[test]
fn explicit_launch_position_starts_playing_immediately() {
    let hermes = Hermes::test_map();
    let default = Game::new(Hermes::test_map());
    let point = default.world.player_position() / METERS_PER_UNIT;
    let game = Game::new_at(
        hermes,
        Some(SpawnPoint {
            point: [point.x, point.y],
            heading: None,
        }),
    )
    .unwrap();
    assert_eq!(game.overlay, Overlay::Driving);
    assert!((game.world.player_position().x / METERS_PER_UNIT - point.x).abs() < 1e-5);
    assert!((game.world.player_position().y / METERS_PER_UNIT - point.y).abs() < 1e-5);
}
#[test]
fn body_follows_mouse_look_and_strafes_without_steering_the_sights() {
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    game.world.prepare_player_pedestrian_collision(&game.hermes);
    game.loadout.equip(Some(weapons::Weapon::Handgun));
    let yaw = game.world.player_yaw();
    let position = game.world.player_position();
    game.aim_motion([-0.8 / AIM_LOOK_SENSITIVITY, -12.]);
    let heading = yaw + 0.8;
    for _ in 0..120 {
        game.simulate(DT);
        assert!((game.world.player_yaw() + game.orbit - heading).cos() > 0.99999);
    }
    assert!((game.world.player_yaw() - heading).cos() > 0.99999);
    assert_eq!(game.world.player_position(), position);
    assert_eq!(game.camera_pitch, -12. * AIM_LOOK_SENSITIVITY);
    game.keys.insert(KeyCode::KeyA);
    for _ in 0..120 {
        game.simulate(DT);
    }
    let travel = (game.world.player_position() - position).truncate();
    let forward = glam::Vec2::from_angle(heading);
    assert!(travel.dot(forward.perp()) > 1., "travel {travel:?}");
    assert!(travel.dot(forward).abs() < 0.1);
    assert!((game.world.player_yaw() + game.orbit - heading).cos() > 0.99999);
}

#[test]
fn walking_left_right_keys_turn_without_strafing() {
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    assert!(game.world.toggle_vehicle(&game.hermes));
    game.orbit = 0.7;
    for (key, direction) in [
        (KeyCode::KeyA, 1.),
        (KeyCode::ArrowLeft, 1.),
        (KeyCode::KeyD, -1.),
        (KeyCode::ArrowRight, -1.),
    ] {
        game.keys.clear();
        game.keys.insert(key);
        let position = game.world.player_position();
        let heading = game.world.player_yaw();
        for _ in 0..3 {
            game.simulate(DT * 2.);
        }
        assert_eq!(game.world.player_position(), position);
        assert!((game.world.player_yaw() - heading) * direction > 0.1);
        assert_eq!(game.orbit, 0.7);
    }
    game.keys.insert(KeyCode::ArrowLeft);
    let heading = game.world.player_yaw();
    game.simulate(DT * 6.);
    assert_eq!(game.world.player_yaw(), heading);
}
#[test]
fn first_menu_freezes_until_start_then_later_pauses_resume() {
    let mut game = Game::new(Hermes::test_map());
    assert_eq!(game.overlay, Overlay::Start);
    let position = game.world.player_position();
    game.keys.insert(KeyCode::KeyW);
    for _ in 0..10 {
        game.simulate(0.1);
    }
    assert_eq!(game.world.player_position(), position);
    assert_eq!(game.game_time, 0.);
    game.set_overlay(game.overlay.escape());
    assert_eq!(game.overlay, Overlay::Driving);
    assert!(game.keys.is_empty());
    game.simulate(DT);
    assert!(game.game_time > 0.);
    game.set_overlay(game.overlay.escape());
    assert_eq!(game.overlay, Overlay::Menu);
    game.set_overlay(game.overlay.escape());
    assert_eq!(game.overlay, Overlay::Driving);
}
#[test]
fn menu_and_map_freeze_the_world_and_resume_without_accumulated_time_or_keys() {
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    game.keys.insert(KeyCode::KeyW);
    game.simulate(0.1);
    for overlay in [Overlay::Menu, Overlay::Map] {
        let position = game.world.player().position;
        let velocity = game.world.player().velocity;
        let time = game.game_time;
        game.set_overlay(overlay);
        assert!(game.keys.is_empty());
        for _ in 0..100 {
            game.simulate(0.1);
        }
        assert_eq!(game.world.player().position, position);
        assert_eq!(game.world.player().velocity, velocity);
        assert_eq!(game.game_time, time);
        game.set_overlay(overlay.escape());
        assert_eq!(game.overlay, Overlay::Driving);
        assert_eq!(game.accumulator, 0.);
        game.simulate(DT);
        assert!(game.game_time > time);
    }
}
#[test]
fn fps_uses_actual_present_intervals_including_slow_frames() {
    let start = Instant::now();
    let mut rate = FrameRate::default();
    rate.presented(start);
    rate.presented(start + Duration::from_millis(100));
    assert_eq!(rate.value, None);
    rate.presented(start + Duration::from_millis(600));
    assert!((rate.value.unwrap() - 2. / 0.6).abs() < 0.001);
    // Pausing starts a new sample window rather than counting the idle gap.
    let mut resumed = FrameRate::default();
    resumed.presented(start + Duration::from_secs(60));
    assert_eq!(resumed.value, None);
}
#[test]
fn idle_overlays_reuse_frozen_scene_and_resume_refreshes_it() {
    let mut game = Game::new(Hermes::test_map());
    game.scene_dirty = false; // Startup has already uploaded scene buffers.
    for overlay in [Overlay::Start, Overlay::Menu, Overlay::Map] {
        game.set_overlay(overlay);
        assert!(game.simulation_paused());
        assert!(
            !game.refresh_scene(),
            "menu input should reuse frozen GPU data"
        );
        let time = game.game_time;
        for _ in 0..120 {
            game.simulate(1.);
        }
        assert_eq!(game.game_time, time);
        assert_eq!(game.accumulator, 0.);
    }
    game.scene_dirty = true;
    assert!(
        game.refresh_scene(),
        "an explicit reset must still upload once"
    );
    game.scene_dirty = false;
    game.set_overlay(Overlay::Driving);
    assert!(!game.simulation_paused());
    assert!(game.refresh_scene());
}
#[test]
fn switching_spaces_parks_timers_and_resumes_without_replaying_the_gap() {
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    #[cfg(not(target_arch = "wasm32"))]
    {
        game.live_reload = Some(reload::LiveReload::new(PathBuf::from("unused.road")));
    }
    let now = Instant::now();
    let later = now + Duration::from_secs(3600);
    game.frame_elapsed(now);
    game.accumulator = DT * 0.5;
    game.keys.insert(KeyCode::KeyW);
    game.cursor_activity(now);
    game.retry_surface(Duration::from_millis(100));
    game.handle_window_activity(&WindowEvent::Occluded(true));
    game.handle_window_activity(&WindowEvent::Focused(false));
    let position = game.world.player_position();
    for _ in 0..3 {
        assert_eq!(game.wait_for_event(later), ControlFlow::Wait);
        assert!(!game.wake_surface(later));
        game.simulate(0.1);
    }
    assert_eq!(game.game_time, 0.);
    assert_eq!(game.accumulator, 0.);
    assert!(game.keys.is_empty());
    assert!(game.cursor_hide_at.is_none());
    assert_eq!(game.world.player_position(), position);

    game.handle_window_activity(&WindowEvent::Occluded(false));
    assert!(!game.window_active());
    game.handle_window_activity(&WindowEvent::Resized(winit::dpi::PhysicalSize::new(0, 0)));
    game.handle_window_activity(&WindowEvent::Focused(true));
    assert!(!game.window_active());
    game.handle_window_activity(&WindowEvent::Resized(winit::dpi::PhysicalSize::new(
        960, 640,
    )));
    assert!(game.window_active());
    assert_eq!(game.overlay, Overlay::Menu);
    assert_eq!(game.frame_elapsed(later), 0.);
    game.set_overlay(Overlay::Driving);
    let elapsed = game.frame_elapsed(later);
    game.simulate(elapsed);
    let elapsed = game.frame_elapsed(later + Duration::from_millis(16));
    game.simulate(elapsed);
    assert!((game.game_time - 0.016).abs() < 0.00001);
}

#[cfg(target_os = "macos")]
#[test]
fn switching_spaces_can_redraw_before_both_visibility_and_focus_return() {
    let mut game = Game::new(Hermes::test_map());
    game.handle_window_activity(&WindowEvent::Focused(false));
    game.handle_window_activity(&WindowEvent::Occluded(true));
    assert!(!game.can_present_frame());

    // Focus returns first, while the last occlusion event still says hidden.
    game.handle_window_activity(&WindowEvent::Focused(true));
    assert!(game.can_present_frame());
    assert!(!game.window_active());
    game.retry_surface(Duration::from_millis(250));
    let retry = game.surface_retry.unwrap();
    assert_eq!(
        game.wait_for_event(Instant::now()),
        ControlFlow::WaitUntil(retry)
    );
    assert!(!game.wake_surface(retry - Duration::from_millis(1)));
    assert!(game.wake_surface(retry));

    // Visibility returns first. The game can repaint while input stays paused.
    game.handle_window_activity(&WindowEvent::Focused(false));
    game.handle_window_activity(&WindowEvent::Occluded(false));
    assert!(game.can_present_frame());
    assert!(!game.window_active());
    assert_eq!(game.wait_for_event(Instant::now()), ControlFlow::Wait);
}

#[test]
fn paused_startup_retries_a_missing_drawable_without_busy_polling() {
    let mut game = Game::new(Hermes::test_map());
    assert!(game.simulation_paused());
    game.retry_surface(Duration::from_millis(100));
    let deadline = game.surface_retry.unwrap();
    assert_eq!(
        game.wait_for_event(deadline - Duration::from_millis(50)),
        ControlFlow::WaitUntil(deadline)
    );
    assert!(!game.wake_surface(deadline - Duration::from_millis(1)));
    assert!(game.wake_surface(deadline));
    assert!(!game.wake_surface(deadline));
    // Presentation cancels the retry; paused idle goes back to waiting for input.
    assert_eq!(game.wait_for_event(deadline), ControlFlow::Wait);
    assert_eq!(game.game_time, 0.);
    assert_eq!(game.first_frame_attempts, 0);
}
#[test]
fn surface_retry_and_cursor_deadlines_use_the_earliest_wakeup() {
    let mut game = Game::new(Hermes::test_map());
    game.set_overlay(Overlay::Driving);
    let now = Instant::now();
    game.cursor_hide_at = Some(now + Duration::from_secs(3));
    game.surface_retry = Some(now + Duration::from_millis(100));
    assert_eq!(
        game.wait_for_event(now),
        ControlFlow::WaitUntil(now + Duration::from_millis(100))
    );
    game.surface_retry = None;
    assert_eq!(
        game.wait_for_event(now),
        ControlFlow::WaitUntil(now + Duration::from_secs(3))
    );
}

#[test]
fn enter_horn_is_held_only_while_driving_and_never_leaks_from_menus() {
    let mut game = Game::new(Hermes::test_map());
    game.keys.insert(KeyCode::Enter);
    assert!(!game.horn_requested());
    game.set_overlay(Overlay::Driving);
    assert!(!game.horn_requested(), "menu Enter is consumed on start");
    for key in [KeyCode::Enter, KeyCode::NumpadEnter] {
        game.keys.insert(key);
        assert!(game.horn_requested());
        game.keys.remove(&key);
        assert!(!game.horn_requested());
    }
    game.keys.insert(KeyCode::Enter);
    game.hidden = true;
    assert!(!game.horn_requested());
    game.hidden = false;
    assert!(game.world.toggle_vehicle(&game.hermes));
    assert!(
        !game.horn_requested(),
        "walking does not sound a parked vehicle's horn"
    );
    game.set_overlay(Overlay::Menu);
    assert!(!game.horn_requested());
}

/// Run explicitly on the native output device to measure the Start CPU path.
#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore]
fn profile_start_transition() {
    let hermes =
        Hermes::load_with_game_data(std::path::Path::new("data/maps/mtl/map.road"), None).unwrap();
    let mut game = Game::new(hermes);
    let started = Instant::now();
    game.audio.prepare();
    eprintln!("Startup audio preparation: {:?}", started.elapsed());
    let started = Instant::now();
    game.set_overlay(Overlay::Driving);
    eprintln!("Start overlay: {:?}", started.elapsed());
    for frame in 0..3 {
        let started = Instant::now();
        game.simulate(1. / 60.);
        eprintln!("Start frame {frame} simulation: {:?}", started.elapsed());
        let p = game.world.player_position() / METERS_PER_UNIT;
        let started = Instant::now();
        game.hermes.update_navigation(p.to_array());
        eprintln!("Start frame {frame} navigation: {:?}", started.elapsed());
        let started = Instant::now();
        game.audio.update(crate::audio::Frame::default(), true);
        eprintln!("Start frame {frame} audio: {:?}", started.elapsed());
    }
}

#[test]
fn season_palette_actions_update_the_live_world_and_reset_on_map_reload() {
    use crate::season::Season;
    let source = include_str!("../../../tests/fixtures/snow_season.road");
    let mut game = Game::new(Hermes::test_map_from_source(source));
    let position = game.world.player_position();
    let route = game.hermes.route_revision();
    game.scene_dirty = false;
    game.apply_palette_action(PaletteAction::Season(Season::Winter));
    assert_eq!(game.world.season(), Season::Winter);
    assert!(game.scene_dirty);
    assert_eq!(game.world.player_position(), position);
    assert_eq!(game.hermes.route_revision(), route);
    game.world.snow.clear_height((0, 0), 0.);
    assert!(game
        .apply_reloaded_map(Hermes::test_map_from_source("player_start(100,100,0);"))
        .is_err());
    assert_eq!(game.world.season(), Season::Winter);
    assert!(!game.world.snow.cells.is_empty());
    game.apply_reloaded_map(Hermes::test_map_from_source(source))
        .unwrap();
    assert_eq!(game.world.season(), Season::Summer);
    assert!(game.world.snow.cells.is_empty());
    assert!(game.world.snow.changes.is_empty());
    game.apply_palette_action(PaletteAction::Season(Season::Winter));
    assert_eq!(game.world.season(), Season::Winter);
    game.apply_palette_action(PaletteAction::Season(Season::Summer));
    assert_eq!(game.world.season(), Season::Summer);
    assert!(game.world.snow.cells.is_empty());
}

#[test]
fn aircraft_keys_split_pitch_roll_yaw_and_power() {
    let input = aircraft_controls(&HashSet::from([
        KeyCode::ArrowUp,
        KeyCode::ArrowLeft,
        KeyCode::KeyD,
    ]));
    assert_eq!(input.throttle, 0.);
    assert_eq!(input.aircraft_pitch, -1.);
    assert_eq!(input.steering, -1.);
    assert_eq!(input.aircraft_yaw, 1.);
    let input = aircraft_controls(&HashSet::from([
        KeyCode::KeyW,
        KeyCode::ArrowDown,
        KeyCode::Space,
    ]));
    assert_eq!(input.throttle, 1.);
    assert_eq!(input.aircraft_pitch, 1.);
    assert!(input.handbrake);
}

#[test]
fn zombie_defeat_respawns_after_the_fall_without_input() {
    use crate::kairos::MissionId;
    let mut game = Game::new(Hermes::test_map());
    game.world.prepare_player_pedestrian_collision(&game.hermes);
    game.set_overlay(Overlay::Driving);
    game.activate_mission_now(MissionId::Zombie);
    for _ in 0..900 {
        game.simulate(DT);
        if game.world.player_health() == 0 {
            break;
        }
    }
    assert_eq!(game.world.player_health(), 0);
    game.set_overlay(Overlay::Menu);
    game.simulate(10.);
    game.set_overlay(Overlay::Driving);
    game.simulate(1.);
    assert_eq!(game.world.player_health(), 0);
    for _ in 0..480 {
        game.simulate(DT);
        if game.world.player_health() > 0 {
            break;
        }
    }
    assert_eq!(game.world.player_health(), 100);
    assert_eq!(game.kairos.active_id(), Some(MissionId::Zombie));
    assert!(game.world.zombie_mode());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires a native GPU adapter"]
fn zombie_melee_contact_frames_in_game() {
    let mut game = Game::new(Hermes::test_map());
    game.world.prepare_player_pedestrian_collision(&game.hermes);
    game.set_overlay(Overlay::Driving);
    game.activate_mission_now(crate::kairos::MissionId::Zombie);
    let mut renderer = pollster::block_on(Renderer::new(None, 960, 640)).unwrap();
    renderer.zombie_mode = true;
    let scenery = Scenery::new(&renderer, &mut game.hermes);
    let mut actors = VehicleScenery::new(&renderer);
    let capture = |name: &str, game: &Game, actors: &mut VehicleScenery| {
        let player = game.world.player_position();
        let forward = glam::Vec2::from_angle(game.world.player_yaw()).extend(0.);
        let look = (player + forward * 0.40 + Vec3::Z * 0.08) / METERS_PER_UNIT;
        let eye = look
            + (forward * -0.7 + forward.cross(Vec3::Z) * 2.7 + Vec3::Z * 0.48) / METERS_PER_UNIT;
        actors.update(&renderer, &game.world);
        let mut meshes = scenery.visible([look.x, look.y]);
        meshes.extend(actors.visible());
        let png = renderer
            .capture_png(|view| {
                renderer.render_scene(view, &meshes, eye, look, 10., None);
                let ctx = egui::Context::default();
                crate::engine::job_feed::install_font(&ctx);
                let frame = |time| {
                    ctx.run_ui(
                        egui::RawInput {
                            time: Some(time),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(
                                    renderer.config.width as f32,
                                    renderer.config.height as f32,
                                ),
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            crate::engine::job_feed::show(
                                ui.ctx(),
                                &mut false,
                                &game.kairos,
                                Overlay::Driving,
                                Some(crate::engine::job_feed::Vitals::from_world(&game.world)),
                                None,
                                None,
                            );
                            crate::engine::job_feed::player_status(
                                ui.ctx(),
                                ui.ctx().content_rect().size(),
                                game.world.player_health(),
                                game.world.chase_status(),
                            );
                        },
                    )
                };
                let mut output = frame(0.);
                output.append(frame(0.5));
                let mut painter = crate::engine::palette::Painter::new(
                    &renderer.device,
                    renderer.config.format.add_srgb_suffix(),
                );
                painter.paint(
                    &renderer,
                    view,
                    ctx.tessellate(output.shapes, output.pixels_per_point),
                    output.textures_delta,
                    output.pixels_per_point,
                );
            })
            .unwrap();
        if let Some(dir) = std::env::var_os("MTL_MELEE_QA_DIR") {
            std::fs::write(std::path::PathBuf::from(dir).join(name), &png).unwrap();
        }
        png
    };
    for _ in 0..480 {
        game.simulate(DT);
        if game.world.player_health() < 100 {
            break;
        }
    }
    assert_eq!(game.world.player_health(), 75);
    let bite = capture("zombie-bite.png", &game, &mut actors);
    for _ in 0..70 {
        game.simulate(DT);
    }
    assert!(game.world.start_punch());
    for _ in 0..38 {
        game.simulate(DT);
    }
    assert_eq!(game.world.zombie_health_meters().next().unwrap().1, 2. / 3.);
    let punch = capture("player-punch.png", &game, &mut actors);
    assert_ne!(punch, bite);
    for _ in 0..900 {
        game.simulate(DT);
        if game.world.player_health() == 0 {
            break;
        }
    }
    game.simulate(1.);
    assert_eq!(game.world.player_health(), 0);
    let fallen = capture("mange.png", &game, &mut actors);
    assert_ne!(fallen, punch);
}

fn cinematic_drive_game() -> Game {
    let mut game = Game::new(Hermes::test_map_from_source(
        "road street: carlane carlane; end
         street(-0.8,0,-0.35,0,0,0,0,0.55); street(0,0.55,0,2.95);
         street(0.5,0.8,0.9,0.8);
         land(-1,-1,1,-1,1,3,-1,3); player_start(-0.65,0,0);",
    ));
    let taxi = crate::dynamic_world::catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-j01")
        .unwrap();
    assert!(game.world.switch_vehicle(taxi));
    game.set_overlay(Overlay::Map);
    game
}

fn choose_drive_point(game: &mut Game, world: [f32; 2]) {
    let uv = game.hermes.map_uv(world);
    let span = 692.;
    game.select_map_point(
        1280.,
        800.,
        [640. + (uv[0] - 0.5) * span, 430. + (uv[1] - 0.5) * span],
    );
}

fn press_drive_button(game: &mut Game) {
    let [x, y, w, h] = hud::full_map_drive_rect(1280.);
    game.select_map_point(1280., 800., [x + w * 0.5, y + h * 0.5]);
}

#[test]
fn cinematic_drive_from_map_follows_a_bend_and_stops_at_destination() {
    let mut game = cinematic_drive_game();
    choose_drive_point(&mut game, [0., 0.8]);
    let start = game.world.player_position();
    press_drive_button(&mut game);
    assert_eq!(game.overlay, Overlay::Driving);
    assert!(game.auto_drive.is_some());
    assert_eq!(game.world.player_position(), start);
    let mut mid_trip = false;
    for _ in 0..1800 {
        game.simulate(1. / 30.);
        let p = game.world.player_position();
        mid_trip |= p.y > 3. && p.y < 60. && p.x < -1.;
        if game.auto_drive.is_none() {
            break;
        }
    }
    assert!(mid_trip);
    let p = game.world.player_position();
    assert_eq!(game.overlay, Overlay::Driving);
    assert!(
        p.truncate().distance(glam::vec2(0., 80.)) < 6.,
        "stopped at {p:?}"
    );
    assert!(game.world.player().velocity.truncate().length() < 0.2);
    assert_eq!(
        game.hermes.route_status(),
        crate::hermes::RouteStatus::Arrived
    );
    assert!(game.auto_drive.is_none());
}

#[test]
fn cinematic_drive_skips_a_kilometre_every_eight_seconds() {
    let mut game = Game::new(Hermes::test_map_from_source(
        "road street: carlane carlane; end street(0,0,30,0);
         land(-1,-1,31,-1,31,1,-1,1); player_start(0.2,0,0);",
    ));
    game.set_overlay(Overlay::Map);
    choose_drive_point(&mut game, [29., 0.]);
    press_drive_button(&mut game);
    for _ in 0..2 {
        let start = game.world.player_position().x;
        for _ in 0..959 {
            game.simulate(DT);
        }
        let before = game.world.player_position().x;
        assert!((0. ..250.).contains(&(before - start)));
        game.simulate(DT);
        let after = game.world.player_position().x;
        assert!((after - before - 1000.).abs() < 3., "{before} -> {after}");
        assert!(game.auto_drive.is_some());
    }
}

#[test]
fn cinematic_drive_requires_a_reachable_route_and_driver_and_yields_to_input() {
    let mut game = cinematic_drive_game();
    press_drive_button(&mut game);
    assert_eq!(game.overlay, Overlay::Map);
    choose_drive_point(&mut game, [0.7, 0.8]);
    assert_eq!(
        game.hermes.route_status(),
        crate::hermes::RouteStatus::Unreachable
    );
    press_drive_button(&mut game);
    assert!(game.auto_drive.is_none());
    choose_drive_point(&mut game, [0., 0.8]);
    assert!(game.world.toggle_vehicle(&game.hermes));
    press_drive_button(&mut game);
    assert_eq!(game.overlay, Overlay::Map);
    assert!(game.auto_drive.is_none());
    assert!(game.world.toggle_vehicle(&game.hermes));
    press_drive_button(&mut game);
    assert!(game.auto_drive.is_some());
    game.keys.insert(KeyCode::KeyA);
    game.simulate(DT);
    assert!(game.auto_drive.is_none());
    assert_eq!(
        game.hermes.route_status(),
        crate::hermes::RouteStatus::Active
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "exercises a full city trip"]
fn cinematic_drive_reaches_first_bus_stop_from_city_spawn() {
    let map =
        Hermes::load_with_game_data(std::path::Path::new("data/maps/mtl/map.road"), None).unwrap();
    let mut game = Game::new(map);
    game.set_overlay(Overlay::Map);
    choose_drive_point(&mut game, [-0.63, -2.]);
    press_drive_button(&mut game);
    assert!(game.auto_drive.is_some());
    for _ in 0..3600 {
        game.simulate(1. / 30.);
        if game.auto_drive.is_none() {
            break;
        }
    }
    assert_eq!(
        game.hermes.route_status(),
        crate::hermes::RouteStatus::Arrived
    );
    assert!(game.auto_drive.is_none());
}

#[test]
fn cinematic_drive_returns_to_map_when_traffic_cannot_clear() {
    let mut game = Game::new(Hermes::test_map_from_source(
        "road street: carlane; end street(-0.5,0,0.5,0);
         parked_vehicle(\"vehicle-j01\",0,0,0); player_start(-0.2,0,0);",
    ));
    game.set_overlay(Overlay::Map);
    choose_drive_point(&mut game, [0.4, 0.]);
    press_drive_button(&mut game);
    assert!(game.auto_drive.is_some());
    for _ in 0..1800 {
        game.simulate(1. / 30.);
        if game.auto_drive.is_none() {
            break;
        }
    }
    assert_eq!(game.overlay, Overlay::Map);
    assert_eq!(game.full_map.selection, hud::MapSelection::DriveBlocked);
    assert!(game.world.player_position().x < 0.);
    choose_drive_point(&mut game, [-0.4, 0.]);
    assert_eq!(game.full_map.selection, hud::MapSelection::Route);
}

#[test]
fn f_equips_skis_before_vehicle_entry_and_prompt_uses_the_same_action() {
    let hermes = Hermes::test_map_from_source("road local:carlane;end local(-2,-1,2,-1);ground=land(-5,-5,5,-5,5,5,-5,5);elevation(ground,0.1);player_start(-0.025,0,0);pedestrian_start(0.01,0,180);ski_set(0,0,0);");
    let mut game = Game::new(hermes);
    game.world
        .set_season(&game.hermes, crate::season::Season::Winter);
    game.use_equipment_or_vehicle();
    assert!(game.world.is_wearing_skis());
    assert!(!game.world.is_driving());
    game.apply_palette_action(PaletteAction::UseSkis);
    assert!(!game.world.is_wearing_skis());
    assert!(!game.world.is_driving());
    game.use_equipment_or_vehicle();
    assert!(game.world.is_wearing_skis());
    game.use_equipment_or_vehicle();
    assert!(!game.world.is_wearing_skis());
    assert_eq!(game.world.skis_hint(), Some("Chausser les skis"));
    let hermes = Hermes::test_map_from_source("road local:carlane;end local(-2,-1,2,-1);ground=land(-5,-5,5,-5,5,5,-5,5);elevation(ground,0.1);player_start(0,0,0);pedestrian_start(-0.003,0.022,0);");
    let mut game = Game::new(hermes);
    game.use_equipment_or_vehicle();
    assert!(
        game.world.is_driving(),
        "F still boards when no ski action is pending"
    );
}

#[test]
fn chairlift_auto_boards_with_skis_and_f_cannot_interrupt_the_ride() {
    let h=Hermes::test_map_from_source("road local:carlane;end local(-3,-1,3,-1);ground=land(-5,-5,5,-5,5,5,-5,5);elevation(ground,0.1);hill=terrain(0.5,-1,0.9,1,0.9,1,3,-1,3);player_start(-0.2,-1,0);pedestrian_start(0.04,0.05,90);ski_set(0.04,0.05,0);chairlift_base(0,0);chairlift_top(0,2);");
    let mut game = Game::new(h);
    game.world
        .set_season(&game.hermes, crate::season::Season::Winter);
    assert!(game.world.toggle_skis());
    game.world.prepare_chairlift_boarding(&game.hermes);
    for _ in 0..720 {
        game.world.step(
            &game.hermes,
            Default::default(),
            Default::default(),
            1. / 60.,
        );
        if game.world.riding_chairlift() {
            break;
        }
    }
    assert!(game.world.riding_chairlift());
    assert!(game.world.is_wearing_skis());
    game.use_equipment_or_vehicle();
    assert!(game.world.riding_chairlift());
    assert!(game.world.is_wearing_skis());
    for _ in 0..481 {
        game.world.step(
            &game.hermes,
            Default::default(),
            Default::default(),
            1. / 60.,
        );
    }
    assert!(!game.world.riding_chairlift());
    assert!(game.world.is_wearing_skis());
}

#[test]
fn ski_inputs_preserve_walking_and_add_independent_pole_strokes() {
    let (_, walk) = movement_controls(
        &HashSet::from([
            KeyCode::KeyW,
            KeyCode::KeyE,
            KeyCode::KeyA,
            KeyCode::ShiftLeft,
            KeyCode::Space,
        ]),
        0.,
    );
    assert_eq!(walk.forward, 1.);
    assert_eq!(walk.turn, 1.);
    assert!(walk.poles && walk.run && walk.jump);
    let (_, walk) = movement_controls(&HashSet::from([KeyCode::KeyE]), 0.);
    assert!(walk.poles);
    assert_eq!(walk.forward, 0.);
}
