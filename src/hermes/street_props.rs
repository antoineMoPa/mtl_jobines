//! Approved offline-workbench GLBs, placed by the shared sidewalk feature layout.
#[cfg(test)]
use super::external_models;
use crate::props::{Kind, Part, Placement};
use glam::Vec3;
#[cfg(test)]
use opencity::runtime::Mesh;
use opencity::runtime::{City, Region, StreetFeatureKind};
#[cfg(test)]
use std::sync::OnceLock;

#[cfg(test)]
const BENCH: &[u8] = include_bytes!("../../data/props/bench.glb");
#[cfg(test)]
const BIN: &[u8] = include_bytes!("../../data/props/bin.glb");
#[cfg(test)]
const LAMP: &[u8] = include_bytes!("../../data/props/lamp.glb");

#[cfg(test)]
#[derive(Clone, Copy)]
enum Model {
    Bench,
    Lamp,
    Bin,
}
#[cfg(test)]
impl Model {
    fn index(self) -> usize {
        match self {
            Self::Bench => 0,
            Self::Lamp => 1,
            Self::Bin => 2,
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
        static MODELS: OnceLock<[Mesh; 3]> = OnceLock::new();
        &MODELS.get_or_init(|| {
            [(Self::Bench, BENCH), (Self::Lamp, LAMP), (Self::Bin, BIN)].map(|(model, bytes)| {
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

#[cfg(test)]
fn placed(model: Model, position: [f32; 3], heading: f32) -> Vec<Mesh> {
    let (sin, cos) = heading.sin_cos();
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
                position[0] + cos * x - sin * y,
                position[1] + sin * x + cos * y,
                position[2] + z,
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
#[cfg(test)]
pub(super) fn groups(city: &City) -> Vec<Vec<Mesh>> {
    let features = city.street_features(Region::WholeMap);
    let originals = city.detail_objects(Region::WholeMap);
    assert_eq!(features.len(), originals.len());
    let bins = super::street_bins::placements(city, &features);
    let mut groups: Vec<_> = features
        .into_iter()
        .zip(originals)
        .map(
            |(feature, original)| match Model::from_feature(feature.kind) {
                Some(model) => placed(model, feature.position, feature.heading),
                None => vec![original],
            },
        )
        .collect();
    groups.extend(
        bins.into_iter()
            .map(|bin| placed(Model::Bin, bin.position, bin.heading)),
    );
    groups
}
// Keep one local mesh per model; placements carry only transforms. Collision hulls
// are still installed by Props near the player and broken assemblies retain their IDs.
fn automatic_parts(kind: Kind, position: [f32; 3], heading: f32) -> Vec<Part> {
    let mut parts = Placement {
        kind,
        position: Vec3::from_array(position),
        heading: heading.to_degrees(),
    }
    .parts();
    for part in &mut parts {
        part.automatic = true;
    }
    parts
}
pub(super) fn parts(city: &City) -> Vec<Vec<Part>> {
    let features = city.street_features(Region::WholeMap);
    let bins = super::street_bins::placements(city, &features);
    let mut groups: Vec<_> = features
        .into_iter()
        .map(|feature| {
            let kind = match feature.kind {
                StreetFeatureKind::Tree => Kind::street_tree(feature.position),
                StreetFeatureKind::Bench => Kind::Bench,
                StreetFeatureKind::Lamp => Kind::Lamp,
            };
            automatic_parts(kind, feature.position, feature.heading)
        })
        .collect();
    groups.extend(
        bins.into_iter()
            .map(|bin| automatic_parts(Kind::TrashCan, bin.position, bin.heading)),
    );
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencity::runtime::{StreetFeature, SurfaceKind};

    #[test]
    #[ignore = "manual current-map memory measurement"]
    fn profile_current_map_street_storage() {
        let started = std::time::Instant::now();
        let hermes = crate::hermes::Hermes::load_with_game_data(
            std::path::Path::new("data/maps/mtl/map.road"),
            None,
        )
        .unwrap();
        let map_ready = started.elapsed();
        let groups = parts(hermes.city());
        let mut allocations = std::collections::HashSet::new();
        let mut logical_vertices = 0;
        let mut resident_vertices = 0;
        let mut placements = 0;
        for part in groups
            .iter()
            .flatten()
            .filter(|part| part.template.is_some_and(|(kind, _)| !kind.is_tree()))
        {
            placements += 1;
            logical_vertices += part.mesh.vertices.len();
            if allocations.insert(std::sync::Arc::as_ptr(&part.mesh)) {
                resident_vertices += part.mesh.vertices.len();
            }
        }
        let bytes = std::mem::size_of::<opencity::runtime::Vertex>();
        eprintln!("STREET_STORAGE map_ms={} parts_ms={} parts={} unique_meshes={} old_vertex_bytes={} shared_vertex_bytes={}",
            map_ready.as_millis(), (started.elapsed() - map_ready).as_millis(), placements,
            allocations.len(), logical_vertices * bytes, resident_vertices * bytes);
        assert!(resident_vertices < logical_vertices);
    }

    #[test]
    fn shared_street_templates_preserve_placed_geometry_and_lamp_parts() {
        for (model, kind) in [
            (Model::Bench, Kind::Bench),
            (Model::Lamp, Kind::Lamp),
            (Model::Bin, Kind::TrashCan),
        ] {
            for heading in [0., 1.7, -2.4] {
                let position = [2., -3., 0.2];
                let legacy = placed(model, position, heading);
                let parts = automatic_parts(kind, position, heading);
                assert_eq!(legacy.len(), parts.len());
                for (index, (expected, part)) in legacy.iter().zip(&parts).enumerate() {
                    assert!(part.automatic);
                    assert_eq!(part.template, Some((kind, index)));
                    assert!(std::sync::Arc::ptr_eq(&part.mesh, &kind.parts()[index]));
                    assert_eq!(expected.vertices.len(), part.mesh.vertices.len());
                    for (a, b) in expected.vertices.iter().zip(&part.mesh.vertices) {
                        let b = part.vertex(*b);
                        for axis in 0..3 {
                            assert!((a.position[axis] - b.position[axis]).abs() < 1e-6);
                            assert!((a.normal[axis] - b.normal[axis]).abs() < 1e-5);
                            assert!((a.color[axis] - b.color[axis]).abs() < 1e-6);
                        }
                        assert_eq!(a.surface[0], b.surface[0]);
                        assert_eq!(a.surface[2..], b.surface[2..]);
                        assert_eq!(b.surface[1], 1., "instance shader coverage");
                    }
                }
            }
        }
    }

    #[test]
    fn repeated_street_placements_share_mesh_allocations() {
        let city = City::from_program(
            &opencity::parse("road street: sidewalk carlane sidewalk; end street(-1,0,1,0);")
                .unwrap(),
            None,
        )
        .unwrap();
        let parts = parts(&city);
        let mut allocations = std::collections::HashSet::new();
        let mut logical_vertices = 0;
        let mut resident_vertices = 0;
        for part in parts.iter().flatten() {
            assert!(part.automatic);
            assert!(part.template.is_some());
            logical_vertices += part.mesh.vertices.len();
            if allocations.insert(std::sync::Arc::as_ptr(&part.mesh)) {
                resident_vertices += part.mesh.vertices.len();
            }
        }
        assert_eq!(allocations.len(), 4, "bench, bin, lamp stem and lamp arm");
        assert!(logical_vertices > resident_vertices * 4);
    }

    #[test]
    fn automatic_trees_use_instanced_templates_for_per_tree_lod() {
        let city = City::from_program(
            &opencity::parse(
                "ground=terrain(0,0,0,0.7,0,0.7,0.7,0,0.7); woods=vegetation(forest,0,0,0.7,0,0.7,0.7,0,0.7);",
            )
            .unwrap(),
            None,
        )
        .unwrap();
        let features = city.street_features(Region::WholeMap);
        assert!(features
            .iter()
            .any(|feature| feature.kind == StreetFeatureKind::Tree));
        let groups = parts(&city);
        for (feature, group) in features.iter().zip(groups.iter()) {
            if feature.kind == StreetFeatureKind::Tree {
                assert_eq!(group.len(), 1);
                assert_eq!(
                    group[0].template,
                    Some((Kind::street_tree(feature.position), 0))
                );
                assert!(group[0].automatic);
                assert!(std::sync::Arc::ptr_eq(
                    &group[0].mesh,
                    &Kind::street_tree(feature.position).parts()[0],
                ));
            }
        }
    }

    #[test]
    fn bundled_glbs_keep_real_scale_baked_colors_and_lamp_diffusers() {
        for (model, bytes, dimensions) in [
            (Model::Bench, BENCH, [0.642107, 1.9, 0.945821]),
            (Model::Lamp, LAMP, [1.77, 0.34, 5.513]),
            (Model::Bin, BIN, [0.606, 0.606, 0.995]),
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
        for model in [Model::Bench, Model::Lamp, Model::Bin] {
            let base = StreetFeature {
                kind: StreetFeatureKind::Bench,
                position: [2., 3., 0.2],
                heading: 0.,
            };
            let rotated = StreetFeature {
                heading: 1.7,
                ..base
            };
            let original: Vec<_> = placed(model, base.position, base.heading)
                .into_iter()
                .flat_map(|m| m.vertices)
                .collect();
            let actual: Vec<_> = placed(model, rotated.position, rotated.heading)
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
        assert!(
            features.len() < groups.len(),
            "valid benches should receive bins"
        );
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
