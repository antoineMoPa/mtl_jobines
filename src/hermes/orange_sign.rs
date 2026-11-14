//! The authored sign follows the same two-part impact path as a street lamp.
use super::external_models;
use crate::props::Part;
use glam::{Vec2, Vec3};
use opencity::runtime::{City, Mesh};
use std::sync::OnceLock;

pub(super) const MODEL_PATH: &str = "models/orange-sign.glb";
const MODEL: &[u8] = include_bytes!("../../data/maps/mtl/models/orange-sign.glb");

fn source() -> &'static Mesh {
    static MESH: OnceLock<Mesh> = OnceLock::new();
    MESH.get_or_init(|| external_models::decode_street_prop(MODEL).expect("validated sign GLB"))
}

pub(super) fn meshes(city: &City) -> Vec<Vec<Mesh>> {
    city.landscape
        .iter()
        .filter(|feature| feature.model.as_deref() == Some(MODEL_PATH))
        .map(|feature| {
            let placed = external_models::place(source(), feature).expect("valid sign footprint");
            let center = feature
                .points
                .iter()
                .fold(Vec2::ZERO, |sum, point| sum + Vec2::from_array(*point))
                / feature.points.len() as f32;
            let mut pole = Mesh::default();
            let mut panel = Mesh::default();
            for triangle in placed.vertices.chunks_exact(3) {
                let is_pole = triangle.iter().all(|vertex| {
                    let position = Vec3::from_array(vertex.position);
                    position.z <= feature.elevation + 0.043
                        && position.truncate().distance_squared(center) < 0.006 * 0.006
                });
                let part = if is_pole { &mut pole } else { &mut panel };
                part.vertices.extend_from_slice(triangle);
            }
            debug_assert!(!pole.vertices.is_empty() && !panel.vertices.is_empty());
            vec![pole, panel]
        })
        .collect()
}

pub(super) fn parts(city: &City) -> Vec<Vec<Part>> {
    meshes(city)
        .into_iter()
        .map(|group| group.into_iter().map(Part::baked).collect())
        .collect()
}
