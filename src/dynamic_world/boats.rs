//! Small-craft simulation in game metres; the GLB origin is the design waterline.
use super::{catalog, Controls, DynamicWorld, SpawnPoint, Vehicle, METERS_PER_UNIT};
use crate::hermes::Hermes;
use glam::{Vec2, Vec3};
use opencity::runtime::{City, SurfaceKind};

pub(super) fn water(city: &City, xy: Vec2) -> Option<f32> {
    let level = city.elevation.as_ref()?.spec.water_level?;
    let ground = city.surface_at((xy / METERS_PER_UNIT).to_array(), level + 0.008)?;
    (ground.kind == SurfaceKind::Water).then_some(level * METERS_PER_UNIT)
}

pub(super) struct Patrol {
    pub vehicle: usize,
    center: Vec2,
    crew: usize,
}
const RADIUS: f32 = 28.;
impl Patrol {
    pub fn controls(&self, boat: &Vehicle) -> Controls {
        let radial = boat.position.truncate() - self.center;
        let course = radial.y.atan2(radial.x)
            + std::f32::consts::FRAC_PI_2
            + ((radial.length() - RADIUS) / 10.).atan();
        let error = course - boat.yaw;
        Controls {
            throttle: (2.2 - boat.velocity.length()).clamp(0.08, 0.8),
            steering: (error.sin().atan2(error.cos()) * 2. + boat.length() / RADIUS).clamp(-1., 1.),
            ..Controls::default()
        }
    }
}

impl DynamicWorld {
    pub(super) fn populate_boats(&mut self, hermes: &Hermes) {
        let city = hermes.city();
        if city
            .elevation
            .as_ref()
            .and_then(|e| e.spec.water_level)
            .is_none()
        {
            return;
        }
        let Some(start) = hermes.default_start() else {
            return;
        };
        let origin = Vec2::from_array(start.point) * METERS_PER_UNIT;
        // Follow three bearings from town to the first shore. Moor just offshore and
        // check the entire patrol circle before assigning a skipper to a second boat.
        for (crew, (id, angle)) in [
            ("vehicle-boat-voile", -1.57_f32),
            ("vehicle-boat-cabine", -2.35),
            ("vehicle-boat-runabout", -0.85),
        ]
        .into_iter()
        .enumerate()
        {
            let out = Vec2::from_angle(angle);
            let model = catalog::definitions()
                .iter()
                .position(|d| d.config.research_id == id)
                .expect("boat model");
            let Some(edge) = (1..=1200)
                .map(|i| origin + out * (i as f32 * 5.))
                .find(|p| water(city, *p).is_some())
            else {
                continue;
            };
            let mooring = edge + out * 3.;
            let spawn = |p: Vec2, yaw: f32| {
                let start = SpawnPoint {
                    point: (p / METERS_PER_UNIT).to_array(),
                    heading: Some(yaw),
                };
                let mut boat = Vehicle::spawn_supported(city, start, true).ok()?;
                boat.set_catalog_model(Some(model));
                boat.position.z = water(city, p)? + boat.ride_height();
                boat.surface = SurfaceKind::Water;
                Some(boat)
            };
            if let Some(boat) = spawn(mooring, angle + std::f32::consts::FRAC_PI_2) {
                self.vehicles.push(boat);
            }
            for distance in (55..=355).step_by(30) {
                let center = edge + out * (distance as f32);
                if !(0..32).all(|i| {
                    water(
                        city,
                        center
                            + Vec2::from_angle(i as f32 * std::f32::consts::TAU / 32.)
                                * (RADIUS + 4.),
                    )
                    .is_some()
                }) {
                    continue;
                }
                if let Some(boat) = spawn(center + Vec2::X * RADIUS, std::f32::consts::FRAC_PI_2) {
                    self.boat_routes.push(Patrol {
                        vehicle: self.vehicles.len(),
                        center,
                        crew,
                    });
                    self.vehicles.push(boat);
                    break;
                }
            }
        }
    }
    pub(super) fn skippers(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.boat_routes.iter().map(|r| (r.vehicle, r.crew))
    }
}

pub(super) fn step(v: &mut Vehicle, city: &City, input: Controls, dt: f32) {
    let xy = v.position.truncate();
    let Some(level) = water(city, xy) else {
        v.velocity = Vec3::ZERO;
        return;
    };
    let physics = &v.definition().expect("boat").config.physics;
    let forward = Vec2::from_angle(v.yaw);
    let lateral = forward.perp();
    let speed = v.velocity.truncate().dot(forward);
    let thrust = input.throttle.clamp(-0.45, 1.) * physics.acceleration;
    let drag = physics.acceleration * speed * speed.abs() / physics.max_speed.powi(2);
    let mut next_speed = speed + (thrust - drag - 0.07 * speed) * dt;
    if input.brake || input.handbrake {
        next_speed -= next_speed.clamp(-3.5 * dt, 3.5 * dt);
    }
    v.steering += (input.steering * physics.max_steer_angle - v.steering) * (1. - (-8. * dt).exp());
    let target_rate = next_speed * v.steering.tan() / physics.wheelbase;
    let inertia = (v.length().powi(2) + v.width().powi(2)) / 12.;
    v.angular_momentum.z +=
        (inertia * target_rate - v.angular_momentum.z) * (1. - (-2.8 * dt).exp());
    v.yaw += v.angular_momentum.z / inertia * dt;
    let velocity =
        forward * next_speed + lateral * v.velocity.truncate().dot(lateral) * (-1.7 * dt).exp();
    let next = xy + velocity * dt;
    let heading = Vec2::from_angle(v.yaw);
    let lead = next + heading * (v.length() * 0.46 * next_speed.signum());
    if water(city, next).is_some() && water(city, lead).is_some() {
        v.position.x = next.x;
        v.position.y = next.y;
        v.velocity = velocity.extend(0.);
    } else {
        v.velocity = Vec3::ZERO;
    }
    v.float_phase = (v.float_phase + dt * 1.6) % std::f32::consts::TAU;
    let phase = v.float_phase + xy.dot(Vec2::new(0.23, 0.14));
    v.pitch += (0.025 * phase.cos() + 0.007 * next_speed - v.pitch) * (1. - (-3. * dt).exp());
    v.roll +=
        (-0.04 * next_speed * target_rate + 0.025 * phase.sin() - v.roll) * (1. - (-3. * dt).exp());
    v.position.z = level + v.ride_height() + 0.045 * phase.sin();
    v.surface = SurfaceKind::Water;
    v.grounded = true;
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Wake {
    origin_direction: [f32; 4],
    // Half-width scale, lateral spread per metre, trail length in metres, opacity.
    shape: [f32; 4],
}

impl Wake {
    fn boat(stern: Vec2, velocity: Vec2, width: f32) -> Self {
        let speed = velocity.length();
        let direction = velocity / speed;
        let fast = (speed / 12.).min(1.);
        Self {
            origin_direction: [stern.x, stern.y, direction.x, direction.y],
            shape: [
                width * 0.4 * speed.min(2.),
                0.5 - 0.41 * fast,
                2.8 + 3.2 * fast,
                1.,
            ],
        }
    }
}

/// Short-lived foam packets stay at their emission positions while the vessel turns.
#[derive(Default)]
pub(super) struct Wakes {
    packets: std::collections::VecDeque<(Wake, f32)>,
    interval: f32,
}
impl DynamicWorld {
    pub(super) fn step_wakes(&mut self, dt: f32) {
        let center = self.player_position();
        let wakes = &mut self.wakes;
        for (_, age) in &mut wakes.packets {
            *age += dt;
        }
        while wakes.packets.front().is_some_and(|(_, age)| *age >= 3.) {
            wakes.packets.pop_front();
        }
        wakes.interval += dt;
        if wakes.interval < 0.20 {
            return;
        }
        wakes.interval %= 0.20;
        for v in &self.vehicles {
            let speed = v.velocity.truncate().length();
            if v.is_boat() && speed > 0.35 && center.distance_squared(v.position) < 180_f32.powi(2)
            {
                let stern = v.position.truncate() - Vec2::from_angle(v.yaw) * v.length() * 0.42;
                wakes
                    .packets
                    .push_back((Wake::boat(stern, v.velocity.truncate(), v.width()), 0.));
            }
        }
        if let Some(p) = self
            .pedestrian
            .as_ref()
            .filter(|p| p.swimming && p.swim_speed() > 0.22)
        {
            let feet = p.position.truncate() - Vec2::from_angle(p.yaw) * 0.7;
            wakes.packets.push_back((
                Wake {
                    origin_direction: [feet.x, feet.y, p.yaw.cos(), p.yaw.sin()],
                    shape: [-0.32, 0.09, 0.75, 1.],
                },
                0.,
            ));
        }
        while wakes.packets.len() > 32 {
            wakes.packets.pop_front();
        }
    }
    pub fn water_wakes(&self) -> [Wake; 32] {
        let mut data = [Wake::default(); 32];
        for (slot, (point, age)) in data.iter_mut().zip(&self.wakes.packets) {
            *slot = *point;
            slot.shape[3] *= 1. - age / 3.;
        }
        data
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    #[test]
    fn fast_wakes_follow_the_course_with_longer_narrower_foam_lines() {
        let course = Vec2::from_angle(1.1);
        let slow = Wake::boat(Vec2::ZERO, course, 2.);
        let fast = Wake::boat(Vec2::ZERO, course * 12., 2.);
        assert!(fast.shape[1] < slow.shape[1]);
        assert!(fast.shape[2] > slow.shape[2]);
        assert!(
            Vec2::new(fast.origin_direction[2], fast.origin_direction[3]).distance(course) < 1e-6
        );
    }

    #[test]
    fn moving_submerged_car_exits_into_swimming() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/mtl/map.road");
        let hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let mut world = DynamicWorld::new(&hermes);
        let model = world.player().definition.unwrap();
        let moored = world.vehicles.iter().position(|v| v.is_boat()).unwrap();
        world.player_vehicle = super::super::VehicleId(moored);
        assert!(world.switch_vehicle(model));
        let car = &mut world.vehicles[moored];
        assert!(!car.is_boat());
        let level = water(hermes.city(), car.position.truncate()).unwrap();
        car.position.z = level - 2.;
        car.velocity = Vec3::X * 8.;
        assert!(world.toggle_vehicle(&hermes));
        let swimmer = world.pedestrian.as_ref().unwrap();
        assert!(swimmer.swimming);
        assert_eq!(swimmer.position.z, level);
        assert!(
            swimmer
                .position
                .truncate()
                .distance(world.player().position.truncate())
                > world.player().width() * 0.5
        );
    }

    #[test]
    fn montreal_fleet_can_be_boarded_driven_and_left_in_water() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/mtl/map.road");
        let hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let mut world = DynamicWorld::new(&hermes);
        assert_eq!(world.boat_routes.len(), 3);
        let boats = world
            .vehicles
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_boat())
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        assert_eq!(boats.len(), 6);
        let moored = boats[0];
        let city = hermes.city();
        world.pedestrian =
            super::super::player::Pedestrian::beside_if(city, &world.vehicles[moored], |_| false);
        assert!(world.pedestrian.as_ref().unwrap().swimming);
        assert!(world.toggle_vehicle(&hermes));
        assert_eq!(world.player_vehicle.0, moored);
        let initial = world.player().position;
        let npc = world.boat_routes[0].vehicle;
        let npc_start = world.vehicles[npc].position;
        for _ in 0..120 {
            world.step(
                &hermes,
                Controls {
                    throttle: 0.8,
                    ..Default::default()
                },
                Default::default(),
                1. / 60.,
            );
        }
        assert!(world.player().position.distance(initial) > 0.5);
        assert!(world.vehicles[npc].position.distance(npc_start) > 0.5);
        assert!(world.toggle_vehicle(&hermes));
        let before = world.player_position();
        for _ in 0..90 {
            world.step(
                &hermes,
                Default::default(),
                super::super::WalkControls {
                    forward: 1.,
                    ..Default::default()
                },
                1. / 60.,
            );
        }
        assert!(world.player_position().distance(before) > 0.4);
        assert!(world.water_wakes().iter().any(|p| p.shape[0] < 0.));
        assert!(world
            .pedestrian
            .as_ref()
            .unwrap()
            .instance(world.season)
            .pose
            .unwrap()
            .iter()
            .all(|m| m.is_finite()));
    }
}
