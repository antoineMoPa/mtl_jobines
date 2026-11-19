//! Authored rear lenses, shaded dynamically without adding lamp geometry.
use super::{Vehicle, METERS_PER_UNIT};
use glam::{Quat, Vec3};
use opencity::runtime::Mesh;

pub(super) struct RearLampState {
    pub braking: bool,
    pub reversing: bool,
    pub flashing: crate::hermes::RouteSignal,
}

impl Vehicle {
    pub(super) fn update_route_signal(&mut self, signal: crate::hermes::RouteSignal, dt: f32) {
        self.signal_time = if self.route_signal != signal {
            0.
        } else {
            (self.signal_time + dt) % 0.8
        };
        self.route_signal = signal;
    }

    pub(super) fn rear_lamp_state(&self) -> RearLampState {
        let longitudinal = self
            .velocity
            .dot(Vec3::new(self.yaw.cos(), self.yaw.sin(), 0.));
        let controls = self.sound_controls;
        let braking = self.sound_running && (controls.brake || controls.handbrake);
        let reversing = self.sound_running
            && !controls.brake
            && !controls.handbrake
            && (controls.throttle < -0.01 || longitudinal < -0.2);
        RearLampState {
            braking,
            reversing,
            flashing: if self.signal_time < 0.4 {
                self.route_signal
            } else {
                crate::hermes::RouteSignal::Off
            },
        }
    }

    pub(super) fn append_lights(&self, output: &mut Mesh) {
        let Some(definition) = self.definition() else {
            return;
        };
        let lamps = self.rear_lamp_state();
        let state = u8::from(lamps.braking)
            + 2 * u8::from(lamps.reversing)
            + 64 * u8::from(self.sound_running);
        let lean =
            Quat::from_rotation_y(-self.body_lean.x) * Quat::from_rotation_x(self.body_lean.y);
        let orientation = self.orientation() * lean;
        output
            .vertices
            .extend(definition.rear_lenses().iter().map(|vertex| {
                let mut vertex = *vertex;
                // Continuous bars select their end in the shader; separate lenses
                // use vehicle-local Y (positive is left).
                let strip = vertex.surface[3] >= 10.;
                vertex.surface[3] += match lamps.flashing {
                    crate::hermes::RouteSignal::Left if strip || vertex.position[1] > 0. => 16.,
                    crate::hermes::RouteSignal::Right if strip || vertex.position[1] < 0. => 32.,
                    _ => 0.,
                };
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
    fn rear_lenses_follow_render_pose_only_for_the_selected_player_vehicle() {
        use super::super::{DynamicWorld, VehicleId};
        use glam::Mat4;

        let hermes = crate::hermes::Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        world.vehicles.truncate(1);
        world.vehicles.push(Vehicle::spawn(hermes.city()));
        world.player_vehicle = VehicleId(1);
        for vehicle in &mut world.vehicles {
            vehicle.set_catalog_model(Some(0));
        }
        let car = &mut world.vehicles[1];
        car.yaw = 0.7;
        car.pitch = 0.12;
        car.body_lean = glam::Vec2::new(0.05, -0.03);
        car.sound_running = true;
        car.sound_controls.brake = true;
        let physics_pose =
            Mat4::from_rotation_translation(car.orientation(), car.position / METERS_PER_UNIT);
        for alpha in [0., 0.25, 0.75, 1.] {
            let car = world.player();
            let render_pose = Mat4::from_rotation_translation(
                (Quat::from_rotation_z(-0.08) * car.orientation()).slerp(car.orientation(), alpha),
                (car.position - Vec3::new(0.4, 0.2, 0.1) * (1. - alpha)) / METERS_PER_UNIT,
            );
            let delta = render_pose * physics_pose.inverse();
            let mut mesh = Mesh::default();
            world.write_moving_mesh_with_player_pose(&mut mesh, delta);
            let mut offset = 0;
            for (index, vehicle) in world.vehicles.iter().enumerate() {
                let definition = vehicle.definition().unwrap();
                let lean = Quat::from_rotation_y(-vehicle.body_lean.x)
                    * Quat::from_rotation_x(vehicle.body_lean.y);
                let pose = if index == 1 {
                    render_pose
                } else {
                    Mat4::from_rotation_translation(
                        vehicle.orientation(),
                        vehicle.position / METERS_PER_UNIT,
                    )
                };
                for original in definition.rear_lenses() {
                    let actual = &mesh.vertices[offset];
                    let expected = pose.transform_point3(
                        lean * Vec3::from_array(original.position) / METERS_PER_UNIT,
                    );
                    assert!(Vec3::from_array(actual.position).distance(expected) < 1e-5);
                    let normal = pose.transform_vector3(lean * Vec3::from_array(original.normal));
                    assert!(Vec3::from_array(actual.normal).distance(normal) < 1e-5);
                    assert_eq!(
                        actual.surface[3],
                        original.surface[3] + if index == 1 { 65. } else { 0. }
                    );
                    offset += 1;
                }
            }
            assert_eq!(mesh.vertices.len(), offset);
        }
    }

    #[test]
    fn route_flashers_blink_on_the_selected_side_and_reset() {
        use crate::hermes::RouteSignal;
        let hermes = crate::hermes::Hermes::test_map();
        let mut car = Vehicle::spawn(hermes.city());
        for id in 0..catalog::definitions().len() {
            car.set_catalog_model(Some(id));
            for (signal, bit) in [(RouteSignal::Left, 16), (RouteSignal::Right, 32)] {
                car.update_route_signal(signal, 0.01);
                let mut mesh = Mesh::default();
                car.append_lights(&mut mesh);
                let originals = car.definition().unwrap().rear_lenses();
                let mut lit = 0;
                for (vertex, original) in mesh.vertices.iter().zip(originals) {
                    let selected = original.surface[3] >= 10.
                        || match signal {
                            RouteSignal::Left => original.position[1] > 0.,
                            RouteSignal::Right => original.position[1] < 0.,
                            RouteSignal::Off => false,
                        };
                    assert_eq!(vertex.surface[3] as u32 & bit != 0, selected);
                    lit += usize::from(selected);
                }
                if car.is_bicycle() || car.is_aircraft() {
                    assert_eq!(lit, 0);
                } else {
                    assert!(lit > 0);
                }
                car.update_route_signal(signal, 0.4);
                let mut mesh = Mesh::default();
                car.append_lights(&mut mesh);
                for (vertex, original) in mesh.vertices.iter().zip(originals) {
                    assert_eq!(vertex.surface[3], original.surface[3]);
                }
            }
            car.update_route_signal(RouteSignal::Off, 0.01);
            assert_eq!(car.route_signal, RouteSignal::Off);
            assert_eq!(car.signal_time, 0.);
        }
    }

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
                definition.rear_lenses().is_empty()
                    == (definition.is_bicycle() || car.is_aircraft()),
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
                    assert_eq!(actual.surface[3], original.surface[3] + state + 64.);
                }
            }
        }
    }
}
