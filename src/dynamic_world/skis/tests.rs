use super::*;
use crate::dynamic_world::{Controls, WalkControls};

fn map() -> Hermes {
    Hermes::test_map_from_source("road local:carlane;end local(-2,-1,2,-1); ground=land(-5,-5,5,-5,5,5,-5,5);elevation(ground,0.1);player_start(-0.2,0,0);pedestrian_start(0.01,0,180);ski_set(0,0,90);")
}
fn equipment(world: &DynamicWorld) -> Mesh {
    let mut mesh = Mesh::default();
    world.append_skis(&mut mesh, Mat4::IDENTITY);
    mesh
}
#[test]
fn winter_pickup_wears_all_six_parts_and_can_be_put_down() {
    let hermes = map();
    let mut world = DynamicWorld::new(&hermes);
    assert!(world.skis_hint().is_none());
    assert!(equipment(&world).vertices.is_empty());
    assert!(!world.toggle_skis());
    world.set_season(&hermes, Season::Winter);
    assert_eq!(world.skis_hint(), Some("Chausser les skis"));
    assert!(meshes().iter().all(|m| !m.vertices.is_empty()));
    let count: usize = meshes().iter().map(|m| m.vertices.len()).sum();
    assert_eq!(equipment(&world).vertices.len(), count);
    assert!(world.toggle_skis());
    assert!(world.is_wearing_skis() && world.skis.slots[0].ground.is_none());
    assert_eq!(world.skis_hint(), Some("Retirer les skis"));
    let worn = equipment(&world);
    assert_eq!(
        worn.vertices.len(),
        count,
        "one worn set, no duplicate pickup"
    );
    let before = world.player_position();
    for _ in 0..90 {
        world.step(
            &hermes,
            Controls::default(),
            WalkControls {
                forward: 1.,
                ..Default::default()
            },
            1. / 60.,
        );
    }
    assert!(world.player_position().distance(before) > 1.);
    assert_ne!(
        worn.vertices[0].position,
        equipment(&world).vertices[0].position
    );
    assert!(world.toggle_skis());
    assert!(!world.is_wearing_skis());
    let dropped = world.skis.slots[0].ground.unwrap().position;
    assert!(
        dropped
            .truncate()
            .distance(world.player_position().truncate())
            < 0.01
    );
    assert!(world.toggle_skis());
    world.set_season(&hermes, Season::Summer);
    assert!(!world.is_wearing_skis());
    assert!(equipment(&world).vertices.is_empty());
    world.set_season(&hermes, Season::Winter);
    assert_eq!(
        world.skis.slots[0].ground.unwrap().position,
        world.skis.slots[0].home.position
    );
}
#[test]
fn pickup_requires_proximity_and_a_healthy_player_on_foot() {
    let hermes = map();
    let mut world = DynamicWorld::new(&hermes);
    world.set_season(&hermes, Season::Winter);
    let original = world.pedestrian.as_ref().unwrap().position;
    world.pedestrian.as_mut().unwrap().position += Vec3::X * 5.;
    assert!(!world.toggle_skis());
    world.pedestrian.as_mut().unwrap().position = original + Vec3::Z * 4.;
    assert!(!world.toggle_skis(), "different floor");
    world.pedestrian.as_mut().unwrap().position = original;
    world.health = 0;
    assert!(!world.toggle_skis());
    world.health = super::super::melee::PLAYER_HEALTH;
    world.pedestrian.as_mut().unwrap().swimming = true;
    assert!(!world.toggle_skis());
    world.pedestrian.as_mut().unwrap().swimming = false;
    assert!(world.toggle_skis());
    world.reset_player(&hermes);
    assert!(!world.is_wearing_skis());
    assert_eq!(
        world.skis.slots[0].ground.unwrap().position,
        world.skis.slots[0].home.position
    );
    assert!(world.switch_vehicle(0));
    assert!(!world.toggle_skis(), "mounted players cannot equip");
}
#[test]
fn map_without_equipment_has_no_pickup_and_vehicle_switch_drops_worn_skis() {
    let hermes = Hermes::test_map();
    let mut world = DynamicWorld::new(&hermes);
    world.set_season(&hermes, Season::Winter);
    assert!(equipment(&world).vertices.is_empty());
    let hermes = map();
    let mut world = DynamicWorld::new(&hermes);
    world.set_season(&hermes, Season::Winter);
    assert!(world.toggle_skis());
    assert!(world.switch_vehicle(0));
    assert!(!world.is_wearing_skis());
    assert!(world.skis.slots[0].ground.is_some());
}
#[test]
fn authored_lift_pickups_are_near_both_stations_supported_and_clear() {
    let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
    let hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let mut world = DynamicWorld::new(&hermes);
    world.set_season(&hermes, Season::Winter);
    assert_eq!(world.skis.slots.len(), 2);
    for (index, station) in [
        glam::Vec2::new(380., -8260.),
        glam::Vec2::new(-220., -8520.),
    ]
    .into_iter()
    .enumerate()
    {
        let ground = world.skis.slots[index].home;
        assert!(
            ground.position.truncate().distance(station) < 12.,
            "pickup {index} should be beside its lift station"
        );
        let surface = hermes
            .city()
            .surface_at(
                (ground.position.truncate() / METERS_PER_UNIT).to_array(),
                f32::INFINITY,
            )
            .unwrap();
        assert_ne!(surface.kind, SurfaceKind::Water);
        let body = ground.position + Vec3::Z * player::HALF_HEIGHT;
        assert!(
            !player::Pedestrian::blocked(hermes.city(), body),
            "pickup {index} city clearance"
        );
        assert!(!world.props.blocked(body), "pickup {index} prop clearance");
        assert!(
            world
                .vehicles
                .iter()
                .all(|v| !player::blocked_by_vehicle(body, v)),
            "pickup {index} vehicle clearance"
        );
        assert!(
            world.chairlifts.iter().all(|lift| !lift.blocked(body)),
            "pickup {index} lift clearance"
        );
        world.pedestrian = Some(player::Pedestrian::new(hermes.city(), body, ground.yaw));
        assert_eq!(world.skis_hint(), Some("Chausser les skis"));
        assert!(world.toggle_skis());
        assert_eq!(world.skis.active_slot, index);
        assert!(world.skis.slots[1 - index].ground.is_some());
        assert!(world.toggle_skis());
    }
}

#[test]
fn skiing_dispatch_uses_contact_pose_rear_camera_and_space_braking_not_a_jump() {
    let h = map();
    let mut world = DynamicWorld::new(&h);
    world.set_season(&h, Season::Winter);
    world.prepare_skiing_run(&h, glam::Vec2::new(0., 10.), 0., 10.);
    let floor = world.player_position().z - player::HALF_HEIGHT;
    for _ in 0..42 {
        world.step(
            &h,
            Controls::default(),
            WalkControls {
                turn: 1.,
                ..Default::default()
            },
            1. / 60.,
        );
    }
    let m = world.skis.motion.as_ref().unwrap();
    assert!(m.yaw > 0. && m.position.y > 10.);
    assert!(m.turn_edge < -0.1);
    assert!(world.skis.pose.as_ref().unwrap().binding_error < 0.001);
    let (eye, target) = world.skiing_camera().unwrap();
    let forward = Vec3::new(m.yaw.cos(), m.yaw.sin(), 0.);
    let delta = eye * METERS_PER_UNIT - m.position;
    assert!(delta.dot(forward) < -6.);
    assert!(delta.dot(Vec3::new(-forward.y, forward.x, 0.)).abs() < 0.001);
    assert!((target * METERS_PER_UNIT - m.position).dot(forward) > 0.);
    assert_eq!(world.ski_snow_contacts(world.player_position()).count(), 2);
    assert!(!world.can_equip_weapon());
    for _ in 0..240 {
        world.step(
            &h,
            Controls::default(),
            WalkControls {
                jump: true,
                turn: 1.,
                ..Default::default()
            },
            1. / 120.,
        );
    }
    let m = world.skis.motion.as_ref().unwrap();
    assert!(m.hockey_side != 0.);
    assert!(m.speed() < 0.05);
    assert!(
        (world.player_position().z - player::HALF_HEIGHT - floor).abs() < 0.001,
        "space never starts a jump"
    );
    assert!(world.toggle_skis());
    assert!(world.skis.motion.is_none() && world.skis.pose.is_none());
    assert!(world.skiing_camera().is_none());
    assert_eq!(world.ski_snow_contacts(world.player_position()).count(), 0);
}

#[test]
fn separate_pickups_choose_nearest_keep_other_set_visible_and_reset_independently() {
    let h=Hermes::test_map_from_source("road local:carlane;end local(-2,-1,2,-1);ground=land(-5,-5,5,-5,5,5,-5,5);elevation(ground,0.1);player_start(-0.2,0,0);pedestrian_start(0.009,0,0);ski_set(0,0,0);ski_set(0.012,0,90);");
    let mut world = DynamicWorld::new(&h);
    world.set_season(&h, Season::Winter);
    let count: usize = meshes().iter().map(|m| m.vertices.len()).sum();
    assert_eq!(equipment(&world).vertices.len(), count * 2);
    let first = world.skis.slots[0].ground.unwrap().position;
    assert!(world.toggle_skis());
    assert_eq!(
        world.skis.active_slot, 1,
        "choose closest set when both are in range"
    );
    assert!(world.skis.slots[1].ground.is_none());
    assert_eq!(world.skis.slots[0].ground.unwrap().position, first);
    assert_eq!(
        equipment(&world).vertices.len(),
        count * 2,
        "one worn set plus the untouched ground set"
    );
    world.pedestrian.as_mut().unwrap().position += Vec3::Y * 5.;
    assert!(world.toggle_skis());
    assert!(
        world.skis.slots[1]
            .ground
            .unwrap()
            .position
            .truncate()
            .distance(world.player_position().truncate())
            < 0.001
    );
    assert_eq!(world.skis.slots[0].ground.unwrap().position, first);
    world.set_season(&h, Season::Summer);
    assert!(equipment(&world).vertices.is_empty());
    world.set_season(&h, Season::Winter);
    for slot in &world.skis.slots {
        assert_eq!(slot.ground.unwrap().position, slot.home.position);
    }
}
