use super::*;

fn opening() -> &'static Scene {
    scene_for(Cue::GameIntro).unwrap()
}

#[test]
fn fabula_clock_waits_for_presentation_and_tracks_audio() {
    let mut player = Fabula::default();
    player.play(
        opening(),
        Anchor {
            position: Vec3::new(400., -120., 2.),
            yaw: 1.2,
        },
    );
    assert!(!player.advance(30., None));
    assert_eq!(player.playback.as_ref().unwrap().elapsed, 0.);
    player.playback.as_mut().unwrap().presented = true;
    assert!(!player.advance(30., Some(1.)));
    assert_eq!(player.playback.as_ref().unwrap().elapsed, 1.);
    // A paused audio clock also freezes the camera/subtitles.
    assert!(!player.advance(10., Some(1.)));
    assert_eq!(player.playback.as_ref().unwrap().elapsed, 1.);
    assert!(player.advance(30., None));
    player.clear();
    assert!(!player.active());
}

#[test]
fn fabula_subtitle_boundaries_and_camera_are_relative_to_loaded_world() {
    let scene = opening();
    let anchor = Anchor {
        position: Vec3::new(200., 300., 5.),
        yaw: std::f32::consts::FRAC_PI_2,
    };
    let mut p = Playback {
        scene,
        anchor,
        elapsed: 0.,
        presented: true,
    };
    assert_eq!(p.subtitle(), None);
    let (eye, _) = p.camera();
    assert!(
        eye.distance(anchor.point(scene.definition.shots[0].eye_from) / METERS_PER_UNIT) < 0.00001
    );
    for s in &scene.definition.subtitles {
        p.elapsed = s.start;
        assert_eq!(p.subtitle(), Some(s.text.as_str()));
        p.elapsed = s.end;
        assert_ne!(p.subtitle(), Some(s.text.as_str()));
    }
    p.elapsed = scene.definition.duration;
    assert_eq!(p.subtitle(), None);
    let camera = p.camera();
    assert!(camera.0.is_finite() && camera.1.is_finite());
}

#[test]
fn fabula_rejects_bad_tracks_and_leaves_unmapped_cues_empty() {
    let json = include_str!("../../../assets/cutscenes/opening/scene.json");
    assert!(Scene::load(
        &json.replace("\"duration\": 7.0", "\"duration\": 1.0"),
        None
    )
    .is_err());
    assert!(Scene::load(&json.replace("\"end\": 2.5", "\"end\": 3.0"), None).is_err());
    for id in [
        crate::kairos::MissionId::Taxi,
        crate::kairos::MissionId::Bus,
        crate::kairos::MissionId::Snowplow,
    ] {
        assert!(scene_for(Cue::BeforeMission(id)).is_none());
        assert!(scene_for(Cue::AfterMission(id)).is_none());
    }
}
