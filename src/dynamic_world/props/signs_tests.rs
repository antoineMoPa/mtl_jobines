use super::*;
use crate::props::{Kind, Placement, SignDesign};

#[test]
fn service_road_destination_signs_point_along_the_road_report_81() {
    let program = opencity::parse(include_str!("../../../data/maps/mtl/map.road")).unwrap();
    let entries = crate::props::read(&program).unwrap();
    let projection = opencity::map::project(&program);
    for (sign, road) in [
        ("sign_ramp_north_destination", "service_north_segment"),
        ("sign_ramp_south_destination", "service_south_segment"),
    ] {
        let entry = entries.iter().find(|e| e.name == sign).unwrap();
        let crate::props::Shape::Single(p) = &entry.shape else {
            panic!("{sign} must be a single sign");
        };
        // These signs sit past the junction, beside a continuing service road.
        // A right arrow sends drivers into a forecourt instead of onward.
        assert!(
            matches!(
                p.sign,
                Some(SignDesign::Highway {
                    arrow: crate::props::SignArrow::Straight,
                    ..
                })
            ),
            "{sign} must direct traffic straight ahead"
        );
        let segment = projection
            .segments
            .iter()
            .find(|s| s.name.as_deref() == Some(road))
            .unwrap();
        let tangent = segment.tangent_at(0.5);
        let travel = Vec3::new(tangent.x, tangent.y, 0.);
        let facing_travel = p.transform().transform_vector3(Vec3::Y);
        let alignment = facing_travel.dot(travel);
        assert!(
            alignment > 0. && alignment * alignment > 0.99 * travel.length_squared(),
            "{sign} must face approaching traffic on {road}"
        );
    }
}

#[test]
fn sign_geometry_is_not_baked_twice_and_hidden_signs_have_no_colliders() {
    let base = "road r: carlane; end r(-1,0,1,0); land(-2,-2,2,-2,2,2,-2,2);";
    let mut plain = crate::hermes::Hermes::test_map_from_source(base);
    let mut signed = crate::hermes::Hermes::test_map_from_source(&format!(
        "{base} a=prop(stop_sign,0,0); b=prop(stop_sign,0,0);"
    ));
    let geometry = |h: &mut crate::hermes::Hermes| {
        let (a, b) = h.take_geometry();
        [a, b]
            .into_iter()
            .flat_map(|m| m.vertices)
            .map(|v| {
                [
                    v.position.as_slice(),
                    v.normal.as_slice(),
                    v.color.as_slice(),
                    v.surface.as_slice(),
                ]
                .concat()
                .into_iter()
                .map(f32::to_bits)
                .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        geometry(&mut plain),
        geometry(&mut signed),
        "signs must use the prop renderer exclusively"
    );
    let signs: Vec<_> = signed
        .prop_parts()
        .into_iter()
        .filter(|p| {
            p.first()
                .is_some_and(|p| p.template.is_some_and(|(k, _)| k == Kind::StopSign))
        })
        .collect();
    assert_eq!(
        signs.len(),
        1,
        "coincident authored signs must produce one assembly"
    );
    let props = Props::from_parts_near(signs, Vec3::ZERO);
    assert_eq!(props.colliders.len(), 2);
    for batch in props.visible_batches(Vec3::ZERO) {
        assert!(
            batch.ids.is_empty(),
            "templated signs must not enter the baked GPU batches"
        );
    }
    let blocked=crate::hermes::Hermes::test_map_from_source(&format!("{base} house=building(0.05,-0.02,-0.02,0.02,-0.02,0.02,0.02,-0.02,0.02); a=prop(stop_sign,0,0);"));
    assert!(
        blocked
            .prop_parts()
            .iter()
            .flatten()
            .all(|p| p.sign_profile.is_none()),
        "hidden signs must have neither meshes nor physics parts"
    );
}

#[test]
fn current_map_contains_all_sign_families_with_supported_mounts() {
    let source = include_str!("../../../data/maps/mtl/map.road");
    let program = opencity::parse(source).unwrap();
    let entries = crate::props::read(&program).unwrap();
    let hermes = crate::hermes::Hermes::load_with_game_data(
        std::path::Path::new("data/maps/mtl/map.road"),
        None,
    )
    .unwrap();
    let signs: Vec<_> = entries
        .iter()
        .filter(|e| SignDesign::for_kind(e.kind()).is_some())
        .collect();
    assert!(signs.len() >= 35);
    let mut failures = Vec::new();
    for kind in [
        Kind::StopSign,
        Kind::SpeedSign,
        Kind::OneWaySign,
        Kind::YieldSign,
        Kind::CurveSign,
        Kind::HighwaySign,
        Kind::ParkingSign,
    ] {
        assert!(signs.iter().any(|e| e.kind() == kind), "missing {:?}", kind);
    }
    for (_, _, p) in crate::props::visible_placements(&entries, hermes.city())
        .into_iter()
        .filter(|(_, _, p)| p.kind.is_road_sign())
    {
        let ground = hermes
            .city()
            .surface_at(p.position.truncate().to_array(), p.position.z + 0.02);
        if let Some(ground) = ground {
            if (ground.height - p.position.z).abs() > 0.006 || ground.kind == SurfaceKind::Water {
                failures.push(format!(
                    "{}: base={}, surface={}",
                    format!("{:?} at {:?}", p.kind, p.position),
                    p.position.z,
                    ground.height
                ));
            }
        } else {
            failures.push(format!(
                "{:?} at {:?}: no supporting surface",
                p.kind, p.position
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
fn placement(design: SignDesign, position: Vec3, heading: f32) -> Placement {
    Placement {
        kind: design.kind(),
        position,
        heading,
        sign: Some(design),
    }
}
#[test]
fn signs_share_collision_shapes_and_leave_space_below_panels() {
    let design = SignDesign::Highway {
        destination: "Québec".into(),
        secondary: "Montréal".into(),
        exit: "12".into(),
        arrow: crate::props::SignArrow::ExitRight,
    };
    let groups = vec![
        placement(design.clone(), Vec3::ZERO, 45.).parts(),
        placement(design, Vec3::new(0.1, 0., 0.), 90.).parts(),
    ];
    let props = Props::from_parts_near(groups, Vec3::ZERO);
    assert!(
        !props.blocked(Vec3::new(0., 0., 0.5)),
        "gap between highway posts must remain open"
    );
    let a = props.items[2].sign.as_ref().unwrap();
    let b = props.items[5].sign.as_ref().unwrap();
    assert!(std::ptr::eq(a.shape.as_ref(), b.shape.as_ref()));
    assert_eq!(a.mass, 42.);
    assert!(
        (props.colliders[props.items[2].collider.unwrap()]
            .rotation()
            .angle()
            - std::f32::consts::FRAC_PI_4)
            .abs()
            < 1e-5
    );
    assert_eq!(props.authored_instances(Vec3::ZERO).len(), 6);
}
#[test]
fn pole_hit_detaches_cluster_once_then_expires_and_resets_without_cones() {
    let hermes = crate::hermes::Hermes::test_map();
    let parts = placement(SignDesign::Stop { cluster: true }, Vec3::ZERO, 47.).parts();
    let mut props = Props::from_parts_near(vec![parts], Vec3::ZERO);
    strike(&mut props, hermes.city());
    assert_eq!(props.active.len(), 4);
    assert!(props.colliders.is_empty());
    for (i, item) in props.items.iter().enumerate() {
        let handle = item.debris.unwrap();
        assert_eq!(props.debris_bodies[handle].colliders().len(), 1);
        let collider = &props.debris_colliders[props.debris_bodies[handle].colliders()[0]];
        assert_eq!(collider.user_data, i as u128 + 1);
        assert!(
            (props.debris_bodies[handle].mass() - item.sign.as_ref().unwrap().mass).abs() < 0.001
        );
    }
    props.step(1. / 60.);
    assert_eq!(props.authored_instances(Vec3::ZERO).len(), 4);
    let p = *props.debris_bodies[props.items[0].debris.unwrap()].translation();
    let mut found = false;
    props.debris_query.intersections_with_shape(
        &props.debris_bodies,
        &props.debris_colliders,
        &Isometry3::translation(p.x, p.y, p.z),
        &Ball::new(0.1),
        QueryFilter::only_dynamic(),
        |_| {
            found = true;
            false
        },
    );
    assert!(
        found,
        "sign debris query must update even with no active cones"
    );
    for _ in 0..660 {
        props.step(1. / 60.);
    }
    assert!(props.active.is_empty());
    assert!(props.authored_instances(Vec3::ZERO).is_empty());
    props.update_focus(Vec3::X * 2000.);
    props.update_focus(Vec3::ZERO);
    assert_eq!(props.colliders.len(), 4);
    assert_eq!(props.authored_instances(Vec3::ZERO).len(), 4);
}
#[test]
fn gentle_sign_contact_stops_without_detaching() {
    let hermes = crate::hermes::Hermes::test_map();
    let mut props = Props::from_parts_near(
        vec![placement(
            SignDesign::for_kind(Kind::StopSign).unwrap(),
            Vec3::ZERO,
            0.,
        )
        .parts()],
        Vec3::ZERO,
    );
    let mut car = Vehicle::spawn(hermes.city());
    car.yaw = 0.;
    car.velocity = Vec3::X;
    car.position = Vec3::new(1., 0., car.ride_height());
    let before = Vec3::new(-4., 0., car.ride_height());
    props.vehicle_move(&mut car, before, hermes.city());
    assert!(car.collisions > 0);
    assert!(props.active.is_empty());
    assert_eq!(props.colliders.len(), 2);
}
