//! Nearby physical traffic: lane pursuit, signals, following, and collision recovery.
mod abandoned;
mod lane_change;
mod lod;
use lane_change::Maneuver;
mod models;
mod police;
pub use police::{ChaseStatus, Crime};
mod signals;
pub use signals::SignalBatch;
#[cfg(test)]
mod tests;
use super::{catalog, Controls, SpawnPoint, Vehicle, METERS_PER_UNIT};
use crate::hermes::traffic_paths::{SpeedClass, TrafficPaths, Turn};
use glam::Vec3;
use opencity::runtime::City;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
pub(super) const CRUISER: &str = "vehicle-j06";
pub(super) const TARGET: usize = 36;
const SPAWN_RADIUS: f32 = 180.;
const RETAIN_RADIUS: f32 = 240.;
fn snowplow(vehicle: &Vehicle) -> bool {
    vehicle
        .definition()
        .is_some_and(|d| d.config.research_id == catalog::SNOWPLOW_ID)
}
struct Driver {
    vehicle: usize,
    spawned_at: f32,
    path: usize,
    distance: f32,
    next: Option<usize>,
    stopped: bool,
    stranded: f32,
    maneuver: Maneuver,
    stop_wait: f32,
    stop_arrival: Option<f32>,
    motion: lod::Motion,
    physical_until: f32,
    collisions: u64,
    target_speed: f32,
    clearance: f32,
}
impl Driver {
    fn new(vehicle: usize, path: usize, distance: f32, spawned_at: f32) -> Self {
        Self {
            vehicle,
            spawned_at,
            path,
            distance,
            next: None,
            stopped: false,
            stranded: 0.,
            maneuver: Maneuver::Following,
            stop_wait: 0.,
            stop_arrival: None,
            motion: lod::Motion::Physical,
            physical_until: 0.,
            collisions: 0,
            target_speed: 0.,
            clearance: f32::INFINITY,
        }
    }
}
pub(super) struct Traffic {
    chase: police::Chase,
    season: crate::season::Season,
    graph: Arc<TrafficPaths>,
    drivers: Vec<Driver>,
    inactive: Vec<usize>,
    abandoned: Vec<usize>,
    spawns: Vec<(usize, SpawnPoint)>,
    pub(super) time: f32,
    seed: u64,
    refill: f32,
    signal_props: super::props::Props,
    signal_models: Vec<models::Standing>,
    failed_junctions: HashSet<usize>,
    crossing: HashMap<usize, usize>,
    physics_lod: bool,
    interacted: HashMap<usize, lod::Protected>,
}
impl Traffic {
    #[cfg(test)]
    pub fn new(graph: &Arc<TrafficPaths>) -> Self {
        Self::with_prop_focus(graph, None)
    }
    pub fn new_near(graph: &Arc<TrafficPaths>, center: Vec3) -> Self {
        Self::with_prop_focus(graph, Some(center))
    }
    fn with_prop_focus(graph: &Arc<TrafficPaths>, center: Option<Vec3>) -> Self {
        let (signal_models, groups): (Vec<_>, Vec<_>) =
            graph.signals.iter().map(models::instantiate).unzip();
        Self {
            season: Default::default(),
            chase: Default::default(),
            graph: graph.clone(),
            drivers: Vec::new(),
            inactive: Vec::new(),
            abandoned: Vec::new(),
            spawns: Vec::new(),
            time: 0.,
            seed: 7193,
            refill: 0.,
            signal_props: match center {
                Some(center) => super::props::Props::from_groups_near(groups, center),
                None => super::props::Props::from_groups(groups),
            },
            signal_models,
            // A junction without space for a supported mast uses the existing
            // all-way-stop behavior instead of planting a pole in a traffic lane.
            failed_junctions: graph
                .signals
                .iter()
                .filter(|signal| signal.mast_base.is_none())
                .map(|signal| signal.junction)
                .collect(),
            crossing: HashMap::new(),
            physics_lod: true,
            interacted: HashMap::new(),
        }
    }
    pub(super) fn bicycle_red_light(&self, end: Vec3, heading: Vec3) -> bool {
        self.graph.signals.iter().any(|signal| {
            !self.failed_junctions.contains(&signal.junction)
                && signal.position.distance(end) < 18.
                && signal.forward.dot(heading) > 0.4
                && !signals::is_green(signal, self.time)
        })
    }
    fn random(&mut self, count: usize) -> usize {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.seed >> 32) as usize % count.max(1)
    }
    pub fn set_season(
        &mut self,
        season: crate::season::Season,
        vehicles: &mut [Vehicle],
        player: usize,
    ) {
        use crate::season::Season;
        if self.season == season {
            return;
        }
        self.season = season;
        let mut retire: Vec<usize> = if season == Season::Summer {
            self.drivers
                .iter()
                .filter(|d| snowplow(&vehicles[d.vehicle]))
                .map(|d| d.vehicle)
                .collect()
        } else {
            let mut candidates: Vec<_> = self
                .drivers
                .iter()
                .filter(|d| d.vehicle != player && !self.interacted.contains_key(&d.vehicle))
                .filter(|d| {
                    vehicles[d.vehicle]
                        .definition()
                        .is_some_and(|d| d.config.dimensions.length < 6.)
                })
                .map(|d| d.vehicle)
                .collect();
            let center = vehicles.get(player).map_or(Vec3::ZERO, |v| v.position);
            candidates.sort_by(|a, b| {
                vehicles[*b]
                    .position
                    .distance_squared(center)
                    .total_cmp(&vehicles[*a].position.distance_squared(center))
            });
            candidates.into_iter().take(2).collect()
        };
        retire.retain(|&id| id != player && self.chase.patrol(id).is_none());
        self.drivers.retain(|d| !retire.contains(&d.vehicle));
        for id in retire {
            if !self.interacted.contains_key(&id) {
                vehicles[id].velocity = Vec3::ZERO;
                vehicles[id].position.z = -1000.;
                self.inactive.push(id);
            }
        }
        self.refill = 0.;
    }
    pub fn populate(&mut self, city: &City, vehicles: &mut Vec<Vehicle>, center: Vec3) {
        let vacant = &mut self.inactive;
        self.drivers.retain(|d| {
            if self.chase.patrol(d.vehicle).is_none()
                && vehicles[d.vehicle].position.distance(center) > RETAIN_RADIUS
            {
                // Player interaction leaves a persistent physical vehicle behind.
                // Only ordinary traffic may be hidden and its slot recycled.
                if !self.interacted.contains_key(&d.vehicle) {
                    vacant.push(d.vehicle);
                    vehicles[d.vehicle].velocity = Vec3::ZERO;
                    vehicles[d.vehicle].position.z = -1000.;
                }
                false
            } else {
                true
            }
        });
        if self.drivers.len() >= crate::performance::npc_budget(TARGET) {
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
                let radius = point.truncate().distance(center.truncate());
                (30. ..SPAWN_RADIUS).contains(&radius)
            })
            .collect();
        if candidates.is_empty() {
            return;
        }
        let models: Vec<_> = catalog::definitions()
            .iter()
            .enumerate()
            .filter(|(_, d)| {
                d.config.road_vehicle
                    && (self.chase.stars == 0 || d.config.research_id != CRUISER)
                    && !d.is_bicycle()
                    && (d.config.dimensions.length < 6.
                        || d.config.category == "bus"
                        || d.config.research_id == "vehicle-j05"
                        || (self.season == crate::season::Season::Winter
                            && d.config.research_id == catalog::SNOWPLOW_ID))
            })
            .map(|(id, _)| id)
            .collect();
        for _ in 0..TARGET * 30 {
            if self.drivers.len() >= crate::performance::npc_budget(TARGET) {
                break;
            }
            let (path, distance) = candidates[self.random(candidates.len())];
            let (point, heading) = self.graph.paths[path].sample(distance);
            let plows = self
                .drivers
                .iter()
                .filter(|d| snowplow(&vehicles[d.vehicle]))
                .count();
            let model = if self.season == crate::season::Season::Winter && plows < 2 {
                models
                    .iter()
                    .copied()
                    .find(|&id| {
                        catalog::definitions()[id].config.research_id == catalog::SNOWPLOW_ID
                    })
                    .unwrap_or_else(|| models[self.random(models.len())])
            } else {
                models[self.random(models.len())]
            };
            let definition = &catalog::definitions()[model];
            let length = definition.config.dimensions.length;
            // Keep long vehicles away from lane ends and existing vehicles.
            if distance < length * 0.5 + 1.5
                || (definition.config.research_id == catalog::SNOWPLOW_ID && plows >= 2)
                || self.graph.paths[path].length - distance < length * 0.5 + 1.5
                || (definition.config.category == "bus"
                    && self
                        .drivers
                        .iter()
                        .filter(|driver| {
                            vehicles[driver.vehicle]
                                .definition()
                                .is_some_and(|d| d.config.category == "bus")
                        })
                        .count()
                        >= 2)
                || (definition.config.research_id == "vehicle-j05"
                    && self
                        .drivers
                        .iter()
                        .filter(|driver| {
                            vehicles[driver.vehicle]
                                .definition()
                                .is_some_and(|d| d.config.research_id == "vehicle-j05")
                        })
                        .count()
                        >= 2)
            {
                continue;
            }
            if vehicles
                .iter()
                .any(|v| v.position.distance(point) < (v.length() + length) * 0.5 + 5.)
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
            vehicle.definition = Some(model);
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
            self.drivers
                .push(Driver::new(index, path, distance, self.time));
        }
    }
    pub(super) fn is_patrol(&self, index: usize) -> bool {
        self.chase.patrol(index).is_some()
    }
    pub fn driven_slots(&self) -> impl Iterator<Item = (usize, f32)> + '_ {
        self.drivers.iter().map(|d| (d.vehicle, self.fade(d)))
    }
    fn fade(&self, driver: &Driver) -> f32 {
        ((self.time - driver.spawned_at) / 0.8).clamp(0., 1.)
    }
    pub fn opacity(&self, index: usize) -> f32 {
        self.drivers
            .iter()
            .find(|d| d.vehicle == index)
            .map_or(1., |d| self.fade(d))
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
        self.chase.release(index);
        self.abandoned.retain(|&id| id != index);
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
        self.update_stop_priority(vehicles, dt);
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
            let responding = self.chase.patrol(self.drivers[i].vehicle);
            let patrol = responding.is_some();
            if let Some(index) = responding {
                let unit = &mut self.chase.patrols[index];
                if unit.officer.is_none() {
                    if let Some(controls) = unit.vehicle_controls(city, vehicles, dt) {
                        self.drivers[i].stopped = false;
                        self.drivers[i].stranded = 0.;
                        result.push((unit.vehicle, controls));
                        continue;
                    }
                }
                let order = self
                    .chase
                    .order(path, self.drivers[i].distance, &self.graph);
                self.chase.patrols[index].road_blocked =
                    self.drivers[i].stopped || matches!(order, police::LaneOrder::Unreachable);
                self.drivers[i].next = order.next();
            } else if self.drivers[i].next.is_none() && !self.graph.paths[path].next.is_empty() {
                let choice = self.random(self.graph.paths[path].next.len());
                self.drivers[i].next = Some(self.graph.paths[path].next[choice]);
            }
            let driver = &mut self.drivers[i];
            let car = &vehicles[driver.vehicle];
            if responding.is_some_and(|i| self.chase.patrols[i].officer.is_some()) {
                driver.target_speed = 0.;
                driver.clearance = 0.;
                result.push((
                    driver.vehicle,
                    Controls {
                        brake: true,
                        handbrake: true,
                        ..Default::default()
                    },
                ));
                continue;
            }
            let ground = car.position - Vec3::Z * car.ride_height();
            let forward = Vec3::new(car.yaw.cos(), car.yaw.sin(), 0.);
            if let Maneuver::Backing { until } = driver.maneuver {
                let rear_clear = vehicles.iter().enumerate().all(|(id, other)| {
                    id == driver.vehicle
                        || car.following_clearance(car.position, -forward.truncate(), other) > 1.
                }) && pedestrian.is_none_or(|p| {
                    let delta = p - car.position;
                    delta.z.abs() > 2.
                        || delta.dot(forward) >= 0.
                        || delta.truncate().length() > car.length() * 0.5 + 4.
                });
                if self.time < until && rear_clear && !patrol {
                    result.push((
                        driver.vehicle,
                        Controls {
                            throttle: if car.velocity.dot(forward) > -2.5 {
                                -0.6
                            } else {
                                0.
                            },
                            ..Default::default()
                        },
                    ));
                    continue;
                }
                driver.maneuver = Maneuver::Following;
                if !patrol {
                    if let Some((id, along)) = driver.alternate_lane(&self.graph, ground, vehicles)
                    {
                        driver.start_merge(id, along);
                    } else if let Some(&next) = self.graph.paths[driver.path]
                        .next
                        .iter()
                        .find(|&&id| Some(id) != driver.next)
                    {
                        driver.next = Some(next);
                        driver.stopped = false;
                    }
                }
            }
            let (distance, error) = match driver.motion {
                lod::Motion::Lane(_) => (driver.distance, 0.),
                lod::Motion::Physical => self.graph.paths[driver.path].project(ground),
            };
            let heading = self.graph.paths[driver.path].sample(distance).1;
            let lane = &self.graph.paths[driver.path];
            if matches!(driver.maneuver, Maneuver::Merging) && error < 1. {
                driver.maneuver = Maneuver::Following;
            }
            let merge = (!patrol
                && lane.next.is_empty()
                && (10. ..35.).contains(&(lane.length - distance)))
            .then(|| driver.alternate_lane(&self.graph, ground, vehicles))
            .flatten();
            if let Some((id, along)) = merge {
                driver.start_merge(id, along);
            } else if (error > 3. && !(matches!(driver.maneuver, Maneuver::Merging) && error < 6.))
                || heading.dot(forward) < 0.1
            {
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
            if remaining < 2.5 && matches!(driver.motion, lod::Motion::Physical) {
                if let Some(next) = driver.next {
                    driver.path = next;
                    driver.distance = 0.;
                    driver.next = None;
                }
            }
            let path = &self.graph.paths[driver.path];
            let remaining = path.length - driver.distance;
            let speed = car.velocity.truncate().length();
            let wheelbase = car.wheelbase();
            let lookahead = (3.5 + speed * 0.7).clamp(wheelbase.max(3.5), 16.);
            // A queue ahead of a siren can clear the intersection once it is physically free.
            let clearing_for_patrol = self.chase.patrols.iter().any(|p| {
                let unit = &vehicles[p.vehicle];
                let behind = car.position - unit.position;
                !patrol
                    && p.officer.is_none()
                    && (0. ..30.).contains(&behind.dot(forward))
                    && behind.z.abs() < 2.
                    && forward.truncate().perp_dot(behind.truncate()).abs() < 2.
                    && (car.yaw - unit.yaw).cos() > 0.85
            });
            let wait_for_signal = !patrol && !clearing_for_patrol;
            let mut target = path.sample(driver.distance + lookahead).0;
            let road_speed = |class: SpeedClass| {
                if patrol {
                    police::pursuit_speed(class)
                } else {
                    class.meters_per_second()
                }
            };
            let mut limit = road_speed(path.speed);
            if snowplow(car) {
                limit = limit.min(20. / 3.6);
            }
            let mut clearance = if patrol {
                self.chase
                    .order(driver.path, driver.distance, &self.graph)
                    .clearance(driver.distance)
            } else {
                f32::INFINITY
            };
            if let Some(next) = driver.next {
                let next_path = &self.graph.paths[next];
                // Brake ahead of lower speed roads and bends, not after entering them.
                limit = limit.min(
                    (road_speed(next_path.speed).powi(2) + 2. * 3. * remaining.max(0.)).sqrt(),
                );
                if let Some(signal) = next_path.signal.filter(|_| wait_for_signal) {
                    let junction = self.graph.signals[signal].junction;
                    let permitted = if self.failed_junctions.contains(&junction) {
                        self.crossing.get(&junction) == Some(&driver.vehicle)
                    } else {
                        signals::is_green(&self.graph.signals[signal], self.time)
                    };
                    if !permitted {
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
                if wait_for_signal && next_path.turn == Turn::Left {
                    if let Some(signal) = next_path.signal {
                        if !self
                            .failed_junctions
                            .contains(&self.graph.signals[signal].junction)
                            && left_turn_blocked(
                                &self.graph,
                                signal,
                                next_path.length,
                                driver.vehicle,
                                vehicles,
                                &turns,
                            )
                        {
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
            let curvature = 2. * angle.sin() / aim.length().max(1.);
            let steering = car.pursuit_steering(curvature, speed);
            limit =
                limit.min(((if patrol { 5. } else { 2.5 }) / curvature.abs().max(0.0001)).sqrt());
            let waiting_for_route = clearance.is_finite() && !path.next.is_empty();
            for (index, other) in vehicles.iter().enumerate() {
                if index == driver.vehicle {
                    continue;
                }
                clearance =
                    clearance.min(car.following_clearance(car.position, forward.truncate(), other));
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
            if responding.is_some_and(|i| self.chase.patrols[i].pull_over) {
                clearance =
                    clearance.min((car.position.distance(self.chase.last_known) - 14.).max(0.));
            }
            limit = limit.min((2. * 3. * clearance).sqrt());
            if clearance < 0.5 || driver.stopped {
                limit = 0.;
            }
            let can_retry = !patrol
                && !waiting_for_route
                && (path.next.len() > 1
                    || path
                        .adjacent
                        .iter()
                        .any(|&id| !self.graph.paths[id].next.is_empty()))
                && pedestrian.is_none_or(|p| p.distance(car.position) > car.length() + 3.);
            if speed < 0.5 && (limit > 2. || can_retry) {
                driver.stranded += dt;
            } else {
                driver.stranded = 0.;
            }
            if driver.stranded > 8. {
                if can_retry {
                    driver.maneuver = Maneuver::Backing {
                        until: self.time + 4.5,
                    };
                    driver.motion = lod::Motion::Physical;
                    driver.stranded = 0.;
                } else if limit > 2. {
                    driver.stopped = true;
                }
            }
            driver.target_speed = limit;
            driver.clearance = clearance;
            result.push((
                driver.vehicle,
                Controls {
                    throttle: if speed < limit - 0.3 {
                        ((limit - speed) * 0.4).min(if patrol { 1. } else { 0.7 })
                    } else {
                        0.
                    },
                    brake: speed > limit + 0.3 || limit < 0.1,
                    steering: steering.clamp(-1., 1.),
                    handbrake: driver.stopped,
                    ..Controls::default()
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
