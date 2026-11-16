//! Compact researched models; geographic placement is authored in the map.
use opencity::runtime::{City, Mesh, SurfaceKind, Vertex};

fn asset(id: &str) -> Result<&'static [u8], String> {
    Ok(match id {
        "study_01" => crate::web_asset!("../../data/landmarks/01.mesh", "data/landmarks/01.mesh"),
        "study_02" => crate::web_asset!("../../data/landmarks/02.mesh", "data/landmarks/02.mesh"),
        "study_03" => crate::web_asset!("../../data/landmarks/03.mesh", "data/landmarks/03.mesh"),
        "study_04" => crate::web_asset!("../../data/landmarks/04.mesh", "data/landmarks/04.mesh"),
        "study_05" => crate::web_asset!("../../data/landmarks/05.mesh", "data/landmarks/05.mesh"),
        "study_06" => crate::web_asset!("../../data/landmarks/06.mesh", "data/landmarks/06.mesh"),
        "study_07" => crate::web_asset!("../../data/landmarks/07.mesh", "data/landmarks/07.mesh"),
        "study_08" => crate::web_asset!("../../data/landmarks/08.mesh", "data/landmarks/08.mesh"),
        "study_09" => crate::web_asset!("../../data/landmarks/09.mesh", "data/landmarks/09.mesh"),
        "study_10" => crate::web_asset!("../../data/landmarks/10.mesh", "data/landmarks/10.mesh"),
        "study_11" => crate::web_asset!("../../data/landmarks/11.mesh", "data/landmarks/11.mesh"),
        "study_12" => crate::web_asset!("../../data/landmarks/12.mesh", "data/landmarks/12.mesh"),
        "study_13" => crate::web_asset!("../../data/landmarks/13.mesh", "data/landmarks/13.mesh"),
        "study_14" => crate::web_asset!("../../data/landmarks/14.mesh", "data/landmarks/14.mesh"),
        "study_17" => crate::web_asset!("../../data/landmarks/17.mesh", "data/landmarks/17.mesh"),
        "study_18" => crate::web_asset!("../../data/landmarks/18.mesh", "data/landmarks/18.mesh"),
        "study_19" => crate::web_asset!("../../data/landmarks/19.mesh", "data/landmarks/19.mesh"),
        "study_20" => crate::web_asset!("../../data/landmarks/20.mesh", "data/landmarks/20.mesh"),
        "study_21" => crate::web_asset!("../../data/landmarks/21.mesh", "data/landmarks/21.mesh"),
        "study_22" => crate::web_asset!("../../data/landmarks/22.mesh", "data/landmarks/22.mesh"),
        "study_23" => crate::web_asset!("../../data/landmarks/23.mesh", "data/landmarks/23.mesh"),
        "study_24" => crate::web_asset!("../../data/landmarks/24.mesh", "data/landmarks/24.mesh"),
        "study_25" => crate::web_asset!("../../data/landmarks/25.mesh", "data/landmarks/25.mesh"),
        "study_26" => crate::web_asset!("../../data/landmarks/26.mesh", "data/landmarks/26.mesh"),
        "study_27" => crate::web_asset!("../../data/landmarks/27.mesh", "data/landmarks/27.mesh"),
        "study_28" => crate::web_asset!("../../data/landmarks/28.mesh", "data/landmarks/28.mesh"),
        "study_29" => crate::web_asset!("../../data/landmarks/29.mesh", "data/landmarks/29.mesh"),
        "study_30" => crate::web_asset!("../../data/landmarks/30.mesh", "data/landmarks/30.mesh"),
        _ => return Err(format!("Unknown landmark asset: {id}")),
    })
}
/// Original model coordinates for the authoring picker and its previews.
pub(crate) fn source_mesh(id: &str) -> Result<Mesh, String> {
    let bytes = asset(id)?;
    let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if bytes.len() != 32 + count * 40 || count % 3 != 0 {
        return Err("Invalid landmark asset".into());
    }
    let mut mesh = Mesh::default();
    for record in bytes[32..].chunks_exact(40) {
        let v: [f32; 10] = std::array::from_fn(|i| {
            f32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap())
        });
        mesh.vertices.push(Vertex {
            position: [v[0], v[1], v[2]],
            normal: [v[3], v[4], v[5]],
            color: [v[6].powf(2.2), v[7].powf(2.2), v[8].powf(2.2)],
            surface: [SurfaceKind::Prop as u32 as f32, v[9], 0., 0.],
        });
    }
    Ok(mesh)
}

pub(crate) fn build(city: &City) -> Result<Mesh, String> {
    let mut mesh = Mesh::default();
    for feature in &city.landscape {
        let Some(id) = feature.model.as_deref() else {
            continue;
        };
        if id.ends_with(".glb") {
            continue;
        }
        if id == "plinth" {
            let top = feature.elevation + feature.height.unwrap_or(0.);
            let points = &feature.points;
            if points.len() != 4 {
                return Err("Landmark plinth needs four corners".into());
            }
            mesh.quad(
                std::array::from_fn(|i| [points[i][0], points[i][1], top]),
                [0.3; 3],
                SurfaceKind::Prop,
            );
            for i in 0..4 {
                let a = points[i];
                let b = points[(i + 1) % 4];
                mesh.quad(
                    [
                        [a[0], a[1], feature.elevation],
                        [b[0], b[1], feature.elevation],
                        [b[0], b[1], top],
                        [a[0], a[1], top],
                    ],
                    [0.25; 3],
                    SurfaceKind::Prop,
                );
            }
            continue;
        }
        let bytes = asset(id)?;
        if bytes.len() < 32 || &bytes[..4] != b"LMK1" {
            return Err("Invalid landmark asset".into());
        }
        let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
        if bytes.len() != 32 + count * 40 || count % 3 != 0 {
            return Err("Truncated landmark asset".into());
        }
        let read =
            |offset: usize| f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let bounds: [f32; 6] = std::array::from_fn(|i| read(8 + i * 4));
        let edge = [
            feature.points[1][0] - feature.points[0][0],
            feature.points[1][1] - feature.points[0][1],
        ];
        let width = edge[0].hypot(edge[1]);
        let next = [
            feature.points[2][0] - feature.points[1][0],
            feature.points[2][1] - feature.points[1][1],
        ];
        let depth = next[0].hypot(next[1]);
        let scale = (width / (bounds[3] - bounds[0])).min(depth / (bounds[4] - bounds[1]));
        // Keep the authored lot fit in XY while honoring the map's building height.
        let height_scale = feature
            .height
            .map_or(scale, |height| height / (bounds[5] - bounds[2]));
        let angle = edge[1].atan2(edge[0]);
        let (s, c) = angle.sin_cos();
        let center = feature.points.iter().fold([0.; 2], |a, p| {
            [
                a[0] + p[0] / feature.points.len() as f32,
                a[1] + p[1] / feature.points.len() as f32,
            ]
        });
        for i in 0..count {
            let v: [f32; 10] = std::array::from_fn(|j| read(32 + i * 40 + j * 4));
            let x = (v[0] - (bounds[0] + bounds[3]) / 2.) * scale;
            let y = (v[1] - (bounds[1] + bounds[4]) / 2.) * scale;
            // Inverse transpose for the independent vertical scale, then rotate in XY.
            let nz = v[5] * scale / height_scale;
            let length = v[3].hypot(v[4]).hypot(nz);
            mesh.vertices.push(Vertex {
                position: [
                    center[0] + c * x - s * y,
                    center[1] + s * x + c * y,
                    feature.elevation + (v[2] - bounds[2]) * height_scale,
                ],
                normal: [
                    (c * v[3] - s * v[4]) / length,
                    (s * v[3] + c * v[4]) / length,
                    nz / length,
                ],
                color: [v[6].powf(2.2), v[7].powf(2.2), v[8].powf(2.2)],
                surface: [SurfaceKind::Prop as u32 as f32, v[9], 0., 0.],
            });
        }
    }
    Ok(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flour_mill_has_red_lettering_on_white_panels_on_both_sides() {
        let mesh = source_mesh("study_25").unwrap();
        for side in [-1., 1.] {
            let on_sign = |v: &&Vertex| v.position[2] > 2.08 && v.normal[1] * side > 0.99;
            let white = mesh
                .vertices
                .iter()
                .filter(on_sign)
                .filter(|v| v.color.iter().all(|c| *c > 0.9))
                .count();
            let red = mesh
                .vertices
                .iter()
                .filter(on_sign)
                .filter(|v| v.color[0] > 0.6 && v.color[1] < 0.01 && v.color[2] < 0.01)
                .count();
            assert_eq!(white, 6, "missing white panel on side {side}");
            assert!(red > 300, "missing lettering on side {side}");
            for v in mesh
                .vertices
                .iter()
                .filter(on_sign)
                .filter(|v| v.color[0] > 0.9)
            {
                // The original black face sits at y=-0.385; the repair must cover it.
                assert!(if side < 0. {
                    v.position[1] < -0.385
                } else {
                    v.position[1] > -0.3225
                });
            }
        }
        for t in mesh
            .vertices
            .chunks_exact(3)
            .filter(|t| t[0].color[0] > 0.6 && t[0].color[1] < 0.01)
        {
            let p = t
                .iter()
                .map(|v| glam::Vec3::from(v.position))
                .collect::<Vec<_>>();
            assert!(
                (p[1] - p[0])
                    .cross(p[2] - p[0])
                    .dot(glam::Vec3::from(t[0].normal))
                    >= 0.
            );
        }
    }

    #[test]
    fn station_model_fits_its_authored_site_without_blocking_the_forecourt() {
        let program = opencity::parse(concat!(
            "station=building(0.0759,\"models/fuel-station.glb\",{collision:none},",
            "0,0,0.344,0,0.344,0.244,0,0.244); elevation(station,0.0976);"
        ))
        .unwrap();
        let city = City::from_program(&program, None).unwrap();
        assert!(!city.building_collision([0.17, 0.12], 0.009, 0.1, 0.12));
        let mesh = super::super::external_models::build(
            &city,
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("data/maps/mtl")
                .as_path(),
        )
        .unwrap();
        assert!(!mesh.vertices.is_empty());
        for vertex in mesh.vertices {
            let [x, y, z] = vertex.position;
            assert!((-0.0001..=0.3441).contains(&x));
            assert!((-0.0001..=0.2441).contains(&y));
            assert!((0.0975..=0.1736).contains(&z));
        }
        let solid = City::from_program(
            &opencity::parse("building(0.1,0,0,1,0,1,1,0,1);").unwrap(),
            None,
        )
        .unwrap();
        assert!(solid.building_collision([0.5, 0.5], 0.009, 0., 0.1));
    }

    fn tower(id: &str, height: f32) -> Mesh {
        let source = format!(
            "tower = building({height}, \"{id}\", 4,6, 5,7, 4,8, 3,7);
             elevation(tower, 0.4);"
        );
        let program = opencity::parse(&source).unwrap();
        build(&City::from_program(&program, None).unwrap()).unwrap()
    }

    #[test]
    fn authored_height_controls_towers_without_moving_their_rotated_lots() {
        for id in ["study_02", "study_14"] {
            let before = tower(id, 0.8);
            let after = tower(id, 1.6);
            assert_eq!(before.vertices.len(), after.vertices.len());
            let mut bottom = f32::INFINITY;
            let mut top = f32::NEG_INFINITY;
            for (a, b) in before.vertices.iter().zip(&after.vertices) {
                assert_eq!(&a.position[..2], &b.position[..2]);
                assert!((b.position[2] - 0.4 - 2. * (a.position[2] - 0.4)).abs() < 1e-5);
                bottom = bottom.min(b.position[2]);
                top = top.max(b.position[2]);
                let n = glam::Vec3::from(b.normal);
                assert!(n.is_finite());
                assert!((n.length() - 1.).abs() < 1e-5);
            }
            assert!((bottom - 0.4).abs() < 1e-5);
            assert!((top - 2.).abs() < 1e-5);
            let mut checked_faces = 0;
            for (original, triangle) in before
                .vertices
                .chunks_exact(3)
                .zip(after.vertices.chunks_exact(3))
            {
                let q: [glam::Vec3; 3] =
                    std::array::from_fn(|i| glam::Vec3::from(original[i].position));
                let original_face = (q[1] - q[0]).cross(q[2] - q[0]);
                // Curved surfaces may intentionally carry smooth vertex normals.
                if original_face.length() < 1e-6
                    || original.iter().any(|v| {
                        (glam::Vec3::from(v.normal).dot(original_face).abs()
                            / original_face.length()
                            - 1.)
                            .abs()
                            > 1e-4
                    })
                {
                    continue;
                }
                let p: [glam::Vec3; 3] =
                    std::array::from_fn(|i| glam::Vec3::from(triangle[i].position));
                let face = (p[1] - p[0]).cross(p[2] - p[0]);
                if face.length() < 1e-6 {
                    continue;
                }
                for vertex in triangle {
                    let normal = glam::Vec3::from(vertex.normal);
                    assert!((normal.dot(face).abs() / face.length() - 1.).abs() < 1e-3);
                }
                checked_faces += 1;
            }
            assert!(checked_faces > 0);
        }
    }

    #[test]
    fn every_compact_asset_has_complete_finite_triangles() {
        for id in (1..=30).filter(|i| !matches!(i, 15 | 16)) {
            let bytes = super::asset(&format!("study_{id:02}")).unwrap();
            let count = u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize;
            assert_eq!(bytes.len(), 32 + count * 40);
            assert_eq!(count % 3, 0);
            assert!(bytes[8..]
                .chunks_exact(4)
                .all(|b| f32::from_le_bytes(b.try_into().unwrap()).is_finite()));
        }
    }
}
