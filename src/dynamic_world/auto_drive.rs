//! Follow the selected GPS route with the player's ordinary vehicle controls.
use super::{Controls, DynamicWorld, METERS_PER_UNIT};
use crate::hermes::{Hermes, RouteStatus};
use glam::{Vec2, Vec3};

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum DriveAvailability {
    #[default]
    SelectDestination,
    NeedVehicle,
    Unreachable,
    Arrived,
    Ready,
}

impl DynamicWorld {
    pub fn drive_availability(&self, map: &Hermes) -> DriveAvailability {
        use DriveAvailability::*;
        let car = self.player();
        if !self.is_driving() || car.is_aircraft() || car.is_boat() || self.player_health() == 0 {
            return NeedVehicle;
        }
        match map.route_status() {
            RouteStatus::Inactive => SelectDestination,
            RouteStatus::Unreachable => Unreachable,
            RouteStatus::Arrived => Arrived,
            RouteStatus::Active => Ready,
        }
    }
}

/// Find a point ahead along connected road geometry, including its elevation.
/// Limit the distance scan to what the car needs for steering and braking.
fn preview(route: &[Vec3], lookahead: f32) -> Option<(Vec3, Vec3, f32)> {
    let mut distance = 0.;
    let mut target = None;
    for pair in route.windows(2) {
        let delta = (pair[1] - pair[0]) * METERS_PER_UNIT;
        let length = delta.length();
        if length < 0.001 {
            continue;
        }
        let direction = delta / length;
        if target.is_none() && distance + length >= lookahead {
            target = Some((
                pair[0] * METERS_PER_UNIT + direction * (lookahead - distance),
                direction,
            ));
        }
        distance += length;
        if distance > 40. {
            break;
        }
    }
    target.map(|(point, heading)| (point, heading, distance))
}

impl DynamicWorld {
    // Distance to the rear bumper ahead, measured along a candidate lane.
    fn traffic_gap(&self, origin: Vec3, heading: Vec2) -> f32 {
        let car = self.player();
        self.vehicles
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                *i != self.player_vehicle.0
                    && !self.traffic.inactive(*i)
                    && !self.cyclists.inactive(*i)
            })
            .map(|(_, other)| car.following_clearance(origin, heading, other))
            .fold(f32::INFINITY, f32::min)
    }

    // GPS geometry follows the centerline. Choose a lane on the same deck and
    // give a queued vehicle space when an adjacent forward lane is clearer.
    fn lane_target(&self, map: &Hermes, point: Vec3, direction: Vec3) -> Vec3 {
        let car = self.player();
        let mut best = (f32::INFINITY, point);
        let mut lanes = Vec::new();
        for path in map
            .traffic_paths()
            .paths
            .iter()
            .filter(|path| path.near(point, 10.))
        {
            let (at, error) = path.project(point);
            if error > 10. {
                continue;
            }
            let (lane, heading) = path.sample(at);
            let offset = lane - point;
            if heading.dot(direction) < 0.8
                || offset.z.abs() > 1.5
                || offset.dot(direction).abs() > 2.
            {
                continue;
            }
            let mut consider = |target: Vec3, penalty: f32| {
                let delta = car.position - target;
                let across = heading.truncate().perp_dot(delta.truncate()).abs();
                let origin = target + heading * delta.dot(heading);
                let gap = self.traffic_gap(origin, heading.truncate());
                let cost = point.distance(target) + across * 0.5 + (22. - gap).max(0.) + penalty;
                if cost < best.0 {
                    best = (cost, target);
                }
            };
            consider(lane, 0.);
            // A rider can filter through a stationary queue where the gap
            // between two adjacent lanes is wide enough for the bicycle.
            if car.is_bicycle() {
                for &(other, other_heading) in &lanes {
                    let separation: Vec3 = lane - other;
                    if (3. ..5.5).contains(&separation.length())
                        && heading.dot(other_heading) > 0.98
                        && separation.dot(heading).abs() < 0.5
                    {
                        consider((lane + other) * 0.5, 5.);
                    }
                }
                lanes.push((lane, heading));
            }
        }
        best.1
    }
}

pub enum DriveStep {
    Apply(Controls),
    Finished,
    Blocked,
}

pub struct AutoDrive {
    pub elapsed: f32,
    stalled: f32,
    next_decision: f32,
    controls: Controls,
    stopping: bool,
}

impl AutoDrive {
    pub fn start(world: &DynamicWorld, map: &Hermes) -> Option<Self> {
        (world.drive_availability(map) == DriveAvailability::Ready).then_some(Self {
            elapsed: 0.,
            stalled: 0.,
            next_decision: 0.,
            controls: Controls::default(),
            stopping: false,
        })
    }

    pub fn step(&mut self, world: &DynamicWorld, map: &Hermes, dt: f32) -> DriveStep {
        self.elapsed += dt;
        let car = world.player();
        let speed = car.velocity.truncate().length();
        match world.drive_availability(map) {
            DriveAvailability::Ready => {}
            DriveAvailability::Arrived => self.stopping = true,
            _ => return DriveStep::Finished,
        }
        if self.stopping {
            return if speed < 0.12 {
                DriveStep::Finished
            } else {
                DriveStep::Apply(Controls {
                    brake: true,
                    ..Default::default()
                })
            };
        }
        // Allow queues more than a full 28-second signal cycle to clear.
        self.stalled = if speed < 0.5 { self.stalled + dt } else { 0. };
        let timeout = if self.controls.throttle > 0.2 {
            12.
        } else {
            45.
        };
        if self.stalled > timeout {
            return DriveStep::Blocked;
        }
        self.next_decision -= dt;
        if self.next_decision > 0. {
            return DriveStep::Apply(self.controls);
        }
        self.next_decision = 0.1;
        let lookahead = (4. + speed * 0.65).clamp(4., 14.);
        let route = map.road_route();
        let Some(end) = route.last() else {
            return DriveStep::Finished;
        };
        let destination = end * METERS_PER_UNIT;
        let (target, distance) = match preview(route, lookahead) {
            Some((point, direction, distance)) => {
                (world.lane_target(map, point, direction), distance)
            }
            // The destination can be directly alongside the car, with little
            // or no longitudinal travel left on the snapped GPS edge.
            _ => (destination, destination.distance(car.position)),
        };
        let forward = Vec2::from_angle(car.yaw);
        let aim = target.truncate() - car.position.truncate();
        let angle = forward.perp_dot(aim).atan2(forward.dot(aim));
        let curvature = 2. * angle.sin() / aim.length().max(1.);
        let steering = if angle.abs() > 1.2 {
            angle.signum()
        } else {
            car.pursuit_steering(curvature, speed)
        };
        // Look along the intended turn so a stopped vehicle in the lane being
        // left does not keep the brakes applied throughout a lane change.
        let travel = Vec2::from_angle(car.yaw + angle.clamp(-0.45, 0.45));
        let clearance = (distance - 2.)
            .max(0.)
            .min(world.traffic_gap(car.position, travel));
        let cruise: f32 = if angle.abs() > 1.2 { 2.8 } else { 11. };
        let limit = cruise
            .min((2.6 / curvature.abs().max(0.001)).sqrt())
            .min((4. * clearance).sqrt());
        self.controls = Controls {
            throttle: ((limit - speed) * 0.45).clamp(0., 0.75),
            brake: speed > limit + 0.25 || limit < 0.1,
            steering: steering.clamp(-1., 1.),
            ..Default::default()
        };
        DriveStep::Apply(self.controls)
    }
}
