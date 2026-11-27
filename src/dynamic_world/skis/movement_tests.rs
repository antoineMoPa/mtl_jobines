use super::{super::pedestrian_gait::Ground, movement::*, pose::Pose};
use glam::Vec3;
fn slope_ground(p: Vec3, slope: f32) -> Ground {
    let a = slope.to_radians().tan();
    let height = |x: f32, y: f32| {
        -a * x
            + if a == 0. {
                0.
            } else {
                0.0007 * y * y + 0.45 * (x * 0.035).sin() * (y * 0.025).sin()
            }
    };
    let e = 0.15;
    Ground {
        point: p.truncate().extend(height(p.x, p.y)),
        gradient: [
            (height(p.x + e, p.y) - height(p.x - e, p.y)) / (2. * e),
            (height(p.x, p.y + e) - height(p.x, p.y - e)) / (2. * e),
        ],
    }
}
#[test]
fn back_shuffles_from_rest_without_snowplow_and_keeps_bindings_fixed() {
    let support = |p| Some(slope_ground(p, 0.));
    let mut m = Motion::new(Vec3::ZERO, 0., support(Vec3::ZERO).unwrap());
    m.input = super::super::WalkControls {
        forward: -1.,
        ..Default::default()
    }
    .into();
    for _ in 0..360 {
        m.step(1. / 120., &support, &|_| false);
        let pose = Pose::build(&mut m, &support);
        assert_eq!(m.brake, 0.);
        assert!(pose.binding_error < 0.001);
        let left = pose.gear[0].x_axis.truncate();
        let right = pose.gear[1].x_axis.truncate();
        assert!(left.dot(right) > 0.999, "reverse keeps the skis parallel");
    }
    assert!(m.position.x < -2., "reverse travel {}", m.position.x);
    assert!(
        (-2.0..-1.0).contains(&m.velocity.x),
        "reverse speed {}",
        m.velocity.x
    );
}

#[test]
fn back_brakes_forward_motion_then_reverses_but_space_remains_a_brake() {
    let support = |p| Some(slope_ground(p, 0.));
    let mut m = Motion::new(Vec3::ZERO, 0., support(Vec3::ZERO).unwrap());
    m.velocity = Vec3::X * 3.;
    m.input = super::super::WalkControls {
        forward: -1.,
        ..Default::default()
    }
    .into();
    m.step(1. / 120., &support, &|_| false);
    assert_eq!(m.brake, 1.);
    assert!((0.0..3.0).contains(&m.velocity.x));
    for _ in 0..360 {
        m.step(1. / 120., &support, &|_| false);
    }
    assert!(m.velocity.x < -1.);
    m.input = super::super::WalkControls {
        forward: -1.,
        jump: true,
        ..Default::default()
    }
    .into();
    for _ in 0..240 {
        m.step(1. / 120., &support, &|_| false);
    }
    assert_eq!(m.speed(), 0.);
    assert_eq!(m.brake, 1.);
}

#[test]
fn level_ground_turns_pivot_at_rest_and_keep_a_tight_glide_radius() {
    let support = |p| Some(slope_ground(p, 0.));
    for speed in [0., 1., 3.5, 7.] {
        for steer in [-1., 1.] {
            let mut m = Motion::new(Vec3::ZERO, 0., support(Vec3::ZERO).unwrap());
            m.velocity = Vec3::X * speed;
            m.input.steer = steer;
            for _ in 0..60 {
                m.step(1. / 120., &support, &|_| false);
            }
            assert!(m.yaw * steer < -0.85, "speed {speed}: yaw {}", m.yaw);
            assert!(m.travel / m.yaw.abs() < 4.5, "speed {speed}: turn too wide");
            let heading = Vec3::new(m.yaw.cos(), m.yaw.sin(), 0.);
            assert!(m.velocity.cross(heading).length() < 0.0001);
            if speed == 0. {
                assert_eq!(m.position, Vec3::ZERO, "pivot without inventing motion");
            }
        }
    }
}

#[test]
fn level_ground_shuffle_reaches_seven_metres_per_second_with_fixed_bindings() {
    let support = |p| Some(slope_ground(p, 0.));
    let mut m = Motion::new(Vec3::ZERO, 0., support(Vec3::ZERO).unwrap());
    m.input.shuffle = true;
    for _ in 0..1440 {
        m.step(1. / 120., &support, &|_| false);
        let pose = Pose::build(&mut m, &support);
        assert!(pose.binding_error < 0.001);
    }
    assert!((7.2..7.5).contains(&m.speed()), "speed {}", m.speed());
    assert!((72.0..76.0).contains(&m.travel), "travel {}", m.travel);
}

#[test]
fn movement_matches_reference_trajectories() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("trajectory-reference.json")).unwrap();
    for case in fixtures.as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let slope = case["slope"].as_f64().unwrap() as f32;
        let support = |p| Some(slope_ground(p, slope));
        let mut m = Motion::new(Vec3::ZERO, 0., support(Vec3::ZERO).unwrap());
        for segment in case["segments"].as_array().unwrap() {
            let input = &segment[1];
            m.input = Input {
                shuffle: input["shuffle"].as_bool().unwrap_or(false),
                backward: input["backward"].as_bool().unwrap_or(false),
                poles: input["pole"].as_bool().unwrap_or(false),
                steer: input["steer"].as_f64().unwrap_or(0.) as f32,
                brake: input["brake"].as_f64().unwrap_or(0.) as f32,
                tuck: input["tuck"].as_bool().unwrap_or(false),
            };
            for _ in 0..(segment[0].as_f64().unwrap() * 120.).round() as usize {
                m.step(1. / 120., &support, &|_| false);
            }
        }
        let p = Vec3::from_array(std::array::from_fn(|i| {
            case["position"][i].as_f64().unwrap() as f32
        }));
        assert!(
            m.position.distance(p) < 0.025,
            "{name}: {:?} != {p:?}; speed {}",
            m.position,
            m.speed()
        );
        assert!(
            (m.speed() - case["speed"].as_f64().unwrap() as f32).abs() < 0.003,
            "{name}: speed {}",
            m.speed()
        );
        assert!(
            (m.yaw - case["yaw"].as_f64().unwrap() as f32).abs() < 0.002,
            "{name}: yaw"
        );
    }
}
#[test]
fn every_supported_ski_pose_keeps_boots_fixed_and_poles_contact_only() {
    let def = &super::super::pedestrian_catalog::definitions()
        [super::super::pedestrian_catalog::PLAYER_WINTER_MODEL];
    for slope in [0., 16., 26.] {
        for steer in [-1., 0., 1.] {
            let support = |p| Some(slope_ground(p, slope));
            let mut m = Motion::new(Vec3::ZERO, 0., support(Vec3::ZERO).unwrap());
            m.input = Input {
                shuffle: true,
                steer,
                ..Default::default()
            };
            for frame in 0..600 {
                if frame == 240 {
                    m.input.shuffle = false;
                    m.input.tuck = true;
                }
                if frame == 480 {
                    m.input.tuck = false;
                    m.input.brake = 1.;
                }
                m.step(1. / 120., &support, &|_| false);
                let pose = Pose::build(&mut m, &support);
                assert!(
                    pose.binding_error < 0.001,
                    "slope {slope} steer {steer} tick {frame}: binding {}",
                    pose.binding_error
                );
                let joints = def.joint_worlds(&pose.palette);
                for (i, side) in ["L", "R"].into_iter().enumerate() {
                    let foot = pose.transform.transform_point3(
                        joints[def.joint(&format!("Foot_{side}"))].w_axis.truncate(),
                    ) * super::super::METERS_PER_UNIT;
                    let ski = pose.gear[i]
                        * glam::Mat4::from_translation(Vec3::new(
                            0.,
                            if i == 0 { 0.13 } else { -0.13 },
                            0.,
                        ));
                    assert!(
                        foot.distance(ski.transform_point3(Vec3::new(-0.005, 0., 0.105))) < 0.001
                    );
                    for d in [-0.85, 0., 0.85] {
                        let tip = ski.transform_point3(Vec3::X * d);
                        assert!(tip.z + 0.001 >= support(tip).unwrap().point.z);
                    }
                }
                if !m.contact {
                    assert_eq!(m.push, 0.);
                }
            }
        }
    }
}
#[test]
fn obstacles_and_missing_support_stop_the_skis_instead_of_tunneling_or_flying() {
    let ground = Ground {
        point: Vec3::ZERO,
        gradient: [0.; 2],
    };
    let mut m = Motion::new(Vec3::ZERO, 0., ground);
    m.velocity = Vec3::X * 20.;
    let support = |p: Vec3| {
        Some(Ground {
            point: p.truncate().extend(0.),
            ..ground
        })
    };
    for _ in 0..120 {
        m.step(1. / 120., &support, &|p| p.x >= 1.);
    }
    assert!(m.position.x < 1.);
    assert_eq!(m.speed(), 0.);
    m.velocity = Vec3::X * 20.;
    m.step(1. / 120., &|_| None, &|_| false);
    assert_eq!(m.speed(), 0.);
}

#[test]
fn turn_balance_and_quiet_arms_survive_the_coordinate_port() {
    let def = &super::super::pedestrian_catalog::definitions()
        [super::super::pedestrian_catalog::PLAYER_WINTER_MODEL];
    let support = |p| Some(slope_ground(p, 16.));
    for steer in [-1., 1.] {
        let mut m = Motion::new(Vec3::ZERO, 0., support(Vec3::ZERO).unwrap());
        m.velocity = Vec3::X * 10.;
        m.input = Input {
            steer,
            ..Default::default()
        };
        for _ in 0..84 {
            m.step(1. / 120., &support, &|_| false);
        }
        let p = Pose::build(&mut m, &support);
        let joints = def.joint_worlds(&p.palette);
        assert!(joints[def.joint("Hips")].w_axis.x * steer < -0.13);
        assert!(joints[def.joint("Chest")].w_axis.x * steer < -0.16);
        let hand_local = |joints: &[glam::Mat4; 32], side: &str| {
            joints[def.joint("Chest")]
                .inverse()
                .transform_point3(joints[def.joint(&format!("Hand_{side}"))].w_axis.truncate())
        };
        let hands = [hand_local(&joints, "L"), hand_local(&joints, "R")];
        m.phase = 0.17;
        m.stride_phase += 1.7;
        let p = Pose::build(&mut m, &support);
        let next = def.joint_worlds(&p.palette);
        for (i, side) in ["L", "R"].into_iter().enumerate() {
            assert!(hand_local(&next, side).distance(hands[i]) < 0.0001);
        }
        m.input.tuck = true;
        for _ in 0..180 {
            m.step(1. / 120., &support, &|_| false);
        }
        let p = Pose::build(&mut m, &support);
        let joints = def.joint_worlds(&p.palette);
        assert!(joints[def.joint("Hips")].w_axis.x * steer < -0.13);
        let hands = [hand_local(&joints, "L"), hand_local(&joints, "R")];
        m.phase = 0.6;
        m.stride_phase += 1.7;
        let p = Pose::build(&mut m, &support);
        let next = def.joint_worlds(&p.palette);
        for (i, side) in ["L", "R"].into_iter().enumerate() {
            assert!(hand_local(&next, side).distance(hands[i]) < 0.0001);
        }
    }
}

#[test]
fn actual_arm_reach_keeps_low_speed_poling_functional_and_contacts_fixed() {
    let support = |p: Vec3| {
        Some(Ground {
            point: p.truncate().extend(0.),
            gradient: [0.; 2],
        })
    };
    let mut m = Motion::new(Vec3::ZERO, 0., support(Vec3::ZERO).unwrap());
    m.input = Input {
        poles: true,
        ..Default::default()
    };
    let mut pushes = 0;
    let mut previous = None;
    for _ in 0..1440 {
        m.step(1. / 120., &support, &|_| false);
        let _pose = Pose::build(&mut m, &support);
        if m.push > 0. {
            assert!(m.contact && m.anchors.is_some());
            pushes += 1;
        }
        if let (Some(old), Some(now)) = (previous, m.anchors) {
            assert_eq!(old, now, "pole tips remain world anchored through a plant");
        }
        previous = m.anchors;
    }
    assert!(
        pushes > 30 && m.travel > 2. && m.speed() > 0.5,
        "poling must propel the real rig: {pushes} pushes, {}m, {}m/s",
        m.travel,
        m.speed()
    );
    m.input.tuck = true;
    for _ in 0..30 {
        m.step(1. / 120., &support, &|_| false);
        let _pose = Pose::build(&mut m, &support);
        assert_eq!(m.push, 0.);
        assert!(!m.contact);
    }
}

#[test]
fn traversing_a_slope_keeps_body_upright_and_each_ski_on_its_own_height() {
    let support = |p| Some(slope_ground(p, 20.));
    for yaw in [std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_2] {
        let mut motion = Motion::new(Vec3::ZERO, yaw, support(Vec3::ZERO).unwrap());
        let pose = Pose::build(&mut motion, &support);
        let body_up = pose.transform.transform_vector3(Vec3::Y);
        assert!(body_up.x.abs() < 1e-5 && body_up.y.abs() < 1e-5 && body_up.z > 0.);
        let skis = [0, 1].map(|i| {
            let side = if i == 0 { 0.13 } else { -0.13 };
            (pose.gear[i] * glam::Mat4::from_translation(Vec3::new(0., side, 0.)))
                .w_axis
                .truncate()
        });
        assert!((skis[0].z - skis[1].z).abs() > 0.08);
        for ski in skis {
            assert!((ski.z - support(ski).unwrap().point.z - 0.008).abs() < 0.002);
        }
        assert!(pose.binding_error < 0.001);
    }
}

#[test]
fn torso_and_head_stay_balanced_when_edging_across_a_steep_slope() {
    use super::super::pedestrian_catalog;
    let def = &pedestrian_catalog::definitions()[pedestrian_catalog::PLAYER_WINTER_MODEL];
    let support = |p| Some(slope_ground(p, 23.));
    for yaw in [std::f32::consts::FRAC_PI_2, -std::f32::consts::FRAC_PI_2] {
        let lateral = Vec3::new(-yaw.sin(), yaw.cos(), 0.);
        for (edge, hockey, tuck) in [(0.17, 0., 0.), (-0.17, 0., 1.), (0., 1., 0.), (0., -1., 0.)] {
            let mut motion = Motion::new(Vec3::ZERO, yaw, support(Vec3::ZERO).unwrap());
            motion.turn_edge = edge;
            motion.hockey_side = hockey;
            motion.hockey_blend = hockey.abs();
            motion.tuck_blend = tuck;
            let pose = Pose::build(&mut motion, &support);
            let joints = def.joint_worlds(&pose.palette);
            let joint_transform = |name| pose.transform * joints[def.joint(name)];
            let hip = joint_transform("Hips").w_axis.truncate();
            let chest = joint_transform("Chest").w_axis.truncate();
            let head_up = joint_transform("Head").transform_vector3(Vec3::Y);
            assert!((chest - hip).dot(lateral).abs() < (chest.z - hip.z) * 0.09);
            assert!(head_up.dot(lateral).abs() < head_up.z * 0.09);
            assert!(head_up.z > 0.);
            assert!(pose.binding_error < 0.001);
        }
    }
}
