//! The imported crossing owns its superstructure; authored roads own tire support.
use opencity::runtime::{City, Mesh, SurfaceKind};

pub(super) const MODEL: &str = "models/champlain-bridge.glb";

/// Remove generated slabs/piers beneath this model, retaining pavement and safety rails.
/// Derive the footprint and deck datum from the placement so moving/scaling it stays safe.
pub(super) fn remove_generated_structure(city: &City, mesh: &mut Mesh) {
    let Some(feature) = city
        .landscape
        .iter()
        .find(|f| f.model.as_deref() == Some(MODEL))
    else {
        return;
    };
    let mut lo = [f32::INFINITY; 2];
    let mut hi = [f32::NEG_INFINITY; 2];
    for p in &feature.points {
        for axis in 0..2 {
            lo[axis] = lo[axis].min(p[axis]);
            hi[axis] = hi[axis].max(p[axis]);
        }
    }
    // Original model spans z=-2..109m and its asphalt is at z=24m.
    let deck = feature.elevation + feature.height.unwrap_or(1.11) * (26. / 111.);
    let mut retained = 0;
    for i in (0..mesh.vertices.len()).step_by(3) {
        let triangle = &mesh.vertices[i..i + 3];
        let generated = triangle.iter().all(|v| {
            v.surface[0] == SurfaceKind::Prop as u32 as f32
                && v.position[0] >= lo[0] - 0.00001
                && v.position[0] <= hi[0] + 0.00001
                && v.position[1] >= lo[1] - 0.00001
                && v.position[1] <= hi[1] + 0.00001
                && v.position[2] <= deck + 0.00001
        });
        if !generated {
            mesh.vertices.copy_within(i..i + 3, retained);
            retained += 3;
        }
    }
    mesh.vertices.truncate(retained);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_replaces_piers_while_retaining_pavement_rails_and_adjacent_geometry() {
        let program = opencity::parse(
            "bridge=building(1.11,\"models/champlain-bridge.glb\",{collision:none},11.72,-30.9,11.72,-36.1,12.28,-36.1,12.28,-30.9);elevation(bridge,-0.02);",
        ).unwrap();
        let city = City::from_program(&program, None).unwrap();
        let mut mesh = Mesh::default();
        mesh.cuboid(
            [11.78, -34., 0.],
            [11.82, -33.9, 0.24],
            [0.3; 3],
            SurfaceKind::Prop,
        );
        let generated = mesh.vertices.len();
        mesh.quad(
            [
                [11.74, -34., 0.2402],
                [11.87, -34., 0.2402],
                [11.87, -33., 0.2402],
                [11.74, -33., 0.2402],
            ],
            [0.3; 3],
            SurfaceKind::Asphalt,
        );
        mesh.cuboid(
            [11.73, -34., 0.2402],
            [11.74, -33., 0.25],
            [0.3; 3],
            SurfaceKind::Prop,
        );
        mesh.cuboid(
            [13., -34., 0.],
            [13.1, -33., 0.239],
            [0.3; 3],
            SurfaceKind::Prop,
        );
        let expected = mesh.vertices[generated..].to_vec();
        remove_generated_structure(&city, &mut mesh);
        assert_eq!(mesh.vertices.len(), expected.len());
        for (actual, expected) in mesh.vertices.iter().zip(expected) {
            assert_eq!(actual.position, expected.position);
            assert_eq!(actual.surface, expected.surface);
        }
    }
}
