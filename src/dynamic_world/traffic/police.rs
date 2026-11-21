//! Responding patrols share the last sighting and a lane graph, and drive and dismount independently.
mod intercept;
use super::{Driver, SpeedClass, TrafficPaths, CRUISER};
use crate::dynamic_world::{catalog, DynamicWorld, SpawnPoint, Vehicle, METERS_PER_UNIT};
use crate::hermes::Hermes;
use glam::{Mat4, Vec3};
use opencity::runtime::City;
use std::{cmp::Reverse, collections::BinaryHeap};

use crate::dynamic_world::{
    crowd::Instance, pedestrian_catalog, player, ragdoll, Pedestrian, WalkControls,
};

pub(super) fn pursuit_speed(class: SpeedClass) -> f32 {
    let kmh = match class {
        SpeedClass::Local => 62.,
        SpeedClass::Arterial => 85.,
        SpeedClass::Highway => 125.,
        SpeedClass::Ramp => 72.,
        SpeedClass::Merge => 92.,
        SpeedClass::Junction => 43.,
    };
    kmh / 3.6
}

pub(super) struct Officer {
    actor: Pedestrian,
    reaction: Option<Box<ragdoll::Reaction>>,
    impact_immunity: f32,
    returning: bool,
    cooldown: f32,
    shot_age: f32,
    aim: Vec3,
    exit_target: Vec3,
}

impl Officer {
    fn instance(&self, season: crate::season::Season) -> Instance<'_> {
        let mut instance = self.actor.instance(season);
        if let Some(reaction) = &self.reaction {
            instance.pose = Some(reaction.palette());
        }
        instance
    }
}

const SEARCH_SECONDS: f32 = 8.;
const ARREST_SHOT_SECONDS: f32 = 4.5;
const SURRENDER_SECONDS: f32 = 3.;
const SIGHT_RADIUS: f32 = 75.;
const ARREST_REACH: f32 = 1.8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crime {
    VehicleTheft,
    CruiserHit,
    Gunshot,
    PedestrianHit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ChaseStatus {
    #[default]
    Clear,
    Reported(Crime),
    Pursued,
    Searching,
    Surrendering,
    Arrested,
}

#[derive(Clone, Copy, Default)]
pub(super) enum LaneOrder {
    #[default]
    Unreachable,
    Turn(usize),
    Stop(f32),
}
impl LaneOrder {
    pub(super) fn next(self) -> Option<usize> {
        match self {
            Self::Turn(id) => Some(id),
            _ => None,
        }
    }
    pub(super) fn clearance(self, distance: f32) -> f32 {
        match self {
            Self::Stop(at) => (at - distance).max(0.),
            Self::Unreachable => 0.,
            Self::Turn(_) => f32::INFINITY,
        }
    }
}

#[derive(Default)]
pub(super) struct Patrol {
    pub(super) vehicle: usize,
    without_car: bool,
    seen: bool,
    stopped_seconds: f32,
    pub(super) officer: Option<Box<Officer>>,
    pub(super) pull_over: bool,
    pub(super) road_blocked: bool,
    intercept: intercept::Intercept,
}
impl Patrol {
    pub(super) fn vehicle_controls(
        &mut self,
        city: &City,
        cars: &[Vehicle],
        dt: f32,
    ) -> Option<crate::dynamic_world::Controls> {
        self.intercept.controls(city, cars, self.vehicle, dt)
    }
}

#[derive(Default)]
pub(super) struct Chase {
    pub(super) patrols: Vec<Patrol>,
    reinforce_in: f32,
    pub(super) stars: u8,
    status: ChaseStatus,
    pub(super) last_known: Vec3,
    planned: Option<Vec3>,
    // Cost from each lane entrance, in centimetres. Used for dispatch and rerouting.
    routes: Vec<(u32, LaneOrder)>,
    incoming: Vec<Vec<usize>>,
    next_update: f32,
    unseen: f32,
    stopped: f32,
    arrest_elapsed: f32,
}

impl Chase {
    pub(super) fn patrol(&self, id: usize) -> Option<usize> {
        self.patrols
            .iter()
            .position(|p| p.vehicle == id && !p.without_car)
    }

    pub(super) fn release(&mut self, id: usize) {
        if let Some(i) = self.patrol(id) {
            if let Some(officer) = &mut self.patrols[i].officer {
                officer.returning = false;
                self.patrols[i].without_car = true;
            } else {
                self.patrols.swap_remove(i);
            }
        }
    }

    fn enlist(&mut self, id: usize) {
        self.patrols.push(Patrol {
            vehicle: id,
            ..Default::default()
        });
    }

    #[cfg(test)]
    fn first_unit(&self) -> Option<usize> {
        self.patrols.first().map(|p| p.vehicle)
    }

    fn plan(&mut self, graph: &TrafficPaths) {
        let goal = self.last_known;
        self.planned = Some(goal);
        if self.incoming.is_empty() {
            self.incoming.resize_with(graph.paths.len(), Vec::new);
            for (id, path) in graph.paths.iter().enumerate() {
                for &next in &path.next {
                    self.incoming[next].push(id);
                }
            }
        }
        let mut nearest = f32::INFINITY;
        let mut ends = Vec::new();
        for (id, path) in graph.paths.iter().enumerate() {
            if path.speed == SpeedClass::Junction || !path.near(goal, 180.) {
                continue;
            }
            let (along, error) = path.project(goal);
            nearest = nearest.min(error);
            ends.push((id, along, error));
        }
        self.routes.clear();
        self.routes
            .resize(graph.paths.len(), (u32::MAX, LaneOrder::Unreachable));
        let mut pending = BinaryHeap::new();
        for (id, along, error) in ends {
            // Both sides of a street can reach a pedestrian standing on its sidewalk.
            if error <= nearest + 4. {
                let cost = (along * 100.) as u32;
                self.routes[id] = (cost, LaneOrder::Stop(along));
                pending.push(Reverse((cost, id)));
            }
        }
        while let Some(Reverse((cost, id))) = pending.pop() {
            if cost != self.routes[id].0 {
                continue;
            }
            for &previous in &self.incoming[id] {
                let total = cost.saturating_add((graph.paths[previous].length * 100.) as u32);
                if total < self.routes[previous].0 {
                    self.routes[previous] = (total, LaneOrder::Turn(id));
                    pending.push(Reverse((total, previous)));
                }
            }
        }
    }

    pub(super) fn order(&self, path: usize, along: f32, graph: &TrafficPaths) -> LaneOrder {
        let order = self.routes[path].1;
        if matches!(order, LaneOrder::Stop(at) if along > at + 6.) {
            // Already past the destination on a one-way lane. Find a legal way back.
            return graph.paths[path]
                .next
                .iter()
                .copied()
                .filter(|&id| self.routes[id].0 < u32::MAX)
                .min_by_key(|&id| self.routes[id].0)
                .map_or(LaneOrder::Unreachable, LaneOrder::Turn);
        }
        order
    }
}

impl DynamicWorld {
    pub fn chase_status(&self) -> ChaseStatus {
        self.traffic.chase.status
    }

    pub(in crate::dynamic_world) fn patrol_contact(
        &mut self,
        contacts: &[crate::dynamic_world::contacts::VehicleContact],
    ) {
        if self.is_driving()
            && self.wanted_stars() == 0
            && contacts
                .iter()
                .filter_map(|contact| contact.struck_by(self.player_vehicle.0))
                .any(|id| {
                    self.vehicles[id]
                        .definition()
                        .is_some_and(|d| d.config.research_id == CRUISER)
                })
        {
            self.report_crime(Crime::CruiserHit);
        }
        let chase = &mut self.traffic.chase;
        for patrol in &mut chase.patrols {
            if let Some(b) = patrol.intercept.suspect {
                let a = patrol.vehicle;
                if contacts
                    .iter()
                    .any(|contact| contact.vehicles == (a, b) || contact.vehicles == (b, a))
                {
                    patrol.intercept.contact();
                }
            }
        }
    }

    pub fn wanted_stars(&self) -> u8 {
        self.traffic.chase.stars
    }

    pub(in crate::dynamic_world) fn officer_out_of(&self, id: usize) -> bool {
        self.traffic
            .chase
            .patrols
            .iter()
            .any(|p| p.vehicle == id && !p.without_car && p.officer.is_some())
    }

    pub(in crate::dynamic_world) fn officer_instances(&self) -> impl Iterator<Item = Instance<'_>> {
        self.traffic
            .chase
            .patrols
            .iter()
            .filter_map(|p| Some(p.officer.as_ref()?.instance(self.season)))
    }

    pub fn officer_handguns(&self) -> impl Iterator<Item = (Vec3, Vec3, f32)> + '_ {
        self.traffic.chase.patrols.iter().filter_map(|p| {
            let officer = p.officer.as_ref()?;
            if officer.returning || officer.reaction.is_some() {
                return None;
            }
            Some((
                Self::right_hand(officer.actor.instance(self.season))?,
                officer.aim,
                officer.shot_age + self.traffic.chase.arrest_elapsed,
            ))
        })
    }

    pub fn arrest_progress(&self) -> Option<f32> {
        let chase = &self.traffic.chase;
        (chase.status == ChaseStatus::Arrested)
            .then_some(chase.arrest_elapsed / ARREST_SHOT_SECONDS)
    }

    /// Bias the final framing toward the closest responding officer or cruiser.
    pub fn arrest_focus(&self) -> Vec3 {
        let player = self.player_position();
        let nearest = self
            .traffic
            .chase
            .patrols
            .iter()
            .map(|p| {
                p.officer
                    .as_ref()
                    .map_or(self.vehicles[p.vehicle].position, |o| o.actor.position)
            })
            .min_by(|a, b| {
                a.distance_squared(player)
                    .total_cmp(&b.distance_squared(player))
            });
        player
            + Vec3::Z * 0.5
            + nearest.map_or(Vec3::ZERO, |p| ((p - player) * 0.35).clamp_length_max(2.5))
    }

    fn begin_arrest(&mut self) {
        self.traffic.chase.status = ChaseStatus::Arrested;
        self.traffic.chase.arrest_elapsed = 0.;
    }

    pub fn nearby_police(&self) -> impl Iterator<Item = (Vec3, bool)> + '_ {
        let center = self.player_position();
        let officers = self
            .traffic
            .chase
            .patrols
            .iter()
            .filter_map(|p| Some((p.officer.as_ref()?.actor.position, true)));
        let cruisers = self.traffic.driven_slots().filter_map(|(id, _)| {
            let car = &self.vehicles[id];
            car.definition()
                .is_some_and(|d| d.config.research_id == CRUISER)
                .then_some((car.position, false))
        });
        officers.chain(cruisers).filter(move |(p, _)| {
            p.truncate().distance_squared(center.truncate()) <= 200_f32.powi(2)
        })
    }

    pub fn set_dispatch_view(&mut self, view: Mat4) {
        self.dispatch_view = Some(view);
    }

    pub fn report_crime(&mut self, crime: Crime) {
        if self.zombie_mode() || self.chase_status() == ChaseStatus::Arrested {
            return;
        }
        let position = self.player_position();
        let chase = &mut self.traffic.chase;
        let severity = match crime {
            Crime::VehicleTheft | Crime::CruiserHit => 1,
            Crime::PedestrianHit => 2,
            Crime::Gunshot => 3,
        };
        chase.stars = (chase.stars + 1).max(severity).min(5);
        chase.last_known = position;
        chase.unseen = 0.;
        chase.stopped = 0.;
        if chase.patrols.is_empty() {
            if !matches!(chase.status, ChaseStatus::Reported(_)) {
                chase.next_update = 0.7;
            }
            chase.status = ChaseStatus::Reported(crime);
        }
    }

    pub(in crate::dynamic_world) fn clear_chase(&mut self) {
        for driver in &mut self.traffic.drivers {
            if self.traffic.chase.patrol(driver.vehicle).is_none() {
                continue;
            }
            driver.next = None;
            driver.stopped = false;
            driver.stranded = 0.;
        }
        self.traffic.chase = Chase::default();
    }

    fn outside_view(&self, point: Vec3) -> bool {
        if let Some(view) = self.dispatch_view {
            let clip = view * (point / METERS_PER_UNIT).extend(1.);
            return clip.w <= 0. || clip.x.abs() > 1.15 * clip.w || clip.y.abs() > 1.15 * clip.w;
        }
        let yaw = self.player_yaw();
        (point - self.player_position()).dot(Vec3::new(yaw.cos(), yaw.sin(), 0.)) < 0.
    }

    fn dispatch_patrol(&mut self, city: &City) -> Option<usize> {
        let goal = self.traffic.chase.last_known;
        let graph = self.traffic.graph.clone();
        let model = catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == CRUISER)?;
        enum Site {
            Driver(usize),
            Lane { path: usize, along: f32 },
        }
        let mut sites = Vec::new();
        // Prefer a nearby compact already on its way if it can beat a fresh dispatch.
        for driver in &self.traffic.drivers {
            let car = &self.vehicles[driver.vehicle];
            let (cost, order) = self.traffic.chase.routes[driver.path];
            if self.traffic.chase.patrol(driver.vehicle).is_none()
                && !driver.stopped
                && cost < u32::MAX
                && !matches!(order, LaneOrder::Stop(at) if at <= driver.distance)
                && !self.traffic.interacted.contains_key(&driver.vehicle)
                && (35. ..115.).contains(&car.position.distance(goal))
                && self.outside_view(car.position)
                && car.definition().is_some_and(|m| {
                    matches!(m.config.research_id.as_str(), "vehicle-v2-09" | CRUISER)
                })
            {
                sites.push((
                    cost as f32 / 100. - driver.distance,
                    Site::Driver(driver.vehicle),
                ));
            }
        }
        for (id, path) in graph.paths.iter().enumerate() {
            let (cost, order) = self.traffic.chase.routes[id];
            if cost == u32::MAX || path.speed == SpeedClass::Junction || !path.near(goal, 160.) {
                continue;
            }
            let mut along = 5.;
            while along < path.length - 4. {
                let (point, _) = path.sample(along);
                let distance = point.distance(goal);
                if (35. ..160.).contains(&distance)
                    && self.outside_view(point)
                    && !matches!(order, LaneOrder::Stop(at) if along >= at)
                {
                    sites.push((cost as f32 / 100. - along, Site::Lane { path: id, along }));
                }
                along += 10.;
            }
        }
        sites.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (distance, site) in sites {
            if distance > 240. {
                break;
            }
            let (path, along) = match site {
                Site::Driver(id) => {
                    self.vehicles[id].set_catalog_model(Some(model));
                    return Some(id);
                }
                Site::Lane { path, along } => (path, along),
            };
            let (point, heading) = graph.paths[path].sample(along);
            let spawn = SpawnPoint {
                point: (point.truncate() / METERS_PER_UNIT).to_array(),
                heading: Some(heading.y.atan2(heading.x)),
            };
            let Ok(mut car) = Vehicle::spawn_at(city, spawn) else {
                continue;
            };
            car.set_catalog_model(Some(model));
            if (car.position.z - car.ride_height() - point.z).abs() > 0.5
                || !self.clear_vehicle_position(city, &car, None)
                || self.crowd.overlaps_vehicle(&car)
            {
                continue;
            }
            // Reuse an offscreen ambient slot at the population cap. Never recycle
            // the player's car or a vehicle they have physically interacted with.
            let recycle = self.traffic.inactive.pop().or_else(|| {
                if self.traffic.drivers.len() < crate::performance::npc_budget(super::TARGET) {
                    return None;
                }
                let driver = self.traffic.drivers.iter().position(|d| {
                    d.vehicle != self.player_vehicle.0
                        && self.traffic.chase.patrol(d.vehicle).is_none()
                        && !self.traffic.interacted.contains_key(&d.vehicle)
                        && self.outside_view(self.vehicles[d.vehicle].position)
                })?;
                Some(self.traffic.drivers.swap_remove(driver).vehicle)
            });
            if recycle.is_none()
                && self.traffic.drivers.len() >= crate::performance::npc_budget(super::TARGET)
            {
                return None;
            }
            let id = if let Some(id) = recycle {
                self.vehicles[id] = car;
                self.vehicle_spawns[id] = spawn;
                id
            } else {
                self.vehicles.push(car);
                self.vehicle_spawns.push(spawn);
                self.vehicles.len() - 1
            };
            self.traffic
                .drivers
                .push(Driver::new(id, path, along, self.traffic.time));
            return Some(id);
        }
        None
    }

    pub(in crate::dynamic_world) fn step_officer_impacts(
        &mut self,
        city: &City,
        walking: Option<(Vec3, Vec3)>,
        dt: f32,
    ) -> bool {
        let mut hit_by_player = false;
        let driving = self.is_driving();
        for officer in self
            .traffic
            .chase
            .patrols
            .iter_mut()
            .filter_map(|p| p.officer.as_mut())
        {
            officer.impact_immunity = (officer.impact_immunity - dt).max(0.);
            if officer.impact_immunity > 0. {
                continue;
            }
            let actor = &mut officer.actor;
            let mut feet = actor.position - Vec3::Z * player::HALF_HEIGHT;
            let instance = actor.instance(self.season);
            let model = instance.model;
            let definition = &pedestrian_catalog::definitions()[model];
            if let Some(reaction) = &mut officer.reaction {
                let step = reaction.step(
                    definition,
                    city,
                    &self.vehicles,
                    dt.min(0.1),
                    &mut feet,
                    true,
                );
                actor.position = feet + Vec3::Z * player::HALF_HEIGHT;
                if step == ragdoll::ReactionStep::Recovered {
                    *actor = Pedestrian::with_model(city, actor.position, actor.yaw, model);
                    officer.reaction = None;
                    officer.impact_immunity = ragdoll::RECOVERY_IMMUNITY;
                }
                continue;
            }
            let velocity = if let Some((id, velocity)) =
                ragdoll::impact(feet, &self.crowd.previous_vehicles, &self.vehicles, dt)
            {
                hit_by_player |= driving && id == self.player_vehicle.0;
                velocity
            } else if let Some(velocity) =
                walking.and_then(|(before, after)| ragdoll::walking_impact(feet, before, after, dt))
            {
                velocity
            } else {
                continue;
            };
            officer.reaction = Some(Box::new(ragdoll::Reaction::Falling(ragdoll::Ragdoll::new(
                definition,
                *instance.pose.unwrap(),
                feet,
                actor.yaw,
                velocity,
                city,
                &self.vehicles,
            ))));
            officer.shot_age = f32::INFINITY;
        }
        hit_by_player
    }

    fn patrol_sees(&self, city: &City, patrol: &Patrol, player: Vec3) -> bool {
        if patrol
            .officer
            .as_ref()
            .is_some_and(|o| o.reaction.is_some())
        {
            return false;
        }
        let car = &self.vehicles[patrol.vehicle];
        let eye = patrol.officer.as_ref().map_or_else(
            || car.position + Vec3::Z * (1.2 - car.ride_height()),
            |o| o.actor.position + Vec3::Z * 0.55,
        );
        let delta = player + Vec3::Z * 0.35 - eye;
        let range = delta.length();
        if range > SIGHT_RADIUS || self.metro.aboard {
            return false;
        }
        range < 0.01
            || self
                .buildings
                .ray_face(city, eye, delta / range, range)
                .is_none()
    }

    fn officer_within_reach(&self, officer: Vec3) -> bool {
        if self.is_driving() {
            self.player().entry_distance(officer) <= ARREST_REACH
        } else {
            self.player_position().distance_squared(officer) <= ARREST_REACH.powi(2)
        }
    }

    fn step_officer(&mut self, city: &City, index: usize, dt: f32) {
        let patrol = &mut self.traffic.chase.patrols[index];
        if patrol
            .officer
            .as_ref()
            .is_some_and(|o| o.reaction.is_some())
        {
            return;
        }
        let id = patrol.vehicle;
        let seen = patrol.seen;
        let without_car = patrol.without_car;
        let target = self.traffic.chase.last_known;
        let car = &self.vehicles[id];
        let at = car.position;
        let blocked = |p| {
            self.props.blocked(p)
                || self
                    .vehicles
                    .iter()
                    .any(|v| player::blocked_by_vehicle(p, v))
        };
        let mut officer = match patrol.officer.take() {
            Some(o) => o,
            None => {
                let exit_range = if patrol.road_blocked {
                    SIGHT_RADIUS
                } else if patrol.stopped_seconds > 2.5 {
                    45.
                } else {
                    23.
                };
                if (!patrol.pull_over && !patrol.road_blocked)
                    || at.distance(target) > exit_range
                    || car.velocity.length() > 1.5
                {
                    return;
                }
                let Some(actor) = Pedestrian::beside_with_model(
                    city,
                    car,
                    pedestrian_catalog::POLICE_OFFICER_MODEL,
                    blocked,
                )
                .filter(|p| !p.swimming) else {
                    return;
                };
                if let Some(door) = car.nearest_door(actor.position) {
                    self.vehicles[id].doors.use_door(door);
                }
                patrol.officer = Some(Box::new(Officer {
                    actor,
                    reaction: None,
                    impact_immunity: 0.,
                    returning: false,
                    cooldown: 0.8,
                    shot_age: f32::INFINITY,
                    aim: target,
                    exit_target: target,
                }));
                return;
            }
        };
        officer.returning |=
            !without_car && at.distance(target) > 40. && target.distance(officer.exit_target) > 20.;
        if officer.returning && car.entry_distance(officer.actor.position) <= 2. {
            if let Some(door) = car.nearest_door(officer.actor.position) {
                self.vehicles[id].doors.use_door(door);
            }
            return;
        }
        let goal = if officer.returning { at } else { target };
        let delta = (goal - officer.actor.position).truncate();
        let range = delta.length();
        if range > 0.1 {
            officer.actor.yaw = delta.y.atan2(delta.x);
        }
        let reached = if seen {
            self.officer_within_reach(officer.actor.position)
        } else {
            range <= ARREST_REACH
        };
        let mut direction = if !reached || officer.returning {
            delta / range.max(0.1)
        } else {
            glam::Vec2::ZERO
        };
        // Walk around the bumper when the suspect is on the other side of the cruiser.
        if !officer.returning
            && player::blocked_by_vehicle(
                officer.actor.position + (direction * 0.8).extend(0.),
                car,
            )
        {
            let forward = glam::Vec2::from_angle(car.yaw);
            direction = if (officer.actor.position - at).truncate().dot(forward) >= 0. {
                forward
            } else {
                -forward
            };
        }
        officer.actor.step(
            city,
            WalkControls {
                aim_move: Some(direction),
                run: range > 12.,
                ..Default::default()
            },
            dt,
            blocked,
        );
        let tire = if seen && self.is_driving() {
            self.player().police_tire_target(officer.actor.position)
        } else {
            None
        };
        let aim = tire.unwrap_or(target + Vec3::Z * 0.25);
        officer.aim = aim;
        officer
            .actor
            .aim_weapon(self.season, (!officer.returning).then_some(aim), false);
        officer.shot_age += dt;
        officer.cooldown -= dt;
        if seen
            && !officer.returning
            && range < 30.
            && officer.cooldown <= 0.
            && !matches!(self.chase_status(), ChaseStatus::Surrendering)
        {
            if let Some(hand) = Self::right_hand(officer.actor.instance(self.season)) {
                let fired = if tire.is_some() || (self.is_driving() && !self.player().is_bicycle())
                {
                    let target = if tire.is_some() {
                        let lateral = glam::Vec2::from_angle(officer.actor.yaw).perp().extend(0.);
                        let spread = 0.06 + 0.008 * range + 0.014 * self.player().velocity.length();
                        let mut offset = || (self.traffic.random(201) as f32 - 100.) * 0.01;
                        aim + (lateral * offset() + Vec3::Z * offset()) * spread
                    } else {
                        aim
                    };
                    self.fire_weapon_at(city, hand, target, 1);
                    true
                } else if self.clear_officer_shot(city, hand, aim) {
                    self.health = self.health.saturating_sub(12);
                    true
                } else {
                    false
                };
                if fired {
                    self.officer_gunshot(hand);
                    officer.shot_age = 0.;
                    officer.cooldown = 1.8 - 0.14 * f32::from(self.wanted_stars());
                } else {
                    officer.cooldown = 0.2;
                }
            }
        }
        self.traffic.chase.patrols[index].officer = Some(officer);
    }

    pub(in crate::dynamic_world) fn step_chase(&mut self, hermes: &Hermes, before: Vec3, dt: f32) {
        use ChaseStatus::*;
        let chase = &mut self.traffic.chase;
        match chase.status {
            Clear => return,
            Arrested => {
                chase.arrest_elapsed += dt;
                if chase.arrest_elapsed >= ARREST_SHOT_SECONDS {
                    self.reset_player(hermes);
                }
                return;
            }
            _ => {}
        }
        chase.next_update -= dt;
        let dispatch_report = matches!(chase.status, Reported(_)) && chase.next_update <= 0.;
        let player = self.player_position();
        let driving = self.is_driving();
        let slow = !driving || self.player().velocity.length() < 3.;
        let suspect = (driving
            && self
                .player()
                .definition()
                .is_some_and(|d| d.config.road_vehicle))
        .then_some(self.player_vehicle.0);
        let still = before.distance_squared(player) < (0.65 * dt).powi(2)
            && (!driving || self.player().velocity.length_squared() < 0.65_f32.powi(2));
        let mut seen = false;
        let mut close = false;
        for i in 0..self.traffic.chase.patrols.len() {
            let patrol = &self.traffic.chase.patrols[i];
            let visible = self.patrol_sees(hermes.city(), patrol, player);
            close |= visible
                && patrol.officer.as_ref().is_some_and(|officer| {
                    !officer.returning && self.officer_within_reach(officer.actor.position)
                });
            seen |= visible;
            let patrol = &mut self.traffic.chase.patrols[i];
            patrol.seen = visible;
            patrol.intercept.suspect = suspect.filter(|_| visible && patrol.officer.is_none());
            patrol.pull_over = visible && slow;
            patrol.stopped_seconds =
                if patrol.pull_over && self.vehicles[patrol.vehicle].velocity.length() < 1.5 {
                    patrol.stopped_seconds + dt
                } else {
                    0.
                };
        }
        let chase = &mut self.traffic.chase;
        if seen {
            chase.last_known = player;
            chase.unseen = 0.;
            chase.stopped = if close && still {
                chase.stopped + dt
            } else {
                0.
            };
            if chase.stopped >= SURRENDER_SECONDS {
                self.begin_arrest();
                return;
            }
            chase.status = if chase.stopped > 0. {
                Surrendering
            } else {
                Pursued
            };
        } else {
            chase.stopped = 0.;
            chase.unseen += dt;
            if chase.unseen >= SEARCH_SECONDS {
                self.clear_chase();
                return;
            }
            if !matches!(chase.status, Reported(_)) || dispatch_report {
                chase.status = Searching;
            }
        }
        for i in 0..chase.patrols.len() {
            self.step_officer(hermes.city(), i, dt);
            if self.health == 0 {
                self.begin_arrest();
                return;
            }
        }
        let chase = &mut self.traffic.chase;
        chase.reinforce_in -= dt;
        if chase.next_update <= 0. {
            chase.next_update = 0.75;
            if chase
                .planned
                .is_none_or(|p| p.distance_squared(chase.last_known) > 1.)
            {
                chase.plan(&self.traffic.graph);
            }
            // Radio every occupied cruiser, including visible cars and newly streamed traffic.
            for driver in &mut self.traffic.drivers {
                if chase.patrol(driver.vehicle).is_none()
                    && self.vehicles[driver.vehicle]
                        .definition()
                        .is_some_and(|d| d.config.research_id == CRUISER)
                {
                    driver.stopped = false;
                    driver.stranded = 0.;
                    driver.next = None;
                    chase.enlist(driver.vehicle);
                }
            }
            // A unit on an isolated road can respond on foot, but cannot fill a dispatch slot on the route.
            let routed = self
                .traffic
                .drivers
                .iter()
                .filter(|d| {
                    chase.patrol(d.vehicle).is_some()
                        && !matches!(
                            chase.order(d.path, d.distance, &self.traffic.graph),
                            LaneOrder::Unreachable
                        )
                })
                .count();
            if routed < usize::from(chase.stars)
                && chase.reinforce_in <= 0.
                && (seen || dispatch_report)
            {
                if let Some(id) = self.dispatch_patrol(hermes.city()) {
                    self.traffic.chase.enlist(id);
                    self.traffic.chase.reinforce_in = 4.;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{Controls, WalkControls};

    fn tick(world: &mut DynamicWorld, map: &Hermes) {
        world.step(map, Controls::default(), WalkControls::default(), 1. / 60.);
    }

    fn world_without_ambient_police(map: &Hermes) -> DynamicWorld {
        let mut world = DynamicWorld::new(map);
        let compact = catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-v2-09")
            .unwrap();
        for driver in &world.traffic.drivers {
            let car = &mut world.vehicles[driver.vehicle];
            if car
                .definition()
                .is_some_and(|d| d.config.research_id == CRUISER)
            {
                car.set_catalog_model(Some(compact));
            }
        }
        world
    }

    #[test]
    fn wanted_crimes_escalate_cap_and_clear() {
        let map = Hermes::test_map();
        let mut w = DynamicWorld::new(&map);
        for (crime, expected) in [
            (Crime::VehicleTheft, 1),
            (Crime::Gunshot, 3),
            (Crime::PedestrianHit, 4),
            (Crime::VehicleTheft, 5),
            (Crime::Gunshot, 5),
        ] {
            w.report_crime(crime);
            assert_eq!(w.wanted_stars(), expected);
        }
        w.clear_chase();
        assert_eq!(w.wanted_stars(), 0);
    }

    #[test]
    fn every_occupied_cruiser_joins_even_at_one_star_and_new_arrivals_join_too() {
        let map = Hermes::test_map_from_source(super::super::tests::CROSS);
        let mut w = DynamicWorld::new(&map);
        let cruiser = catalog::definitions()
            .iter()
            .position(|m| m.config.research_id == CRUISER)
            .unwrap();
        let ids: Vec<_> = w
            .traffic
            .drivers
            .iter()
            .take(4)
            .map(|d| d.vehicle)
            .collect();
        assert_eq!(ids.len(), 4);
        for &id in &ids[..3] {
            w.vehicles[id].set_catalog_model(Some(cruiser));
        }
        let compact = catalog::definitions()
            .iter()
            .position(|m| m.config.research_id == "vehicle-v2-09")
            .unwrap();
        w.vehicles[ids[3]].set_catalog_model(Some(compact));
        let at = w.player_position();
        w.report_crime(Crime::VehicleTheft);
        w.step_chase(&map, at, 0.8);
        assert_eq!(w.wanted_stars(), 1);
        assert!(ids[..3].iter().all(|&id| w.traffic.is_patrol(id)));
        assert!(!w.traffic.is_patrol(ids[3]));
        w.vehicles[ids[3]].set_catalog_model(Some(cruiser));
        w.step_chase(&map, at, 0.8);
        assert!(ids.iter().all(|&id| w.traffic.is_patrol(id)));
        let audible = |world: &DynamicWorld, id: usize| {
            world
                .sound_frame(
                    world.vehicles[id].position,
                    Vec3::X,
                    Vec3::ZERO,
                    Default::default(),
                )
                .cars
                .iter()
                .any(|car| car.id == id && car.siren)
        };
        assert!(ids.iter().all(|&id| audible(&w, id)));
        w.traffic.release(ids[1]);
        assert!(!audible(&w, ids[1]));
        assert!(!w.traffic.is_patrol(ids[1]));
        assert!(w.traffic.is_patrol(ids[0]));
        w.clear_chase();
        assert!(ids.iter().all(|&id| !w.traffic.is_patrol(id)));
    }

    #[test]
    fn two_officers_can_dismount_and_stealing_one_cruiser_keeps_both_in_pursuit() {
        let map = Hermes::test_map_from_source(
            "road avenue: sidewalk carlane carlane middle carlane carlane sidewalk; end
            avenue(-3,0,3,0); land(-4,-2,4,-2,4,2,-4,2); player_start(0,0,0);",
        );
        let mut w = world_without_ambient_police(&map);
        assert!(w.toggle_vehicle(&map));
        let cruiser = catalog::definitions()
            .iter()
            .position(|m| m.config.research_id == CRUISER)
            .unwrap();
        let ids = [w.traffic.drivers[0].vehicle, w.traffic.drivers[1].vehicle];
        for (id, x) in ids.into_iter().zip([-0.14, 0.14]) {
            w.vehicles[id] = Vehicle::spawn_at(
                map.city(),
                SpawnPoint {
                    point: [x, -0.025],
                    heading: Some(0.),
                },
            )
            .unwrap();
            w.vehicles[id].set_catalog_model(Some(cruiser));
        }
        let officer_model = pedestrian_catalog::POLICE_OFFICER_MODEL;
        assert_eq!(
            w.seated_drivers()
                .filter(|d| d.model == officer_model)
                .count(),
            2
        );
        let target = w.player_position();
        w.report_crime(Crime::VehicleTheft);
        w.step_chase(&map, target, 0.8);
        w.step_chase(&map, target, 0.02);
        assert_eq!(
            w.officer_instances()
                .filter(|o| o.model == officer_model)
                .count(),
            2
        );
        assert!(w.seated_drivers().all(|d| d.model != officer_model));
        assert!(ids.into_iter().all(|id| w.officer_out_of(id)));
        assert_eq!(w.officer_handguns().count(), 2);
        assert_eq!(w.nearby_police().filter(|(_, walking)| *walking).count(), 2);
        w.pedestrian.as_mut().unwrap().position = w.vehicles[ids[0]].position;
        assert!(w.toggle_vehicle(&map));
        assert_eq!(w.player_vehicle.0, ids[0]);
        assert!(w
            .seated_drivers()
            .any(|d| d.model == pedestrian_catalog::player_model(w.season)));
        assert_eq!(w.officer_handguns().count(), 2);
        assert!(!w.traffic.is_patrol(ids[0]));
        assert!(w.traffic.is_patrol(ids[1]));
    }

    #[test]
    fn responding_unit_crosses_a_red_or_failed_signal_at_pursuit_speed() {
        let map = Hermes::test_map_from_source(super::super::tests::CROSS);
        let graph = map.traffic_paths();
        let path = graph
            .paths
            .iter()
            .position(|p| p.points[0].x < -150. && p.points.last().unwrap().x > -30.)
            .unwrap();
        let distance = graph.paths[path].length - 5.;
        let (point, heading) = graph.paths[path].sample(distance);
        for failed in [false, true] {
            let mut traffic = super::super::Traffic::new(graph);
            traffic.chase.last_known = Vec3::X * 90.;
            traffic.chase.plan(graph);
            let turn = traffic.chase.order(path, distance, graph).next().unwrap();
            let signal = &graph.signals[graph.paths[turn].signal.unwrap()];
            traffic.time = (14. * (1 - signal.phase) as f32 - (signal.junction % 11) as f32 * 3.)
                .rem_euclid(28.);
            if failed {
                traffic.failed_junctions.insert(signal.junction);
            } else {
                traffic.failed_junctions.clear();
            }
            traffic.chase.enlist(0);
            traffic.drivers.push(Driver::new(0, path, distance, 0.));
            let mut car = Vehicle::spawn_at(
                map.city(),
                SpawnPoint {
                    point: (point.truncate() / METERS_PER_UNIT).to_array(),
                    heading: Some(heading.y.atan2(heading.x)),
                },
            )
            .unwrap();
            car.position = point + Vec3::Z * car.ride_height();
            let input = traffic.controls(map.city(), &[car], None, 0.01)[0].1;
            assert!(!input.brake && input.throttle > 0.7);
            assert!(traffic.drivers[0].target_speed > SpeedClass::Local.meters_per_second());
        }
    }

    #[test]
    fn officer_leaves_cruiser_fires_and_returns_to_the_same_seat() {
        let map = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane middle carlane carlane sidewalk; end
            street(-2,0,2,0); land(-3,-2,3,-2,3,2,-3,2); player_start(0,0,0);",
        );
        let mut w = world_without_ambient_police(&map);
        w.report_crime(Crime::VehicleTheft);
        w.step_chase(&map, w.player_position(), 1.);
        let id = w.traffic.chase.first_unit().unwrap();
        assert!(w.toggle_vehicle(&map));
        let at = w.vehicles[id].position;
        let car = &w.vehicles[id];
        let side = glam::Vec2::from_angle(car.yaw).perp().extend(0.);
        let target = at + side * 18.;
        w.pedestrian.as_mut().unwrap().position = target;
        let seats = w.seated_drivers().count();
        w.step_chase(&map, target, 0.02);
        assert!(w.officer_out_of(id));
        assert_eq!(w.seated_drivers().count() + 1, seats);
        assert!(w.nearby_police().any(|(_, walking)| walking));
        for _ in 0..90 {
            w.step_chase(&map, target - side, 1. / 60.);
        }
        assert!(w.health < 100, "officer never hit exposed moving player");
        assert!(w.officer_handguns().next().unwrap().2.is_finite());
        w.pedestrian.as_mut().unwrap().position = at + side * 65.;
        for _ in 0..240 {
            w.step_chase(&map, at, 1. / 60.);
            if !w.officer_out_of(id) {
                break;
            }
        }
        assert!(!w.officer_out_of(id));
        assert_eq!(w.traffic.chase.first_unit(), Some(id));
        w.pedestrian.as_mut().unwrap().position = target;
        assert_eq!(w.seated_drivers().count(), seats);
    }

    #[test]
    fn pursuing_officer_ragdolls_on_contact_and_resumes_after_getting_up() {
        let map = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane middle carlane carlane sidewalk; end
            street(-2,0,2,0); land(-3,-2,3,-2,3,2,-3,2); player_start(0,0,0);",
        );
        for driving in [false, true] {
            let mut w = world_without_ambient_police(&map);
            w.report_crime(Crime::VehicleTheft);
            w.step_chase(&map, w.player_position(), 1.);
            let id = w.traffic.chase.first_unit().unwrap();
            let car = &w.vehicles[id];
            let outward = glam::Vec2::from_angle(car.yaw).perp().extend(0.);
            let target = car.position + outward * 14.;
            if !driving {
                assert!(w.toggle_vehicle(&map));
                w.pedestrian.as_mut().unwrap().position = target;
            } else {
                w.vehicles[w.player_vehicle.0].position = target;
            }
            w.step_chase(&map, target, 0.02);
            assert!(w.officer_out_of(id));
            // Let the officer clear the door and approach the suspect.
            for _ in 0..120 {
                w.step_chase(&map, target, 1. / 60.);
            }
            let position = w
                .officer_instances()
                .next()
                .unwrap()
                .transform
                .w_axis
                .truncate()
                * METERS_PER_UNIT;
            let toward = (target - position).truncate();
            let direction = toward / toward.length();
            let yaw = (-direction.y).atan2(-direction.x);
            if driving {
                let car = &mut w.vehicles[w.player_vehicle.0];
                car.position =
                    position + (direction * (car.length() * 0.5 + 0.75)).extend(car.ride_height());
                car.yaw = yaw;
                car.velocity = (-direction * 8.).extend(0.);
            } else {
                w.pedestrian = Some(Pedestrian::new(
                    map.city(),
                    position + (direction * 0.8).extend(player::HALF_HEIGHT),
                    yaw,
                ));
            }
            let stars = w.wanted_stars();
            for _ in 0..40 {
                w.step(
                    &map,
                    Controls::default(),
                    WalkControls {
                        forward: 1.,
                        run: true,
                        ..Default::default()
                    },
                    1. / 60.,
                );
                if w.traffic.chase.patrols[0]
                    .officer
                    .as_ref()
                    .unwrap()
                    .reaction
                    .is_some()
                {
                    break;
                }
            }
            let patrol = &w.traffic.chase.patrols[0];
            let officer = patrol.officer.as_ref().unwrap();
            assert!(
                matches!(
                    officer.reaction.as_deref(),
                    Some(ragdoll::Reaction::Falling(_))
                ),
                "driving={driving}"
            );
            assert_eq!(w.wanted_stars(), stars + u8::from(driving));
            let model = officer.instance(w.season).model;
            let pose = *officer.instance(w.season).pose.unwrap();
            let health = w.player_health();
            // Remove the impactor and keep the suspect visible to a standing officer.
            if driving {
                w.vehicles[w.player_vehicle.0].position = target;
                w.vehicles[w.player_vehicle.0].velocity = Vec3::ZERO;
            }
            let mut rose = false;
            for frame in 0..360 {
                let officer = w.traffic.chase.patrols[0].officer.as_ref().unwrap();
                let instance = w.officer_instances().next().unwrap();
                assert_eq!(instance.model, model);
                if officer.reaction.is_none() {
                    break;
                }
                rose |= matches!(
                    officer.reaction.as_deref(),
                    Some(ragdoll::Reaction::Rising { .. })
                );
                if frame == 30 {
                    assert_ne!(*instance.pose.unwrap(), pose);
                }
                assert!(w.officer_handguns().next().is_none());
                assert_eq!(w.chase_status(), ChaseStatus::Searching);
                assert_eq!(w.player_health(), health);
                tick(&mut w, &map);
            }
            assert!(rose);
            assert!(w.officer_handguns().next().is_some());
            assert!(matches!(
                w.chase_status(),
                ChaseStatus::Pursued | ChaseStatus::Surrendering
            ));
        }
    }

    #[test]
    fn officer_tire_shots_can_miss_and_a_moving_suspect_is_harder_to_hit() {
        let map = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane middle carlane carlane sidewalk; end
            street(-2,0,2,0); land(-3,-2,3,-2,3,2,-3,2); player_start(0,0,0);",
        );
        let mut w = world_without_ambient_police(&map);
        let compact = catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-v2-09")
            .unwrap();
        let player = w.player_vehicle.0;
        w.report_crime(Crime::VehicleTheft);
        w.step_chase(&map, w.player_position(), 1.);
        let id = w.traffic.chase.first_unit().unwrap();
        let car = &w.vehicles[id];
        let forward = glam::Vec2::from_angle(car.yaw).extend(0.);
        let target = car.position + forward * 17.;
        let yaw = car.yaw + std::f32::consts::FRAC_PI_2;
        w.vehicles[player] = Vehicle::spawn_at(
            map.city(),
            SpawnPoint {
                point: (target.truncate() / METERS_PER_UNIT).to_array(),
                heading: Some(yaw),
            },
        )
        .unwrap();
        w.vehicles[player].set_catalog_model(Some(compact));
        let target = w.player_position();
        for _ in 0..360 {
            w.step_chase(&map, target - Vec3::X, 1. / 60.);
            if w.traffic.chase.patrols[0].officer.is_some() {
                break;
            }
        }
        // Replay the same rounds from the same spot against a fresh tire each time.
        let seed = w.traffic.seed;
        let [parked, moving] = [0., 16.].map(|speed| {
            w.traffic.seed = seed;
            w.vehicles[player].velocity = forward * speed;
            let mut pops = 0;
            for _ in 0..100 {
                w.vehicles[player].flat_tires = [false; 4];
                w.traffic.chase.patrols[0]
                    .officer
                    .as_mut()
                    .unwrap()
                    .cooldown = 0.;
                w.step_officer(map.city(), 0, 0.);
                let count = w.drain_player_tire_pops().count();
                assert_eq!(count, usize::from(w.player().flat_tires.contains(&true)));
                pops += count;
            }
            pops
        });
        assert!(
            0 < moving && moving < parked && parked < 100,
            "{moving} moving, {parked} parked"
        );
        w.vehicles[player].velocity = Vec3::ZERO;
        w.vehicles[player].flat_tires = [false; 4];
        for _ in 0..360 {
            w.step_chase(&map, target - Vec3::X, 1. / 60.);
            if w.player().flat_tires.contains(&true) {
                break;
            }
        }
        assert!(w.player().flat_tires.contains(&true));
        assert_eq!(w.wanted_stars(), 1);
        assert_eq!(w.health, 100);
    }

    #[test]
    fn driving_away_interrupts_capture_and_eight_unseen_seconds_clear_all_stars() {
        let map = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane middle carlane carlane sidewalk; end
            street(-3,0,3,0); land(-4,-3,4,-3,4,3,-4,3); player_start(0,0,0);",
        );
        let mut w = world_without_ambient_police(&map);
        let compact = catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-v2-09")
            .unwrap();
        let player = w.player_vehicle.0;
        w.vehicles[player].set_catalog_model(Some(compact));
        w.report_crime(Crime::Gunshot);
        w.step_chase(&map, w.player_position(), 1.);
        let patrol = w.traffic.chase.first_unit().unwrap();
        let car = &mut w.vehicles[patrol];
        car.velocity = Vec3::ZERO;
        let side = glam::Vec2::from_angle(car.yaw).perp().extend(0.);
        w.vehicles[player].position = car.position + side * 8.;
        w.step_chase(&map, w.player_position(), 0.02);
        assert_eq!(w.chase_status(), ChaseStatus::Pursued);
        let officer = w.traffic.chase.patrols[0]
            .officer
            .as_ref()
            .unwrap()
            .actor
            .position;
        let outward = if (officer - w.vehicles[patrol].position).dot(side) > 0. {
            side
        } else {
            -side
        };
        w.vehicles[player].position = officer + outward * 2.;
        w.vehicles[player].yaw = outward.y.atan2(outward.x);
        w.step_chase(&map, w.player_position(), 2.);
        assert_eq!(w.chase_status(), ChaseStatus::Surrendering);
        for _ in 0..45 {
            w.step(
                &map,
                Controls {
                    throttle: 1.,
                    ..Default::default()
                },
                WalkControls::default(),
                1. / 60.,
            );
            if w.chase_status() == ChaseStatus::Pursued {
                break;
            }
        }
        assert_eq!(w.chase_status(), ChaseStatus::Pursued);
        let last_seen = w.traffic.chase.last_known;
        let far = last_seen + outward * 180.;
        w.vehicles[player].position = far;
        w.step_chase(&map, last_seen, 7.75);
        assert_eq!(w.chase_status(), ChaseStatus::Searching);
        assert_eq!(w.traffic.chase.last_known, last_seen);
        assert_eq!(w.wanted_stars(), 3);
        w.vehicles[player].position = last_seen;
        w.step_chase(&map, far, 0.02);
        assert_eq!(w.chase_status(), ChaseStatus::Pursued);
        w.vehicles[player].position = far;
        w.step_chase(&map, last_seen, 7.75);
        assert_eq!(w.wanted_stars(), 3);
        w.step_chase(&map, far, 0.25);
        assert_eq!(w.chase_status(), ChaseStatus::Clear);
        assert_eq!(w.wanted_stars(), 0);
        assert_eq!(w.player_position(), far);
        assert_eq!(w.player_vehicle.0, player);
    }

    #[test]
    fn occupied_entry_reports_once_and_a_parked_vehicle_is_free_to_enter() {
        let map = Hermes::test_map();
        let mut w = DynamicWorld::new(&map);
        assert!(w.toggle_vehicle(&map) && w.toggle_vehicle(&map));
        assert_eq!(w.chase_status(), ChaseStatus::Clear);
        assert!(w.toggle_vehicle(&map));
        let id = w.traffic.drivers[0].vehicle;
        w.pedestrian.as_mut().unwrap().position = w.vehicles[id].position;
        assert!(w.toggle_vehicle(&map));
        assert_eq!(w.player_vehicle.0, id);
        assert_eq!(w.chase_status(), ChaseStatus::Reported(Crime::VehicleTheft));
        w.clear_chase();
        assert!(w.toggle_vehicle(&map) && w.toggle_vehicle(&map));
        assert_eq!(w.chase_status(), ChaseStatus::Clear);
    }

    #[test]
    fn bumping_a_cruiser_while_driving_starts_a_one_star_chase() {
        let map = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane middle carlane carlane sidewalk; end
            street(-2,0,2,0); land(-3,-2,3,-2,3,2,-3,2); player_start(0,0,0);",
        );
        let model = |name| {
            catalog::definitions()
                .iter()
                .position(|d| d.config.research_id == name)
                .unwrap()
        };
        let compact = model("vehicle-v2-09");
        let cruiser = model(CRUISER);
        for (other_model, driving, player_speed, other_speed, expected) in [
            (cruiser, true, 1., 0., 1),
            (cruiser, true, 0., -3., 0),
            (compact, true, 2., 0., 0),
            (cruiser, false, 2., 0., 0),
        ] {
            let mut w = world_without_ambient_police(&map);
            let player = w.player_vehicle.0;
            w.vehicles[player].set_catalog_model(Some(compact));
            if !driving {
                assert!(w.toggle_vehicle(&map));
            }
            let mut other = Vehicle::spawn(map.city());
            other.set_catalog_model(Some(other_model));
            other.yaw = 0.;
            let gap = (w.vehicles[player].length() + other.length()) * 0.5 - 0.01;
            let origin = w.vehicles[player].position;
            let id = w.vehicles.len();
            w.vehicles.push(other);
            for _ in 0..2 {
                w.vehicles[player].yaw = 0.;
                w.vehicles[player].position = origin;
                w.vehicles[player].velocity = Vec3::X * player_speed;
                w.vehicles[id].position = origin + Vec3::X * gap;
                w.vehicles[id].velocity = Vec3::X * other_speed;
                let collisions = w.vehicles[id].collisions;
                tick(&mut w, &map);
                assert!(w.vehicles[id].collisions > collisions);
                assert_eq!(w.wanted_stars(), expected);
            }
            if expected == 1 {
                assert_eq!(w.chase_status(), ChaseStatus::Reported(Crime::CruiserHit));
            }
        }
    }

    #[test]
    fn only_the_driving_players_civilian_knockdown_is_reported() {
        let map = Hermes::test_map();
        for player_driving in [true, false] {
            let mut w = DynamicWorld::new(&map);
            let (walker, yaw) = w.crowd.prepare_collision();
            let id = if player_driving {
                w.player_vehicle.0
            } else {
                w.traffic.drivers[0].vehicle
            };
            let car = &mut w.vehicles[id];
            car.position = walker + Vec3::Z * car.ride_height();
            car.yaw = yaw;
            car.velocity = Vec3::new(yaw.cos(), yaw.sin(), 0.) * 7.;
            tick(&mut w, &map);
            assert_eq!(w.pedestrian_reactions()[1], 1);
            assert_eq!(
                w.chase_status(),
                if player_driving {
                    ChaseStatus::Reported(Crime::PedestrianHit)
                } else {
                    ChaseStatus::Clear
                }
            );
        }
    }

    #[test]
    fn cruiser_drives_to_player_searches_last_seen_and_can_be_dispatched_again() {
        let map = Hermes::test_map_from_source(
            "road street: sidewalk carlane middle carlane sidewalk; end
            street(-2,0,2,0); land(-4,-3,4,-3,4,3,-4,3); player_start(0,0,0);
            building(0.12,-0.9,-0.1,0.9,-0.1,0.9,-0.2,-0.9,-0.2);",
        );
        let mut w = world_without_ambient_police(&map);
        w.report_crime(Crime::VehicleTheft);
        let mut unit = None;
        let mut start = Vec3::ZERO;
        for _ in 0..300 {
            tick(&mut w, &map);
            if let Some(id) = w.traffic.chase.first_unit() {
                if unit.is_none() {
                    start = w.vehicles[id].position;
                    assert!(w.outside_view(start));
                    unit = Some(id);
                }
                if w.vehicles[id].position.distance(start) > 2. {
                    break;
                }
            }
        }
        let id = unit.expect("cruiser on a connected street");
        assert!(
            w.vehicles[id].position.distance(w.player_position()) + 1.
                < start.distance(w.player_position())
        );
        assert_eq!(w.chase_status(), ChaseStatus::Pursued);
        w.report_crime(Crime::VehicleTheft);
        assert_eq!(w.traffic.chase.first_unit(), Some(id));
        assert!(w.toggle_vehicle(&map));
        let last_seen = w.player_position();
        w.step_chase(&map, last_seen, 0.02);
        w.pedestrian.as_mut().unwrap().position.y = -25.;
        let hidden = w.player_position();
        assert!(!w.patrol_sees(map.city(), &w.traffic.chase.patrols[0], hidden));
        w.step_chase(&map, last_seen, 1.);
        assert!(matches!(w.chase_status(), ChaseStatus::Searching));
        assert_eq!(w.traffic.chase.last_known, last_seen);
        w.pedestrian.as_mut().unwrap().position = last_seen;
        w.step_chase(&map, hidden, 0.02);
        assert_eq!(w.chase_status(), ChaseStatus::Pursued);
        w.pedestrian.as_mut().unwrap().position = hidden;
        for _ in 0..15 {
            w.step_chase(&map, hidden, 1.);
        }
        assert_eq!(w.chase_status(), ChaseStatus::Clear);
        assert_eq!(w.traffic.chase.first_unit(), None);

        w.pedestrian.as_mut().unwrap().position = last_seen;
        w.report_crime(Crime::VehicleTheft);
        w.step_chase(&map, last_seen, 1.);
        let second = w.traffic.chase.first_unit().expect("second dispatch");
        let near = w.vehicles[second].position + Vec3::Y * 4.;
        w.pedestrian.as_mut().unwrap().position = near;
        w.vehicles[second].velocity = Vec3::ZERO;
        w.step_chase(&map, near, 2.);
        assert_eq!(w.chase_status(), ChaseStatus::Pursued);
        let officer = w
            .traffic
            .chase
            .patrols
            .iter()
            .find(|p| p.vehicle == second)
            .unwrap()
            .officer
            .as_ref()
            .unwrap()
            .actor
            .position;
        let near = officer + Vec3::Y * 0.6;
        w.pedestrian.as_mut().unwrap().position = near;
        w.step_chase(&map, near, 2.);
        assert!(matches!(w.chase_status(), ChaseStatus::Surrendering));
        w.step_chase(&map, near + Vec3::X * 2., 1.);
        assert_eq!(w.chase_status(), ChaseStatus::Pursued);
        for _ in 0..3 {
            w.step_chase(&map, near, 1.);
        }
        assert_eq!(w.chase_status(), ChaseStatus::Arrested);
        assert_eq!(w.arrest_progress(), Some(0.));
        w.step(
            &map,
            Controls::default(),
            WalkControls {
                forward: 1.,
                run: true,
                ..Default::default()
            },
            3.,
        );
        assert_eq!(w.player_position(), near);
        assert_eq!(w.traffic.chase.first_unit(), Some(second));
        assert!(w.arrest_progress().unwrap() > 0.5);
        w.step(&map, Controls::default(), WalkControls::default(), 1.5);
        assert!(w.is_driving());
        assert_eq!(w.traffic.chase.first_unit(), None);
        assert_eq!(w.chase_status(), ChaseStatus::Clear);
    }

    #[test]
    fn patrol_turns_onto_the_connected_exit_at_a_four_way_intersection() {
        let map = Hermes::test_map_from_source(super::super::tests::CROSS);
        let graph = map.traffic_paths();
        let mut chase = Chase {
            last_known: Vec3::new(0., 70., 0.),
            ..Default::default()
        };
        chase.plan(graph);
        let west = graph
            .paths
            .iter()
            .position(|p| p.points[0].x < -150. && p.points.last().unwrap().x > -30.)
            .unwrap();
        let turn = chase.order(west, 0., graph).next().unwrap();
        assert!(graph.paths[west].next.contains(&turn));
        assert_eq!(graph.paths[turn].speed, SpeedClass::Junction);
        let exit = chase.order(turn, 0., graph).next().unwrap();
        assert!(graph.paths[turn].next.contains(&exit));
        let LaneOrder::Stop(at) = chase.order(exit, 0., graph) else {
            panic!("expected arrival")
        };
        assert!(graph.paths[exit].sample(at).0.distance(chase.last_known) < 6.);
    }
}
