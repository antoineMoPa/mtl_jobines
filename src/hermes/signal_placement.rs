//! Physical signal bases follow pavement; traffic stop lines remain lane-centred.
use super::SignalApproach;
use crate::dynamic_world::METERS_PER_UNIT;
use glam::Vec3;
use opencity::runtime::{City, SurfaceKind};

pub(super) fn sidewalk_base(
    city: &City,
    position: Vec3,
    direction: Vec3,
    existing: &[SignalApproach],
) -> Option<Vec3> {
    let heading = direction.y.atan2(direction.x);
    let forward = Vec3::new(heading.cos(), heading.sin(), 0.);
    let right = Vec3::new(forward.y, -forward.x, 0.);
    let mut best = None;
    let mut score = f32::INFINITY;
    // Search outward across all incoming lanes and slightly behind the stop line
    // to clear rounded junction corners. Work in metres until querying the map.
    for (inside, steps) in [(false, 0..=24), (true, 1..=24)] {
        // Zero-length street stubs have no pavement behind the stop line.
        // Fall back to supported sidewalk on the junction corner itself.
        if inside && best.is_some() {
            break;
        }
        for back_step in steps {
            let back = back_step as f32 * 0.25;
            for side_step in 3..=80 {
                let side = side_step as f32 * 0.25;
                let cost = side + back * 2.;
                if cost >= score {
                    break;
                }
                let center = position + right * side + forward * if inside { back } else { -back };
                if existing.iter().filter_map(|s| s.mast_base).any(|base| {
                    base.truncate().distance(center.truncate()) < 0.9
                        && (base.z - center.z).abs() < 1.
                }) {
                    continue;
                }
                let mut low = f32::INFINITY;
                let mut high = f32::NEG_INFINITY;
                // Include the complete footplate plus a curb margin, on this deck.
                let supported = [(0., 0.), (-1., -1.), (-1., 1.), (1., -1.), (1., 1.)]
                    .into_iter()
                    .all(|(x, y)| {
                        let point =
                            (center + right * (x * 0.3) + forward * (y * 0.3)) / METERS_PER_UNIT;
                        let Some(surface) = city
                            .surface_at([point.x, point.y], (position.z + 0.75) / METERS_PER_UNIT)
                        else {
                            return false;
                        };
                        let height = surface.height * METERS_PER_UNIT;
                        low = low.min(height);
                        high = high.max(height);
                        surface.kind == SurfaceKind::Sidewalk && (height - position.z).abs() < 0.75
                    });
                if !supported || high - low > 0.08 {
                    continue;
                }
                let point = center / METERS_PER_UNIT;
                if city.building_collision(
                    [point.x, point.y],
                    0.0035,
                    high / METERS_PER_UNIT,
                    (high + 5.5) / METERS_PER_UNIT,
                ) {
                    continue;
                }
                best = Some(Vec3::new(center.x, center.y, high));
                score = cost;
                break;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_roads_use_the_sidewalk_on_each_heading_and_deck() {
        for forward in [Vec3::X, Vec3::Y, -Vec3::X, -Vec3::Y] {
            for height in [0., 0.102] {
                let source = format!(
                    "road r: sidewalk carlane carlane carlane \
                     carlane carlane carlane sidewalk; end \
                     segment=r({},{},{},{}); grade(segment,{height},{height});",
                    -forward.x, -forward.y, forward.x, forward.y,
                );
                let city = City::from_program(&opencity::parse(&source).unwrap(), None).unwrap();
                let position = Vec3::Z * (height * METERS_PER_UNIT);
                let right = Vec3::new(forward.y, -forward.x, 0.);
                let old = (position + right * 2.8) / METERS_PER_UNIT;
                assert_ne!(
                    city.surface_at([old.x, old.y], height + 0.01).unwrap().kind,
                    SurfaceKind::Sidewalk
                );
                let base = sidewalk_base(&city, position, forward, &[]).unwrap();
                assert!(base.dot(right) > 2.8);
                for x in [-0.3, 0., 0.3] {
                    for y in [-0.3, 0., 0.3] {
                        let point = (base + right * x + forward * y) / METERS_PER_UNIT;
                        let support = city
                            .surface_at([point.x, point.y], point.z + 0.001)
                            .unwrap();
                        assert_eq!(support.kind, SurfaceKind::Sidewalk);
                        assert!((support.height - point.z).abs() < 1e-6);
                    }
                }
                let existing = [SignalApproach {
                    position,
                    forward,
                    mast_base: Some(base),
                    junction: 0,
                    phase: 0,
                }];
                let second = sidewalk_base(&city, position, forward, &existing).unwrap();
                assert!(second.distance(base) >= 0.9);
            }
        }
    }

    #[test]
    fn missing_or_other_deck_sidewalks_never_receive_a_pole() {
        for source in [
            "road r: carlane carlane; end r(-1,0,1,0);",
            "road r: carlane carlane; end r(-1,0,1,0); \
             road bridge: sidewalk carlane sidewalk; end \
             upper=bridge(-1,0,1,0); grade(upper,0.1,0.1);",
        ] {
            let city = City::from_program(&opencity::parse(source).unwrap(), None).unwrap();
            assert!(sidewalk_base(&city, Vec3::ZERO, Vec3::X, &[]).is_none());
        }
    }
}
