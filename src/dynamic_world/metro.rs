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
const DWELL: f32 = 18.;
const DOOR_TIME: f32 = 1.2;
const MAX_SPEED: f32 = 18.;
const ACCEL: f32 = 1.1;
const BRAKE: f32 = 1.3;
pub(super) struct Metro {
    pub map: MetroMap,
    pub train: Option<Vehicle>,
    distance: f32,
    speed: f32,
    stop: usize,
    dwell: f32,
    pub aboard: bool,
}
impl Metro {
    pub fn new(hermes: &Hermes) -> Self {
        let map = MetroMap::from_city(hermes.city());
        let mut service = Self {
            map,
            train: None,
            distance: 0.,
            speed: 0.,
            stop: 0,
            dwell: DWELL,
            aboard: false,
        };
        if service.map.stations.is_empty() || service.map.samples.len() < 2 {
            return service;
        }
        if let Some(id) = catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-r01")
        {
            let mut train = Vehicle::spawn(hermes.city());
            train.set_catalog_model(Some(id));
            service.distance = service.map.stations[0].distance;
            service.train = Some(train);
            service.place();
        }
        service
    }
    fn place(&mut self) {
        if let Some(train) = &mut self.train {
            let (p, yaw) = self.map.sample(self.distance);
            train.position = p * METERS_PER_UNIT + Vec3::Z * train.ride_height();
            train.yaw = yaw;
            train.pitch = 0.;
            train.roll = 0.;
            train.steering = 0.;
            train.body_lean = glam::Vec2::ZERO;
            train.velocity = Vec3::new(yaw.cos(), yaw.sin(), 0.) * self.speed;
            train.sound_running = false;
        }
    }
    pub fn step(&mut self, dt: f32, player: Option<&mut Pedestrian>) {
        if self.train.is_none() {
            return;
        }
        let old = self.train.as_ref().map(|t| (t.position, t.yaw)).unwrap();
        let mut time = dt.min(0.25);
        while time > 0. {
            let h = time.min(1. / 120.);
            time -= h;
            if self.dwell > 0. {
                self.dwell = (self.dwell - h).max(0.);
                let elapsed = DWELL - self.dwell;
                let doors = (elapsed / DOOR_TIME)
                    .min(self.dwell / DOOR_TIME)
                    .clamp(0., 1.);
                self.train.as_mut().unwrap().metro_doors = doors;
                if self.dwell == 0. {
                    self.stop = (self.stop + 1) % self.map.stations.len();
                }
            } else {
                let target = self.map.stations[self.stop].distance;
                let remaining = (target - self.distance).rem_euclid(self.map.length);
                let desired = (2. * BRAKE * remaining).sqrt().min(MAX_SPEED);
                self.speed += (desired - self.speed).clamp(-BRAKE * h, ACCEL * h);
                let travel = self.speed * h;
                if remaining <= travel.max(0.002) {
                    self.distance = target;
                    self.speed = 0.;
                    self.dwell = DWELL;
                } else {
                    self.distance = (self.distance + travel).rem_euclid(self.map.length);
                }
                self.train.as_mut().unwrap().wheel_angle += travel / 0.4;
            }
        }
        self.place();
        if self.aboard {
            if let Some(player) = player {
                let train = self.train.as_ref().unwrap();
                let rotation = Quat::from_rotation_z(train.yaw - old.1);
                player.carry(rotation, old.0, train.position);
            } else {
                self.aboard = false;
            }
        }
    }
    fn local(&self, position: Vec3) -> Option<Vec3> {
        let t = self.train.as_ref()?;
        Some(Quat::from_rotation_z(-t.yaw) * (position - t.position + Vec3::Z * t.ride_height()))
    }
    fn open(&self) -> bool {
        self.speed < 0.01 && self.train.as_ref().is_some_and(|t| t.metro_doors > 0.95)
    }
    fn portal(x: f32) -> bool {
        [4.96, -3.04].iter().any(|d| (x - d).abs() < 0.43)
    }
    pub fn blocked(&self, position: Vec3) -> bool {
        let Some(p) = self.local(position) else {
            return false;
        };
        if p.z < 0.5 || p.z > 3.5 || p.x.abs() > 8.35 || p.y.abs() > 1.85 {
            return false;
        }
        // Leave only the platform-side door portals open; closed train walls remain solid.
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
        if !(inside || portal) {
            return None;
        }
        Some(Ground {
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
        if let Some(p) = self.local(player.position) {
            if (p.z - (1.025 + super::player::HALF_HEIGHT)).abs() > 0.5 {
                return;
            }
            if p.x.abs() < 7.35 && p.y.abs() < 1.2 && (self.aboard || self.open()) {
                self.aboard = true;
            }
            if p.y > 1.6 && self.open() {
                self.aboard = false;
            }
            // A person standing in a closing doorway holds the dwell until clear.
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
                    return local.x.abs() > 7.5 || local.y.abs() > 1.05 || local.z > 3.05;
                }
            }
            for s in &self.map.stations {
                if s.center.distance(anchor.truncate()) > 0.5 {
                    continue;
                }
                let v = p.truncate() - s.center;
                let a = v.dot(s.forward);
                let b = v.dot(glam::Vec2::new(-s.forward.y, s.forward.x));
                let stair = a > -0.405 && a < -0.18 && b > 0.043 && b < 0.077;
                if stair {
                    return false;
                }
                if a.abs() < 0.225 {
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
        if let Some(t) = &self.train {
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
        self.metro.dwell = 10.;
        self.metro.place();
        self.metro.train.as_mut().unwrap().metro_doors = 1.;
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
