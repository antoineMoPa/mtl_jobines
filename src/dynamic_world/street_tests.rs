//! Reported junctions must remain continuous at driving height in every direction.
use super::{Controls, SpawnPoint, Vehicle, METERS_PER_UNIT};
use glam::Vec2;
use opencity::{
    map,
    runtime::{City, SurfaceKind},
    Argument, Statement,
};
use std::collections::HashSet;

#[test]
fn reported_junctions_have_level_aligned_mouths_and_can_be_driven_across() {
    let mut program = opencity::parse(include_str!("../../data/maps/mtl/map.road")).unwrap();
    let definitions: HashSet<_> = program
        .statements
        .iter()
        .filter_map(|s| match s {
            Statement::Road(r) => Some(r.name.clone()),
            _ => None,
        })
        .collect();
    // Keep authored road geometry; omit city population and elevation rasterization.
    program.statements.retain(|s| match s {
        Statement::Road(_) => true,
        Statement::Call(c) => {
            definitions.contains(&c.name)
                || c.name == "map_point"
                || c.name == "grade"
                || c.name.starts_with("intersection")
        }
        _ => false,
    });
    let projection = map::project(&program);
    let junctions: Vec<_> = map::intersections(&program, &projection)
        .into_iter()
        .filter(|j| matches!(j.point_ref.as_str(), "sd_p1196" | "sd_p1329" | "sd_p1333"))
        .collect();
    assert_eq!(junctions.len(), 3);
    let roads: HashSet<_> = projection
        .segments
        .iter()
        .filter(|s| {
            junctions.iter().any(|j| {
                [s.start, s.end]
                    .iter()
                    .any(|p| (p.x - j.position.x).hypot(p.y - j.position.y) < 0.7)
            })
        })
        .filter_map(|s| s.name.clone())
        .collect();
    program.statements.retain(|s| match s {
        Statement::Road(_) => true,
        Statement::Call(c) if c.name == "map_point" => true,
        Statement::Call(c) if definitions.contains(&c.name) => {
            c.assigns.first().is_some_and(|n| roads.contains(n))
        }
        Statement::Call(c) if c.name == "grade" => {
            matches!(c.args.first(), Some(Argument::Identifier(n)) if roads.contains(n))
        }
        Statement::Call(c) if c.name.starts_with("intersection") => {
            matches!(c.args.first(), Some(Argument::Identifier(n))
                if junctions.iter().any(|j| &j.point_ref == n))
        }
        _ => false,
    });
    let projection = map::project(&program);
    let geometry = opencity::road_geometry::RoadGeometry::compile(&program, &projection);
    let city = City::from_program(&program, None).unwrap();
    for junction in map::intersections(&program, &projection) {
        let rotation = map::intersection_rotation(&projection, &junction);
        let (sn, cs) = rotation.to_radians().sin_cos();
        let seams = geometry.junction_seams(&projection, &junction);
        assert_eq!(seams.len(), 4);
        let height = seams[0].1.height;
        for (_, a, b) in seams {
            assert!((a.height - height).abs() < 0.00001);
            assert!((b.height - height).abs() < 0.00001);
            let local = |p: map::WorldPoint| {
                Vec2::new(
                    (p.x - junction.position.x) * cs + (p.y - junction.position.y) * sn,
                    -(p.x - junction.position.x) * sn + (p.y - junction.position.y) * cs,
                )
            };
            let middle = (local(a.point) + local(b.point)) * 0.5;
            assert!(
                middle.x.abs().min(middle.y.abs()) < 0.00002,
                "off-centre mouth at {}: {middle:?}",
                junction.point_ref
            );
        }
        for direction in 0..4 {
            let yaw = rotation.to_radians() + direction as f32 * std::f32::consts::FRAC_PI_2;
            let forward = Vec2::new(yaw.cos(), yaw.sin());
            let left = Vec2::new(-forward.y, forward.x);
            let center = Vec2::new(junction.position.x, junction.position.y);
            let start = center - forward * (junction.footprint_half + 0.04) - left * 0.025;
            let mut car = Vehicle::spawn_at(
                &city,
                SpawnPoint {
                    point: start.to_array(),
                    heading: Some(yaw),
                },
            )
            .unwrap();
            let distance = (junction.footprint_half * 2. + 0.08) * METERS_PER_UNIT;
            for _ in 0..900 {
                car.step(
                    &city,
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    1. / 120.,
                );
                assert_eq!(
                    car.collisions, 0,
                    "barrier at {} direction {direction}",
                    junction.point_ref
                );
                assert_eq!(car.surface, SurfaceKind::Asphalt);
                assert!(car.grounded);
                if (car.position.truncate() - start * METERS_PER_UNIT).dot(forward) >= distance {
                    break;
                }
            }
            let traveled = (car.position.truncate() - start * METERS_PER_UNIT).dot(forward);
            assert!(
                traveled >= distance,
                "stopped after {traveled} m at {}",
                junction.point_ref
            );
        }
    }
}
