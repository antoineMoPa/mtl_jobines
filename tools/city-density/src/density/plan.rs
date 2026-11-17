//! Explicit authoring plans for the source-scale city and the playable inner ring.
use super::DENSITY_CENTER;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Plan {
    SourceCity,
    InnerLoop,
}
impl Plan {
    pub fn street(self, name: &str) -> bool {
        match self {
            Self::SourceCity => matches!(name, "sd_street" | "urban_arterial"),
            Self::InnerLoop => matches!(name, "cross_street" | "cycling_street"),
        }
    }
    pub fn center(self) -> [f32; 2] {
        match self {
            Self::SourceCity => DENSITY_CENTER,
            Self::InnerLoop => [10., 11.],
        }
    }
    pub fn contains(self, p: [f32; 2]) -> bool {
        match self {
            Self::SourceCity => true,
            // Stay inside the ring and behind the existing frontage/access roads.
            Self::InnerLoop => (-10.2..25.5).contains(&p[0]) && (0.35..22.).contains(&p[1]),
        }
    }
    pub fn cell(self) -> f32 {
        match self {
            Self::SourceCity => 0.5,
            Self::InnerLoop => 1.2,
        }
    }
    pub fn reach(self) -> f32 {
        match self {
            Self::SourceCity => 1.8,
            Self::InnerLoop => 3.6,
        }
    }
    pub fn footprints(self) -> [(f32, f32); 4] {
        match self {
            Self::SourceCity => [(0.35, 0.35), (0.27, 0.27), (0.22, 0.22), (0.16, 0.20)],
            Self::InnerLoop => [(0.55, 0.5), (0.46, 0.4), (0.32, 0.3), (0.20, 0.24)],
        }
    }
    pub fn building(self, p: [f32; 2], seed: usize, frontage: bool) -> (f32, String) {
        if self == Self::SourceCity {
            let central = (36. ..52.).contains(&p[0]) && (-3. ..20.).contains(&p[1]);
            let height = if central {
                0.10 + (seed % 4) as f32 * 0.025
            } else {
                0.075 + (seed % 3) as f32 * 0.025
            };
            let use_kind = if frontage && seed % 9 == 0 {
                "mixed_use"
            } else {
                "residential"
            };
            let material = if seed % 4 == 0 { "concrete" } else { "brick" };
            return (height, format!("use:{use_kind},material:{material}"));
        }
        let downtown = (8.3..23.).contains(&p[0]) && (7.5..15.7).contains(&p[1]);
        if downtown {
            let (family, material, palette, floors) = match seed % 5 {
                0 => ("heritage", "red_brick", "brownstone", 6),
                1 => ("mixed", "glass", "sandstone", 10),
                2 => ("curtain_wall", "glass", "granite", 16),
                3 => ("podium", "glass", "chalk", 20),
                _ => ("concrete_frame", "concrete", "concrete", 8),
            };
            (floors as f32 * 0.0325, format!("use:mixed_use,material:{material},family:{family},palette:{palette},floor_height:0.0325,ground:retail,glazing:teal,front_edge:0,seed:{seed}"))
        } else {
            let material = if seed % 3 == 0 {
                "brown_brick"
            } else {
                "red_brick"
            };
            let floors = 2 + seed % 3;
            (floors as f32 * 0.0315, format!("use:residential,material:{material},family:legacy,floor_height:0.0315,front_edge:0"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn station_openings_and_authored_fixtures_remain_clear() {
        let p = opencity::parse(
            "mouth = terrain_cut(0.1, {slope_y: -0.6}, 1,1, 2,1, 2,2, 1,2);
            t = prop(tree, 3,3); parked_vehicle(\"vehicle-12\", 4,4, 0);",
        )
        .unwrap();
        let mut index = super::super::Index::default();
        reserve_fixtures(&p, &mut index);
        for [x, y] in [[1.5, 1.5], [3., 3.], [4., 4.]] {
            let lot = [
                [x - 0.01, y - 0.01],
                [x + 0.01, y - 0.01],
                [x + 0.01, y + 0.01],
                [x - 0.01, y + 0.01],
            ];
            assert!(index.overlaps(&lot, None, false));
        }
    }
    #[test]
    fn inner_ring_maps_local_streets_and_downtown_scale() {
        let plan = Plan::InnerLoop;
        assert!(plan.street("cycling_street"));
        assert!(plan.street("cross_street"));
        assert!(!plan.street("metro_track"));
        assert!(!plan.street("east_clover_interstate_three_lane"));
        assert!(!plan.contains([28., 12.]));
        assert!(!plan.contains([10., 24.]));
        let (height, style) = plan.building([17., 12.], 3, true);
        assert_eq!(height, 0.65);
        assert!(style.contains("family:podium"));
        let (height, style) = plan.building([3., 12.], 3, true);
        assert_eq!(height, 0.063);
        assert!(style.contains("use:residential"));
    }
}

// Surface openings and authored props must remain accessible after filling a block.
pub(super) fn reserve_fixtures(program: &opencity::Program, index: &mut super::Index) {
    use opencity::{Argument, Statement};
    for statement in &program.statements {
        let Statement::Call(call) = statement else {
            continue;
        };
        if call.name == "terrain_cut" {
            let coordinates: Vec<f32> = call
                .args
                .iter()
                .skip(1)
                .filter_map(|a| {
                    if let Argument::Number(n) = a {
                        n.parse().ok()
                    } else {
                        None
                    }
                })
                .collect();
            let polygon: Vec<_> = coordinates.chunks_exact(2).map(|p| [p[0], p[1]]).collect();
            if polygon.len() >= 3 {
                index.insert(polygon);
            }
        }
        if matches!(call.name.as_str(), "prop" | "parked_vehicle") {
            if let (Some(Argument::Number(x)), Some(Argument::Number(y))) =
                (call.args.get(1), call.args.get(2))
            {
                if let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>()) {
                    let half = if call.name == "parked_vehicle" {
                        0.035
                    } else {
                        0.07
                    };
                    index.insert(vec![
                        [x - half, y - half],
                        [x + half, y - half],
                        [x + half, y + half],
                        [x - half, y + half],
                    ]);
                }
            }
        }
    }
}
