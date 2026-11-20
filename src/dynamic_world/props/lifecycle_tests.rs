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

#[test]
fn a_slow_cone_bump_topples_one_weighted_body_and_resets_after_departure() {
    let hermes = crate::hermes::Hermes::test_map();
    let placement = crate::props::Placement {
        kind: crate::props::Kind::Cone,
        position: Vec3::ZERO,
        heading: 0.,
        sign: None,
    };
    let mut props = Props::from_parts_near(vec![placement.parts()], Vec3::ZERO);
    let mut car = Vehicle::spawn(hermes.city());
    car.yaw = 0.;
    car.velocity = Vec3::X;
    car.position = Vec3::new(2., 0., car.ride_height());
    let before = Vec3::new(-3., 0., car.ride_height());
    props.vehicle_move(&mut car, before, hermes.city());
    assert!(!props.group_intact(0));
    assert!(car.velocity.x > 0.9, "a cone should barely slow the car");
    assert_eq!(props.debris_bodies.len(), 1);
    assert_eq!(props.debris_colliders.len(), 3);
    assert_eq!(props.authored_instances(Vec3::ZERO).len(), 1);
    let body = props.items[0].debris.unwrap();
    assert!((props.debris_bodies[body].mass() - 4.).abs() < 0.01);
    for _ in 0..60 {
        props.step(1. / 120.);
    }
    assert!(props.debris_bodies[body].rotation().angle() > 0.01);
    props.update_focus(Vec3::X * 2_000.);
    assert!(props.debris_bodies.is_empty());
    assert!(props.debris_colliders.is_empty());
    props.update_focus(Vec3::ZERO);
    assert!(props.group_intact(0));
    assert!(props.blocked(Vec3::new(0., 0., 0.5)));
    assert_eq!(props.authored_instances(Vec3::ZERO).len(), 1);
}

#[test]
fn a_second_pass_pushes_a_fallen_cone_instead_of_ignoring_it() {
    let hermes = crate::hermes::Hermes::test_map();
    let placement = crate::props::Placement {
        kind: crate::props::Kind::Cone,
        position: Vec3::ZERO,
        heading: 0.,
        sign: None,
    };
    let mut props = Props::from_parts_near(vec![placement.parts()], Vec3::ZERO);
    strike(&mut props, hermes.city());
    let handle = props.items[0].debris.unwrap();
    props.debris_bodies[handle].set_translation(vector![0., 0., props.items[0].half.z], true);
    props.debris_bodies[handle].set_linvel(vector![0., 0., 0.], true);
    props.debris_bodies[handle].set_angvel(vector![0., 0., 0.], true);
    props.step(1. / 120.);
    let mut car = Vehicle::spawn(hermes.city());
    car.yaw = 0.;
    car.velocity = Vec3::X * 5.;
    car.position = Vec3::new(3., 0., car.ride_height());
    let before = Vec3::new(-3., 0., car.ride_height());
    props.vehicle_move(&mut car, before, hermes.city());
    assert!(
        props.debris_bodies[handle].linvel().x > 2.,
        "velocity {:?}; car contacts {}",
        props.debris_bodies[handle].linvel(),
        car.collisions
    );
    assert!(car.collisions > 0);
    assert!(car.velocity.x > 4.8);
}

#[test]
fn distant_copies_share_model_storage_and_only_nearby_collision_is_resident() {
    let groups = (0..1000)
        .map(|i| {
            crate::props::Placement {
                kind: crate::props::Kind::Cone,
                position: Vec3::X * (i as f32 * 2.),
                heading: 0.,
                sign: None,
            }
            .parts()
        })
        .collect();
    let props = Props::from_parts_near(groups, Vec3::ZERO);
    assert_eq!(props.items.len(), 1000);
    assert!(std::sync::Arc::ptr_eq(
        &props.items[0].mesh.mesh,
        &props.items[999].mesh.mesh
    ));
    assert!(props.colliders.len() <= 3);
    assert_eq!(props.authored_instances(Vec3::ZERO).len(), 1);
    assert!(props.debris_bodies.is_empty());
}

#[test]
fn airport_fence_stops_gentle_contacts_and_detaches_only_the_struck_quad() {
    let mut hermes = crate::hermes::Hermes::test_map_from_source(
        "airport_shore=land(-6,-22,6,-22,6,-2,-6,-2); player_start(0,-10,0);",
    );
    let (static_world, _) = hermes.take_geometry();
    assert!(!static_world
        .vertices
        .iter()
        .any(|v| v.surface[0] == SurfaceKind::Fence as u32 as f32));
    let groups = hermes.prop_parts();
    let center_of = |panel: &Mesh| {
        let a = Vec3::from_array(panel.vertices[0].position);
        let b = Vec3::from_array(panel.vertices[2].position);
        (a + b) * (METERS_PER_UNIT * 0.5)
    };
    let index = groups
        .iter()
        .position(|parts| {
            let vertices = &parts[0].mesh.vertices;
            vertices.len() == 6
                && vertices[0].surface[0] == SurfaceKind::Fence as u32 as f32
                && vertices[0].position[0] == -5.
                && vertices[0].position[1] < -10.
        })
        .unwrap();
    let at = center_of(&groups[index][0].mesh);
    let neighbor = center_of(&groups[index + 1][0].mesh);
    let appearance: Vec<_> = groups[index][0]
        .mesh
        .vertices
        .iter()
        .map(|v| v.surface)
        .collect();
    let mut props = Props::from_parts_near(groups, at);
    assert!(props.blocked(at) && props.blocked(neighbor));
    let city = hermes.city();
    let mut car = Vehicle::spawn_at(
        city,
        crate::dynamic_world::SpawnPoint {
            point: ((at.truncate() - glam::Vec2::X * 5.) / METERS_PER_UNIT).to_array(),
            heading: Some(0.),
        },
    )
    .unwrap();
    let before = car.position;
    for speed in [1., 12.] {
        car.position = before + Vec3::X * 10.;
        car.velocity = Vec3::X * speed;
        props.vehicle_move(&mut car, before, city);
        if speed == 1. {
            assert!(car.position.x < at.x && props.blocked(at));
        }
    }
    assert_eq!(props.damaged_groups().collect::<Vec<_>>(), [index]);
    assert!(!props.blocked(at) && props.blocked(neighbor));
    props.step(1. / 120.);
    let mut debris = Mesh::default();
    props.append_moving(&mut debris, at);
    assert_eq!(
        debris
            .vertices
            .iter()
            .map(|v| v.surface)
            .collect::<Vec<_>>(),
        appearance
    );
}

#[test]
fn gun_impact_topples_and_can_restrike_a_cone() {
    let h = crate::hermes::Hermes::test_map();
    let placement = crate::props::Placement {
        kind: crate::props::Kind::Cone,
        position: Vec3::ZERO,
        heading: 0.,
        sign: None,
    };
    let mut props = Props::from_parts_near(vec![placement.parts()], Vec3::ZERO);
    let origin = Vec3::new(-2., 0., 1.45);
    let (id, t, _) = props.ray_hit(origin, Vec3::X, 5.).unwrap();
    assert_eq!(
        props.shot_surface(id),
        crate::dynamic_world::ShotSurface::Rubber
    );
    props.strike_prop(id, origin + Vec3::X * t, Vec3::X, 1., h.city());
    let body = props.items[id].debris.unwrap();
    assert!(props.debris_bodies[body].linvel().x > 0.5);
    assert!(props.debris_bodies[body].angvel().norm() > 0.1);
    props.step(1. / 60.);
    let (again, t, _) = props.ray_hit(origin, Vec3::X, 5.).unwrap();
    assert_eq!(id, again);
    let old_speed = props.debris_bodies[body].linvel().x;
    props.strike_prop(id, origin + Vec3::X * t, Vec3::X, 1., h.city());
    assert_eq!(props.debris_bodies.len(), 1);
    assert!(props.debris_bodies[body].linvel().x > old_speed);
}
