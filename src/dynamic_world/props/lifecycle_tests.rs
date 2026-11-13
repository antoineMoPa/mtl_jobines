use super::*;

fn mesh(x: f32) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.cuboid(
        [(x - 0.2) / METERS_PER_UNIT, -0.002, 0.],
        [(x + 0.2) / METERS_PER_UNIT, 0.002, 0.01],
        [0.5; 3],
        SurfaceKind::Prop,
    );
    mesh
}

fn strike(props: &mut Props, city: &City) {
    let mut car = Vehicle::spawn(city);
    car.yaw = 0.;
    car.velocity = Vec3::X * 12.;
    car.position = Vec3::new(4., 0., car.ride_height());
    let before = Vec3::new(-4., 0., car.ride_height());
    props.vehicle_move(&mut car, before, city);
    assert!(car.collisions > 0);
}

#[test]
fn repeated_departure_and_return_restore_collision_without_retaining_debris() {
    let hermes = crate::hermes::Hermes::test_map();
    let mut props = Props::from_groups_near(vec![vec![mesh(0.)]], Vec3::ZERO);
    let at = Vec3::new(0., 0., 0.5);
    for _ in 0..24 {
        assert!(props.blocked(at));
        strike(&mut props, hermes.city());
        assert!(!props.group_intact(0));
        assert!(!props.blocked(at));
        props.update_focus(Vec3::X * 2_000.);
        assert!(props.group_intact(0));
        assert!(props.active.is_empty());
        assert!(props.damaged.is_empty());
        assert!(props.debris_bodies.is_empty());
        assert!(props.debris_colliders.is_empty());
        assert!(props.colliders.is_empty());
        props.update_focus(Vec3::ZERO);
        assert_eq!(props.colliders.len(), 1);
        assert_eq!(props.visible_batches(at)[0].ids, vec![0]);
    }
}

#[test]
fn nearby_expired_debris_does_not_respawn_until_the_player_leaves() {
    let hermes = crate::hermes::Hermes::test_map();
    let mut props = Props::from_groups_near(vec![vec![mesh(0.)]], Vec3::ZERO);
    strike(&mut props, hermes.city());
    for _ in 0..1201 {
        props.update_focus(Vec3::ZERO);
        props.step(1. / 120.);
    }
    assert!(!props.group_intact(0));
    assert!(props.debris_bodies.is_empty());
    assert!(props.debris_colliders.is_empty());
    props.update_focus(Vec3::X * 2_000.);
    props.update_focus(Vec3::ZERO);
    assert!(props.blocked(Vec3::new(0., 0., 0.5)));
}

#[test]
fn a_displaced_piece_near_the_player_prevents_assembly_reset() {
    let hermes = crate::hermes::Hermes::test_map();
    let mut props = Props::from_groups_near(vec![vec![mesh(0.)]], Vec3::ZERO);
    strike(&mut props, hermes.city());
    let body = props.items[0].debris.unwrap();
    props.debris_bodies[body].set_translation(vector![2_000., 0., 0.5], true);
    props.update_focus(Vec3::X * 2_000.);
    assert!(!props.group_intact(0));
    assert!(props.items[0].debris.is_some());
    props.update_focus(Vec3::X * 4_000.);
    assert!(props.group_intact(0));
    assert!(props.debris_bodies.is_empty());
    assert!(props.debris_colliders.is_empty());
}

#[test]
fn collision_residency_depends_on_nearby_tiles_and_survives_teleports() {
    let groups = (-300..=300)
        .map(|tile| vec![mesh(tile as f32 * TILE)])
        .collect();
    let mut props = Props::from_groups_near(groups, Vec3::ZERO);
    assert_eq!(props.colliders.len(), 5);
    for tile in [-200, -31, -1, 0, 1, 31, 200, 0] {
        let at = Vec3::new(tile as f32 * TILE, 0., 0.5);
        props.update_focus(at);
        assert_eq!(props.colliders.len(), 5, "city-wide colliders accumulated");
        assert!(props.blocked(at), "incoming query tree was not refreshed");
        assert!(!props.blocked(at + Vec3::X * 3. * TILE));
        let handles: Vec<_> = props.colliders.iter().map(|(handle, _)| handle).collect();
        props.update_focus(at + Vec3::X);
        assert_eq!(
            handles,
            props
                .colliders
                .iter()
                .map(|(handle, _)| handle)
                .collect::<Vec<_>>()
        );
    }
    props.update_focus(Vec3::Y * 4_000.);
    assert!(props.colliders.is_empty());
    assert!(props
        .query
        .cast_ray(
            &props.bodies,
            &props.colliders,
            &Ray::new(point![0., 0., 10.], vector![0., 0., -1.]),
            20.,
            true,
            QueryFilter::default()
        )
        .is_none());
}

#[test]
fn a_large_assembly_has_a_fixed_live_body_and_floor_budget() {
    let hermes = crate::hermes::Hermes::test_map();
    let mut props = Props::from_groups_near(vec![vec![mesh(0.); MAX_DEBRIS + 32]], Vec3::ZERO);
    strike(&mut props, hermes.city());
    assert_eq!(props.active.len(), MAX_DEBRIS);
    assert_eq!(props.debris_bodies.len(), MAX_DEBRIS);
    assert_eq!(props.debris_colliders.len(), MAX_DEBRIS * 2);
    props.update_focus(Vec3::X * 2_000.);
    assert!(props.active.is_empty());
    assert!(props.debris_bodies.is_empty());
    assert!(props.debris_colliders.is_empty());
    props.update_focus(Vec3::ZERO);
    assert_eq!(props.colliders.len(), MAX_DEBRIS + 32);
}
