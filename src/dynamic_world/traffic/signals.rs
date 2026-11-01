//! Opposing approaches share green: 10 s green, 2 s amber, 2 s clearance per axis.
use super::Traffic;
use crate::hermes::traffic_paths::SignalApproach;
use glam::Vec3;
use opencity::runtime::{Mesh, SurfaceKind};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Aspect {
    Red,
    Amber,
    Green,
}
fn aspect(signal: &SignalApproach, time: f32) -> Aspect {
    // Stable junction offsets avoid every block changing at the same instant.
    let clock = (time + (signal.junction % 11) as f32 * 3.).rem_euclid(28.);
    let phase = (clock / 14.) as usize;
    let elapsed = clock % 14.;
    if phase != signal.phase || elapsed >= 12. {
        Aspect::Red
    } else if elapsed >= 10. {
        Aspect::Amber
    } else {
        Aspect::Green
    }
}
pub(super) fn is_green(signal: &SignalApproach, time: f32) -> bool {
    aspect(signal, time) == Aspect::Green
}
impl Traffic {
    pub fn vehicle_move(
        &mut self,
        vehicle: &mut crate::dynamic_world::Vehicle,
        before: Vec3,
        city: &opencity::runtime::City,
    ) {
        self.signal_props.vehicle_move(vehicle, before, city);
        for (id, signal) in self.graph.signals.iter().enumerate() {
            if !self.signal_props.group_intact(id) {
                self.failed_junctions.insert(signal.junction);
            }
        }
    }
    pub fn step_debris(&mut self, dt: f32) {
        self.signal_props.step(dt);
    }
    /// Failed signals become an all-way stop. The oldest completed stop owns
    /// the crossing until it leaves; all approaches share that reservation.
    pub(super) fn update_stop_priority(
        &mut self,
        vehicles: &[crate::dynamic_world::Vehicle],
        dt: f32,
    ) {
        self.crossing.retain(|junction, owner| {
            self.drivers.iter().any(|driver| {
                driver.vehicle == *owner
                    && (self.graph.paths[driver.path].junction == Some(*junction)
                        || driver
                            .next
                            .is_some_and(|next| self.graph.paths[next].junction == Some(*junction)))
            })
        });
        for driver in &mut self.drivers {
            let car = &vehicles[driver.vehicle];
            let path = &self.graph.paths[driver.path];
            let failed = driver
                .next
                .and_then(|next| self.graph.paths[next].signal)
                .is_some_and(|signal| {
                    self.failed_junctions
                        .contains(&self.graph.signals[signal].junction)
                });
            let (along, error) = path.project(car.position - Vec3::Z * car.ride_height());
            let to_line = path.length - along - car.length() * 0.5 - 1.5;
            if failed
                && path.junction.is_none()
                && error < 2.
                && to_line < 1.5
                && to_line > -1.
                && car.velocity.length() < 0.5
            {
                driver.stop_wait += dt;
                driver.stop_arrival.get_or_insert(self.time);
            } else {
                driver.stop_wait = 0.;
                driver.stop_arrival = None;
            }
        }
        for &junction in &self.failed_junctions {
            if self.crossing.contains_key(&junction) {
                continue;
            }
            let waiting = self
                .drivers
                .iter()
                .filter(|driver| {
                    driver.stop_wait >= 1.
                        && driver
                            .next
                            .is_some_and(|next| self.graph.paths[next].junction == Some(junction))
                })
                .min_by(|a, b| {
                    a.stop_arrival
                        .unwrap_or(f32::INFINITY)
                        .total_cmp(&b.stop_arrival.unwrap_or(f32::INFINITY))
                        .then(a.vehicle.cmp(&b.vehicle))
                });
            let Some(waiting) = waiting else {
                continue;
            };
            let crossing_paths: Vec<_> = self
                .graph
                .paths
                .iter()
                .filter(|path| path.junction == Some(junction))
                .collect();
            // A human driver or displaced vehicle already crossing still has priority.
            let occupied = vehicles.iter().enumerate().any(|(id, car)| {
                id != waiting.vehicle
                    && crossing_paths.iter().any(|path| {
                        let ground = car.position - Vec3::Z * car.ride_height();
                        let (along, error) = path.project(ground);
                        let at = path.sample(along).0;
                        along > 2.
                            && along < path.length - 2.
                            && error < car.width() * 0.5 + 0.6
                            && (at.z - ground.z).abs() < 2.
                    })
            });
            if !occupied {
                self.crossing.insert(junction, waiting.vehicle);
            }
        }
    }
    pub fn append_signals(&self, mesh: &mut Mesh, center: Vec3) {
        for (id, signal) in self.graph.signals.iter().enumerate() {
            if signal.position.distance(center) > 360. || !self.signal_props.group_intact(id) {
                continue;
            }
            let active = (!self.failed_junctions.contains(&signal.junction))
                .then(|| aspect(signal, self.time));
            mesh.vertices.extend(signal_mesh(signal, active).vertices);
        }
        self.signal_props.append_moving(mesh, center);
    }
}
pub(super) fn signal_mesh(signal: &SignalApproach, active: Option<Aspect>) -> Mesh {
    let mut mesh = Mesh::default();
    let right = Vec3::new(signal.forward.y, -signal.forward.x, 0.);
    let base = signal.position + right * 2.8;
    let mut cube = |offset: Vec3, size: Vec3, color: [f32; 3], kind| {
        let at = base + offset;
        mesh.cuboid(
            ((at - size * 0.5) / 100.).to_array(),
            ((at + size * 0.5) / 100.).to_array(),
            color,
            kind,
        );
    };
    cube(
        Vec3::Z * 1.8,
        Vec3::new(0.12, 0.12, 3.6),
        [0.15, 0.17, 0.18],
        SurfaceKind::Prop,
    );
    cube(
        Vec3::Z * 3.5,
        Vec3::new(0.46, 0.46, 1.25),
        [0.08, 0.08, 0.07],
        SurfaceKind::Prop,
    );
    for (lamp, z, color) in [
        (Aspect::Red, 3.9, [1., 0.04, 0.02]),
        (Aspect::Amber, 3.5, [1., 0.65, 0.01]),
        (Aspect::Green, 3.1, [0.02, 1., 0.12]),
    ] {
        let color = if active == Some(lamp) {
            color
        } else {
            color.map(|c| c * 0.12)
        };
        cube(
            Vec3::Z * z - signal.forward * 0.25,
            Vec3::splat(0.28),
            color,
            if active == Some(lamp) {
                SurfaceKind::Light
            } else {
                SurfaceKind::Prop
            },
        );
    }
    for vertex in &mut mesh.vertices {
        if vertex.surface[0] == SurfaceKind::Light as u32 as f32 {
            vertex.surface[3] = 1.;
        }
    }
    mesh
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opposing_directions_share_green_and_cross_traffic_waits() {
        let signals: Vec<_> = (0..4)
            .map(|approach| SignalApproach {
                position: Vec3::ZERO,
                forward: Vec3::X,
                junction: 0,
                phase: approach / 2,
            })
            .collect();
        for tick in 0..560 {
            let time = tick as f32 * 0.1;
            assert_eq!(aspect(&signals[0], time), aspect(&signals[1], time));
            assert_eq!(aspect(&signals[2], time), aspect(&signals[3], time));
            assert!(!(is_green(&signals[0], time) && is_green(&signals[2], time)));
        }
        assert_eq!(aspect(&signals[0], 0.), Aspect::Green);
        assert_eq!(aspect(&signals[0], 10.), Aspect::Amber);
        assert_eq!(aspect(&signals[0], 12.), Aspect::Red);
        assert_eq!(aspect(&signals[2], 14.), Aspect::Green);
        assert_eq!(aspect(&signals[0], 28.), Aspect::Green);
    }
}
