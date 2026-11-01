//! Authored rear lenses, shaded dynamically without adding lamp geometry.
use super::{Vehicle, METERS_PER_UNIT};
use glam::{Quat, Vec3};
use opencity::runtime::Mesh;

impl Vehicle {
    pub(super) fn append_lights(&self, output: &mut Mesh) {
        let Some(definition) = self.definition() else {
            return;
        };
        let longitudinal = self
            .velocity
            .dot(Vec3::new(self.yaw.cos(), self.yaw.sin(), 0.));
        let controls = self.sound_controls;
        let braking = self.sound_running && (controls.brake || controls.handbrake);
        let reversing = self.sound_running
            && !controls.brake
            && !controls.handbrake
            && (controls.throttle < -0.01 || longitudinal < -0.2);
        let state = u8::from(braking) + 2 * u8::from(reversing);
        let lean =
            Quat::from_rotation_y(-self.body_lean.x) * Quat::from_rotation_x(self.body_lean.y);
        let orientation = self.orientation() * lean;
        output
            .vertices
            .extend(definition.rear_lenses().iter().map(|vertex| {
                let mut vertex = *vertex;
                vertex.surface[3] += state as f32;
                vertex.position = ((orientation * Vec3::from_array(vertex.position)
                    + self.position)
                    / METERS_PER_UNIT)
                    .to_array();
                vertex.normal = (orientation * Vec3::from_array(vertex.normal)).to_array();
                vertex
            }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{catalog, Controls};
    #[test]
    fn every_model_reuses_authored_lenses_without_extra_geometry_in_each_driving_state() {
        let hermes = crate::hermes::Hermes::test_map();
        let mut car = Vehicle::spawn(hermes.city());
        car.sound_running = true;
        car.yaw = 0.7;
        car.pitch = 0.12;
        car.body_lean = glam::Vec2::new(0.05, -0.03);
        for (id, definition) in catalog::definitions().iter().enumerate() {
            car.set_catalog_model(Some(id));
            assert!(
                !definition.rear_lenses().is_empty(),
                "{}",
                definition.config.name
            );
            for (controls, state) in [
                (Controls::default(), 0.),
                (
                    Controls {
                        brake: true,
                        ..Default::default()
                    },
                    1.,
                ),
                (
                    Controls {
                        handbrake: true,
                        ..Default::default()
                    },
                    1.,
                ),
                (
                    Controls {
                        throttle: -1.,
                        ..Default::default()
                    },
                    2.,
                ),
            ] {
                car.sound_controls = controls;
                let mut mesh = Mesh::default();
                car.append_lights(&mut mesh);
                assert_eq!(mesh.vertices.len(), definition.rear_lenses().len());
                let orientation = car.orientation()
                    * Quat::from_rotation_y(-car.body_lean.x)
                    * Quat::from_rotation_x(car.body_lean.y);
                for (actual, original) in mesh.vertices.iter().zip(definition.rear_lenses()) {
                    let expected = (orientation * Vec3::from_array(original.position)
                        + car.position)
                        / METERS_PER_UNIT;
                    assert!(Vec3::from_array(actual.position).distance(expected) < 1e-5);
                    assert_eq!(
                        actual.color, original.color,
                        "authored lens color remains available for the unlit shader"
                    );
                    assert_eq!(actual.surface[3], original.surface[3] + state);
                }
            }
        }
    }
}
