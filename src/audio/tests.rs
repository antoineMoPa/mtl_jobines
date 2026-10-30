use super::*;
#[test]
fn camera_basis_preserves_physical_directions_and_velocity() {
    assert_eq!(listener_vector(Vec3::X, Vec3::X), [0., 1., 0.]);
    assert_eq!(listener_vector(-Vec3::Y, Vec3::X), [1., 0., 0.]);
    assert_eq!(listener_vector(Vec3::Z, Vec3::X), [0., 0., 1.]);
    assert_eq!(listener_vector(Vec3::Y, Vec3::Y), [0., 1., 0.]);
    let p = Vec3::new(4., 7., 3.);
    let v = Vec3::new(-4., -7., -3.);
    for forward in [Vec3::X, Vec3::Y, Vec3::new(0.8, 0.3, -0.4)] {
        let a = listener_vector(p, forward);
        let b = listener_vector(v, forward);
        assert!((a.iter().map(|n| n * n).sum::<f32>() - p.length_squared()).abs() < 0.001);
        assert!(a.iter().zip(b).map(|(a, b)| a * b).sum::<f32>() < 0.);
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn missing_device_is_a_single_fallible_attempt_and_menu_does_not_initialize() {
    let mut audio = Audio::default();
    audio.ensure_backend(false, || panic!("menus must not open devices"));
    assert!(!audio.attempted);
    audio.ensure_backend(true, || Err("offline simulated missing device".into()));
    assert!(audio.attempted && audio.backend.is_none());
    audio.ensure_backend(true, || panic!("must not retry on every frame"));
    audio.update(Frame::default(), false);
    audio.silence();
}

#[test]
fn bundled_machinery_has_authored_sound_and_horns_independent_of_mass() {
    let definitions = crate::dynamic_world::catalog::definitions();
    let find = |id: &str| {
        definitions
            .iter()
            .find(|d| d.config.research_id == id)
            .unwrap()
            .config
            .sound
    };
    let fire = find("vehicle-j07");
    let refuse = find("vehicle-j05");
    let compact = find("vehicle-01");
    assert_eq!(fire.engine, EngineKind::Diesel);
    assert_eq!(refuse.engine, EngineKind::Diesel);
    assert_eq!(fire.horn.kind, HornKind::Air);
    assert_eq!(refuse.horn.kind, HornKind::Air);
    assert_ne!(fire.horn.frequencies_hz, refuse.horn.frequencies_hz);
    assert!(refuse.machinery_gain > fire.machinery_gain);
    assert!(refuse.idle_rpm < compact.idle_rpm && refuse.body_gain > compact.body_gain);
    assert_eq!(find("vehicle-j01").engine, EngineKind::Electric);
    for definition in definitions {
        definition.config.sound.validate().unwrap();
    }
}
