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
fn port_matches_the_qa_approved_javascript_trajectories() {
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
            "{name}: {:?} != {p:?}",
            m.position
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
