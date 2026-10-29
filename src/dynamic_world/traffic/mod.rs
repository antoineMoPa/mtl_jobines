//! Nearby physical traffic: lane pursuit, signals, following, and collision recovery.
mod signals;
#[cfg(test)]
mod tests;
use super::{catalog, Controls, SpawnPoint, Vehicle, METERS_PER_UNIT};
use crate::hermes::traffic_paths::{SpeedClass, TrafficPaths, Turn};
use glam::Vec3;
use opencity::runtime::City;
use std::sync::Arc;
const TARGET: usize = 36;
const SPAWN_RADIUS: f32 = 180.;
const RETAIN_RADIUS: f32 = 240.;
struct Driver {
    vehicle: usize,
    path: usize,
    distance: f32,
    next: Option<usize>,
    stopped: bool,
    stranded: f32,
}
pub(super) struct Traffic {
    graph: Arc<TrafficPaths>,
    drivers: Vec<Driver>,
    inactive: Vec<usize>,
    spawns: Vec<(usize, SpawnPoint)>,
    pub(super) time: f32,
    seed: u64,
    refill: f32,
}
impl Traffic {
    pub fn new(graph: &Arc<TrafficPaths>) -> Self {
        Self {
            graph: graph.clone(),
            drivers: Vec::new(),
            inactive: Vec::new(),
            spawns: Vec::new(),
            time: 0.,
            seed: 7193,
            refill: 0.,
        }
    }
    fn random(&mut self, count: usize) -> usize {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.seed >> 32) as usize % count.max(1)
    }
    pub fn populate(&mut self, city: &City, vehicles: &mut Vec<Vehicle>, center: Vec3) {
        let vacant = &mut self.inactive;
        self.drivers.retain(|d| {
            if vehicles[d.vehicle].position.distance(center) > RETAIN_RADIUS {
                vacant.push(d.vehicle);
                vehicles[d.vehicle].velocity = Vec3::ZERO;
                vehicles[d.vehicle].position.z = -1000.;
                false
            } else {
                true
            }
        });
        if self.drivers.len() >= TARGET {
            return;
        }
        let candidates: Vec<_> = self
            .graph
            .paths
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.speed != SpeedClass::Junction && p.length > 12. && p.near(center, SPAWN_RADIUS)
            })
            .flat_map(|(id, p)| {
                let steps = (p.length / 14.).ceil() as usize;
                (0..steps).map(move |i| (id, (i as f32 + 0.5) * p.length / steps as f32))
            })
            .filter(|&(id, distance)| {
                let point = self.graph.paths[id].sample(distance).0;
                let radius = point.distance(center);
                (25. ..SPAWN_RADIUS).contains(&radius)
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        let models: Vec<_> = catalog::definitions()
            .iter()
            .enumerate()
            .filter(|(_, d)| d.config.road_vehicle && d.config.dimensions.length < 6.)
            .map(|(id, _)| id)
            .collect();
        for _ in 0..TARGET * 30 {
            if self.drivers.len() >= TARGET {
                break;
            }
            let (path, distance) = candidates[self.random(candidates.len())];
            let (point, heading) = self.graph.paths[path].sample(distance);
            if vehicles
                .iter()
                .any(|v| v.position.distance(point) < v.length() * 0.5 + 7.)
            {
                continue;
            }
            let Ok(mut vehicle) = Vehicle::spawn_at(
                city,
                SpawnPoint {
                    point: (point.truncate() / METERS_PER_UNIT).to_array(),
                    heading: Some(heading.y.atan2(heading.x)),
                },
            ) else {
                continue;
            };
            vehicle.definition = Some(models[self.random(models.len())]);
            vehicle.position.z = point.z + vehicle.ride_height();
            // Reject stacked decks and unsupported lane geometry.
            if !city
                .surface_at(
                    (point.truncate() / METERS_PER_UNIT).to_array(),
                    (point.z + 0.5) / METERS_PER_UNIT,
                )
                .is_some_and(|s| (s.height * METERS_PER_UNIT - point.z).abs() < 0.5)
            {
                continue;
            }
            let index = self.inactive.pop().unwrap_or(vehicles.len());
            if index == vehicles.len() {
                vehicles.push(vehicle);
            } else {
                vehicles[index] = vehicle;
            }
            self.spawns.push((
                index,
                SpawnPoint {
                    point: (point.truncate() / METERS_PER_UNIT).to_array(),
                    heading: Some(heading.y.atan2(heading.x)),
                },
            ));
            self.drivers.push(Driver {
                vehicle: index,
                path,
                distance,
                next: None,
                stopped: false,
                stranded: 0.,
            });
        }
    }
    pub fn inactive(&self, index: usize) -> bool {
        self.inactive.contains(&index)
    }
    pub fn take_spawns(&mut self) -> Vec<(usize, SpawnPoint)> {
        std::mem::take(&mut self.spawns)
    }
    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.drivers.len()
    }
    pub fn release(&mut self, index: usize) {
        self.drivers.retain(|d| d.vehicle != index);
    }
    pub fn controls(
        &mut self,
        city: &City,
        vehicles: &[Vehicle],
        pedestrian: Option<Vec3>,
        dt: f32,
    ) -> Vec<(usize, Controls)> {
        self.time += dt;
        let mut turns = vec![None; vehicles.len()];
        for driver in &self.drivers {
            let movement = driver
                .next
                .map(|id| &self.graph.paths[id])
                .filter(|p| p.junction.is_some())
                .unwrap_or(&self.graph.paths[driver.path]);
            turns[driver.vehicle] = Some(movement.turn);
        }
        let mut result = Vec::with_capacity(self.drivers.len());
        for i in 0..self.drivers.len() {
            let path = self.drivers[i].path;
            if self.drivers[i].next.is_none() && !self.graph.paths[path].next.is_empty() {
                let choice = self.random(self.graph.paths[path].next.len());
                self.drivers[i].next = Some(self.graph.paths[path].next[choice]);
            }
            let driver = &mut self.drivers[i];
            let car = &vehicles[driver.vehicle];
            let ground = car.position - Vec3::Z * car.ride_height();
            let forward = Vec3::new(car.yaw.cos(), car.yaw.sin(), 0.);
            let (distance, error) = self.graph.paths[driver.path].project(ground);
            let heading = self.graph.paths[driver.path].sample(distance).1;
            if error > 3. || heading.dot(forward) < 0.1 {
                // Collision recovery only considers reachable, forward-facing nearby lanes.
                let recovery = self
                    .graph
                    .paths
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.speed != SpeedClass::Junction && p.near(ground, 10.))
                    .filter_map(|(id, p)| {
                        let (along, error) = p.project(ground);
                        let (point, heading) = p.sample(along);
                        if error > 10.
                            || (point.z - ground.z).abs() > 1.5
                            || heading.dot(forward) < 0.3
                            || p.length - along < 5.
                            || p.speed == SpeedClass::Junction
                        {
                            return None;
                        }
                        let clear = (1..=8).all(|step| {
                            let at = ground.lerp(point, step as f32 / 8.);
                            city.surface_at(
                                (at.truncate() / METERS_PER_UNIT).to_array(),
                                (at.z + 0.6) / METERS_PER_UNIT,
                            )
                            .is_some_and(|surface| {
                                (surface.height * METERS_PER_UNIT - at.z).abs() < 0.6
                            }) && !city.building_collision(
                                (at.truncate() / METERS_PER_UNIT).to_array(),
                                car.width() * 0.5 / METERS_PER_UNIT,
                                (at.z + 0.1) / METERS_PER_UNIT,
                                (at.z + car.height()) / METERS_PER_UNIT,
                            ) && city
                                .road_barrier_contact(
                                    (at.truncate() / METERS_PER_UNIT).to_array(),
                                    car.width() * 0.5 / METERS_PER_UNIT,
                                    (at.z + 0.1) / METERS_PER_UNIT,
                                    (at.z + car.height()) / METERS_PER_UNIT,
                                )
                                .is_none()
                        });
                        clear.then_some((id, along, error))
                    })
                    .min_by(|a, b| a.2.total_cmp(&b.2));
                if let Some((id, along, _)) = recovery {
                    driver.path = id;
                    driver.distance = along;
                    driver.next = None;
                } else {
                    driver.stopped = true;
                }
            } else {
                driver.distance = distance;
            }
            let path = &self.graph.paths[driver.path];
            let remaining = path.length - driver.distance;
            if remaining < 2.5 {
                if let Some(next) = driver.next {
                    driver.path = next;
                    driver.distance = 0.;
                    driver.next = None;
                }
            }
            let path = &self.graph.paths[driver.path];
            let remaining = path.length - driver.distance;
            let speed = car.velocity.truncate().length();
            let lookahead = (3.5 + speed * 0.7).clamp(3.5, 16.);
            let mut target = path.sample(driver.distance + lookahead).0;
            let mut limit = path.speed.meters_per_second();
            let mut clearance = f32::INFINITY;
            if let Some(next) = driver.next {
                let next_path = &self.graph.paths[next];
                // Brake ahead of lower speed roads and bends, not after entering them.
                limit = limit.min(
                    (next_path.speed.meters_per_second().powi(2) + 2. * 3. * remaining.max(0.))
                        .sqrt(),
                );
                if let Some(signal) = next_path.signal {
                    if !signals::is_green(&self.graph.signals[signal], self.time) {
                        clearance = clearance.min((remaining - car.length() * 0.5 - 1.5).max(0.));
                    }
                }
                if next_path.junction.is_some()
                    && vehicles.iter().enumerate().any(|(id, other)| {
                        if id == driver.vehicle {
                            return false;
                        }
                        let ground = other.position - Vec3::Z * other.ride_height();
                        let (along, error) = next_path.project(ground);
                        along > 3.
                            && along < next_path.length - 2.
                            && error < (car.width() + other.width()) * 0.5 + 1.
                    })
                {
                    clearance = clearance.min((remaining - car.length() * 0.5 - 1.5).max(0.));
                }
                if next_path.turn == Turn::Left {
                    if let Some(signal) = next_path.signal {
                        if left_turn_blocked(
                            &self.graph,
                            signal,
                            next_path.length,
                            driver.vehicle,
                            vehicles,
                            &turns,
                        ) {
                            clearance =
                                clearance.min((remaining - car.length() * 0.5 - 1.5).max(0.));
                        }
                    }
                }
                if remaining < lookahead {
                    target = next_path.sample(lookahead - remaining).0;
                }
            } else {
                clearance = clearance.min((remaining - car.length() * 0.5 - 1.).max(0.));
            }
            let aim = target.truncate() - car.position.truncate();
            let angle = forward
                .truncate()
                .perp_dot(aim)
                .atan2(forward.truncate().dot(aim));
            // Pure pursuit with the same speed-dependent steering range as Vehicle.
            let steering_angle = (2. * 2.6 * angle.sin() / aim.length().max(1.)).atan();
            let steering = steering_angle * (1. + speed * 0.025) / 0.52;
            limit = limit
                .min((2.5 / (2. * angle.sin().abs() / aim.length().max(1.)).max(0.0001)).sqrt());
            for (index, other) in vehicles.iter().enumerate() {
                if index == driver.vehicle {
                    continue;
                }
                let delta = other.position - car.position;
                let along = delta.dot(forward);
                let across = forward.truncate().perp_dot(delta.truncate()).abs();
                if delta.z.abs() < 2.
                    && along > 0.
                    && across < (car.width() + other.width()) * 0.5 + 0.5
                {
                    clearance =
                        clearance.min((along - (car.length() + other.length()) * 0.5 - 2.).max(0.));
                }
            }
            if let Some(point) = pedestrian {
                let delta = point - car.position;
                if delta.z.abs() < 2.
                    && delta.dot(forward) > 0.
                    && forward.truncate().perp_dot(delta.truncate()).abs() < car.width() * 0.5 + 1.
                {
                    clearance =
                        clearance.min((delta.dot(forward) - car.length() * 0.5 - 2.).max(0.));
                }
            }
            limit = limit.min((2. * 3. * clearance).sqrt());
            if clearance < 0.5 || driver.stopped {
                limit = 0.;
            }
            if speed < 0.5 && limit > 2. {
                driver.stranded += dt;
            } else {
                driver.stranded = 0.;
            }
            if driver.stranded > 8. {
                driver.stopped = true;
            }
            result.push((
                driver.vehicle,
                Controls {
                    throttle: if speed < limit - 0.3 {
                        ((limit - speed) * 0.4).min(0.7)
                    } else {
                        0.
                    },
                    brake: speed > limit + 0.3 || limit < 0.1,
                    steering: steering.clamp(-1., 1.),
                    handbrake: driver.stopped,
                },
            ));
        }
        result
    }
    pub fn needs_refill(&mut self, dt: f32) -> bool {
        self.refill -= dt;
        if self.refill <= 0. {
            self.refill = 1.;
            true
        } else {
            false
        }
    }
}

// Left turns yield to the opposing approach's straight/right traffic, including the player.
fn left_turn_blocked(
    graph: &TrafficPaths,
    signal: usize,
    turn_length: f32,
    car: usize,
    vehicles: &[Vehicle],
    turns: &[Option<Turn>],
) -> bool {
    let approach = &graph.signals[signal];
    graph
        .signals
        .iter()
        .filter(|other| {
            other.junction == approach.junction && other.forward.dot(approach.forward) < -0.7
        })
        .any(|opposite| {
            vehicles.iter().enumerate().any(|(id, vehicle)| {
                if id == car || turns[id] == Some(Turn::Left) {
                    return false;
                }
                let forward = glam::Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
                if forward.dot(opposite.forward.truncate()) < 0.75 {
                    return false;
                }
                let delta = opposite.position - vehicle.position;
                let along = delta.truncate().dot(forward);
                let across = forward.perp_dot(delta.truncate()).abs();
                let crossing_time = turn_length / SpeedClass::Junction.meters_per_second() + 1.;
                delta.z.abs() < 2.
                    && across < vehicle.width() * 0.5 + 1.5
                    && along > -vehicle.length()
                    && along < (vehicle.velocity.length() * crossing_time + 5.).max(15.)
            })
        })
}
