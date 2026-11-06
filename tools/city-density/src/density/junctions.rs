//! Reserve rendered junction pavement, leaving the grass outside curved corners available.
use super::Index;
use opencity::{map, network, Program};
use std::{collections::BTreeMap, error::Error};

pub(super) fn reserve(
    program: &Program,
    projection: &map::Projection,
    index: &mut Index,
    widths: &BTreeMap<String, f32>,
    terrain: Option<&opencity::heightmap::ElevationMap>,
) -> Result<(), Box<dyn Error>> {
    let mut geometry = opencity::road_geometry::RoadGeometry::compile(program, projection);
    geometry.configure_safety(program, projection, terrain);
    for junction in map::intersections(program, projection) {
        let instance = network::default_four_way_instance(program, projection, &junction)
            .or_else(|| network::default_three_way_instance(program, projection, &junction))
            .or_else(|| network::default_right_turn_instance(program, projection, &junction));
        if let Some(instance) =
            instance.filter(|_| !geometry.junction_is_protected(&junction.point_ref))
        {
            let width = std::iter::once(junction.road_name.as_str())
                .chain(junction.arm_roads.iter().map(|(_, name)| name.as_str()))
                .filter_map(|name| widths.get(name))
                .copied()
                .fold(0., f32::max)
                / 2.;
            let half = map::intersection_footprint_half(&junction, width);
            for patch in opencity::junction_mesh::patches(&instance.geometry, Some(half)) {
                if matches!(
                    patch.material,
                    network::Material::Verge
                        | network::Material::RoadElement(opencity::RoadElementKind::Verge)
                ) {
                    continue;
                }
                let q = patch
                    .points
                    .into_iter()
                    .map(|p| {
                        let p = instance.world_point(p);
                        [p.x, p.y]
                    })
                    .collect::<Vec<_>>();
                if q.len() >= 3 {
                    index.insert(q);
                }
            }
        } else {
            let (s, c) = map::intersection_rotation(projection, &junction)
                .to_radians()
                .sin_cos();
            let h = junction.footprint_half + 0.025;
            index.insert(
                [[-h, -h], [h, -h], [h, h], [-h, h]]
                    .map(|v| {
                        [
                            junction.position.x + c * v[0] - s * v[1],
                            junction.position.y + s * v[0] + c * v[1],
                        ]
                    })
                    .to_vec(),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curved_junction_keeps_its_pavement_and_releases_the_outer_grass_corner() {
        let program = opencity::parse(include_str!(
            "../../../../opencity/examples/default_intersection.road"
        ))
        .unwrap();
        let projection = map::project(&program);
        let junction = map::intersections(&program, &projection).remove(0);
        let mut index = Index::default();
        reserve(
            &program,
            &projection,
            &mut index,
            &BTreeMap::from([("street".into(), 0.154)]),
            None,
        )
        .unwrap();
        let h = junction.footprint_half;
        let square = |x: f32, y: f32| {
            vec![
                [x - 0.002, y - 0.002],
                [x + 0.002, y - 0.002],
                [x + 0.002, y + 0.002],
                [x - 0.002, y + 0.002],
            ]
        };
        assert!(index.overlaps(&square(0., 0.), None, false));
        assert!(!index.overlaps(&square(h * 0.9, h * 0.9), None, false));
    }
}
