use super::*;
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
    game.hood = true;
    game.sound_eye = Some(Vec3::splat(100.));
    let before = game.world.player_position();
    game.apply_palette_action(PaletteAction::Respawn("site_model_01".into()));
    let position = game.world.player_position();
    assert!(position.distance(before) > 50.);
    assert_eq!(game.overlay, Overlay::Driving);
    assert!(game.world.is_driving());
    assert_eq!(game.orbit, 0.);
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
    assert!(driving.handbrake);
    assert!(!driving.brake);
    assert!(!walking.run);
    for key in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
        let (driving, walking) = movement_controls(&HashSet::from([key]), 10.);
        assert!(!driving.handbrake);
        assert!(!driving.brake);
        assert!(walking.run);
    }
}

#[test]
fn cursor_idle_deadline_resets_and_wakes_even_while_paused() {
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
            let expected = game.menu_selection.other();
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
        game.simulate(DT * 6.);
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
