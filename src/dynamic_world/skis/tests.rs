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
    assert!(world.is_wearing_skis() && world.skis.ground.is_none());
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
    let dropped = world.skis.ground.unwrap().position;
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
        world.skis.ground.unwrap().position,
        world.skis.home.unwrap().position
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
        world.skis.ground.unwrap().position,
        world.skis.home.unwrap().position
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
    assert!(world.skis.ground.is_some());
}
#[test]
fn authored_chalet_pickup_is_on_supported_clear_ground() {
    let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
    let hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let world = DynamicWorld::new(&hermes);
    let ground = world.skis.home.expect("ski set by the chalet");
    assert!(
        ground
            .position
            .truncate()
            .distance(glam::Vec2::new(410., -8252.))
            < 0.1
    );
    let support = hermes
        .city()
        .surface_at(
            (ground.position.truncate() / METERS_PER_UNIT).to_array(),
            f32::INFINITY,
        )
        .unwrap();
    assert_ne!(support.kind, SurfaceKind::Water);
    assert!(!world
        .props
        .blocked(ground.position + Vec3::Z * player::HALF_HEIGHT));
    assert!(world
        .vehicles
        .iter()
        .all(|v| !player::blocked_by_vehicle(ground.position + Vec3::Z * player::HALF_HEIGHT, v)));
}
