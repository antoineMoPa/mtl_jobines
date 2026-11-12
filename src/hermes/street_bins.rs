//! Bins accompany approved bench placements only where their whole base fits.
use opencity::runtime::{City, StreetFeature, StreetFeatureKind, SurfaceKind};

pub(super) struct Placement {
    pub position: [f32; 3],
    pub heading: f32,
}

pub(super) fn placements(city: &City, features: &[StreetFeature]) -> Vec<Placement> {
    let mut bins: Vec<Placement> = Vec::new();
    for bench in features
        .iter()
        .filter(|f| f.kind == StreetFeatureKind::Bench)
    {
        let (sin, cos) = bench.heading.sin_cos();
        for side in [1., -1.] {
            let center = [
                bench.position[0] - sin * 0.016 * side,
                bench.position[1] + cos * 0.016 * side,
            ];
            let mut low = f32::INFINITY;
            let mut high = f32::NEG_INFINITY;
            let supported = [(0., 0.), (-1., -1.), (-1., 1.), (1., -1.), (1., 1.)]
                .into_iter()
                .all(|(dx, dy)| {
                    let point = [center[0] + dx * 0.0033, center[1] + dy * 0.0033];
                    let Some(surface) = city.surface_at(point, bench.position[2] + 0.02) else {
                        return false;
                    };
                    low = low.min(surface.height);
                    high = high.max(surface.height);
                    surface.kind == SurfaceKind::Sidewalk
                        && (surface.height - bench.position[2]).abs() < 0.003
                });
            if !supported || high - low > 0.00035 {
                continue;
            }
            if city.building_collision(center, 0.004, high, high + 0.011) {
                continue;
            }
            let blocked = features.iter().any(|f| {
                let clearance = match f.kind {
                    StreetFeatureKind::Bench => 0.013,
                    StreetFeatureKind::Lamp => 0.0065,
                    StreetFeatureKind::Tree => 0.018,
                };
                (f.position[0] - center[0]).hypot(f.position[1] - center[1]) < clearance
            }) || bins.iter().any(|bin| {
                (bin.position[0] - center[0]).hypot(bin.position[1] - center[1]) < 0.012
            });
            if blocked {
                continue;
            }
            bins.push(Placement {
                position: [center[0], center[1], high],
                heading: bench.heading,
            });
            break;
        }
    }
    bins
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencity::runtime::Region;

    fn city(source: &str) -> City {
        City::from_program(&opencity::parse(source).unwrap(), None).unwrap()
    }

    #[test]
    fn bins_fit_pavement_clear_the_bench_and_are_deterministic() {
        let city = city("road r: sidewalk carlane sidewalk; end r(-1,0,1,0);");
        let features = city.street_features(Region::WholeMap);
        let bins = placements(&city, &features);
        assert!(!bins.is_empty());
        let duplicate = placements(&city, &features);
        assert_eq!(bins.len(), duplicate.len());
        for (bin, copy) in bins.iter().zip(duplicate) {
            assert_eq!(bin.position, copy.position);
            assert_eq!(bin.heading, copy.heading);
            for (dx, dy) in [(-1., -1.), (-1., 1.), (1., -1.), (1., 1.)] {
                let point = [bin.position[0] + dx * 0.0033, bin.position[1] + dy * 0.0033];
                let support = city.surface_at(point, bin.position[2] + 0.02).unwrap();
                assert_eq!(support.kind, SurfaceKind::Sidewalk);
                assert!((support.height - bin.position[2]).abs() < 0.00035);
            }
            assert!(features.iter().all(|feature| {
                (feature.position[0] - bin.position[0]).hypot(feature.position[1] - bin.position[1])
                    > 0.013
            }));
        }
    }

    #[test]
    fn bins_require_supported_sidewalk_and_do_not_fill_a_building() {
        let empty = city("road r: carlane; end r(-1,0,1,0);");
        assert!(placements(&empty, &empty.street_features(Region::WholeMap)).is_empty());
        let city = city(concat!(
            "road r: sidewalk carlane sidewalk; end r(-1,0,1,0);",
            "wall=building(0.05,-2,-0.2,2,-0.2,2,0.2,-2,0.2);"
        ));
        let features = [StreetFeature {
            kind: StreetFeatureKind::Bench,
            position: [0., 0.0385, 0.0015],
            heading: -std::f32::consts::FRAC_PI_2,
        }];
        assert!(placements(&city, &features).is_empty());
    }
}
