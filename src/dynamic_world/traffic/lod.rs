//! Distant undisturbed traffic follows lane geometry; interactions retain rigid-body physics.
use super::*;
use glam::Vec2;

const PROMOTE_RADIUS: f32 = 60.;
const DEMOTE_RADIUS: f32 = 85.;
const APPROACH_SECONDS: f32 = 1.5;
const CONTACT_HOLD: f32 = 5.;

#[derive(Clone, Copy, Debug)]
pub(super) struct LaneMotion {
    offset: Vec3,
    yaw_offset: f32,
    pitch_offset: f32,
    roll: f32,
}
#[derive(Clone, Copy, Debug)]
pub(super) enum Motion {
    Physical,
    Lane(LaneMotion),
}
#[derive(Clone, Copy)]
pub(super) enum Protected {
    Awake { quiet: f32 },
    Sleeping,
}

fn approaching(car: &Vehicle, point: Vec3, velocity: Vec3, radius: f32) -> bool {
    let delta = car.position - point;
    let distance = delta.length();
    let closing = (velocity - car.velocity).dot(delta) / distance.max(0.001);
    distance <= radius + closing.max(0.) * APPROACH_SECONDS
}

impl Traffic {
    pub(in crate::dynamic_world) fn wake_after_tire_hit(&mut self, vehicle: usize) {
        self.interacted
            .insert(vehicle, Protected::Awake { quiet: 0. });
    }

    pub(in crate::dynamic_world) fn select_physics(
        &mut self,
        vehicles: &[Vehicle],
        player: usize,
        center: Vec3,
        velocity: Vec3,
    ) {
        self.interacted
            .insert(player, Protected::Awake { quiet: 0. });
        for driver in &mut self.drivers {
            let car = &vehicles[driver.vehicle];
            if driver.collisions != car.collisions {
                driver.collisions = car.collisions;
                driver.physical_until = self.time + CONTACT_HOLD;
            }
            let radius = match driver.motion {
                Motion::Physical => DEMOTE_RADIUS,
                Motion::Lane(_) => PROMOTE_RADIUS,
            };
            let interacting = self.interacted.iter().any(|(&id, _)| {
                id != player
                    && id != driver.vehicle
                    && approaching(car, vehicles[id].position, vehicles[id].velocity, 20.)
            });
            if self.chase.patrol(driver.vehicle).is_some()
                || !self.physics_lod
                || driver.vehicle == player
                || self.interacted.contains_key(&driver.vehicle)
                || self.time < driver.physical_until
                || approaching(car, center, velocity, radius)
                || interacting
            {
                // Promotion keeps the existing pose, velocities and angular momentum.
                driver.motion = Motion::Physical;
                continue;
            }
            if matches!(driver.motion, Motion::Lane(_)) {
                continue;
            }
            let path = &self.graph.paths[driver.path];
            let ground = car.position - Vec3::Z * car.ride_height();
            let (distance, error) = path.project(ground);
            let (point, heading) = path.sample(distance);
            let yaw = heading.y.atan2(heading.x);
            let yaw_delta = car.yaw - yaw;
            let yaw_offset = yaw_delta.sin().atan2(yaw_delta.cos());
            let pitch = heading.z.atan2(heading.truncate().length());
            // A displaced, sliding or airborne chassis must settle/recover physically.
            if !car.grounded
                || error > 0.5
                || yaw_offset.abs() > 0.15
                || car.roll.abs() > 0.15
                || (car.pitch - pitch).abs() > 0.15
                || car.angular_velocity().length_squared() > 0.25
                || (car.velocity - heading * car.velocity.dot(heading)).length_squared() > 0.25
            {
                continue;
            }
            driver.distance = distance;
            driver.motion = Motion::Lane(LaneMotion {
                offset: ground - point,
                yaw_offset,
                pitch_offset: car.pitch - pitch,
                roll: car.roll,
            });
        }
    }

    pub(in crate::dynamic_world) fn step_lane(
        &mut self,
        index: usize,
        car: &mut Vehicle,
        input: Controls,
        dt: f32,
    ) -> bool {
        let Some(driver) = self
            .drivers
            .iter_mut()
            .find(|driver| driver.vehicle == index)
        else {
            return false;
        };
        let Motion::Lane(mut motion) = driver.motion else {
            return false;
        };
        let heading = self.graph.paths[driver.path].sample(driver.distance).1;
        let speed = car.velocity.dot(heading).max(0.);
        let target = if input.handbrake {
            0.
        } else {
            driver.target_speed
        };
        let mut next_speed = if speed < target {
            (speed + 3. * dt).min(target)
        } else {
            (speed - 6. * dt).max(target)
        };
        let requested = (speed + next_speed) * 0.5 * dt;
        let mut travel = requested.min(driver.clearance.max(0.));
        if travel < requested {
            next_speed = 0.;
        }
        // Carry distance across short connector paths instead of snapping to their start.
        // The same route choices and signal clearances are computed by controls().
        for _ in 0..16 {
            let path = &self.graph.paths[driver.path];
            let remaining = (path.length - driver.distance).max(0.);
            if travel < remaining {
                driver.distance += travel;
                break;
            }
            travel -= remaining;
            driver.distance = path.length;
            let Some(next) = driver.next else {
                next_speed = 0.;
                break;
            };
            driver.path = next;
            driver.distance = 0.;
            let exits = &self.graph.paths[next].next;
            driver.next = if exits.is_empty() {
                None
            } else {
                self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                Some(exits[((self.seed >> 32) as usize) % exits.len()])
            };
            if travel == 0. {
                break;
            }
        }
        let (point, heading) = self.graph.paths[driver.path].sample(driver.distance);
        let decay = (-dt * 1.5).exp();
        motion.offset *= decay;
        motion.yaw_offset *= decay;
        motion.pitch_offset *= decay;
        motion.roll *= decay;
        car.follow_lane(
            point + Vec3::Z * car.ride_height() + motion.offset,
            heading,
            next_speed,
            Vec3::new(motion.yaw_offset, motion.pitch_offset, motion.roll),
            input,
            dt,
        );
        driver.motion = Motion::Lane(motion);
        true
    }

    pub(in crate::dynamic_world) fn record_contacts(
        &mut self,
        contacts: &[(usize, usize)],
        player: usize,
    ) {
        for &(a, b) in contacts {
            for driver in &mut self.drivers {
                if driver.vehicle == a || driver.vehicle == b {
                    driver.motion = Motion::Physical;
                    driver.physical_until = self.time + CONTACT_HOLD;
                }
            }
            // A sleeping body can receive an impulse before its next simulation tick.
            for id in [a, b] {
                if self.interacted.contains_key(&id) {
                    self.interacted.insert(id, Protected::Awake { quiet: 0. });
                }
            }
        }
        // A pile-up's contact ordering must not lose the player's indirect interactions.
        loop {
            let mut added = false;
            for &(a, b) in contacts {
                if a == player
                    || b == player
                    || self.interacted.contains_key(&a)
                    || self.interacted.contains_key(&b)
                {
                    for id in [a, b] {
                        if let std::collections::hash_map::Entry::Vacant(entry) =
                            self.interacted.entry(id)
                        {
                            entry.insert(Protected::Awake { quiet: 0. });
                            added = true;
                        }
                    }
                }
            }
            if !added {
                break;
            }
        }
    }

    pub(in crate::dynamic_world) fn sleep_if_settled(
        &mut self,
        index: usize,
        car: &mut Vehicle,
        input: Controls,
        center: Vec3,
        dt: f32,
    ) -> bool {
        if !self.physics_lod {
            return false;
        }
        let Some(state) = self.interacted.get_mut(&index) else {
            return false;
        };
        if car.position.distance(center) < DEMOTE_RADIUS
            || input.throttle != 0.
            || car.velocity.length_squared() > 0.01
            || car.angular_velocity().length_squared() > 0.0025
        {
            *state = Protected::Awake { quiet: 0. };
            return false;
        }
        match state {
            Protected::Awake { quiet } => {
                *quiet += dt;
                if *quiet < 4. {
                    return false;
                }
                car.velocity = Vec3::ZERO;
                car.angular_momentum = Vec3::ZERO;
                car.body_lean_rate = Vec2::ZERO;
                *state = Protected::Sleeping;
            }
            Protected::Sleeping => {}
        }
        true
    }

    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub(in crate::dynamic_world) fn use_full_physics(&mut self) {
        self.physics_lod = false;
        for driver in &mut self.drivers {
            driver.motion = Motion::Physical;
        }
        for state in self.interacted.values_mut() {
            *state = Protected::Awake { quiet: 0. };
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::dynamic_world) fn physics_counts(&self, total: usize) -> [usize; 3] {
        let lane = self
            .drivers
            .iter()
            .filter(|driver| matches!(driver.motion, Motion::Lane(_)))
            .count();
        let sleeping = self
            .interacted
            .values()
            .filter(|state| matches!(state, Protected::Sleeping))
            .count();
        [
            total - self.inactive.len() - lane - sleeping,
            lane,
            sleeping,
        ]
    }
}

#[cfg(test)]
mod tests;
