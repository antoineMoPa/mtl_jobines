//! Three 3.2 m chain-link bays share a textured, double-sided quad.
use crate::dynamic_world::METERS_PER_UNIT;
use glam::Vec2;
use opencity::runtime::{City, Mesh, SurfaceKind, Vertex};

// Walk from one side of the terminal access road around the reclaimed shore
// to the other. Keep the fence in the grass, clear of both runway thresholds.
const PERIMETER: [[f32; 2]; 6] = [
    [-0.50, -3.02],
    [-5.00, -3.02],
    [-5.00, -21.68],
    [5.00, -21.68],
    [5.00, -3.73],
    [0.80, -3.73],
];
const PANEL_WIDTH: f32 = 9.6 / METERS_PER_UNIT;
const HEIGHT: f32 = 2.4 / METERS_PER_UNIT;
// Game shader extension to OpenCity's procedural FencePattern values.
const PNG_PATTERN: f32 = 2.;

pub(super) fn panels(city: &City) -> Vec<Mesh> {
    let mut meshes = Vec::new();
    let Some(shore) = city
        .landscape
        .iter()
        .find(|f| f.name.as_deref() == Some("airport_shore"))
    else {
        return meshes;
    };
    let ground = |p: Vec2| {
        p.extend(
            city.surface_at(p.to_array(), f32::INFINITY)
                .map_or(shore.elevation, |s| s.height),
        )
    };
    for edge in PERIMETER.windows(2) {
        let start = Vec2::from_array(edge[0]);
        let end = Vec2::from_array(edge[1]);
        let delta = end - start;
        let length = delta.length();
        let normal = [delta.y / length, -delta.x / length, 0.];
        let panels = (length / PANEL_WIDTH).round() as usize;
        let mut a = ground(start);
        for i in 1..=panels {
            let b = ground(start.lerp(end, i as f32 / panels as f32));
            let up = glam::Vec3::Z * HEIGHT;
            let mut mesh = Mesh::default();
            for (position, u, v) in [
                (a, 0., 1.),
                (b, 1., 1.),
                (b + up, 1., 0.),
                (a, 0., 1.),
                (b + up, 1., 0.),
                (a + up, 0., 0.),
            ] {
                mesh.vertices.push(Vertex {
                    position: position.to_array(),
                    normal,
                    color: [1.; 3],
                    surface: [SurfaceKind::Fence as u32 as f32, u, v, PNG_PATTERN],
                });
            }
            meshes.push(mesh);
            a = b;
        }
    }
    meshes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airport_panels_join_on_grass_and_leave_the_road_open() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let hermes = crate::hermes::Hermes::load_with_game_data(&path, None).unwrap();
        let city = hermes.city();
        let panels = panels(city);
        assert_eq!(panels.len(), 576);
        let mut previous = PERIMETER[0];
        for panel in panels {
            let quad = panel.vertices;
            assert_eq!(quad.len(), 6);
            let a = glam::Vec3::from_array(quad[0].position);
            let b = glam::Vec3::from_array(quad[1].position);
            assert!(a.truncate().distance(Vec2::from_array(previous)) < 0.00001);
            assert!((a.distance(b) - PANEL_WIDTH).abs() < 0.002);
            let mid = a.lerp(b, 0.5);
            let support = city
                .surface_at(mid.truncate().to_array(), f32::INFINITY)
                .unwrap();
            assert_eq!(support.kind, SurfaceKind::Grass, "panel at {mid:?}");
            assert!((support.height - mid.z).abs() < 0.001);
            previous = b.truncate().to_array();
        }
        assert!(Vec2::from_array(previous).distance(Vec2::from_array(PERIMETER[5])) < 0.00001);
    }
}
