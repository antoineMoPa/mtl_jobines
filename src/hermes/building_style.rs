//! Game-owned selection of generic building-detail patterns.
use opencity::runtime::{
    BuildingDetailConfig, BuildingPattern, FacadeKind, GardenEdge, GardenProp, RoofKind, StairKind,
};

pub fn residential() -> BuildingDetailConfig {
    let metal = [0.07, 0.10, 0.08];
    let pattern = |floors, weight, facade, roof, stair| BuildingPattern {
        floors,
        weight,
        facade,
        roof,
        stair,
        trim: match roof {
            RoofKind::Cornice | RoofKind::Mansard => [0.72, 0.70, 0.62],
            _ => [0.55, 0.56, 0.49],
        },
        metal,
        door: match facade {
            FacadeKind::Stacked => [0.16, 0.07, 0.04],
            FacadeKind::WideStacked | FacadeKind::Detached => [0.08, 0.15, 0.14],
            _ => [0.10, 0.16, 0.09],
        },
    };
    BuildingDetailConfig {
        seed: 0x4d54_4c5f_484f_5553,
        // One map unit is 100 m in this game. Patterns are a design mix for the
        // test map, not measured neighbourhood frequencies.
        unit: 0.01,
        maximum_height: 0.15,
        minimum_frontage: 0.055,
        road_reach: 0.5,
        garden_depth: 0.037,
        garden_chance: 1.0,
        garden_edges: vec![
            (GardenEdge::Open, 1),
            (GardenEdge::StoneWall, 1),
            (GardenEdge::BrickWall, 1),
            (GardenEdge::IronFence, 18),
            (GardenEdge::ConcreteEdging, 1),
            (GardenEdge::WireFence, 12),
            (GardenEdge::Hedge, 1),
        ],
        garden_props: vec![
            (GardenProp::None, 1),
            (GardenProp::Planter, 3),
            (GardenProp::Bench, 1),
            (GardenProp::Bin, 1),
            (GardenProp::Bicycle, 2),
        ],
        patterns: vec![
            pattern(
                1,
                5,
                FacadeKind::Compact,
                RoofKind::Parapet,
                StairKind::None,
            ),
            pattern(
                1,
                3,
                FacadeKind::Detached,
                RoofKind::Pitched,
                StairKind::None,
            ),
            pattern(
                2,
                2,
                FacadeKind::Cottage,
                RoofKind::Parapet,
                StairKind::None,
            ),
            pattern(
                2,
                3,
                FacadeKind::Stacked,
                RoofKind::Cornice,
                StairKind::Straight,
            ),
            pattern(
                2,
                4,
                FacadeKind::Stacked,
                RoofKind::Cornice,
                StairKind::Curved,
            ),
            pattern(
                2,
                2,
                FacadeKind::Stacked,
                RoofKind::Parapet,
                StairKind::QuarterTurn,
            ),
            pattern(
                2,
                3,
                FacadeKind::SimpleStacked,
                RoofKind::Parapet,
                StairKind::Straight,
            ),
            pattern(
                2,
                3,
                FacadeKind::WideStacked,
                RoofKind::Coping,
                StairKind::QuarterTurn,
            ),
            pattern(
                3,
                2,
                FacadeKind::Rowhouse,
                RoofKind::Mansard,
                StairKind::None,
            ),
            pattern(
                3,
                3,
                FacadeKind::Stacked,
                RoofKind::Cornice,
                StairKind::Curved,
            ),
            pattern(
                3,
                2,
                FacadeKind::Stacked,
                RoofKind::Parapet,
                StairKind::QuarterTurn,
            ),
            pattern(
                4,
                3,
                FacadeKind::Rowhouse,
                RoofKind::Cornice,
                StairKind::None,
            ),
            pattern(
                4,
                2,
                FacadeKind::Rowhouse,
                RoofKind::Parapet,
                StairKind::None,
            ),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_patterns_cover_the_researched_forms_and_visual_details() {
        let config = residential();
        let mut forms = Vec::new();
        for pattern in &config.patterns {
            let form = (pattern.floors, pattern.facade);
            if !forms.contains(&form) {
                forms.push(form);
            }
        }
        assert_eq!(forms.len(), 9);
        for stair in [
            StairKind::Straight,
            StairKind::Curved,
            StairKind::QuarterTurn,
        ] {
            assert!(config.patterns.iter().any(|pattern| pattern.stair == stair));
        }
        for roof in [
            RoofKind::Coping,
            RoofKind::Cornice,
            RoofKind::Parapet,
            RoofKind::Pitched,
            RoofKind::Mansard,
        ] {
            assert!(config.patterns.iter().any(|pattern| pattern.roof == roof));
        }
        assert!(config
            .garden_edges
            .iter()
            .any(|(edge, _)| *edge == GardenEdge::Open));
        assert!(config
            .garden_props
            .iter()
            .any(|(prop, _)| *prop == GardenProp::Bicycle));
    }
}
