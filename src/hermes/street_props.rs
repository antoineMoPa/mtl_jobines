//! Approved offline-workbench GLBs, placed by the shared sidewalk feature layout.
use super::external_models;
use opencity::runtime::{City, Mesh, Region, StreetFeature, StreetFeatureKind};
use std::sync::OnceLock;

const BENCH: &[u8] = include_bytes!("../../data/props/bench.glb");
const LAMP: &[u8] = include_bytes!("../../data/props/lamp.glb");

#[derive(Clone, Copy)]
enum Model {
    Bench,
    Lamp,
}
impl Model {
    fn index(self) -> usize {
        match self {
            Self::Bench => 0,
            Self::Lamp => 1,
        }
    }
    fn from_feature(kind: StreetFeatureKind) -> Option<Self> {
        match kind {
            StreetFeatureKind::Bench => Some(Self::Bench),
            StreetFeatureKind::Lamp => Some(Self::Lamp),
            StreetFeatureKind::Tree => None,
        }
    }
    fn mesh(self) -> &'static Mesh {
        static MODELS: OnceLock<[Mesh; 2]> = OnceLock::new();
        &MODELS.get_or_init(|| {
            [(Self::Bench, BENCH), (Self::Lamp, LAMP)].map(|(model, bytes)| {
                let mut mesh = external_models::decode_street_prop(bytes)
                    .expect("validated bundled street prop");
                for v in &mut mesh.vertices {
                    // GLB X spans the bench seat; shared sidewalk local X faces traffic.
                    if matches!(model, Self::Bench) {
                        let [x, y, z] = v.position;
                        v.position = [-y, x, z];
                        let [x, y, z] = v.normal;
                        v.normal = [-y, x, z];
                    }
                    v.position = v.position.map(|value| value / 100.);
                }
                mesh
            })
        })[self.index()]
    }
}

fn placed(model: Model, feature: StreetFeature) -> Vec<Mesh> {
    let (sin, cos) = feature.heading.sin_cos();
    let mut lower = Mesh::default();
    let mut upper = Mesh::default();
    for triangle in model.mesh().vertices.chunks_exact(3) {
        // Keep the overhead lamp arm out of the stem's convex collision hull.
        // Both parts remain one breakable assembly in the existing Props system.
        let high = matches!(model, Model::Lamp) && triangle.iter().all(|v| v.position[2] > 0.047);
        let mesh = if high { &mut upper } else { &mut lower };
        for mut v in triangle.iter().copied() {
            let [x, y, z] = v.position;
            v.position = [
                feature.position[0] + cos * x - sin * y,
                feature.position[1] + sin * x + cos * y,
                feature.position[2] + z,
            ];
            let [x, y, z] = v.normal;
            v.normal = [cos * x - sin * y, sin * x + cos * y, z];
            mesh.vertices.push(v);
        }
    }
    [lower, upper]
        .into_iter()
        .filter(|mesh| !mesh.vertices.is_empty())
        .collect()
}

pub(super) fn groups(city: &City) -> Vec<Vec<Mesh>> {
    let features = city.street_features(Region::WholeMap);
    let originals = city.detail_objects(Region::WholeMap);
    assert_eq!(features.len(), originals.len());
    features
        .into_iter()
        .zip(originals)
        .map(
            |(feature, original)| match Model::from_feature(feature.kind) {
                Some(model) => placed(model, feature),
                None => vec![original],
            },
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencity::runtime::SurfaceKind;

    #[test]
    fn bundled_glbs_keep_real_scale_baked_colors_and_lamp_diffusers() {
        for (model, bytes, dimensions) in [
            (Model::Bench, BENCH, [0.642107, 1.9, 0.945821]),
            (Model::Lamp, LAMP, [1.77, 0.34, 5.513]),
        ] {
            let raw = external_models::decode_street_prop(bytes).unwrap();
            let glb = gltf::Gltf::from_slice(bytes).unwrap();
            let mut expected = Vec::new();
            for primitive in glb.meshes().next().unwrap().primitives() {
                let reader = primitive.reader(|_| glb.blob.as_deref());
                let colors: Vec<_> = reader.read_colors(0).unwrap().into_rgb_f32().collect();
                for index in reader.read_indices().unwrap().into_u32() {
                    expected.push(colors[index as usize]);
                }
            }
            assert_eq!(raw.vertices.len(), expected.len());
            for (v, color) in raw.vertices.iter().zip(expected) {
                assert_eq!(v.color, color, "baked AO must not be applied twice");
                assert!(v.color.iter().all(|v| (0.0..=1.).contains(v)));
                assert_eq!(
                    v.surface[3], 0.,
                    "props must not opt into building weathering"
                );
            }
            let mesh = model.mesh();
            let (lo, hi) = external_models::bounds(mesh);
            for i in 0..3 {
                assert!(((hi[i] - lo[i]) * 100. - dimensions[i]).abs() < 0.001);
            }
            assert!(lo.z.abs() < 1e-7, "object must sit on its authored support");
            let lights = mesh
                .vertices
                .iter()
                .filter(|v| v.surface[0] == SurfaceKind::Light as u32 as f32)
                .count();
            assert_eq!(lights > 0, matches!(model, Model::Lamp));
        }
    }

    #[test]
    fn placement_rotates_normals_and_geometry_without_moving_the_pivot() {
        for model in [Model::Bench, Model::Lamp] {
            let base = StreetFeature {
                kind: StreetFeatureKind::Bench,
                position: [2., 3., 0.2],
                heading: 0.,
            };
            let rotated = StreetFeature {
                heading: 1.7,
                ..base
            };
            let original: Vec<_> = placed(model, base)
                .into_iter()
                .flat_map(|m| m.vertices)
                .collect();
            let actual: Vec<_> = placed(model, rotated)
                .into_iter()
                .flat_map(|m| m.vertices)
                .collect();
            let (s, c) = rotated.heading.sin_cos();
            for (a, b) in original.iter().zip(actual) {
                let x = a.position[0] - base.position[0];
                let y = a.position[1] - base.position[1];
                assert!((b.position[0] - (2. + c * x - s * y)).abs() < 1e-6);
                assert!((b.position[1] - (3. + s * x + c * y)).abs() < 1e-6);
                assert_eq!(a.position[2], b.position[2]);
                assert_eq!(a.color, b.color);
                let n = glam::Vec3::from_array(b.normal);
                assert!((n.length() - 1.).abs() < 1e-5);
                assert_eq!(a.surface, b.surface);
            }
        }
    }

    #[test]
    fn road_features_use_exported_models_and_keep_overhead_collision_separate() {
        let city = City::from_program(
            &opencity::parse("road street: sidewalk carlane sidewalk; end street(-1,0,1,0);")
                .unwrap(),
            None,
        )
        .unwrap();
        let features = city.street_features(Region::WholeMap);
        let groups = groups(&city);
        assert_eq!(features.len(), groups.len());
        for (feature, group) in features.iter().zip(groups) {
            let vertices: usize = group.iter().map(|m| m.vertices.len()).sum();
            match feature.kind {
                StreetFeatureKind::Bench => {
                    assert_eq!(vertices, Model::Bench.mesh().vertices.len());
                    assert_eq!(group.len(), 1);
                }
                StreetFeatureKind::Lamp => {
                    assert_eq!(vertices, Model::Lamp.mesh().vertices.len());
                    assert_eq!(group.len(), 2);
                    let (_, hi) = external_models::bounds(&group[0]);
                    let (lo, _) = external_models::bounds(&group[1]);
                    assert!(hi.z - feature.position[2] < 0.049);
                    assert!(lo.z - feature.position[2] > 0.047);
                }
                StreetFeatureKind::Tree => unreachable!(),
            }
        }
    }
}
