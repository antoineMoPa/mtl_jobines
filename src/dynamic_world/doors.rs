//! Door openings in vehicle-local metres (+X forward, +Y left, +Z up from the road).
use super::Vehicle;
use glam::{Mat4, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Door {
    FrontLeft,
    FrontRight,
    RearLeft,
    RearRight,
}
impl Door {
    pub const ALL: [Self; 4] = [
        Self::FrontLeft,
        Self::FrontRight,
        Self::RearLeft,
        Self::RearRight,
    ];
    pub fn side(self) -> f32 {
        match self {
            Self::FrontLeft | Self::RearLeft => 1.,
            _ => -1.,
        }
    }
    fn rear(self) -> bool {
        matches!(self, Self::RearLeft | Self::RearRight)
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Doors {
    fraction: [f32; 4],
    hold: [f32; 4],
}
impl Doors {
    pub fn use_door(&mut self, door: Door) {
        self.hold[door as usize] = 0.65;
    }
    pub fn abandon(&mut self, door: Door) {
        self.fraction[door as usize] = 1.;
        self.hold[door as usize] = f32::INFINITY;
    }
    fn toggle(&mut self, door: Door) -> bool {
        let hold = &mut self.hold[door as usize];
        *hold = if *hold > 0. { 0. } else { f32::INFINITY };
        *hold > 0.
    }
    pub fn claim(&mut self) {
        self.hold = [0.; 4];
    }
    pub fn set_fraction(&mut self, door: Door, fraction: f32) {
        self.fraction[door as usize] = fraction;
    }
    pub fn fraction(&self, door: Door) -> f32 {
        self.fraction[door as usize]
    }
    pub fn step(&mut self, dt: f32) {
        for (fraction, hold) in self.fraction.iter_mut().zip(&mut self.hold) {
            let opening = dt.min(*hold);
            *fraction = ((*fraction + opening * 3.5).min(1.) - (dt - opening) * 2.8).max(0.);
            *hold = (*hold - dt).max(0.);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Motion {
    Swing,
    Slide,
    Metro,
}
#[derive(Clone, Copy)]
pub(super) struct Opening {
    pub rear: f32,
    pub front: f32,
    pub sill: f32,
    pub top: f32,
    pub inner: f32,
    pub hinge_y: f32,
    pub motion: Motion,
}

impl Opening {
    pub fn transform(self, door: Door, fraction: f32, leaf: f32) -> Mat4 {
        let side = door.side();
        match self.motion {
            Motion::Swing => {
                let pivot = Vec3::new(self.front, self.hinge_y * side, 0.);
                Mat4::from_translation(pivot)
                    * Mat4::from_rotation_z(-side * fraction * 1.16)
                    * Mat4::from_translation(-pivot)
            }
            Motion::Metro => Mat4::from_translation(Vec3::X * (1.4 * leaf * fraction)),
            Motion::Slide => Mat4::from_translation(
                Vec3::new((self.front - self.rear) * leaf, side * 0.10, 0.) * fraction,
            ),
        }
    }
}

// (front row, optional rear row), bottom/top and inboard cut line measured on each shell.
pub(super) fn opening(id: &str, door: Door) -> Option<Opening> {
    use Motion::*;
    let (front, rear, sill, top, inner, hinge_y, motion) = match id {
        "vehicle-v2-09" | "vehicle-j01" | "vehicle-j06" => (
            (-0.17, 0.90),
            Some((-1.23, -0.17)),
            0.27,
            1.265,
            0.59,
            0.81,
            Swing,
        ),
        "vehicle-v2-01" => (
            (0.16, 1.05),
            Some((-0.86, 0.16)),
            0.49,
            1.84,
            0.72,
            0.97,
            Swing,
        ),
        "vehicle-12" => (
            (-0.13, 0.82),
            Some((-1.12, -0.13)),
            0.48,
            1.60,
            0.67,
            0.90,
            Swing,
        ),
        "vehicle-j02" => (
            (3.97, 5.20),
            Some((-2.28, -1.38)),
            0.43,
            2.48,
            1.05,
            1.31,
            Slide,
        ),
        "vehicle-j03" => ((0.41, 2.10), None, 1.51, 3.09, 0.94, 1.12, Swing),
        "vehicle-j05" => ((2.04, 3.67), None, 0.88, 3.18, 0.85, 1.14, Swing),
        "vehicle-j07" => ((1.61, 3.82), None, 0.65, 2.29, 1.08, 1.25, Swing),
        "vehicle-r01" => (
            (4.28, 5.64),
            Some((-3.72, -2.36)),
            1.02,
            3.39,
            1.2,
            1.27,
            Metro,
        ),
        "vehicle-r02" => (
            (4.65, 5.89),
            Some((-3.85, -2.61)),
            0.39,
            2.62,
            1.16,
            1.45,
            Slide,
        ),
        "vehicle-air-estuaire" if door == Door::FrontLeft => {
            ((9.09, 9.95), None, 2.0, 4.28, 1.30, 1.82, Swing)
        }
        "vehicle-air-fleche" if door == Door::FrontLeft => {
            ((2.47, 3.13), None, 0.83, 1.87, 0.60, 0.82, Swing)
        }
        _ => return None,
    };
    let (rear, front) = if door.rear() { rear? } else { front };
    Some(Opening {
        rear,
        front,
        sill,
        top,
        inner,
        hinge_y,
        motion,
    })
}
impl Vehicle {
    pub(super) fn access_point(&self, door: Door) -> Option<glam::Vec2> {
        let opening = opening(&self.definition()?.config.research_id, door)?;
        let f = glam::Vec2::from_angle(self.yaw);
        Some(
            self.position.truncate()
                + f * ((opening.rear + opening.front) * 0.5)
                + f.perp() * door.side() * (opening.hinge_y + 0.90),
        )
    }
    pub(super) fn nearest_door(&self, point: Vec3) -> Option<Door> {
        self.nearest_door_among(point, Door::ALL)
    }

    pub(super) fn nearest_front_door(&self, point: Vec3) -> Option<Door> {
        self.nearest_door_among(point, [Door::FrontLeft, Door::FrontRight])
    }

    fn nearest_door_among(
        &self,
        point: Vec3,
        doors: impl IntoIterator<Item = Door>,
    ) -> Option<Door> {
        doors
            .into_iter()
            .filter_map(|door| {
                Some((
                    door,
                    self.access_point(door)?.distance_squared(point.truncate()),
                ))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(door, _)| door)
    }
}

impl super::DynamicWorld {
    pub fn toggle_door(&mut self) -> bool {
        if self.health == 0 || self.metro.aboard || self.entry.is_some() {
            return false;
        }
        let target = if let Some(person) = &self.pedestrian {
            self.vehicles
                .iter()
                .enumerate()
                .filter(|(_, car)| {
                    (car.position.z - car.ride_height() + super::player::HALF_HEIGHT
                        - person.position.z)
                        .abs()
                        < 2.
                        && car.velocity.length_squared() <= 9.
                })
                .filter_map(|(index, car)| {
                    let door = car.nearest_door(person.position)?;
                    let distance = car
                        .access_point(door)?
                        .distance_squared(person.position.truncate());
                    (distance <= 1.8_f32.powi(2)).then_some((index, door, distance))
                })
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .map(|(id, door, _)| (id, door))
        } else {
            let car = self.player();
            car.access_point(Door::FrontLeft)
                .filter(|_| {
                    car.velocity.length_squared() <= 9. && (!car.is_aircraft() || car.grounded)
                })
                .map(|_| (self.player_vehicle.0, Door::FrontLeft))
        };
        if let Some((id, door)) = target {
            let open = self.vehicles[id].doors.toggle(door);
            if let Some(p) = &self.pedestrian {
                self.entry = super::vehicle_entry::Entry::new(
                    &self.vehicles[id],
                    p,
                    self.season,
                    Some(door),
                );
                if let Some(gesture) = &mut self.entry {
                    gesture.operate_handle(id, open);
                }
            }
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{catalog, pedestrian_catalog, DynamicWorld};
    use crate::hermes::Hermes;
    const PAD: &str = "pad=concrete(-2,-2,2,-2,2,2,-2,2); elevation(pad,0.1); player_start(0,0,0);";

    #[test]
    fn every_enclosed_vehicle_has_articulated_access_and_a_player_in_the_cab() {
        let map = Hermes::test_map_from_source(PAD);
        let mut world = DynamicWorld::new(&map);
        for (model, def) in catalog::definitions().iter().enumerate() {
            assert!(world.switch_vehicle(model));
            let enclosed = !world.player().is_boat()
                && !world.player().is_bicycle()
                && def.config.research_id != super::super::baggage::MODEL_ID
                && def.config.research_id != catalog::SNOWMOBILE_ID;
            assert_eq!(world.toggle_door(), enclosed);
            if !enclosed {
                continue;
            }
            let before = def.part_transforms(world.player());
            let instances: Vec<_> = world.seated_drivers().collect();
            assert_eq!(instances.len(), 1);
            assert_eq!(
                instances[0].model,
                pedestrian_catalog::player_model(world.season())
            );
            assert!(instances[0].pose.is_some());
            for door in Door::ALL {
                if opening(&def.config.research_id, door).is_none() {
                    continue;
                }
                let car = &mut world.vehicles[world.player_vehicle.0];
                car.doors = Doors::default();
                car.doors.toggle(door);
                car.doors.step(1.);
                let open = def.part_transforms(car);
                assert_eq!(open[0], before[0]);
                assert!(open != before, "{} {door:?} stayed shut", def.config.name);
                car.doors.toggle(door);
                car.doors.step(1.);
                assert_eq!(def.part_transforms(car), before);
            }
        }
    }

    #[test]
    fn exit_and_entry_use_the_nearby_door_and_transfer_the_player() {
        let map = Hermes::test_map_from_source(PAD);
        let mut world = DynamicWorld::new(&map);
        assert!(world.switch_vehicle(0));
        assert!(world.toggle_vehicle(&map));
        assert_eq!(world.seated_drivers().count(), 0);
        for _ in 0..30 {
            world.step(&map, Default::default(), Default::default(), 1. / 60.);
        }
        assert_eq!(world.player().doors.fraction(Door::FrontLeft), 1.);
        for _ in 0..95 {
            world.step(&map, Default::default(), Default::default(), 1. / 60.);
        }
        let right = world.player().access_point(Door::RearRight).unwrap();
        let foot = world.pedestrian.as_mut().unwrap();
        foot.position = right.extend(foot.position.z);
        assert!(world.toggle_door());
        for _ in 0..20 {
            world.step(&map, Default::default(), Default::default(), 1. / 60.);
        }
        assert_eq!(world.player().doors.fraction(Door::RearRight), 0.);
        for _ in 0..65 {
            world.step(&map, Default::default(), Default::default(), 1. / 60.);
        }
        assert!(world.entry.is_none());
        assert_eq!(world.player().doors.fraction(Door::RearRight), 1.);
        assert!(world.toggle_vehicle(&map));
        assert_eq!(world.seated_drivers().count(), 1);
        world.vehicles[0].doors.step(2.);
        assert_eq!(world.player().doors.fraction(Door::RearRight), 0.);
    }
}
