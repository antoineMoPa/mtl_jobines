//! Autonomous rail service and pedestrian boarding. Trains never enter road-car physics.
use super::{
    catalog, pedestrian_gait::Ground, player::Pedestrian, LocalLight, Vehicle, WalkControls,
    METERS_PER_UNIT,
};
use crate::hermes::{
    metro::{MetroMap, FLOOR_HEIGHT, TRACK_HEIGHT},
    Hermes,
};
use glam::{Quat, Vec3};
const DWELL: f32 = 10.;
const DOOR_TIME: f32 = 0.8;
const TRAVEL_TIME: f32 = 15.;
const CAR_OFFSETS: [f32; 3] = [0., 16.6, -16.6];
pub(super) struct Metro {
    pub map: MetroMap,
    pub train: Option<Vehicle>,
    other_cars: Vec<Vehicle>,
    distance: f32,
    speed: f32,
    stop: usize,
    dwell: f32,
    travel: f32,
    leg_start: f32,
    leg_distance: f32,
    pub aboard: bool,
    rider: usize,
}
impl Metro {
    pub fn new(hermes: &Hermes) -> Self {
        let map = MetroMap::from_city(hermes.city());
        let mut service = Self {
            map,
            train: None,
            other_cars: Vec::new(),
            distance: 0.,
            speed: 0.,
            stop: 0,
            dwell: DWELL,
            travel: 0.,
            leg_start: 0.,
            leg_distance: 0.,
            aboard: false,
            rider: 0,
        };
        if service.map.stations.is_empty() || service.map.samples.len() < 2 {
            return service;
        }
        if let Some(id) = catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-r01")
        {
            let mut cars = (0..3)
                .map(|_| {
                    let mut car = Vehicle::spawn(hermes.city());
                    car.set_catalog_model(Some(id));
                    car
                })
                .collect::<Vec<_>>();
            service.train = Some(cars.remove(0));
            service.other_cars = cars;
            service.distance = service.map.stations[0].distance;
            service.place();
        }
        service
    }
    pub fn cars(&self) -> impl Iterator<Item = &Vehicle> {
        self.train.iter().chain(self.other_cars.iter())
    }
    fn car(&self, index: usize) -> Option<&Vehicle> {
        if index == 0 {
            self.train.as_ref()
        } else {
            self.other_cars.get(index - 1)
        }
    }
    fn place(&mut self) {
        let doors = self.train.as_ref().map_or(0., |t| t.metro_doors);
        for (index, car) in self
            .train
            .iter_mut()
            .chain(self.other_cars.iter_mut())
            .enumerate()
        {
            let (p, yaw) = self.map.sample(self.distance + CAR_OFFSETS[index]);
            car.position = p * METERS_PER_UNIT + Vec3::Z * car.ride_height();
            car.yaw = yaw;
            car.pitch = 0.;
            car.roll = 0.;
            car.steering = 0.;
            car.body_lean = glam::Vec2::ZERO;
            car.velocity = Vec3::new(yaw.cos(), yaw.sin(), 0.) * self.speed;
            car.sound_running = false;
            car.metro_doors = doors;
        }
    }
    pub fn step(&mut self, dt: f32, player: Option<&mut Pedestrian>) {
        if self.train.is_none() || !dt.is_finite() || dt <= 0. {
            return;
        }
        let old = self.car(self.rider).map(|t| (t.position, t.yaw)).unwrap();
        let mut time = dt;
        while time > 0. {
            let h = time.min(1. / 120.);
            time -= h;
            if self.dwell > 0. {
                self.dwell = (self.dwell - h).max(0.);
                let elapsed = DWELL - self.dwell;
                self.train.as_mut().unwrap().metro_doors = (elapsed / DOOR_TIME)
                    .min(self.dwell / DOOR_TIME)
                    .clamp(0., 1.);
                if self.dwell == 0. {
                    self.stop = (self.stop + 1) % self.map.stations.len();
                    self.travel = 0.;
                    self.leg_start = self.distance;
                    self.leg_distance = (self.map.stations[self.stop].distance - self.distance)
                        .rem_euclid(self.map.length);
                }
            } else {
                let previous = self.distance;
                self.travel = (self.travel + h).min(TRAVEL_TIME);
                let t = self.travel / TRAVEL_TIME;
                let ease = t * t * (3. - 2. * t);
                self.distance =
                    (self.leg_start + self.leg_distance * ease).rem_euclid(self.map.length);
                self.speed = self.leg_distance * 6. * t * (1. - t) / TRAVEL_TIME;
                let moved = (self.distance - previous).rem_euclid(self.map.length);
                for car in self.train.iter_mut().chain(self.other_cars.iter_mut()) {
                    car.wheel_angle += moved / 0.4;
                }
                if self.travel >= TRAVEL_TIME {
                    self.distance = self.map.stations[self.stop].distance;
                    self.speed = 0.;
                    self.dwell = DWELL;
                }
            }
        }
        self.place();
        if self.aboard {
            if let Some(player) = player {
                let car = self.car(self.rider).unwrap();
                let q = Quat::from_rotation_z(car.yaw - old.1);
                player.carry(q, old.0, car.position);
            } else {
                self.aboard = false;
            }
        }
    }
    fn car_local(car: &Vehicle, position: Vec3) -> Vec3 {
        Quat::from_rotation_z(-car.yaw) * (position - car.position + Vec3::Z * car.ride_height())
    }
    fn local_car(&self, position: Vec3) -> Option<(usize, Vec3)> {
        if self.aboard {
            let car = self.car(self.rider)?;
            return Some((self.rider, Self::car_local(car, position)));
        }
        self.cars()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                a.position
                    .distance_squared(position)
                    .total_cmp(&b.position.distance_squared(position))
            })
            .map(|(i, c)| (i, Self::car_local(c, position)))
    }
    fn local(&self, position: Vec3) -> Option<Vec3> {
        self.local_car(position).map(|(_, p)| p)
    }
    fn open(&self) -> bool {
        self.speed < 0.01 && self.train.as_ref().is_some_and(|t| t.metro_doors > 0.95)
    }
    fn portal(x: f32) -> bool {
        [4.96, -3.04].iter().any(|x0| (x - x0).abs() < 0.43)
    }
    pub fn blocked(&self, position: Vec3) -> bool {
        let Some((index, p)) = self.local_car(position) else {
            return false;
        };
        let roof = self
            .car(index)
            .unwrap()
            .definition()
            .unwrap()
            .config
            .dimensions
            .height;
        if p.z < 0.5 || p.z > roof + 0.8 || p.x.abs() > 8.35 || p.y.abs() > 1.85 {
            return false;
        }
        if p.x.abs() < 7.5 && p.y.abs() < 1.1 && p.z + super::player::HALF_HEIGHT > roof - 0.08 {
            return true;
        }
        let inside = p.x.abs() < 7.35 && p.y.abs() < 0.62;
        let doorway = self.open() && Self::portal(p.x) && p.y >= 0. && p.y < 1.85;
        !(inside || doorway)
    }
    fn ground(&self, p: Vec3) -> Option<Ground> {
        let local = self.local(p)?;
        if (p.z - FLOOR_HEIGHT * METERS_PER_UNIT).abs() > 0.45 {
            return None;
        }
        let inside = local.x.abs() < 7.4 && local.y.abs() < 0.99;
        let portal = self.open() && Self::portal(local.x) && local.y >= 0. && local.y < 1.65;
        (inside || portal).then_some(Ground {
            point: p.truncate().extend(FLOOR_HEIGHT * METERS_PER_UNIT),
            gradient: [0.; 2],
        })
    }
    pub fn walk(
        &mut self,
        hermes: &Hermes,
        player: &mut Pedestrian,
        input: WalkControls,
        dt: f32,
        blocked: impl Fn(Vec3) -> bool,
    ) {
        if self.train.is_none() {
            player.step(hermes.city(), input, dt, blocked);
            return;
        }
        let previous = self.local(player.position);
        player.step_supported(
            hermes.city(),
            input,
            dt,
            |p| blocked(p) || self.blocked(p),
            |p| self.ground(p),
        );
        if let Some((index, p)) = self.local_car(player.position) {
            if (p.z - (1.025 + super::player::HALF_HEIGHT)).abs() > 0.5 {
                return;
            }
            if p.x.abs() < 7.35 && p.y.abs() < 1.2 && (self.aboard || self.open()) {
                self.aboard = true;
                self.rider = index;
            }
            if p.y > 1.6 && self.open() {
                self.aboard = false;
            }
            if previous.is_some_and(|p| p.y > 0.6 && p.y < 1.75 && p.z < 3. && Self::portal(p.x))
                && self.speed == 0.
                && self.dwell < 2.5
            {
                self.dwell = 2.5;
            }
        }
    }
    pub fn camera_eye(&self, anchor: Vec3, desired: Vec3) -> Vec3 {
        if self.train.is_none() || anchor.z >= 0.098 {
            return desired;
        }
        let blocked = |p: Vec3| {
            if self.aboard {
                if let Some(local) = self.local(p * 100.) {
                    return local.x.abs() > 7.5 || local.y.abs() > 1.05 || local.z > 3.9;
                }
            }
            for s in &self.map.stations {
                if s.center.distance(anchor.truncate()) > 0.6 {
                    continue;
                }
                let v = p.truncate() - s.center;
                let a = v.dot(s.forward);
                let b = v.dot(glam::Vec2::new(-s.forward.y, s.forward.x));
                if a > -0.505 && a < -0.28 && b > 0.043 && b < 0.077 {
                    return false;
                }
                if a.abs() < 0.325 {
                    return b < -0.035 || b > 0.115 || p.z > TRACK_HEIGHT + 0.066;
                }
            }
            false
        };
        for i in 1..=80 {
            let t = i as f32 / 80.;
            if blocked(anchor.lerp(desired, t)) {
                return anchor.lerp(desired, ((i as f32 - 2.) / 80.).max(0.));
            }
        }
        desired
    }
    pub fn append_lights(&self, lights: &mut Vec<LocalLight>, center: Vec3) {
        for station in &self.map.stations {
            if station.center.distance(center.truncate() / 100.) > 1.6 {
                continue;
            }
            for a in [-0.18, -0.09, 0., 0.09, 0.18] {
                let p = station.point(a, 0.055, TRACK_HEIGHT + 0.062);
                lights.push(LocalLight {
                    position_range: p.extend(0.17).to_array(),
                    direction_outer: [0., 0., -1., -1.],
                    color_intensity: [1., 0.88, 0.67, 28.],
                    beam: [-0.9, 8., 1., 1.],
                });
            }
        }
        // Regular tunnel fixtures remain on even at noon, selected near the player.
        for p in self
            .map
            .tunnel_fixtures()
            .filter(|p| p.distance(center / 100.) < 1.0)
        {
            lights.push(LocalLight {
                position_range: p.extend(0.15).to_array(),
                direction_outer: [0., 0., -1., -1.],
                color_intensity: [0.75, 0.85, 1., 18.],
                beam: [-0.9, 8., 1., 1.],
            });
        }
        for t in self.cars() {
            if self.aboard || t.position.distance(center) < 40. {
                for x in [-5., 0., 5.] {
                    let p =
                        (t.position + Quat::from_rotation_z(t.yaw) * Vec3::new(x, 0., 2.3)) / 100.;
                    lights.push(LocalLight {
                        position_range: p.extend(0.07).to_array(),
                        direction_outer: [0., 0., -1., -1.],
                        color_intensity: [1., 0.93, 0.78, 12.],
                        beam: [-0.9, 4., 1., 1.],
                    });
                }
            }
        }
    }
}
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
impl super::DynamicWorld {
    pub fn preview_metro_station(
        &mut self,
        hermes: &Hermes,
        id: crate::hermes::metro::StationId,
    ) -> bool {
        let Some(index) = self.metro.map.stations.iter().position(|s| s.id == id) else {
            return false;
        };
        self.metro.stop = index;
        self.metro.distance = self.metro.map.stations[index].distance;
        self.metro.speed = 0.;
        self.metro.dwell = DWELL - 2.;
        self.metro.train.as_mut().unwrap().metro_doors = 1.;
        self.metro.place();
        let s = &self.metro.map.stations[index];
        let position =
            s.point(0.05, 0.06, FLOOR_HEIGHT) * 100. + Vec3::Z * super::player::HALF_HEIGHT;
        self.pedestrian = Some(Pedestrian::new(
            hermes.city(),
            position,
            s.forward.y.atan2(s.forward.x),
        ));
        true
    }
}
