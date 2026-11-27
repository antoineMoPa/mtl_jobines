use super::*;
use crate::{
    dynamic_world::{Controls, WalkControls},
    season::Season,
};
fn hill() -> Hermes {
    Hermes::test_map_from_source("road local:carlane;end local(-3,-1,3,-1);ground=land(-5,-5,5,-5,5,5,-5,5);elevation(ground,0.1);hill=terrain(0.5,-1,0.9,1,0.9,1,3,-1,3);player_start(-0.2,-1,0);pedestrian_start(0.04,0.05,90);chairlift_base(0,0);chairlift_top(0,2);")
}
fn at_boarding(world: &mut DynamicWorld, hermes: &Hermes) {
    world.prepare_chairlift_boarding(hermes);
}
fn catch_chair(world: &mut DynamicWorld, hermes: &Hermes) {
    for _ in 0..720 {
        world.step(
            hermes,
            Controls::default(),
            WalkControls::default(),
            1. / 60.,
        );
        if world.riding_chairlift() {
            return;
        }
    }
    panic!("next passing chair did not pick up the rider");
}
#[test]
fn ride_cuts_middle_keeps_player_seated_and_unloads_on_supported_ground() {
    let hermes = hill();
    let mut world = DynamicWorld::new(&hermes);
    at_boarding(&mut world, &hermes);
    assert!(world.can_load_chairlift());
    catch_chair(&mut world, &hermes);
    assert!(!world.toggle_vehicle(&hermes));
    assert!(!world.can_equip_weapon());
    let start = world.player_position();
    let mut before_cut = start;
    let mut after_cut = start;
    let mut max_fade: f32 = 0.;
    for frame in 0..480 {
        world.step(
            &hermes,
            Controls::default(),
            WalkControls {
                forward: 1.,
                jump: true,
                ..Default::default()
            },
            1. / 60.,
        );
        if frame == 228 {
            before_cut = world.player_position();
        }
        if frame == 252 {
            after_cut = world.player_position();
        }
        max_fade = max_fade.max(world.chairlift_fade());
        if frame < 478 {
            assert!(world.riding_chairlift());
            let rider = world.chairlift_rider_instance().unwrap();
            assert!(rider.pose.unwrap().iter().all(|m| m.is_finite()));
        }
    }
    // One extra step handles floating-point accumulation around exactly eight seconds.
    world.step(
        &hermes,
        Controls::default(),
        WalkControls::default(),
        1. / 60.,
    );
    assert!(!world.riding_chairlift());
    assert!(after_cut.distance(before_cut) > 150.);
    assert!(before_cut.distance(start) < 20.);
    assert!(max_fade > 0.95);
    let feet = world.player_position() - Vec3::Z * player::HALF_HEIGHT;
    let supported = ground(hermes.city(), feet).unwrap();
    assert!(feet.distance(supported) < 0.1);
    assert!(feet.distance(world.chairlifts.first().unwrap().top) < 20.);
    assert!(world.player_position().z > start.z + 20.);
}
#[test]
fn reset_and_vehicle_switch_cannot_leave_an_orphaned_rider() {
    let h = hill();
    let mut w = DynamicWorld::new(&h);
    at_boarding(&mut w, &h);
    catch_chair(&mut w, &h);
    assert!(!w.switch_vehicle(0));
    w.reset_player(&h);
    assert!(!w.riding_chairlift());
    assert_eq!(w.chairlift_fade(), 0.);
    at_boarding(&mut w, &h);
    catch_chair(&mut w, &h);
}
#[test]
fn all_models_are_shared_and_transform_finite_and_cable_clears_ground() {
    let h = hill();
    let w = DynamicWorld::new(&h);
    let assets = w.chairlift_model_meshes();
    assert!(assets.iter().all(|m| !m.vertices.is_empty()));
    assert!(w.chairlift_instances().iter().all(|(_, t)| t.is_finite()));
    let lift = w.chairlifts.first().unwrap();
    for p in &lift.path {
        let floor = ground(h.city(), *p).unwrap();
        assert!(
            p.z - floor.z >= 3.0,
            "cable below clearance at {p:?}, floor {floor:?}"
        );
    }
    for model in [
        pedestrian_catalog::PLAYER_SUMMER_MODEL,
        pedestrian_catalog::PLAYER_WINTER_MODEL,
    ] {
        let pose = seated_pose(model);
        assert!(pose.iter().all(|m| m.is_finite()));
        let def = &pedestrian_catalog::definitions()[model];
        let joints = def.joint_worlds(&pose);
        let hip = joints[def.joint("Hips")].w_axis.truncate();
        let ankle = joints[def.joint("Foot_L")].w_axis.truncate();
        assert!(hip.y > ankle.y + 0.35);
        assert!(ankle.z > hip.z + 0.2);
    }
}
#[test]
fn map_without_lift_has_no_interaction_or_geometry() {
    let h = Hermes::test_map();
    let w = DynamicWorld::new(&h);
    assert!(w.chairlifts.is_empty());
    assert!(w.chairlift_instances().is_empty());
}
#[test]
fn authored_route_starts_near_chalet_and_can_complete_real_hill_ride() {
    let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
    let h = Hermes::load_with_game_data(&path, None).unwrap();
    let mut w = DynamicWorld::new(&h);
    w.set_season(&h, Season::Winter);
    let l = w.chairlifts.first().expect("authored ski hill lift");
    assert!(l.base.truncate().distance(glam::Vec2::new(410., -8260.)) < 35.);
    assert!(l.top.z - l.base.z > 80.);
    eprintln!(
        "chairlift base {:?}, top {:?}, rise {}m, cable {}m",
        l.base,
        l.top,
        l.top.z - l.base.z,
        l.uphill_end
    );
    for p in &l.path {
        let floor = ground(h.city(), *p).unwrap();
        assert!(
            p.z - floor.z >= 2.7,
            "cable clearance at {p:?}, floor {floor:?}"
        );
    }
    at_boarding(&mut w, &h);
    catch_chair(&mut w, &h);
    for _ in 0..481 {
        w.step(&h, Controls::default(), WalkControls::default(), 1. / 60.);
    }
    assert!(!w.riding_chairlift());
    assert!(
        w.player_position()
            .distance(w.chairlifts.first().unwrap().landing + Vec3::Z * player::HALF_HEIGHT)
            < 0.1
    );
}

#[test]
fn seated_legs_have_bent_knees_level_thighs_and_upright_torso() {
    for model in [
        pedestrian_catalog::PLAYER_SUMMER_MODEL,
        pedestrian_catalog::PLAYER_WINTER_MODEL,
    ] {
        let def = &pedestrian_catalog::definitions()[model];
        let worlds = def.joint_worlds(&seated_pose(model));
        let point = |name: &str| worlds[def.joint(name)].w_axis.truncate();
        for side in ["L", "R"] {
            let hip = point(&format!("Thigh_{side}"));
            let knee = point(&format!("Shin_{side}"));
            let ankle = point(&format!("Foot_{side}"));
            let a = hip - knee;
            let b = ankle - knee;
            let angle = (a.dot(b) / (a.length() * b.length()))
                .clamp(-1., 1.)
                .acos()
                .to_degrees();
            eprintln!("model {model} {side}: hip {hip:?}; knee {knee:?}; ankle {ankle:?}; knee {angle:.1} degrees");
            assert!((80.0..115.0).contains(&angle));
            assert!((knee.y - hip.y).abs() < 0.10);
            assert!(knee.z - hip.z > 0.30);
            assert!(knee.y - ankle.y > 0.30);
        }
        let torso = point("Head") - point("Spine");
        assert!(torso.y > 0.40);
        assert!(torso.z.abs() < 0.08);
        assert!((point("Hips").y - 0.12).abs() < 0.005);
    }
}
#[test]
fn only_the_next_chair_picks_up_a_person_in_the_loading_lane() {
    let h = hill();
    let mut w = DynamicWorld::new(&h);
    at_boarding(&mut w, &h);
    let lift = &w.chairlifts[0];
    w.pedestrian.as_mut().unwrap().position += lift.right * 1.3;
    let lift = w.chairlifts.first_mut().unwrap();
    lift.clock = 1.;
    for _ in 0..120 {
        w.step(&h, Controls::default(), WalkControls::default(), 1. / 60.);
    }
    assert!(
        !w.riding_chairlift(),
        "standing in the lane must wait for a chair"
    );
    catch_chair(&mut w, &h);
    let lift = w.chairlifts.first().unwrap();
    let r = lift.ride.as_ref().unwrap();
    let (_, spacing) = lift.spacing();
    let (chair, _) = lift.at(lift.clock * SPEED + r.chair as f32 * spacing);
    let rider = w.player_position() - Vec3::Z * player::HALF_HEIGHT;
    let expected = chair + lift.right * r.seat_offset - Vec3::Z * 2.;
    assert!(rider.distance(expected) < 0.001);
    w.reset_player(&h);
    at_boarding(&mut w, &h);
    let right = w.chairlifts.first().unwrap().right;
    w.pedestrian.as_mut().unwrap().position += right * 2.;
    for _ in 0..720 {
        w.step(&h, Controls::default(), WalkControls::default(), 1. / 60.);
    }
    assert!(
        !w.riding_chairlift(),
        "beside the lane is not a loading position"
    );
}

#[test]
fn ramp_leads_to_the_loading_deck_without_a_jump_or_boarding_action() {
    let h = hill();
    let mut w = DynamicWorld::new(&h);
    let lift = w.chairlifts.first().unwrap();
    let start = ground(
        h.city(),
        lift.base + lift.right * TRACK_HALF + lift.forward * 7.3,
    )
    .unwrap();
    let yaw = lift.yaw + PI;
    w.pedestrian = Some(player::Pedestrian::new(
        h.city(),
        start + Vec3::Z * player::HALF_HEIGHT,
        yaw,
    ));
    for _ in 0..360 {
        let lift = w.chairlifts.first().unwrap();
        let ahead = (w.player_position() - lift.base).dot(lift.forward);
        if ahead < 0.5 {
            break;
        }
        w.step(
            &h,
            Controls::default(),
            WalkControls {
                forward: 1.,
                ..Default::default()
            },
            1. / 60.,
        );
    }
    if !w.riding_chairlift() {
        let l = w.chairlifts.first().unwrap();
        let feet = w.player_position() - Vec3::Z * player::HALF_HEIGHT;
        assert!((feet.z - l.base.z - 0.6).abs() < 0.03);
        assert!((feet - l.boarding()).truncate().length() < 0.8);
        catch_chair(&mut w, &h);
    }
    assert!(w.riding_chairlift());
}

#[test]
fn side_ramp_reaches_the_wider_loading_area_on_skis() {
    let h = hill();
    let mut w = DynamicWorld::new(&h);
    w.set_season(&h, Season::Winter);
    let lift = &w.chairlifts[0];
    let start = lift.base + lift.right * 11. + lift.forward * BOARDING_OFFSET;
    // Stop inside the new outline, outside the previous boarding trigger.
    let target = TRACK_HALF + 1.3;
    let yaw = (-lift.right.y).atan2(-lift.right.x);
    w.prepare_skiing_run(&h, start.truncate(), yaw, 1.5);
    for _ in 0..480 {
        let lift = &w.chairlifts[0];
        let across = (w.player_position() - lift.base).dot(lift.right);
        if w.riding_chairlift() || across < target {
            break;
        }
        w.step(
            &h,
            Controls::default(),
            WalkControls {
                forward: 1.,
                ..Default::default()
            },
            1. / 60.,
        );
    }
    if !w.riding_chairlift() {
        let lift = &w.chairlifts[0];
        let feet = w.player_position() - Vec3::Z * player::HALF_HEIGHT;
        assert!(
            (feet.z - lift.base.z - DECK_HEIGHT).abs() < 0.03,
            "{feet:?}"
        );
        assert!((feet - lift.boarding()).dot(lift.right).abs() < BOARDING_HALF_WIDTH);
        assert!(w.is_wearing_skis());
    }
}

#[test]
fn ski_mission_starts_on_the_lower_side_ramp() {
    let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
    let h = Hermes::load_with_game_data(&path, None).unwrap();
    let mut world = DynamicWorld::new(&h);
    let lift = &world.chairlifts[0];
    let pickup = h.ski_sets[0].point;
    let terrain = ground(
        h.city(),
        Vec3::new(pickup[0], pickup[1], 0.) * METERS_PER_UNIT,
    )
    .unwrap();
    let ramp = lift.side_ramps[0]
        .as_ref()
        .unwrap()
        .surface(terrain, lift.right, lift.forward)
        .unwrap();
    assert!(ramp.point.z > terrain.z);

    world.start_skiing_at_base(&h).unwrap();
    let expected = ramp.point + Vec3::Z * player::HALF_HEIGHT;
    assert!(world.player_position().distance(expected) < 0.01);
    world.step(&h, Controls::default(), WalkControls::default(), 1. / 60.);
    assert!(world.player_position().distance(expected) < 0.01);
}

#[test]
fn both_authored_lifts_board_and_return_to_the_imported_pistes() {
    let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
    let h = Hermes::load_with_game_data(&path, None).unwrap();
    assert_eq!(h.chairlifts.len(), 2);
    for line in 0..2 {
        let mut w = DynamicWorld::new(&h);
        assert_eq!(
            w.chairlifts.len(),
            2,
            "every route must have valid supports and a clear landing"
        );
        let lift = &w.chairlifts[line];
        assert!(lift.top.z > lift.base.z + 5.);
        let point = lift.landing.truncate() / METERS_PER_UNIT;
        assert_eq!(
            h.ski_hill.as_ref().unwrap().area_at(point.to_array()),
            Some(crate::hermes::ski_hill::Area::Piste)
        );
        if line == 0 {
            // The chalet chair should unload on the crest, with downhill snow
            // available in several directions from the dismount.
            let downhill_piste_directions = (0..8)
                .filter(|&step| {
                    let direction =
                        glam::Vec2::from_angle(step as f32 * std::f32::consts::TAU / 8.);
                    let sample = (point + direction * (20. / METERS_PER_UNIT)).to_array();
                    h.ski_hill.as_ref().unwrap().area_at(sample)
                        == Some(crate::hermes::ski_hill::Area::Piste)
                        && h.city()
                            .surface_at(sample, f32::INFINITY)
                            .is_some_and(|surface| {
                                surface.height * METERS_PER_UNIT < lift.landing.z - 0.25
                            })
                })
                .count();
            assert!(
                downhill_piste_directions >= 6,
                "{downhill_piste_directions} downhill piste directions"
            );
            assert!(!w
                .chairlifts
                .iter()
                .any(|lift_station| lift_station
                    .blocked(lift.landing + Vec3::Z * player::HALF_HEIGHT)));
        }
        let feet = lift.boarding();
        let yaw = lift.yaw;
        w.pedestrian = Some(player::Pedestrian::new(
            h.city(),
            feet + Vec3::Z * player::HALF_HEIGHT,
            yaw,
        ));
        catch_chair(&mut w, &h);
        assert!(w.chairlifts[line].ride.is_some());
        for _ in 0..481 {
            w.step(&h, Controls::default(), WalkControls::default(), 1. / 60.);
        }
        assert!(!w.riding_chairlift());
        assert!(
            w.player_position()
                .distance(w.chairlifts[line].landing + Vec3::Z * player::HALF_HEIGHT)
                < 0.1
        );
    }
    let groups: Vec<_> = h
        .external_models
        .iter()
        .filter(|g| g.mesh.vertices.len() == 108)
        .collect();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].transforms.len(), 366);
    assert_eq!(groups[0].mesh.vertices.len() / 3, 36);
}

#[test]
fn station_foundations_embed_in_terrain_and_keep_the_loading_deck_height() {
    let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
    let h = Hermes::load_with_game_data(&path, None).unwrap();
    let w = DynamicWorld::new(&h);
    for model in &model_assets()[2..] {
        let (lo, _) = external_models::bounds(model);
        assert!(lo.z <= -3.99);
    }
    for lift in &w.chairlifts {
        for (center, sign, width, back) in [(lift.base, 1., 5.2, 11.1), (lift.top, -1., 4.7, 9.1)] {
            for x in [-width, width] {
                for y in [-back, 3.7] {
                    let p = center + lift.right * x + lift.forward * y * sign;
                    let terrain = ground(h.city(), p).unwrap();
                    assert!(
                        center.z - 4. < terrain.z - 0.2,
                        "foundation above terrain at {p:?}"
                    );
                }
            }
            let deck = center + lift.right * 2.4 + lift.forward * sign * 0.3 + Vec3::Z * 0.6;
            let support = lift.platform(deck).unwrap();
            assert!((support.point.z - center.z - 0.6).abs() < 0.001);
        }
    }
}
