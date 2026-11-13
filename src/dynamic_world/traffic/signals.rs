//! Opposing approaches share green: 10 s green, 2 s amber, 2 s clearance per axis.
use super::Traffic;
use crate::hermes::traffic_paths::SignalApproach;
use glam::Vec3;
use opencity::runtime::Mesh;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Aspect {
    Red,
    Amber,
    Green,
}
/// Exact standing mesh selection. Position is immutable until a map reload;
/// phase and power are the only changes while the signal assembly is intact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SignalBatch {
    pub id: usize,
    pub(super) active: Option<Aspect>,
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
    pub(crate) fn standing_signal_batches(
        &self,
        center: Vec3,
    ) -> impl Iterator<Item = SignalBatch> + '_ {
        self.graph
            .signals
            .iter()
            .enumerate()
            .filter(move |(id, signal)| {
                signal.position.distance(center) <= 360. && self.signal_props.group_intact(*id)
            })
            .map(|(id, signal)| SignalBatch {
                id,
                active: (!self.failed_junctions.contains(&signal.junction))
                    .then(|| aspect(signal, self.time)),
            })
    }
    pub(crate) fn append_signal_batch(&self, batch: &SignalBatch, mesh: &mut Mesh) {
        // A previously selected batch must never restore a broken assembly.
        if self.signal_props.group_intact(batch.id) {
            self.signal_models[batch.id].append(mesh, batch.active);
        }
    }
    pub(crate) fn append_signal_debris(&self, mesh: &mut Mesh, center: Vec3) {
        self.signal_props.append_moving(mesh, center);
    }
    // Retain the original combined path as a geometry regression reference.
    #[cfg(test)]
    pub fn append_signals(&self, mesh: &mut Mesh, center: Vec3) {
        for (id, signal) in self.graph.signals.iter().enumerate() {
            if signal.position.distance(center) > 360. || !self.signal_props.group_intact(id) {
                continue;
            }
            let active = (!self.failed_junctions.contains(&signal.junction))
                .then(|| aspect(signal, self.time));
            self.signal_models[id].append(mesh, active);
        }
        self.signal_props.append_moving(mesh, center);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opposing_directions_share_green_and_cross_traffic_waits() {
        let signals: Vec<_> = (0..4)
            .map(|approach| SignalApproach {
                position: Vec3::ZERO,
                mast_base: Some(Vec3::ZERO),
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

    fn traffic() -> Traffic {
        let program =
            opencity::parse(include_str!("../../../tests/fixtures/signal_cache.road")).unwrap();
        let city = opencity::runtime::City::from_program(&program, None).unwrap();
        let graph = std::sync::Arc::new(crate::hermes::traffic_paths::TrafficPaths::new(
            &city, &program,
        ));
        Traffic::new(&graph)
    }

    fn signature(mesh: &Mesh) -> Vec<[u32; 13]> {
        mesh.vertices
            .iter()
            .map(|v| {
                let mut values = v
                    .position
                    .iter()
                    .chain(&v.normal)
                    .chain(&v.color)
                    .chain(&v.surface);
                std::array::from_fn(|_| values.next().unwrap().to_bits())
            })
            .collect()
    }

    #[test]
    fn cached_signal_selections_match_legacy_geometry_for_every_phase_and_power_state() {
        let mut traffic = traffic();
        assert_eq!(traffic.standing_signal_batches(Vec3::ZERO).count(), 4);
        for time in [0., 0.01, 9.99, 10., 11.99, 12., 14., 24., 26., 28.] {
            traffic.time = time;
            for failed in [false, true] {
                traffic.failed_junctions.clear();
                if failed {
                    traffic
                        .failed_junctions
                        .extend(traffic.graph.signals.iter().map(|signal| signal.junction));
                }
                let mut expected = Mesh::default();
                traffic.append_signals(&mut expected, Vec3::ZERO);
                let mut actual = Mesh::default();
                for batch in traffic.standing_signal_batches(Vec3::ZERO) {
                    traffic.append_signal_batch(&batch, &mut actual);
                }
                traffic.append_signal_debris(&mut actual, Vec3::ZERO);
                assert_eq!(signature(&actual), signature(&expected));
            }
        }
        traffic.failed_junctions.clear();
        traffic.time = 0.;
        let before: Vec<_> = traffic.standing_signal_batches(Vec3::ZERO).collect();
        traffic.time = 0.01;
        assert_eq!(
            before,
            traffic
                .standing_signal_batches(Vec3::ZERO)
                .collect::<Vec<_>>()
        );
        traffic.time = 10.01;
        assert_ne!(
            before,
            traffic
                .standing_signal_batches(Vec3::ZERO)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn standing_signal_selection_preserves_the_exact_distance_boundary() {
        let traffic = traffic();
        let at = traffic.graph.signals[0].position;
        for (distance, visible) in [(359.99, true), (360., true), (360.01, false)] {
            assert_eq!(
                traffic
                    .standing_signal_batches(at + Vec3::Z * distance)
                    .any(|batch| batch.id == 0),
                visible
            );
        }
    }
}
