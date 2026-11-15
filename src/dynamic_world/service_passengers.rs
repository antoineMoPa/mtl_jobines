//! Reserved mission passengers, separate from the recyclable sidewalk crowd.
use super::{crowd::Instance, pedestrian_catalog, Vehicle};
use glam::{Vec2, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Waiting,
    Boarding,
    Onboard,
    Alighting,
    Gone,
}

struct Passenger {
    id: u32,
    model: usize,
    position: Vec3,
    from: Vec3,
    to: Vec3,
    yaw: f32,
    time: f32,
    duration: f32,
    state: State,
}

#[derive(Default)]
pub(super) struct ServicePassengers {
    actors: Vec<Passenger>,
    pending: usize,
    finished: bool,
    active: bool,
    queued_board: Vec<u32>,
    door: Vec3,
}

impl ServicePassengers {
    pub(super) fn clear(&mut self) {
        self.actors.clear();
        self.pending = 0;
        self.finished = false;
        self.active = false;
        self.queued_board.clear();
    }

    pub(super) fn spawn(&mut self, id: u32, point: Vec3) {
        if self.actors.iter().any(|actor| actor.id == id) {
            return;
        }
        let model = id as usize % pedestrian_catalog::definitions().len();
        self.actors.push(Passenger {
            id,
            model,
            position: point,
            from: point,
            to: point,
            yaw: 0.,
            time: 0.,
            duration: 1.,
            state: State::Waiting,
        });
    }

    pub(super) fn begin(
        &mut self,
        board: &[u32],
        alight: &[u32],
        vehicle: &Vehicle,
        sidewalk: Vec3,
    ) -> bool {
        if self.active {
            return false;
        }
        if board.iter().any(|id| {
            !self
                .actors
                .iter()
                .any(|actor| actor.id == *id && actor.state == State::Waiting)
        }) || alight.iter().any(|id| {
            !self
                .actors
                .iter()
                .any(|actor| actor.id == *id && actor.state == State::Onboard)
        }) {
            return false;
        }
        let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
        let left = Vec2::new(-forward.y, forward.x);
        let side = if (sidewalk.truncate() - vehicle.position.truncate()).dot(left) >= 0. {
            left
        } else {
            -left
        };
        let door = vehicle.position
            + (side * (vehicle.width() * 0.5 + 0.25) - forward * (vehicle.length() * 0.15))
                .extend(0.);
        self.door = door;
        self.queued_board.extend_from_slice(board);
        self.active = true;
        for (index, id) in alight.iter().enumerate() {
            let actor = self
                .actors
                .iter_mut()
                .find(|actor| actor.id == *id)
                .unwrap();
            actor.position = door;
            actor.from = door;
            actor.to = sidewalk + Vec3::X * (index as f32 * 0.7);
            actor.yaw = (actor.to.y - door.y).atan2(actor.to.x - door.x);
            actor.time = 0.;
            actor.duration = door.distance(actor.to).clamp(1., 7.) / 1.4;
            actor.state = State::Alighting;
        }
        self.pending = alight.len();
        if self.pending == 0 {
            self.next_boarder();
        }
        true
    }

    fn next_boarder(&mut self) {
        if self.queued_board.is_empty() {
            self.active = false;
            self.finished = true;
            return;
        }
        let id = self.queued_board.remove(0);
        let actor = self.actors.iter_mut().find(|actor| actor.id == id).unwrap();
        actor.from = actor.position;
        actor.to = self.door;
        actor.yaw = (self.door.y - actor.position.y).atan2(self.door.x - actor.position.x);
        actor.time = 0.;
        actor.duration = actor.position.distance(self.door).clamp(1., 9.) / 1.4;
        actor.state = State::Boarding;
        self.pending = 1;
    }

    pub(super) fn step(&mut self, dt: f32) {
        for actor in &mut self.actors {
            if !matches!(actor.state, State::Boarding | State::Alighting) {
                continue;
            }
            actor.time = (actor.time + dt).min(actor.duration);
            let t = actor.time / actor.duration;
            actor.position = actor.from.lerp(actor.to, t);
            if t >= 1. {
                actor.state = if actor.state == State::Boarding {
                    State::Onboard
                } else {
                    State::Gone
                };
                self.pending -= 1;
            }
        }
        if self.active && self.pending == 0 {
            self.next_boarder();
        }
    }

    pub(super) fn take_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished)
    }

    pub(super) fn instances(&self) -> impl Iterator<Item = Instance<'_>> + '_ {
        self.actors
            .iter()
            .filter(|actor| {
                matches!(
                    actor.state,
                    State::Waiting | State::Boarding | State::Alighting
                )
            })
            .map(|actor| Instance {
                model: actor.model,
                transform: pedestrian_catalog::instance_transform(actor.position, actor.yaw),
                opacity: 1.,
                idle_time: actor.id as f32 * 0.4,
                walk_time: actor.time,
                movement: if actor.state == State::Waiting {
                    0.
                } else {
                    1.
                },
                pose: None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn riders_board_one_at_a_time_then_alight() {
        let hermes = crate::hermes::Hermes::test_map();
        let vehicle = Vehicle::spawn(hermes.city());
        let sidewalk = vehicle.position + Vec3::Y * 3.;
        let mut service = ServicePassengers::default();
        service.spawn(1, sidewalk);
        service.spawn(2, sidewalk + Vec3::X);
        assert!(service.begin(&[1, 2], &[], &vehicle, sidewalk));
        assert_eq!(service.actors[0].state, State::Boarding);
        assert_eq!(service.actors[1].state, State::Waiting);
        for _ in 0..2500 {
            service.step(1. / 120.);
        }
        assert!(service.take_finished());
        assert!(!service.take_finished());
        assert_eq!(service.instances().count(), 0);
        assert!(service.begin(&[], &[1, 2], &vehicle, sidewalk));
        for _ in 0..1500 {
            service.step(1. / 120.);
        }
        assert!(service.take_finished());
        assert_eq!(service.instances().count(), 0);
    }
}
