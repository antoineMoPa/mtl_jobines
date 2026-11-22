//! Bounded nearby sidewalk population, independent of vehicle physics.
mod zombies;
use super::{
    pedestrian_catalog::{self, JOINT_LIMIT},
    pedestrian_gait::{footing, Gait},
    props::Props,
    ragdoll::{self, Reaction},
    Vehicle, METERS_PER_UNIT,
};
use crate::hermes::walking_paths::{support, WalkingPaths};
use glam::{Mat4, Vec2, Vec3};
use opencity::runtime::City;
use std::sync::Arc;

pub(crate) const CAPACITY: usize = 32;
pub(crate) const KIOSK_CAPACITY: usize = 3;
const SPAWN_RADIUS: f32 = 100.;
const RETAIN_RADIUS: f32 = 140.;
const DRAW_RADIUS: f32 = 100.;
const KIOSK_SPAWN_RADIUS: f32 = 70.;
const KIOSK_RETAIN_RADIUS: f32 = 85.;
struct KioskSite {
    front: Vec2,
    toward_front: Vec2,
    elevation: f32,
}
struct StandingActor {
    model: usize,
    position: Vec3,
    yaw: f32,
    idle_time: f32,
}
impl KioskSite {
    fn from_city(city: &City) -> Option<Self> {
        let feature = city.landscape.iter().find(|feature| {
            feature.model.as_deref() == Some("study_01")
                && feature.name.as_deref() == Some("site_model_01")
                && feature.points.len() == 4
        })?;
        let front_edge = (Vec2::from_array(feature.points[0])
            + Vec2::from_array(feature.points[1]))
            * (METERS_PER_UNIT * 0.5);
        let back = (Vec2::from_array(feature.points[2]) + Vec2::from_array(feature.points[3]))
            * (METERS_PER_UNIT * 0.5);
        let along = (Vec2::from_array(feature.points[1]) - Vec2::from_array(feature.points[0]))
            * METERS_PER_UNIT;
        // The kiosk occupies the positive-X side of the source sphere mesh.
        // Match the XY fit used by Hermes' landmark mesh placement.
        let scale = (along.length() / 2.9923563).min(front_edge.distance(back) / 3.196178);
        let front = front_edge + along / along.length().max(0.0001) * (0.76 * scale);
        Some(Self {
            front,
            toward_front: (front_edge - back) / front_edge.distance(back).max(0.0001),
            elevation: feature.elevation,
        })
    }
    fn actors(&self, city: &City) -> Vec<StandingActor> {
        // Guests wait in front of the original kiosk frontage.
        let positions = [(-2.6, 1), (-5.0, 3), (-7.4, 5)];
        positions
            .into_iter()
            .map(|(offset, model)| {
                let xy = self.front - self.toward_front * offset;
                let map_xy = (xy / METERS_PER_UNIT).to_array();
                let z = city
                    .surface_at(map_xy, self.elevation + 0.05)
                    .map_or(self.elevation, |surface| surface.height)
                    * METERS_PER_UNIT;
                let direction = -self.toward_front;
                StandingActor {
                    model,
                    position: xy.extend(z),
                    yaw: direction.y.atan2(direction.x),
                    idle_time: model as f32 * 0.37,
                }
            })
            .collect()
    }
}
struct Walker {
    spawned_at_kiosk: bool,
    serial: u32,
    model: usize,
    path: usize,
    distance: f32,
    position: Vec3,
    yaw: f32,
    lane: f32,
    preferred_speed: f32,
    speed: f32,
    movement: f32,
    walk_time: f32,
    idle_time: f32,
    pause: f32,
    until_pause: f32,
    blocked_time: f32,
    age: f32,
    immunity: f32,
    health: u8,
    stagger: f32,
    deferred_dt: f32,
    preview_cache: Option<(usize, f32, f32, Option<Vec3>)>,
    reaction: Option<Box<Reaction>>,
    returning: Option<ReturnPoint>,
    gait: Gait,
}
impl Walker {
    fn fall(&mut self, city: &City, vehicles: &[Vehicle], velocity: Vec3) {
        self.gait.melee = Default::default();
        self.reaction = Some(Box::new(Reaction::Falling(ragdoll::Ragdoll::new(
            &pedestrian_catalog::definitions()[self.model],
            self.gait.palette,
            self.position,
            self.yaw,
            velocity,
            city,
            vehicles,
        ))));
        self.speed = 0.;
        self.movement = 0.;
        self.deferred_dt = 0.;
    }
}
#[derive(Clone, Copy)]
struct ReturnPoint {
    position: Vec3,
    path: usize,
    distance: f32,
}
pub struct Instance<'a> {
    pub zombie: bool,
    pub model: usize,
    pub transform: Mat4,
    pub opacity: f32,
    pub idle_time: f32,
    pub walk_time: f32,
    pub movement: f32,
    pub pose: Option<&'a [Mat4; JOINT_LIMIT]>,
}
pub(crate) struct Crowd {
    graph: Arc<WalkingPaths>,
    walkers: Vec<Walker>,
    seed: u32,
    serial: u32,
    step_number: u64,
    refill: f32,
    pub(super) previous_vehicles: Vec<Vec3>,
    pub(super) vehicle_knockdowns: Vec<usize>,
    pub(super) zombies: bool,
    kiosk_checked: bool,
    kiosk_spawned: bool,
    kiosk_site: Option<KioskSite>,
    kiosk_actors: Vec<StandingActor>,
}
fn random(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    (*seed >> 8) as f32 / 16777216.
}
fn angle_delta(from: f32, to: f32) -> f32 {
    (to - from + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}
fn path_distance(graph: &WalkingPaths, path: usize, position: Vec3) -> f32 {
    let mut best = (0., f32::INFINITY);
    let mut along = 0.;
    for pair in graph.paths[path].points.windows(2) {
        let delta = pair[1] - pair[0];
        let length = delta.length();
        let t = ((position - pair[0]).dot(delta) / length.powi(2).max(0.0001)).clamp(0., 1.);
        let error = position.distance_squared(pair[0] + delta * t);
        if error < best.1 {
            best = (along + length * t, error);
        }
        along += length;
    }
    best.0
}
fn nearest_sidewalk(graph: &WalkingPaths, position: Vec3) -> Option<(usize, f32)> {
    graph
        .paths
        .iter()
        .enumerate()
        .filter(|(_, path)| !path.connector && path.length > 1.)
        .map(|(index, path)| {
            let distance = path_distance(graph, index, position);
            let (sample, _) = path.sample(distance);
            (index, distance, sample.distance_squared(position))
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(index, distance, _)| (index, distance))
}
impl StandingActor {
    fn into_walker(
        self,
        city: &City,
        serial: u32,
        path: usize,
        distance: f32,
        spawned_at_kiosk: bool,
    ) -> Walker {
        let definition = &pedestrian_catalog::definitions()[self.model];
        Walker {
            spawned_at_kiosk,
            serial,
            model: self.model,
            path,
            distance,
            position: self.position,
            yaw: self.yaw,
            lane: 0.42,
            preferred_speed: definition.walk_speed,
            speed: 0.,
            movement: 0.,
            walk_time: 0.,
            idle_time: self.idle_time,
            pause: 0.,
            until_pause: 12.,
            blocked_time: 0.,
            age: 1.,
            immunity: 0.,
            health: super::melee::ZOMBIE_HEALTH,
            stagger: 0.,
            deferred_dt: 0.,
            preview_cache: None,
            reaction: None,
            returning: None,
            gait: Gait::new(definition, self.position, self.yaw, serial, |p| {
                footing(city, p, false)
            }),
        }
    }
}
fn collide_queue(
    actors: &mut Vec<StandingActor>,
    walkers: &mut Vec<Walker>,
    graph: &WalkingPaths,
    serial: &mut u32,
    city: &City,
    vehicles: &[Vehicle],
    vehicle_knockdowns: &mut Vec<usize>,
    mut impact: impl FnMut(Vec3) -> Option<(Option<usize>, Vec3)>,
) {
    let mut index = 0;
    while index < actors.len() {
        let Some((vehicle, velocity)) = impact(actors[index].position) else {
            index += 1;
            continue;
        };
        let Some((path, distance)) = nearest_sidewalk(graph, actors[index].position) else {
            index += 1;
            continue;
        };
        let actor = actors.remove(index);
        let mut walker = actor.into_walker(city, *serial, path, distance, true);
        if velocity != Vec3::ZERO {
            walker.fall(city, vehicles, velocity);
            if let Some(id) = vehicle {
                vehicle_knockdowns.push(id);
            }
        }
        walkers.push(walker);
        *serial = serial.wrapping_add(1);
    }
}
fn placement(
    city: &City,
    graph: &WalkingPaths,
    path: usize,
    distance: f32,
) -> Option<(Vec3, Vec3)> {
    placement_offset(city, graph, path, distance, 0.42)
}
fn placement_offset(
    city: &City,
    graph: &WalkingPaths,
    path: usize,
    distance: f32,
    lane: f32,
) -> Option<(Vec3, Vec3)> {
    let (center, forward) = graph.paths[path].sample(distance);
    // Keep right in both directions. Fall back to the center on narrow corners.
    let right = Vec3::new(forward.y, -forward.x, 0.);
    let position = support(city, center + right * lane).or_else(|| support(city, center))?;
    Some((position, forward))
}
fn blocked_by_vehicle(position: Vec3, vehicles: &[Vehicle]) -> bool {
    vehicles.iter().any(|v| {
        let delta = position - v.position;
        if delta.z.abs() > 2. {
            return false;
        }
        let f = Vec3::new(v.yaw.cos(), v.yaw.sin(), 0.);
        let side = Vec3::new(-f.y, f.x, 0.);
        // Current and near-future footprints allow a walker to yield before contact.
        [delta, delta - v.velocity * 0.65].into_iter().any(|d| {
            d.dot(f).abs() < v.length() * 0.5 + 0.55 && d.dot(side).abs() < v.width() * 0.5 + 0.55
        })
    })
}
impl Crowd {
    pub(super) fn accept_occupant(
        &mut self,
        city: &City,
        model: usize,
        person: &super::Pedestrian,
    ) -> bool {
        let position = person.position - Vec3::Z * super::player::HALF_HEIGHT;
        let Some((path, distance)) = nearest_sidewalk(&self.graph, position) else {
            return false;
        };
        let Some((destination, _)) = placement(city, &self.graph, path, distance) else {
            return false;
        };
        let actor = StandingActor {
            model,
            position,
            yaw: person.yaw,
            idle_time: 0.,
        };
        let mut walker = actor.into_walker(city, self.serial, path, distance, false);
        walker.returning = Some(ReturnPoint {
            position: destination,
            path,
            distance,
        });
        self.walkers.push(walker);
        self.serial = self.serial.wrapping_add(1);
        true
    }
    pub fn new(graph: &Arc<WalkingPaths>) -> Self {
        Self {
            graph: Arc::clone(graph),
            walkers: Vec::with_capacity(CAPACITY),
            seed: 0x70656431,
            serial: 0,
            step_number: 0,
            refill: 0.,
            previous_vehicles: Vec::new(),
            vehicle_knockdowns: Vec::new(),
            zombies: false,
            kiosk_checked: false,
            kiosk_spawned: false,
            kiosk_site: None,
            kiosk_actors: Vec::new(),
        }
    }
    pub fn populate(&mut self, city: &City, center: Vec3, vehicles: &[Vehicle], props: &Props) {
        if !self.kiosk_checked {
            self.kiosk_site = KioskSite::from_city(city);
            self.kiosk_checked = true;
        }
        self.walkers.retain(|w| {
            (w.reaction.is_some() && w.health > 0)
                || w.position.distance_squared(center) < RETAIN_RADIUS.powi(2)
        });
        if self.previous_vehicles.is_empty() {
            self.previous_vehicles
                .extend(vehicles.iter().map(|v| v.position));
        }
        let nearby: Vec<_> = self
            .graph
            .paths
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.connector && p.length > 8. && p.near(center, SPAWN_RADIUS))
            .map(|(i, _)| i)
            .collect();
        if nearby.is_empty() {
            return;
        }
        for _ in 0..CAPACITY * 24 {
            if self.walkers.len() >= crate::performance::npc_budget(CAPACITY) {
                break;
            }
            let path = nearby[(random(&mut self.seed) * nearby.len() as f32) as usize];
            let length = self.graph.paths[path].length;
            let distance = 1. + random(&mut self.seed) * (length - 2.);
            let Some((position, direction)) = placement(city, &self.graph, path, distance) else {
                continue;
            };
            let reach = position.distance(center);
            if !(12. ..SPAWN_RADIUS).contains(&reach)
                || (position.z - center.z).abs() > 8.
                || self
                    .walkers
                    .iter()
                    .any(|w| w.position.distance_squared(position) < 2.8f32.powi(2))
                || blocked_by_vehicle(position, vehicles)
                || props.blocked(position + Vec3::Z * 0.8)
            {
                continue;
            }
            let definitions = pedestrian_catalog::definitions();
            let model = self.serial as usize % pedestrian_catalog::CROWD_MODEL_COUNT;
            let definition = &definitions[model];
            let preferred_speed = definition.walk_speed * (0.88 + random(&mut self.seed) * 0.24);
            let pause = if random(&mut self.seed) < 0.2 {
                random(&mut self.seed) * 3.
            } else {
                0.
            };
            self.walkers.push(Walker {
                spawned_at_kiosk: false,
                serial: self.serial,
                model,
                path,
                distance,
                position,
                yaw: direction.y.atan2(direction.x),
                lane: 0.42,
                preferred_speed,
                speed: if pause > 0. { 0. } else { preferred_speed },
                movement: if pause > 0. { 0. } else { 1. },
                walk_time: random(&mut self.seed) * definition.walk_duration,
                idle_time: random(&mut self.seed) * 3.,
                pause,
                until_pause: 10. + random(&mut self.seed) * 35.,
                blocked_time: 0.,
                age: 0.,
                immunity: 0.,
                health: super::melee::ZOMBIE_HEALTH,
                stagger: 0.,
                deferred_dt: 0.,
                preview_cache: None,
                reaction: None,
                returning: None,
                gait: Gait::new(
                    definition,
                    position,
                    direction.y.atan2(direction.x),
                    self.serial,
                    |p| footing(city, p, true),
                ),
            });
            self.serial = self.serial.wrapping_add(1);
        }
    }
    pub(super) fn overlaps_vehicle(&self, car: &Vehicle) -> bool {
        self.walkers
            .iter()
            .any(|w| super::player::blocked_by_vehicle(w.position, car))
            || self
                .kiosk_actors
                .iter()
                .any(|a| super::player::blocked_by_vehicle(a.position, car))
    }
    pub fn player_move(
        &mut self,
        city: &City,
        before: Vec3,
        after: Vec3,
        dt: f32,
        vehicles: &[Vehicle],
    ) {
        if self.zombies {
            return;
        }
        for walker in &mut self.walkers {
            if walker.reaction.is_some() || walker.immunity > 0. {
                continue;
            }
            if let Some(velocity) = ragdoll::walking_impact(walker.position, before, after, dt) {
                walker.fall(city, vehicles, velocity);
            }
        }
        collide_queue(
            &mut self.kiosk_actors,
            &mut self.walkers,
            &self.graph,
            &mut self.serial,
            city,
            vehicles,
            &mut self.vehicle_knockdowns,
            |position| ragdoll::walking_impact(position, before, after, dt).map(|v| (None, v)),
        );
    }
    pub fn step(
        &mut self,
        city: &City,
        center: Vec3,
        player_on_foot: bool,
        vehicles: &[Vehicle],
        props: &Props,
        dt: f32,
    ) {
        self.step_inner(city, center, player_on_foot, vehicles, props, dt, None);
    }
    pub(super) fn step_profiled(
        &mut self,
        city: &City,
        center: Vec3,
        player_on_foot: bool,
        vehicles: &[Vehicle],
        props: &Props,
        dt: f32,
        profile: &mut [u64; 13],
    ) {
        self.step_inner(
            city,
            center,
            player_on_foot,
            vehicles,
            props,
            dt,
            Some(profile),
        );
    }
    fn step_inner(
        &mut self,
        city: &City,
        center: Vec3,
        player_on_foot: bool,
        vehicles: &[Vehicle],
        props: &Props,
        dt: f32,
        mut profile: Option<&mut [u64; 13]>,
    ) {
        self.vehicle_knockdowns.clear();
        let setup_started = profile.as_ref().map(|_| web_time::Instant::now());
        // A public world step can also be used by tools; bound displacement before
        // collision sampling even if a tool sends a long frame.
        let dt = dt.min(0.1);
        self.step_number = self.step_number.wrapping_add(1);
        if let Some(site) = &self.kiosk_site {
            let distance = center.truncate().distance(site.front);
            if distance > KIOSK_RETAIN_RADIUS && self.kiosk_spawned {
                self.kiosk_actors = Vec::new();
                self.walkers.retain(|walker| !walker.spawned_at_kiosk);
                self.kiosk_spawned = false;
            } else if distance < KIOSK_SPAWN_RADIUS && !self.kiosk_spawned {
                let room = (crate::performance::npc_budget(CAPACITY) + KIOSK_CAPACITY)
                    .saturating_sub(self.walkers.len());
                self.kiosk_actors = site.actors(city).into_iter().take(room).collect();
                self.kiosk_spawned = true;
            }
        }
        for actor in &mut self.kiosk_actors {
            actor.idle_time += dt;
        }
        collide_queue(
            &mut self.kiosk_actors,
            &mut self.walkers,
            &self.graph,
            &mut self.serial,
            city,
            vehicles,
            &mut self.vehicle_knockdowns,
            |position| {
                if self.zombies {
                    Some((None, Vec3::ZERO))
                } else {
                    ragdoll::impact(position, &self.previous_vehicles, vehicles, dt)
                        .map(|(id, v)| (Some(id), v))
                }
            },
        );
        self.refill -= dt;
        if self.refill <= 0. {
            self.refill = 1.;
            self.populate(city, center, vehicles, props);
        }
        let neighbors: Vec<_> = self
            .walkers
            .iter()
            .map(|w| (w.serial, w.position, w.yaw))
            .collect();
        if let (Some(profile), Some(started)) = (profile.as_deref_mut(), setup_started) {
            profile[0] += started.elapsed().as_nanos() as u64;
        }
        let walking_started = profile.as_ref().map(|_| web_time::Instant::now());
        for walker in &mut self.walkers {
            let distance_band = profile.as_ref().map(|_| {
                let distance = walker.position.distance_squared(center);
                if distance < 30. * 30. {
                    0
                } else if distance < 60. * 60. {
                    1
                } else {
                    2
                }
            });
            let walker_started = profile.as_ref().map(|_| web_time::Instant::now());
            walker.age += dt;
            walker.immunity = (walker.immunity - dt).max(0.);
            if let Some(reaction) = &mut walker.reaction {
                let definition = &pedestrian_catalog::definitions()[walker.model];
                match reaction.step(
                    definition,
                    city,
                    vehicles,
                    dt,
                    &mut walker.position,
                    !self.zombies || walker.health > 0,
                ) {
                    ragdoll::ReactionStep::Ongoing => {}
                    ragdoll::ReactionStep::StartedRising => {
                        let destination = return_point(city, &self.graph, walker, vehicles, props);
                        walker.path = destination.path;
                        walker.distance = destination.distance;
                        walker.returning = Some(destination);
                    }
                    ragdoll::ReactionStep::Recovered => {
                        walker.lane = 0.42;
                        walker.speed = 0.;
                        walker.movement = 0.;
                        walker.pause = 0.;
                        walker.blocked_time = 0.;
                        walker.until_pause = 12.;
                        walker.immunity = ragdoll::RECOVERY_IMMUNITY;
                        walker.gait = Gait::new(
                            definition,
                            walker.position,
                            walker.yaw,
                            walker.serial,
                            |p| footing(city, p, false),
                        );
                        walker.reaction = None;
                    }
                }
                continue;
            }
            let impact_started = profile.as_ref().map(|_| web_time::Instant::now());
            if walker.immunity <= 0. {
                if let Some((id, velocity)) =
                    ragdoll::impact(walker.position, &self.previous_vehicles, vehicles, dt)
                {
                    walker.fall(city, vehicles, velocity);
                    self.vehicle_knockdowns.push(id);
                    continue;
                }
            }
            if let (Some(profile), Some(started)) = (profile.as_deref_mut(), impact_started) {
                profile[6] += started.elapsed().as_nanos() as u64;
            }
            if self.zombies {
                zombies::pursue(walker, city, center, vehicles, props, &neighbors, dt);
                continue;
            }
            // Keep contact reactions at full rate. Distant walking can use a
            // longer step and alternate walkers across physics ticks.
            let coarse =
                walker.position.distance_squared(center) > 60. * 60. && dt <= 1. / 120. + 0.00001;
            if coarse && (self.step_number + walker.serial as u64) & 1 == 0 {
                walker.deferred_dt += dt;
                continue;
            }
            let dt = dt + std::mem::take(&mut walker.deferred_dt);
            walker.idle_time += dt;
            walker.until_pause -= dt;
            if walker.until_pause <= 0. {
                walker.pause = 1.2 + random(&mut self.seed) * 3.2;
                walker.until_pause = 15. + random(&mut self.seed) * 35.;
            }
            walker.pause = (walker.pause - dt).max(0.);
            if let Some(destination) = walker.returning {
                if walker
                    .position
                    .truncate()
                    .distance(destination.position.truncate())
                    < 0.16
                {
                    walker.returning = None;
                    walker.distance = path_distance(&self.graph, walker.path, walker.position);
                }
            }
            if walker.returning.is_none() {
                let path = &self.graph.paths[walker.path];
                if walker.distance >= path.length - 0.08 {
                    let next = if path.next.is_empty() {
                        path.reverse
                    } else {
                        path.next[(random(&mut self.seed) * path.next.len() as f32) as usize]
                    };
                    if next == path.reverse {
                        walker.lane = -walker.lane;
                    }
                    walker.path = next;
                    walker.distance = 0.;
                }
            }
            let forward = Vec3::new(walker.yaw.cos(), walker.yaw.sin(), 0.);
            let preview_started = profile.as_ref().map(|_| web_time::Instant::now());
            let preview = if let Some(destination) = walker.returning {
                Some(destination.position)
            } else {
                let distance = (walker.distance + 0.70).min(self.graph.paths[walker.path].length);
                match walker.preview_cache {
                    Some((path, cached_distance, lane, point))
                        if path == walker.path
                            && cached_distance == distance
                            && lane == walker.lane =>
                    {
                        point
                    }
                    _ => {
                        let point =
                            placement_offset(city, &self.graph, walker.path, distance, walker.lane)
                                .map(|p| p.0);
                        walker.preview_cache = Some((walker.path, distance, walker.lane, point));
                        point
                    }
                }
            };
            if let (Some(profile), Some(started)) = (profile.as_deref_mut(), preview_started) {
                profile[5] += started.elapsed().as_nanos() as u64;
            }
            let blocked_started = profile.as_ref().map(|_| web_time::Instant::now());
            let blocked = preview.is_none_or(|p| {
                props.blocked(p + Vec3::Z * 0.8)
                    || blocked_by_vehicle(p, vehicles)
                    || (player_on_foot && p.distance_squared(center) < 1.1f32.powi(2))
                    || neighbors.iter().any(|&(serial, other, heading)| {
                        if serial == walker.serial {
                            return false;
                        }
                        let delta = other - walker.position;
                        let approaching =
                            forward.dot(Vec3::new(heading.cos(), heading.sin(), 0.)) < -0.25;
                        delta.z.abs() < 0.8
                            && delta.truncate().length() < 1.2
                            && delta.dot(forward) > 0.
                            && (p - other).truncate().length() < 0.65
                            && (!approaching || walker.serial > serial)
                    })
            });
            if let (Some(profile), Some(started)) = (profile.as_deref_mut(), blocked_started) {
                profile[3] += started.elapsed().as_nanos() as u64;
            }
            let aim = preview.unwrap_or(walker.position + forward);
            let delta = aim - walker.position;
            let turn = if delta.truncate().length_squared() > 0.000001 {
                angle_delta(walker.yaw, delta.y.atan2(delta.x))
            } else {
                0.
            };
            let target = if blocked || walker.pause > 0. || turn.abs() > 1.2 {
                0.
            } else {
                walker.preferred_speed * turn.cos().max(0.)
            };
            walker.speed += (target - walker.speed).clamp(-2. * dt, 1.2 * dt);
            if walker.pause <= 0. {
                walker.yaw += turn.clamp(-2.2 * dt, 2.2 * dt);
            }
            let movement_started = profile.as_ref().map(|_| web_time::Instant::now());
            let mut moved = 0.;
            let travel = (walker.speed * dt).min(delta.truncate().length());
            if turn.abs() < 1.2 && travel > 0. {
                let candidate =
                    walker.position + Vec3::new(walker.yaw.cos(), walker.yaw.sin(), 0.) * travel;
                if let Some(ground) =
                    footing(city, candidate, walker.returning.is_none()).filter(|g| {
                        !props.blocked(g.point + Vec3::Z * 0.8)
                            && !blocked_by_vehicle(g.point, vehicles)
                            && (!player_on_foot
                                || g.point.distance_squared(center) >= 0.7f32.powi(2))
                            && walker.gait.permits(g.point)
                    })
                {
                    let delta = ground.point - walker.position;
                    // Spend the same travel budget climbing a curb instead of
                    // snapping the body vertically to a newly sampled surface.
                    let next =
                        walker.position + delta * (travel / delta.length().max(0.00001)).min(1.);
                    moved = walker.position.distance(next);
                    walker.position = next;
                    if walker.returning.is_none() {
                        walker.distance = path_distance(&self.graph, walker.path, walker.position);
                        walker.lane += (0.42 - walker.lane).clamp(-0.25 * dt, 0.25 * dt);
                    }
                } else {
                    walker.speed = 0.;
                }
            }
            if let (Some(profile), Some(started)) = (profile.as_deref_mut(), movement_started) {
                profile[4] += started.elapsed().as_nanos() as u64;
            }
            let definition = &pedestrian_catalog::definitions()[walker.model];
            let actual_speed = if dt > 0. { moved / dt } else { 0. };
            let gait_started = profile.as_ref().map(|_| web_time::Instant::now());
            let desired_speed = if target > 0. {
                walker.preferred_speed
            } else {
                0.
            };
            let sample = |p| {
                footing(city, p, walker.returning.is_none()).filter(|g| {
                    !props.blocked(g.point + Vec3::Z * 0.8)
                        && !blocked_by_vehicle(g.point, vehicles)
                })
            };
            if coarse {
                walker.gait.advance_coarse(
                    definition,
                    walker.position,
                    walker.yaw,
                    desired_speed,
                    aim,
                    dt,
                    sample,
                );
            } else {
                walker.gait.advance(
                    definition,
                    walker.position,
                    walker.yaw,
                    desired_speed,
                    aim,
                    dt,
                    sample,
                );
            }
            if let (Some(profile), Some(started)) = (profile.as_deref_mut(), gait_started) {
                profile[2] += started.elapsed().as_nanos() as u64;
            }
            walker.speed = actual_speed;
            walker.blocked_time = if blocked {
                walker.blocked_time + dt
            } else {
                0.
            };
            if walker.returning.is_none()
                && walker.blocked_time > 2.5 + (walker.serial % 5) as f32 * 0.2
            {
                let current = &self.graph.paths[walker.path];
                walker.distance = current.length - walker.distance;
                walker.path = current.reverse;
                walker.lane = -walker.lane;
                walker.blocked_time = 0.;
                walker.speed = 0.;
                walker.pause = 0.3 + random(&mut self.seed) * 0.6;
            }
            let definition = &pedestrian_catalog::definitions()[walker.model];
            // Distance drives foot cadence. Stopping never resets a shared clock.
            walker.walk_time = (walker.walk_time + moved / definition.walk_speed)
                .rem_euclid(definition.walk_duration);
            let movement = if moved > 0.00001 {
                (walker.speed / walker.preferred_speed).clamp(0., 1.)
            } else {
                0.
            };
            walker.movement += (movement - walker.movement).clamp(-dt * 4., dt * 4.);
            if let (Some(profile), Some(started), Some(distance_band)) =
                (profile.as_deref_mut(), walker_started, distance_band)
            {
                profile[7 + distance_band] += started.elapsed().as_nanos() as u64;
                profile[10 + distance_band] += 1;
            }
        }
        if let (Some(profile), Some(started)) = (profile.as_deref_mut(), walking_started) {
            profile[1] += started.elapsed().as_nanos() as u64;
        }
        self.previous_vehicles.clear();
        self.previous_vehicles
            .extend(vehicles.iter().map(|v| v.position));
    }
    pub fn instances(&self, center: Vec3) -> impl Iterator<Item = Instance<'_>> + '_ {
        self.walkers
            .iter()
            .filter_map(move |w| {
                let distance = w.position.distance(center);
                let opacity = ((crate::performance::draw_radius(DRAW_RADIUS) - distance) / 15.)
                    .clamp(0., 1.)
                    * (w.age / 0.6).clamp(0., 1.);
                (opacity > 0.).then(|| Instance {
                    zombie: self.zombies,
                    model: w.model,
                    transform: pedestrian_catalog::instance_transform(w.position, w.yaw),
                    opacity,
                    idle_time: w.idle_time,
                    walk_time: w.walk_time,
                    movement: w.movement,
                    pose: Some(w.reaction.as_ref().map_or(&w.gait.palette, |r| r.palette())),
                })
            })
            .chain(self.kiosk_actors.iter().map(|actor| Instance {
                zombie: false,
                model: actor.model,
                transform: pedestrian_catalog::instance_transform(actor.position, actor.yaw),
                opacity: 1.,
                idle_time: actor.idle_time,
                walk_time: 0.,
                movement: 0.,
                pose: None,
            }))
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn prepare_collision(&mut self) -> (Vec3, f32) {
        self.walkers.truncate(1);
        self.refill = 1000.;
        let walker = &mut self.walkers[0];
        walker.pause = 2.;
        walker.until_pause = 100.;
        walker.speed = 0.;
        walker.movement = 0.;
        walker.age = 1.;
        self.previous_vehicles.clear();
        (walker.position, walker.yaw)
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn reaction_counts(&self) -> [usize; 3] {
        let mut counts = [0; 3];
        for w in &self.walkers {
            counts[match w.reaction.as_deref() {
                None => 0,
                Some(Reaction::Falling(_)) => 1,
                Some(Reaction::Rising { .. }) => 2,
            }] += 1;
        }
        counts
    }
}

fn return_point(
    city: &City,
    graph: &WalkingPaths,
    walker: &Walker,
    vehicles: &[Vehicle],
    props: &Props,
) -> ReturnPoint {
    let mut best = None;
    for (path, route) in graph
        .paths
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.connector && p.length > 1. && p.near(walker.position, 60.))
    {
        let mut along = 0.;
        let mut projected = (0., f32::INFINITY);
        for pair in route.points.windows(2) {
            let delta = pair[1] - pair[0];
            let length = delta.length();
            let t =
                ((walker.position - pair[0]).dot(delta) / length.powi(2).max(0.0001)).clamp(0., 1.);
            let error = walker.position.distance_squared(pair[0] + delta * t);
            if error < projected.1 {
                projected = (along + length * t, error);
            }
            along += length;
        }
        for offset in [0., 2., -2., 4., -4., 8., -8., 16., -16.] {
            let distance = (projected.0 + offset).clamp(0.1, route.length - 0.1);
            let Some((position, _)) = placement(city, graph, path, distance) else {
                continue;
            };
            if (position.z - walker.position.z).abs() > 1.5
                || blocked_by_vehicle(position, vehicles)
                || props.blocked(position + Vec3::Z * 0.8)
            {
                continue;
            }
            let error = position.distance_squared(walker.position);
            if best.as_ref().is_none_or(|(_, score)| error < *score) {
                best = Some((
                    ReturnPoint {
                        position,
                        path,
                        distance,
                    },
                    error,
                ));
            }
        }
    }
    best.map_or_else(
        || {
            let position = placement(city, graph, walker.path, walker.distance)
                .map_or(walker.position, |p| p.0);
            ReturnPoint {
                position,
                path: walker.path,
                distance: walker.distance,
            }
        },
        |p| p.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hermes::Hermes;
    #[test]
    fn kiosk_queue_exists_only_near_its_authored_frontage() {
        let program = opencity::parse(concat!(
            "concrete(-1,-1,1,-1,1,1,-1,1);",
            "site_model_01=building(0.2,\"study_01\",0,0,0.12,0,0.12,0.12,0,0.12);",
            "elevation(site_model_01,0.02);"
        ))
        .unwrap();
        let city = City::from_program(&program, None).unwrap();
        let graph = Arc::new(WalkingPaths::new(&city));
        let mut crowd = Crowd::new(&graph);
        let props = Props::new(Vec::new());
        crowd.populate(&city, Vec3::X * 200., &[], &props);
        assert!(crowd.kiosk_actors.is_empty());
        crowd.step(&city, Vec3::X * 6., false, &[], &props, 0.1);
        assert_eq!(crowd.kiosk_actors.len(), KIOSK_CAPACITY);
        assert_eq!(crowd.instances(Vec3::X * 6.).count(), KIOSK_CAPACITY);
        assert!(crowd.kiosk_actors[0].position.x > 6.);
        assert!(crowd.kiosk_actors.iter().all(|actor| actor.position.y < 0.));
        assert_eq!(
            crowd
                .kiosk_actors
                .iter()
                .map(|actor| actor.model)
                .collect::<Vec<_>>(),
            vec![1, 3, 5]
        );
        crowd.step(&city, Vec3::X * 200., false, &[], &props, 0.1);
        assert!(crowd.kiosk_actors.is_empty());
        assert_eq!(crowd.kiosk_actors.capacity(), 0);
    }
    #[test]
    fn kiosk_queue_contacts_become_pedestrians_returning_to_sidewalks() {
        let program = opencity::parse(concat!(
            "concrete(-1,-1,1,-1,1,1,-1,1);",
            "road street: sidewalk carlane carlane sidewalk; end street(-1,-0.1,1,-0.1);",
            "site_model_01=building(0.2,\"study_01\",0,0,0.12,0,0.12,0.12,0,0.12);",
            "elevation(site_model_01,0.02);"
        ))
        .unwrap();
        let city = City::from_program(&program, None).unwrap();
        let graph = Arc::new(WalkingPaths::new(&city));
        assert!(!graph.paths.is_empty());
        let props = Props::new(Vec::new());
        let mut crowd = Crowd::new(&graph);
        crowd.populate(&city, Vec3::X * 200., &[], &props);
        crowd.refill = 1000.;
        crowd.step(&city, Vec3::X * 6., false, &[], &props, 0.1);
        let first = crowd.kiosk_actors[0].position;
        crowd.player_move(
            &city,
            first - Vec3::X * 2. + Vec3::Z * 0.9,
            first + Vec3::X * 2. + Vec3::Z * 0.9,
            0.1,
            &[],
        );
        assert_eq!(crowd.kiosk_actors.len(), 2);
        assert!(crowd.walkers[0].spawned_at_kiosk);
        assert!(matches!(
            crowd.walkers[0].reaction.as_deref(),
            Some(Reaction::Falling(_))
        ));

        let second = crowd.kiosk_actors[0].position;
        let mut car = Vehicle::spawn(&city);
        car.position = second + Vec3::Z * car.ride_height();
        car.velocity = Vec3::X * 8.;
        car.yaw = 0.;
        crowd.previous_vehicles = vec![car.position - Vec3::X * 5.];
        crowd.step(&city, Vec3::X * 6., false, &[car], &props, 0.1);
        assert_eq!(crowd.kiosk_actors.len(), 1);
        assert_eq!(
            crowd.walkers.iter().filter(|w| w.spawned_at_kiosk).count(),
            2
        );
        let mut returning = false;
        for _ in 0..900 {
            crowd.step(&city, Vec3::X * 6., false, &[], &props, 1. / 60.);
            returning |= crowd.walkers[0].returning.is_some();
            if returning && support(&city, crowd.walkers[0].position).is_some() {
                break;
            }
        }
        assert!(
            returning,
            "hit guest never got up and headed for a sidewalk"
        );
        assert!(support(&city, crowd.walkers[0].position).is_some());
        crowd.step(&city, Vec3::X * 200., false, &[], &props, 0.1);
        assert!(crowd.kiosk_actors.is_empty());
        assert!(!crowd.walkers.iter().any(|w| w.spawned_at_kiosk));
    }
    #[test]
    fn walking_and_sprinting_bumps_use_ragdolls_then_get_up_and_walk_again() {
        let hermes = Hermes::test_map();
        let city = hermes.city();
        let props = Props::new(Vec::new());
        for speed in [1., 5.2, 15.6] {
            let mut crowd = Crowd::new(hermes.walking_paths());
            crowd.populate(city, Vec3::ZERO, &[], &props);
            let (target, _) = crowd.prepare_collision();
            let mut player = target - Vec3::X * 1.5 + Vec3::Z * super::super::player::HALF_HEIGHT;
            let mut falling = false;
            let mut rising = false;
            let mut recovered = None;
            for frame in 0..720 {
                let before = player;
                if (player.x - target.x) < 2. {
                    player.x += speed / 60.;
                }
                crowd.player_move(city, before, player, 1. / 60., &[]);
                crowd.step(city, player, true, &[], &props, 1. / 60.);
                let counts = crowd.reaction_counts();
                falling |= counts[1] > 0;
                rising |= counts[2] > 0;
                if rising && counts[0] == 1 && recovered.is_none() {
                    recovered = Some(crowd.walkers[0].position);
                }
                assert!(
                    crowd.walkers[0]
                        .reaction
                        .as_ref()
                        .is_none_or(|r| r.palette().iter().all(|m| m.is_finite())),
                    "invalid player-bump ragdoll at {frame}"
                );
            }
            assert!(
                falling && rising,
                "speed {speed} did not trigger full ragdoll recovery"
            );
            assert_eq!(crowd.reaction_counts(), [1, 0, 0]);
            assert!(crowd.walkers[0].position.distance(recovered.unwrap()) > 0.3);
            // Recovery immunity prevents immediately knocking down the same walker again.
            crowd.walkers[0].immunity = 3.;
            let target = crowd.walkers[0].position;
            crowd.player_move(
                city,
                target + Vec3::new(-0.8, 0., 0.9),
                target + Vec3::new(0.8, 0., 0.9),
                0.1,
                &[],
            );
            assert_eq!(crowd.reaction_counts(), [1, 0, 0]);
        }
    }
    #[test]
    fn get_up_stays_at_impact_site_then_ik_steps_walk_back_from_road_to_sidewalk() {
        let hermes = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane sidewalk; end street(-1,0,1,0);",
        );
        let city = hermes.city();
        let props = Props::new(Vec::new());
        let mut crowd = Crowd::new(hermes.walking_paths());
        crowd.populate(city, Vec3::ZERO, &[], &props);
        crowd.walkers.truncate(1);
        crowd.refill = 100.;
        let walker = &mut crowd.walkers[0];
        let sidewalk = walker.position;
        let displaced = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y]
            .into_iter()
            .find_map(|direction| {
                (2..=5).find_map(|distance| {
                    footing(city, sidewalk + direction * distance as f32, false)
                        .filter(|g| support(city, g.point).is_none())
                        .map(|g| g.point)
                })
            })
            .expect("adjacent supported roadway");
        walker.position = displaced;
        let destination = return_point(city, &crowd.graph, walker, &[], &props);
        let definition = &pedestrian_catalog::definitions()[walker.model];
        walker.reaction = Some(Box::new(Reaction::Rising {
            source: Box::new(definition.pose(0.3, 0., 0.)),
            palette: Box::new(definition.pose(0.3, 0., 0.)),
            elapsed: 0.,
        }));
        walker.path = destination.path;
        walker.distance = destination.distance;
        walker.returning = Some(destination);
        let mut return_steps = 0;
        for frame in 0..900 {
            let before = crowd.walkers[0].position;
            let rising = crowd.walkers[0].reaction.is_some();
            crowd.step(city, Vec3::ZERO, false, &[], &props, 1. / 60.);
            let w = &crowd.walkers[0];
            if rising {
                assert_eq!(w.position, before, "get-up translated toward sidewalk");
            } else {
                assert!(
                    w.position.distance(before) <= w.preferred_speed / 60. + 0.002,
                    "return teleported"
                );
                let delta = w.position - before;
                assert!(
                    delta.dot(Vec3::new(w.yaw.cos(), w.yaw.sin(), 0.)) >= -0.0001,
                    "return moved sideways/backward"
                );
                return_steps += usize::from(delta.length() > 0.001);
                assert!(w.gait.palette.iter().all(|p| p.is_finite()));
            }
            if frame == 899 {
                assert!(
                    w.returning.is_none(),
                    "never reached sidewalk: {:?} -> {:?}",
                    w.position,
                    destination.position
                );
                assert!(support(city, w.position).is_some());
            }
        }
        assert!(return_steps > 90, "return did not use walking steps");
    }
    #[test]
    fn vehicle_bumps_knock_down_walkers_and_zombies_then_they_recover() {
        let hermes = Hermes::test_map();
        let props = Props::new(Vec::new());
        let mut crowd = Crowd::new(hermes.walking_paths());
        crowd.populate(hermes.city(), Vec3::ZERO, &[], &props);
        crowd.prepare_collision();
        let mut car = Vehicle::spawn(hermes.city());
        car.yaw = 0.;
        for zombie_mode in [false, true] {
            if zombie_mode {
                crowd.toggle_zombies(hermes.city(), &[], &props);
            }
            let target = crowd.walkers[0].position;
            car.position = target - Vec3::X * 5. + Vec3::Z * car.ride_height();
            car.velocity = Vec3::X * 8.;
            crowd.previous_vehicles = vec![car.position];
            let mut falling = false;
            let mut rising = false;
            let mut recovered = None;
            for frame in 0..600 {
                car.position.x += 8. / 60.;
                crowd.step(
                    hermes.city(),
                    car.position,
                    false,
                    std::slice::from_ref(&car),
                    &props,
                    1. / 60.,
                );
                let counts = crowd.reaction_counts();
                falling |= counts[1] > 0;
                rising |= counts[2] > 0;
                if rising && counts[0] == 1 && recovered.is_none() {
                    recovered = Some(crowd.walkers[0].position);
                }
                assert!(crowd
                    .instances(Vec3::ZERO)
                    .all(|i| i.pose.is_none_or(|p| p.iter().all(|m| m.is_finite()))));
                if frame == 599 {
                    assert_eq!(
                        counts,
                        [1, 0, 0],
                        "pedestrian did not recover after impact (zombie mode {zombie_mode})"
                    );
                }
            }
            assert!(falling && rising);
            assert!(
                crowd.walkers[0].position.distance(recovered.unwrap()) > 0.5,
                "pedestrian never resumed walking"
            );
            assert!(footing(hermes.city(), crowd.walkers[0].position, !zombie_mode).is_some());
        }
    }
    #[test]
    fn distant_walkers_share_ticks_and_resume_full_rate_near_player() {
        let hermes = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane sidewalk; end street(-5,0,5,0);",
        );
        let props = Props::new(Vec::new());
        let mut crowd = Crowd::new(hermes.walking_paths());
        crowd.populate(hermes.city(), Vec3::ZERO, &[], &props);
        crowd.refill = 10000.;
        assert!(!crowd.walkers.is_empty());
        let far = Vec3::new(10000., 0., 0.);
        crowd.step(hermes.city(), far, false, &[], &props, 1. / 120.);
        let skipped: Vec<_> = crowd
            .walkers
            .iter()
            .enumerate()
            .filter_map(|(index, w)| (w.deferred_dt > 0.).then_some(index))
            .collect();
        assert!(!skipped.is_empty());
        crowd.step(hermes.city(), far, false, &[], &props, 1. / 120.);
        assert!(skipped
            .iter()
            .all(|&index| crowd.walkers[index].deferred_dt == 0.));
        let near = crowd.walkers[0].position;
        crowd.step(hermes.city(), near, false, &[], &props, 1. / 120.);
        assert_eq!(crowd.walkers[0].deferred_dt, 0.);
        assert!(crowd
            .walkers
            .iter()
            .all(|w| support(hermes.city(), w.position).is_some()));
    }
    #[test]
    fn distant_walker_still_reacts_to_vehicle_on_deferred_tick() {
        let hermes = Hermes::test_map();
        let props = Props::new(Vec::new());
        let mut crowd = Crowd::new(hermes.walking_paths());
        crowd.populate(hermes.city(), Vec3::ZERO, &[], &props);
        let (target, _) = crowd.prepare_collision();
        let mut car = Vehicle::spawn(hermes.city());
        car.yaw = 0.;
        car.velocity = Vec3::X * 8.;
        car.position = target - Vec3::X * 5. + Vec3::Z * car.ride_height();
        crowd.previous_vehicles = vec![car.position];
        car.position = target + Vec3::X * 5. + Vec3::Z * car.ride_height();
        let far = target + Vec3::X * 10000.;
        crowd.step_number = 1 - (crowd.walkers[0].serial as u64 & 1);
        crowd.step(
            hermes.city(),
            far,
            false,
            std::slice::from_ref(&car),
            &props,
            1. / 120.,
        );
        assert!(crowd.walkers[0].reaction.is_some());
    }
    #[test]
    fn pedestrian_corner_and_reversal_steps_are_continuous_and_remain_on_sidewalks() {
        let hermes = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane sidewalk; end
            street(-0.5,0,0,0); street(0,0,0,0.5);",
        );
        let props = Props::new(Vec::new());
        let mut crowd = Crowd::new(hermes.walking_paths());
        crowd.populate(hermes.city(), Vec3::ZERO, &[], &props);
        crowd.refill = 10000.;
        let mut transitions = 0;
        let started = std::time::Instant::now();
        for _ in 0..3600 {
            let previous: Vec<_> = crowd.walkers.iter().map(|w| (w.position, w.path)).collect();
            crowd.step(hermes.city(), Vec3::ZERO, false, &[], &props, 1. / 60.);
            for (w, (before, path)) in crowd.walkers.iter().zip(previous) {
                let delta = w.position - before;
                assert!(
                    delta.length() <= w.preferred_speed / 60. + 0.001,
                    "walker jumped at route transition: {delta:?}"
                );
                assert!(
                    delta.dot(Vec3::new(w.yaw.cos(), w.yaw.sin(), 0.)) >= -0.001,
                    "walker moved backwards at corner"
                );
                assert!(support(hermes.city(), w.position).is_some());
                transitions += usize::from(path != w.path);
            }
        }
        assert!(transitions > 10, "test did not exercise route ends");
        eprintln!("Pedestrian corner QA: {} walkers, {} route transitions, {:.3} ms/step including assertions", crowd.walkers.len(), transitions,
            started.elapsed().as_secs_f64() * 1000. / 3600.);
    }
    #[test]
    fn pedestrian_population_is_bounded_diverse_and_recycled_near_the_player() {
        let hermes = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane sidewalk; end street(-5,0,5,0);",
        );
        let props = Props::new(Vec::new());
        let mut crowd = Crowd::new(hermes.walking_paths());
        let center = Vec3::ZERO;
        crowd.populate(hermes.city(), center, &[], &props);
        assert_eq!(crowd.walkers.len(), CAPACITY);
        let mut models = crowd.walkers.iter().map(|w| w.model).collect::<Vec<_>>();
        models.sort();
        models.dedup();
        assert_eq!(models.len(), 6);
        assert!(crowd
            .walkers
            .windows(2)
            .any(|w| (w[0].walk_time - w[1].walk_time).abs() > 0.1));
        let original = crowd.walkers[0].position;
        for _ in 0..1200 {
            crowd.step(hermes.city(), center, false, &[], &props, 1. / 60.);
        }
        assert!(crowd.walkers[0].position.distance(original) > 3.);
        for w in &crowd.walkers {
            assert!(support(hermes.city(), w.position).is_some());
        }
        let destination = Vec3::new(350., 0., 0.);
        crowd.populate(hermes.city(), destination, &[], &props);
        assert_eq!(crowd.walkers.len(), CAPACITY);
        assert!(crowd
            .walkers
            .iter()
            .all(|w| w.position.distance(destination) < RETAIN_RADIUS));
    }
    #[test]
    fn pedestrian_stops_for_player_and_turns_without_walking_backwards() {
        let hermes = Hermes::test_map();
        let props = Props::new(Vec::new());
        let mut crowd = Crowd::new(hermes.walking_paths());
        crowd.populate(hermes.city(), Vec3::ZERO, &[], &props);
        crowd.walkers.truncate(1);
        let walker = &mut crowd.walkers[0];
        walker.until_pause = 100.;
        walker.pause = 0.;
        let ahead = placement(
            hermes.city(),
            &crowd.graph,
            walker.path,
            walker.distance + 0.8,
        )
        .unwrap()
        .0;
        let before = walker.position;
        crowd.refill = 100.;
        for _ in 0..60 {
            crowd.step(hermes.city(), ahead, true, &[], &props, 1. / 60.);
        }
        assert!(crowd.walkers[0].position.distance(before) < 0.6);
        assert!(crowd.walkers[0].movement < 0.05);
        assert!(crowd.walkers[0].speed < 0.01);
        // Exercise the real end-of-path reversal over several seconds.
        let w = &mut crowd.walkers[0];
        w.distance = crowd.graph.paths[w.path].length;
        w.position = placement_offset(hermes.city(), &crowd.graph, w.path, w.distance, w.lane)
            .unwrap()
            .0;
        w.gait = Gait::new(
            &pedestrian_catalog::definitions()[w.model],
            w.position,
            w.yaw,
            w.serial,
            |p| footing(hermes.city(), p, true),
        );
        for _ in 0..180 {
            let before = crowd.walkers[0].position;
            crowd.step(hermes.city(), Vec3::ZERO, false, &[], &props, 1. / 60.);
            let w = &crowd.walkers[0];
            let delta = w.position - before;
            if delta.length() > 0.01 {
                assert!(delta.dot(Vec3::new(w.yaw.cos(), w.yaw.sin(), 0.)) >= -0.001);
            }
        }
    }
}
