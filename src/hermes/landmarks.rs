//! Compact researched models; geographic placement is authored in the map.
use opencity::runtime::{City, Mesh, SurfaceKind, Vertex};

fn asset(id: &str) -> Result<&'static [u8], String> {
    Ok(match id {
        "study_01" => include_bytes!("../../data/landmarks/01.mesh"),
        "study_02" => include_bytes!("../../data/landmarks/02.mesh"),
        "study_03" => include_bytes!("../../data/landmarks/03.mesh"),
        "study_04" => include_bytes!("../../data/landmarks/04.mesh"),
        "study_05" => include_bytes!("../../data/landmarks/05.mesh"),
        "study_06" => include_bytes!("../../data/landmarks/06.mesh"),
        "study_07" => include_bytes!("../../data/landmarks/07.mesh"),
        "study_08" => include_bytes!("../../data/landmarks/08.mesh"),
        "study_09" => include_bytes!("../../data/landmarks/09.mesh"),
        "study_10" => include_bytes!("../../data/landmarks/10.mesh"),
        "study_11" => include_bytes!("../../data/landmarks/11.mesh"),
        "study_12" => include_bytes!("../../data/landmarks/12.mesh"),
        "study_13" => include_bytes!("../../data/landmarks/13.mesh"),
        "study_14" => include_bytes!("../../data/landmarks/14.mesh"),
        "study_17" => include_bytes!("../../data/landmarks/17.mesh"),
        "study_18" => include_bytes!("../../data/landmarks/18.mesh"),
        "study_19" => include_bytes!("../../data/landmarks/19.mesh"),
        "study_20" => include_bytes!("../../data/landmarks/20.mesh"),
        "study_21" => include_bytes!("../../data/landmarks/21.mesh"),
        "study_22" => include_bytes!("../../data/landmarks/22.mesh"),
        "study_23" => include_bytes!("../../data/landmarks/23.mesh"),
        "study_24" => include_bytes!("../../data/landmarks/24.mesh"),
        "study_25" => include_bytes!("../../data/landmarks/25.mesh"),
        "study_26" => include_bytes!("../../data/landmarks/26.mesh"),
        "study_27" => include_bytes!("../../data/landmarks/27.mesh"),
        "study_28" => include_bytes!("../../data/landmarks/28.mesh"),
        "study_29" => include_bytes!("../../data/landmarks/29.mesh"),
        "study_30" => include_bytes!("../../data/landmarks/30.mesh"),
        _ => return Err(format!("Unknown landmark asset: {id}")),
    })
}
pub(super) fn build(city: &City) -> Result<Mesh, String> {
    let mut mesh = Mesh::default();
    for feature in &city.landscape {
        let Some(id) = feature.model.as_deref() else {
            continue;
        };
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
