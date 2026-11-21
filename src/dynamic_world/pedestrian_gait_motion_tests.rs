use super::*;
use crate::dynamic_world::pedestrian_catalog;

fn flat(p: Vec3) -> Option<Ground> {
    Some(Ground {
        point: p.truncate().extend(0.),
        gradient: [0., 0.],
    })
}

#[test]
fn run_has_flight_and_keeps_support_fixed_before_toe_off() {
    let model = &pedestrian_catalog::definitions()[pedestrian_catalog::PLAYER_SUMMER_MODEL];
    let mut gait = Gait::new_player(model, Vec3::ZERO, 0., flat);
    gait.set_running(true, 1.);
    let mut flight_frames = 0;
    let mut contact_frames = 0;
    let mut previous = gait.feet.map(|f| f.position);
    for frame in 1..=360 {
        let position = Vec3::X * frame as f32 * 3.45 / 120.;
        gait.advance(
            model,
            position,
            0.,
            3.45,
            position + Vec3::X,
            1. / 120.,
            flat,
        );
        assert!(gait.reach_error < 0.06, "reach {}", gait.reach_error);
        assert!(gait.palette.iter().all(|m| m.is_finite()));
        if gait.phase > 0.15 && gait.phase < 0.6 {
            let support = 1 - gait.swing;
            assert!(gait.feet[support].position.distance(previous[support]) < 0.0001);
            contact_frames += 1;
        }
        if gait
            .feet
            .iter()
            .enumerate()
            .all(|(i, f)| f.position.y > gait.rig.ankles[i].y + 0.005)
        {
            flight_frames += 1;
        }
        for (i, foot) in gait.feet.iter().enumerate() {
            assert!(
                foot.position.distance(previous[i]) < 0.14,
                "step discontinuity"
            );
            let outward = if i == 0 {
                foot.position.x
            } else {
                -foot.position.x
            };
            assert!(
                (0.10..0.12).contains(&outward),
                "foot left its stance lane: {outward}"
            );
        }
        previous = gait.feet.map(|f| f.position);
    }
    assert!(contact_frames > 20);
    assert!(flight_frames > 20, "running never entered flight");
}

#[test]
fn jump_tucks_at_apex_reaches_before_landing_and_blends_on_contact() {
    let model = &pedestrian_catalog::definitions()[pedestrian_catalog::PLAYER_SUMMER_MODEL];
    let mut gait = Gait::new_player(model, Vec3::ZERO, 0., flat);
    for frame in 0..100 {
        let t = frame as f32 / 120.;
        gait.set_air_velocity(5.4 - 9.81 * t);
        gait.airborne(model, Vec3::Z, 0., JumpMotion::default(), 1. / 120.);
        if frame == 60 {
            assert!(gait.air_tuck() > 0.9);
        }
    }
    gait.set_air_velocity(-5.4);
    assert_eq!(gait.air_tuck(), 0.);
    let before = gait.palette;
    gait.land(model, Vec3::ZERO, 0., flat);
    for (a, b) in before.iter().zip(gait.palette) {
        assert!(a.abs_diff_eq(b, 0.0001));
    }
    for _ in 0..30 {
        gait.advance(model, Vec3::ZERO, 0., 0., Vec3::X, 1. / 120., flat);
        assert!(gait.palette.iter().all(|m| m.is_finite()));
    }
    assert!(gait.landing.is_none());

    // An immediate second jump must not freeze the preceding landing blend.
    gait.land(model, Vec3::ZERO, 0., flat);
    for _ in 0..24 {
        gait.prepare_jump(
            model,
            Vec3::ZERO,
            0.,
            JumpPreparation {
                compression: 0.5,
                extension: 0.,
                motion: JumpMotion::default(),
            },
            1. / 120.,
            flat,
        );
    }
    assert!(gait.landing.is_none());
}

#[test]
fn sustained_running_has_visible_forward_torso_lean() {
    let model = &pedestrian_catalog::definitions()[pedestrian_catalog::PLAYER_SUMMER_MODEL];
    let mut gait = Gait::new_player(model, Vec3::ZERO, 0., flat);
    for frame in 1..=240 {
        gait.set_running(true, 1. / 120.);
        let position = Vec3::X * frame as f32 * 3.45 / 120.;
        gait.advance(
            model,
            position,
            0.,
            3.45,
            position + Vec3::X,
            1. / 120.,
            flat,
        );
        if frame > 120 {
            let joints = model.joint_worlds(&gait.palette);
            let torso = joints[model.joint("Neck")].w_axis.truncate()
                - joints[model.joint("Hips")].w_axis.truncate();
            assert!(
                torso.z.atan2(torso.y) > 0.35,
                "run torso too upright: {torso:?}"
            );
            assert!(gait.reach_error < 0.06);
        }
    }
}

#[test]
fn rendered_player_feet_face_forward_in_both_outfits() {
    let models = pedestrian_catalog::definitions();
    let gait = Gait::new_player(
        &models[pedestrian_catalog::PLAYER_SUMMER_MODEL],
        Vec3::ZERO,
        0.,
        flat,
    );
    for index in [
        pedestrian_catalog::PLAYER_SUMMER_MODEL,
        pedestrian_catalog::PLAYER_WINTER_MODEL,
    ] {
        let model = &models[index];
        for side in ["L", "R"] {
            let foot = model.joint(&format!("Foot_{side}"));
            let toe = model.joint(&format!("Toe_{side}"));
            let points: Vec<Vec3> = model
                .vertices
                .iter()
                .zip(&model.skin)
                .filter_map(|(v, skin)| {
                    let foot_weight: f32 = skin
                        .joints
                        .iter()
                        .zip(skin.weights)
                        .filter(|(j, _)| **j as usize == foot || **j as usize == toe)
                        .map(|(_, w)| w)
                        .sum();
                    if v.position[1] >= 0.09 || foot_weight < 0.5 {
                        return None;
                    }
                    Some(
                        skin.joints
                            .iter()
                            .zip(skin.weights)
                            .fold(Vec3::ZERO, |p, (&j, w)| {
                                p + gait.palette[j as usize]
                                    .transform_point3(Vec3::from(v.position))
                                    * w
                            }),
                    )
                })
                .collect();
            assert!(points.len() > 20);
            let center = points.iter().copied().sum::<Vec3>() / points.len() as f32;
            let (mut xx, mut zz, mut xz) = (0., 0., 0.);
            for p in points {
                let d = p - center;
                xx += d.x * d.x;
                zz += d.z * d.z;
                xz += d.x * d.z;
            }
            let heading = 0.5 * (2. * xz).atan2(zz - xx);
            assert!(
                heading.abs() < 6f32.to_radians(),
                "outfit {index}, foot {side}: {} degrees",
                heading.to_degrees()
            );
        }
    }
}

#[test]
fn blocked_pivot_keeps_both_feet_facing_with_the_body() {
    let model = &pedestrian_catalog::definitions()[pedestrian_catalog::PLAYER_SUMMER_MODEL];
    for turn in [-2.62, 2.62] {
        let mut gait = Gait::new_player(model, Vec3::ZERO, 0., flat);
        let anchors = gait.feet.map(|foot| foot.position);
        // No landing sample is available at an edge or on rejected terrain.
        // Turning must still swivel the shoes without moving their anchors.
        for frame in 1..=360 {
            let yaw = frame as f32 * turn / 120.;
            gait.advance(
                model,
                Vec3::ZERO,
                yaw,
                0.,
                Vec3::new(yaw.cos(), yaw.sin(), 0.),
                1. / 120.,
                |_| None,
            );
            for (side, foot) in gait.feet.iter().enumerate() {
                assert_eq!(foot.position, anchors[side]);
                assert!(
                    angle_delta(yaw, foot.yaw).abs() <= 0.401,
                    "blocked turn {turn}, frame {frame}, foot {side}: heading {}",
                    angle_delta(yaw, foot.yaw)
                );
            }
            assert!(gait.palette.iter().all(|matrix| matrix.is_finite()));
        }
    }
}
