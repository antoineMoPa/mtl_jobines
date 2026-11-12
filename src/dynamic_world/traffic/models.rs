//! Cached approved signal bodies and switchable lens/pedestrian GLB variants.
use super::signals::Aspect;
use crate::hermes::{external_models, traffic_paths::SignalApproach};
use glam::Vec3;
use opencity::runtime::{Mesh, SurfaceKind};
use std::sync::OnceLock;

const ASSETS: [&[u8]; 4] = [
    include_bytes!("../../../data/props/signal-red.glb"),
    include_bytes!("../../../data/props/signal-amber.glb"),
    include_bytes!("../../../data/props/signal.glb"),
    include_bytes!("../../../data/props/signal-off.glb"),
];
struct Templates {
    body: Mesh,
    phases: [Mesh; 4],
}
pub(super) struct Standing {
    body: Mesh,
    phases: [Mesh; 4],
}
fn phase_index(active: Option<Aspect>) -> usize {
    match active {
        Some(Aspect::Red) => 0,
        Some(Aspect::Amber) => 1,
        Some(Aspect::Green) => 2,
        None => 3,
    }
}
fn decode(bytes: &[u8]) -> (Mesh, Mesh) {
    let raw = external_models::decode_street_prop(bytes).expect("validated signal GLB");
    let glb = gltf::Gltf::from_slice(bytes).expect("bundled signal metadata");
    let mut body = Mesh::default();
    let mut lenses = Mesh::default();
    let mut cursor = 0;
    for primitive in glb.meshes().next().expect("signal mesh").primitives() {
        let count = primitive
            .reader(|_| glb.blob.as_deref())
            .read_indices()
            .expect("indexed signal primitive")
            .into_u32()
            .count();
        let moving = match primitive.material().name() {
            Some("signal_red" | "signal_amber" | "signal_green" | "pedestrian") => true,
            Some("paint" | "metal") => false,
            name => panic!("unexpected signal material {name:?}"),
        };
        let target = if moving { &mut lenses } else { &mut body };
        target
            .vertices
            .extend_from_slice(&raw.vertices[cursor..cursor + count]);
        cursor += count;
    }
    assert_eq!(cursor, raw.vertices.len());
    for v in &mut lenses.vertices {
        if v.surface[0] == SurfaceKind::Light as u32 as f32 {
            v.surface[3] = 1.;
        }
    }
    (body, lenses)
}
fn templates() -> &'static Templates {
    static CACHE: OnceLock<Templates> = OnceLock::new();
    CACHE.get_or_init(|| {
        let decoded = ASSETS.map(decode);
        let body = decoded[3].0.clone();
        let phases = decoded.map(|(_, lenses)| lenses);
        Templates { body, phases }
    })
}
fn placed(source: &Mesh, signal: &SignalApproach) -> Mesh {
    let length = signal.forward.x.hypot(signal.forward.y).max(1e-6);
    let forward = Vec3::new(signal.forward.x / length, signal.forward.y / length, 0.);
    let right = Vec3::new(forward.y, -forward.x, 0.);
    let base = signal.position + right * 2.8;
    let mut result = Mesh::default();
    result.vertices.reserve(source.vertices.len());
    for triangle in source.vertices.chunks_exact(3) {
        // Reflect the mast toward the road, face the lenses toward approaching
        // drivers, and reverse winding to match the reflected vertex normals.
        for i in [0, 2, 1] {
            let mut v = triangle[i];
            let p = Vec3::from_array(v.position);
            v.position = ((base - right * p.x + forward * p.y + Vec3::Z * p.z) / 100.).to_array();
            let n = Vec3::from_array(v.normal);
            v.normal = (-right * n.x + forward * n.y + Vec3::Z * n.z).to_array();
            result.vertices.push(v);
        }
    }
    result
}
pub(super) fn instantiate(signal: &SignalApproach) -> (Standing, Vec<Mesh>) {
    let model = templates();
    let body = placed(&model.body, signal);
    let phases = std::array::from_fn(|i| placed(&model.phases[i], signal));
    let mut lower = Mesh::default();
    let mut overhead = Mesh::default();
    let cutoff = signal.position.z / 100. + 0.035;
    for triangle in body
        .vertices
        .chunks_exact(3)
        .chain(phases[3].vertices.chunks_exact(3))
    {
        let target = if triangle.iter().all(|v| v.position[2] > cutoff) {
            &mut overhead
        } else {
            &mut lower
        };
        target.vertices.extend_from_slice(triangle);
    }
    (Standing { body, phases }, vec![lower, overhead])
}
impl Standing {
    pub(super) fn append(&self, mesh: &mut Mesh, active: Option<Aspect>) {
        mesh.vertices.extend_from_slice(&self.body.vertices);
        mesh.vertices
            .extend_from_slice(&self.phases[phase_index(active)].vertices);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powered_states_have_the_correct_lens_and_unpowered_is_fully_dark() {
        let model = templates();
        assert!(model.body.vertices.iter().all(|v| v.surface[0] != 9.));
        for (index, phase) in model.phases.iter().enumerate() {
            let lit: Vec<_> = phase
                .vertices
                .iter()
                .filter(|v| v.surface[0] == 9.)
                .collect();
            if index == 3 {
                assert!(lit.is_empty());
                continue;
            }
            assert!(!lit.is_empty());
            let lens: Vec<_> = lit.into_iter().filter(|v| v.position[2] > 3.).collect();
            assert!(!lens.is_empty());
            assert!(lens.iter().all(|v| match index {
                0 => v.color[0] > v.color[1] * 3.,
                1 => v.color[0] > v.color[1] && v.color[1] > v.color[2] * 4.,
                2 => v.color[1] > v.color[0] * 3.,
                _ => unreachable!(),
            }));
            assert!(lens.iter().all(|v| v.surface[3] == 1.));
        }
        // Green displays the walking figure; red/amber/off use the stop hand.
        assert_ne!(
            model.phases[0].vertices.len(),
            model.phases[2].vertices.len()
        );
    }

    #[test]
    fn signals_face_incoming_drivers_and_keep_the_mast_overhead() {
        for forward in [Vec3::X, Vec3::Y, -Vec3::X, -Vec3::Y] {
            let signal = SignalApproach {
                position: Vec3::new(10., 20., 0.15),
                forward,
                junction: 0,
                phase: 0,
            };
            let right = Vec3::new(forward.y, -forward.x, 0.);
            let base = signal.position + right * 2.8;
            let (standing, groups) = instantiate(&signal);
            assert_eq!(groups.len(), 2);
            assert!(groups[1].vertices.iter().all(|v| v.position[2] > 0.035));
            let lenses = &standing.phases[2];
            let faces: Vec<_> = lenses
                .vertices
                .iter()
                .filter(|v| {
                    v.surface[0] == 9.
                        && v.position[2] * 100. > 3.5
                        && Vec3::from_array(v.normal).dot(-forward) > 0.99
                })
                .collect();
            assert!(!faces.is_empty());
            assert!(faces
                .iter()
                .all(|v| { (Vec3::from_array(v.position) * 100. - base).dot(-right) > 2. }));
            for triangle in standing.body.vertices.chunks_exact(3) {
                let p: [Vec3; 3] = std::array::from_fn(|i| Vec3::from_array(triangle[i].position));
                let area = (p[1] - p[0]).cross(p[2] - p[0]);
                if area.length() > 1e-12 {
                    assert!(area.dot(Vec3::from_array(triangle[0].normal)) > 0.);
                }
            }
        }
    }
}
