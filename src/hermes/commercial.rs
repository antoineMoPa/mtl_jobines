//! Game-owned commercial architectural recipes; placement is shared with OpenCity.
pub(super) fn seed(name: &str) -> u64 {
    name.bytes().fold(0xcbf29ce484222325, |s, b| {
        (s ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Family {
    Plex,
    Conversion,
    Shop,
    Canopy,
    Warehouse,
    Commercial,
    Strip,
    Village,
}
impl Family {
    pub(super) fn from_seed(s: u64) -> Self {
        match s % 8 {
            0 => Self::Plex,
            1 => Self::Conversion,
            2 => Self::Shop,
            3 => Self::Canopy,
            4 => Self::Warehouse,
            5 => Self::Commercial,
            6 => Self::Strip,
            _ => Self::Village,
        }
    }
    pub(super) fn floor_height(self) -> f32 {
        match self {
            Self::Commercial => 0.042,
            Self::Warehouse => 0.038,
            _ => 0.034,
        }
    }
}

#[cfg(test)]
use opencity::runtime::scaffold::{contains, overlap, prepare, reservations, strip_footprints};
#[cfg(test)]
use opencity::runtime::{City, Region};
#[cfg(test)]
use opencity::{BuildingUse, LandscapeKind, Program};
#[cfg(test)]
mod tests {
    #[test]
    fn straight_corridors_coalesce_without_losing_curved_strip_coverage() {
        let source = "road local: sidewalk carlane sidewalk; end
            straight=local(-2,0,2,0); curved=local(-2,1,-1,1,1,1.5,2,2);
            a=frontage_zone(straight,{use:commercial,side:both,start:0,end:1,depth:0.16,seed:1});
            b=frontage_zone(curved,{use:commercial,side:both,start:0,end:1,depth:0.16,seed:2});";
        let p = opencity::parse(source).unwrap();
        let zones = opencity::project_frontage_zones(&p).zones;
        assert_eq!(strip_footprints(&zones[0]).len(), 2);
        assert!(strip_footprints(&zones[1]).len() > 2);
        for zone in zones {
            let compact = strip_footprints(&zone);
            for q in zone.strip_quads(128) {
                for p in q {
                    assert!(compact.iter().any(|cell| contains(cell, [p[0], p[1]])));
                }
            }
        }
    }
    use super::*;
    fn sample(extra: &str) -> Program {
        opencity::parse(&format!(
            "road local: sidewalk carlane sidewalk; end
            street = local(-2,0,2,0);
            ground = land(-3,-2,3,-2,3,2,-3,2); elevation(ground,0.02);
            zone = frontage_zone(street,{{use:mixed_use,side:both,
                start:0.12,end:0.88,depth:0.16,seed:42}});
            residential_area(42,-2,-1,2,1,0,0,0);
            authored = building(0.1,{{use:commercial,front_edge:0}},
                -1.1,0.09,-0.98,0.09,-0.98,0.2,-1.1,0.2);
            {extra}"
        ))
        .unwrap()
    }
    fn generated(program: &Program) -> Vec<opencity::LandscapeFeature> {
        let (p, _) = super::super::expand(program, |p| City::from_program(p, None)).unwrap();
        let mut f: Vec<_> = opencity::project_landscape_features(&p)
            .features
            .into_iter()
            .filter(|f| {
                f.name
                    .as_ref()
                    .is_some_and(|n| n.starts_with("generated_shop_"))
            })
            .collect();
        f.sort_by(|a, b| a.name.cmp(&b.name));
        f
    }
    #[test]
    fn reproducible_ids_and_geometry_survive_statement_order_and_unrelated_zones() {
        let p = sample("");
        let a = generated(&p);
        assert!(a.len() > 20, "got {}", a.len());
        let mut reordered = p.clone();
        reordered.statements.reverse();
        let b = generated(&reordered);
        assert_eq!(a, b);
        let other = sample(
            "far = local(-2,4,2,4);
            elsewhere = frontage_zone(far,{use:commercial,side:left,
                start:0.2,end:0.8,depth:0.16,seed:9});",
        );
        let c = generated(&other);
        assert!(a.iter().all(|f| c.contains(f)));
    }
    #[test]
    fn shops_and_homes_preserve_authored_shell_and_never_overlap() {
        let p = sample("");
        let (p, city) = super::super::expand(&p, |p| City::from_program(p, None)).unwrap();
        let f = opencity::project_landscape_features(&p).features;
        assert!(f.iter().any(|f| f.name.as_deref() == Some("authored")));
        let buildings: Vec<_> = f
            .iter()
            .filter(|f| f.kind == LandscapeKind::Building)
            .collect();
        for (i, a) in buildings.iter().enumerate() {
            for b in &buildings[i + 1..] {
                assert!(
                    !overlap(&a.points, &b.points),
                    "overlap {:?} {:?}",
                    a.name,
                    b.name
                );
            }
        }
        for (_, f) in city.query(Region::WholeMap).buildings {
            if !f
                .name
                .as_ref()
                .is_some_and(|n| n.starts_with("generated_shop_"))
            {
                continue;
            }
            let p = f
                .points
                .iter()
                .fold([0., 0.], |s, p| [s[0] + p[0] / 4., s[1] + p[1] / 4.]);
            assert!(city.building_collision(p, 0., f.elevation, f.elevation + 0.01));
            assert!(f.front_edge.is_some());
            assert_eq!(f.building_use, BuildingUse::MixedUse);
        }
    }
    #[test]
    fn sparse_branding_has_independent_ten_percent_stream() {
        let count = (0..10000)
            .filter(|i| seed(&format!("generated_shop_{i:016x}:branding")) % 10 == 0)
            .count();
        assert!((850..1150).contains(&count), "{count}");
    }
    #[test]
    fn closed_polygon_edges_and_protected_authored_uses_are_preserved() {
        let a = vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.], [0., 0.]];
        assert!(overlap(
            &a,
            &[[0.5, 0.5], [1.5, 0.5], [1.5, 1.5], [0.5, 1.5]]
        ));
        assert!(!overlap(&a, &[[2., 0.], [3., 0.], [3., 1.], [2., 1.]]));
        let p = opencity::parse(
            "tower=building(0.4,0,0,1,0,1,1,0,1);
            mixed=building(0.1,{use:mixed_use},0,0,1,0,1,1,0,1);
            legacy=building(0.1,0,0,1,0,1,1,0,1);",
        )
        .unwrap();
        let out = prepare(&p, &[a]);
        let source = out.to_road_source();
        assert!(source.contains("tower = building"));
        assert!(source.contains("mixed = building"));
        assert!(!source.contains("legacy = building"));
    }
    #[test]
    fn curved_one_sided_slots_stay_in_reserved_strip() {
        let p = opencity::parse(
            "road local: sidewalk carlane sidewalk; end
            curved=local(-2,0,-1,0,1,0.5,2,1);
            ground=land(-3,-2,3,-2,3,3,-3,3);elevation(ground,0.02);
            zone=frontage_zone(curved,{use:commercial,side:right,
                start:0.1,end:0.9,depth:0.16,seed:42});",
        )
        .unwrap();
        let strips = reservations(&p).unwrap();
        let buildings = generated(&p);
        assert!(buildings.len() > 10, "{}", buildings.len());
        for f in buildings {
            assert!(f
                .points
                .iter()
                .all(|&p| strips.iter().any(|q| contains(q, p))));
        }
    }
}
