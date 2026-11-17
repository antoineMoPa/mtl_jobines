//! World-space emitters in map units; vehicle and prop lifetimes own the lights.
use super::{DynamicWorld, METERS_PER_UNIT};
use bytemuck::{Pod, Zeroable};
use glam::{Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RearLight {
    Tail,
    Brake,
    Indicator,
    Reverse,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct LocalLight {
    pub position_range: [f32; 4],
    pub direction_outer: [f32; 4],
    pub color_intensity: [f32; 4],
    pub beam: [f32; 4],
}
impl LocalLight {
    pub fn rear(position: Vec3, orientation: Quat, kind: RearLight, strength: f32) -> Self {
        let (range, intensity, color) = match kind {
            RearLight::Tail => (0.045, 1., [1., 0.001, 0.0005]),
            RearLight::Brake => (0.075, 8., [1., 0.001, 0.0005]),
            RearLight::Indicator => (0.06, 5., [1., 0.3, 0.001]),
            RearLight::Reverse => (0.075, 4., [1., 1., 0.98]),
        };
        Self {
            position_range: position.extend(range).to_array(),
            direction_outer: (orientation * Quat::from_rotation_y(-0.22) * -Vec3::X)
                .extend(0.)
                .to_array(),
            color_intensity: [color[0], color[1], color[2], intensity * strength],
            beam: [0.80, 1.5, 0., 0.],
        }
    }
    pub fn streetlamp(position: Vec3) -> Self {
        Self {
            position_range: position.extend(0.16).to_array(),
            direction_outer: [0., 0., -1., 0.20],
            color_intensity: [1., 0.72, 0.38, 18.],
            beam: [0.85, 4., 1., 0.],
        }
    }
    pub fn headlight(position: Vec3, orientation: Quat, strength: f32) -> Self {
        let tilt = Quat::from_rotation_y(0.04);
        Self {
            position_range: position.extend(0.45).to_array(),
            direction_outer: (orientation * tilt * Vec3::X)
                .extend(32_f32.to_radians().cos())
                .to_array(),
            color_intensity: [1., 0.94, 0.82, 100. * strength],
            beam: [16_f32.to_radians().cos(), 16., 0., 0.],
        }
    }
    pub fn position(self) -> Vec3 {
        Vec3::from_slice(&self.position_range)
    }
}
impl DynamicWorld {
    #[cfg(any(test, feature = "test-support"))]
    pub fn preview_vehicle_signal(&mut self, signal: crate::hermes::RouteSignal, phase: f32) {
        let vehicle = &mut self.vehicles[self.player_vehicle.0];
        vehicle.update_route_signal(signal, 0.);
        vehicle.signal_time = phase;
    }
    pub fn local_lights(&self) -> Vec<LocalLight> {
        let center = self.player_position();
        let mut lights = Vec::new();
        self.props.append_local_lights(&mut lights, center);
        for (id, vehicle) in self.vehicles.iter().enumerate() {
            if self.traffic.inactive(id)
                || !vehicle.sound_running
                || vehicle.position.distance(center) > 150.
            {
                continue;
            }
            let Some(definition) = vehicle.definition() else {
                continue;
            };
            let orientation = vehicle.orientation()
                * Quat::from_rotation_y(-vehicle.body_lean.x)
                * Quat::from_rotation_x(vehicle.body_lean.y);
            lights.extend(definition.headlights().iter().map(|&local| {
                LocalLight::headlight(
                    (vehicle.position + orientation * local) / METERS_PER_UNIT,
                    orientation,
                    self.traffic.opacity(id),
                )
            }));
            let lamps = vehicle.rear_lamp_state();
            for &local in definition.rear_emitters() {
                let flashing = match lamps.flashing {
                    crate::hermes::RouteSignal::Left => local.y > 0.,
                    crate::hermes::RouteSignal::Right => local.y < 0.,
                    crate::hermes::RouteSignal::Off => false,
                };
                let kind = if flashing {
                    RearLight::Indicator
                } else if lamps.braking {
                    RearLight::Brake
                } else {
                    RearLight::Tail
                };
                let position = (vehicle.position + orientation * local) / METERS_PER_UNIT;
                let strength = self.traffic.opacity(id);
                lights.push(LocalLight::rear(position, orientation, kind, strength));
                if lamps.reversing {
                    lights.push(LocalLight::rear(
                        position,
                        orientation,
                        RearLight::Reverse,
                        strength,
                    ));
                }
            }
        }
        lights
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authored_headlights_follow_each_vehicle_body() {
        let hermes = crate::hermes::Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        for model in 0..super::super::catalog::definitions().len() {
            let definition = &super::super::catalog::definitions()[model];
            assert_eq!(
                definition.headlights().len(),
                if definition.is_bicycle() { 1 } else { 2 },
                "{}",
                definition.config.name
            );
            assert_eq!(
                definition.rear_emitters().len(),
                if definition.is_bicycle() { 0 } else { 2 },
                "{}",
                definition.config.name
            );
            let car = &mut world.vehicles[0];
            car.set_catalog_model(Some(model));
            car.sound_running = true;
            car.yaw = 1.2;
            car.pitch = 0.17;
            car.roll = -0.08;
            let orientation = car.orientation();
            let position = car.position;
            let sources = world.local_lights();
            for &local in definition.headlights() {
                let expected = (position + orientation * local) / METERS_PER_UNIT;
                let light = sources
                    .iter()
                    .find(|l| l.position().distance(expected) < 1e-5)
                    .unwrap();
                let direction = Vec3::from_slice(&light.direction_outer);
                assert!(
                    direction.distance(orientation * Quat::from_rotation_y(0.04) * Vec3::X) < 1e-5
                );
            }
        }
        world
            .vehicles
            .iter_mut()
            .for_each(|v| v.sound_running = false);
        assert!(world.local_lights().iter().all(|l| l.beam[2] == 1.));
    }

    #[test]
    fn rear_spill_matches_brakes_reverse_and_both_flashing_phases_for_every_model() {
        use crate::{dynamic_world::Controls, hermes::RouteSignal};
        let hermes = crate::hermes::Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        world
            .vehicles
            .iter_mut()
            .for_each(|v| v.sound_running = false);
        for model in 0..super::super::catalog::definitions().len() {
            let definition = &super::super::catalog::definitions()[model];
            for controls in [
                Controls::default(),
                Controls {
                    brake: true,
                    ..Default::default()
                },
                Controls {
                    handbrake: true,
                    ..Default::default()
                },
                Controls {
                    throttle: -1.,
                    ..Default::default()
                },
            ] {
                for signal in [RouteSignal::Off, RouteSignal::Left, RouteSignal::Right] {
                    for phase in [0., 0.399, 0.4, 0.799] {
                        let car = &mut world.vehicles[0];
                        car.set_catalog_model(Some(model));
                        car.sound_running = true;
                        car.sound_controls = controls;
                        car.route_signal = signal;
                        car.signal_time = phase;
                        car.yaw = 0.73;
                        car.body_lean = glam::Vec2::new(0.05, -0.03);
                        let orientation = car.orientation()
                            * Quat::from_rotation_y(-car.body_lean.x)
                            * Quat::from_rotation_x(car.body_lean.y);
                        let position = car.position;
                        let sources = world.local_lights();
                        for &local in definition.rear_emitters() {
                            let flashing = phase < 0.4
                                && match signal {
                                    RouteSignal::Left => local.y > 0.,
                                    RouteSignal::Right => local.y < 0.,
                                    RouteSignal::Off => false,
                                };
                            let kind = if flashing {
                                RearLight::Indicator
                            } else if controls.brake || controls.handbrake {
                                RearLight::Brake
                            } else {
                                RearLight::Tail
                            };
                            let expected = LocalLight::rear(
                                (position + orientation * local) / METERS_PER_UNIT,
                                orientation,
                                kind,
                                1.,
                            );
                            assert!(sources.iter().any(|light| light
                                .position()
                                .distance(expected.position())
                                < 1e-6
                                && light.color_intensity == expected.color_intensity));
                            let reverse = sources
                                .iter()
                                .filter(|light| {
                                    light.position().distance(expected.position()) < 1e-6
                                        && light.color_intensity[1] == 1.
                                })
                                .count();
                            assert_eq!(reverse, usize::from(controls.throttle < -0.01));
                        }
                    }
                }
            }
        }
    }
}
