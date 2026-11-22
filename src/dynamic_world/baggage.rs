//! Hitch positions and axle tracking for the airport baggage train, in metres.
use super::{DynamicWorld, Vehicle, METERS_PER_UNIT};
use glam::{Mat4, Quat, Vec3};

pub(super) const MODEL_ID: &str = "vehicle-airport-baggage-tractor";
const CAPACITY: usize = 4;
const PIN_X: f32 = -1.79;
const DRAWBAR: f32 = 2.32;
const CART_LENGTH_TO_PIN: f32 = DRAWBAR + 1.55;

pub(super) fn hitch(ride_height: f32) -> Vec3 {
    Vec3::new(PIN_X, 0., 0.52 - ride_height)
}

#[derive(Clone, Copy, Default)]
pub(super) struct Link {
    // Heading relative to the tractor, wheel rotation in radians.
    yaw: f32,
    spin: f32,
}

#[derive(Clone, Copy)]
pub(super) struct Cart {
    pub eye: Vec3,
    pub rotation: Quat,
    pub spin: f32,
}

impl Cart {
    pub fn axle(self) -> Vec3 {
        self.eye - self.rotation * Vec3::X * DRAWBAR
    }
    fn pin(self) -> Vec3 {
        self.eye - self.rotation * Vec3::X * CART_LENGTH_TO_PIN
    }
    pub fn blocks(self, point: Vec3, radius: f32) -> bool {
        let local = self.rotation.conjugate() * (point - self.axle());
        local.z > -0.52
            && local.z < 1.55
            && local.x.abs() < 1.46 + radius
            && local.y.abs() < 0.81 + radius
    }
}

impl Vehicle {
    pub(super) fn is_baggage_tug(&self) -> bool {
        self.definition()
            .is_some_and(|d| d.config.research_id == MODEL_ID)
    }

    pub(super) fn load_airport_train(&mut self) {
        self.baggage_links = vec![Link::default(); CAPACITY];
    }

    pub(super) fn contact_reach(&self) -> f32 {
        if self.baggage_links.is_empty() {
            self.length().hypot(self.width()) * 0.5
        } else {
            -PIN_X + self.baggage_links.len() as f32 * CART_LENGTH_TO_PIN + 0.2
        }
    }

    fn tow_pin(&self) -> Vec3 {
        self.position + self.orientation() * hitch(self.ride_height())
    }

    pub(super) fn carts(&self) -> impl Iterator<Item = Cart> + Clone + '_ {
        let frame = self.orientation();
        self.baggage_links
            .iter()
            .scan(self.tow_pin(), move |pin, link| {
                let cart = Cart {
                    eye: *pin,
                    rotation: frame * Quat::from_rotation_z(link.yaw),
                    spin: link.spin,
                };
                *pin = cart.pin();
                Some(cart)
            })
    }

    pub(super) fn previous_axles(&self) -> [Vec3; CAPACITY] {
        let mut axles = [Vec3::ZERO; CAPACITY];
        for (dst, cart) in axles.iter_mut().zip(self.carts()) {
            *dst = cart.axle();
        }
        axles
    }

    pub(super) fn follow_hitches(&mut self, axles: [Vec3; CAPACITY]) {
        let frame = self.orientation();
        let mut eye = self.tow_pin();
        let mut parent_yaw = 0.;
        for (link, previous) in self.baggage_links.iter_mut().zip(axles) {
            let pull = frame.conjugate() * (eye - previous);
            let angle = pull.y.atan2(pull.x) - parent_yaw;
            link.yaw = parent_yaw + angle.sin().atan2(angle.cos()).clamp(-1.3, 1.3);
            let forward = frame * Quat::from_rotation_z(link.yaw) * Vec3::X;
            let axle = eye - forward * DRAWBAR;
            link.spin += (axle - previous).dot(forward) / 0.28;
            eye -= forward * CART_LENGTH_TO_PIN;
            parent_yaw = link.yaw;
        }
    }
}

impl DynamicWorld {
    fn can_operate_hitch(&self) -> bool {
        self.baggage_shift.is_none()
            && self.is_driving()
            && self.health > 0
            && self.crash.is_none()
            && self.entry.is_none()
            && self.player().is_baggage_tug()
            && self.player().grounded
            && self.player().velocity.length_squared() < 0.25
    }

    fn next_baggage_cart(&self) -> Option<usize> {
        let tug = self.player();
        if tug.baggage_links.len() >= CAPACITY {
            return None;
        }
        let (pin, rotation) = tug
            .carts()
            .last()
            .map_or((tug.tow_pin(), tug.orientation()), |c| {
                (c.pin(), c.rotation)
            });
        self.parked_carts
            .iter()
            .enumerate()
            .filter_map(|(i, c)| {
                let delta = rotation.conjugate() * (c.eye - pin);
                let aligned = (rotation * Vec3::X).dot(c.rotation * Vec3::X) > 0.82;
                (aligned && delta.x.abs() <= 0.85 && delta.y.abs() <= 0.4 && delta.z.abs() <= 0.25)
                    .then_some((i, delta.length_squared()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    /// Parked tug HUD: attached count, attach available, detach available.
    pub fn baggage_hitch_status(&self) -> Option<(usize, bool, bool)> {
        if !self.can_operate_hitch() {
            return None;
        }
        let count = self.player().baggage_links.len();
        Some((count, self.next_baggage_cart().is_some(), count > 0))
    }

    pub fn detach_baggage_cart(&mut self) -> bool {
        if !self.can_operate_hitch() {
            return false;
        }
        let Some(cart) = self.player().carts().last() else {
            return false;
        };
        self.vehicles[self.player_vehicle.0].baggage_links.pop();
        self.parked_carts.push(cart);
        true
    }

    pub fn attach_baggage_cart(&mut self) -> bool {
        if !self.can_operate_hitch() {
            return false;
        }
        let Some(i) = self.next_baggage_cart() else {
            return false;
        };
        let cart = self.parked_carts.swap_remove(i);
        let tug = &mut self.vehicles[self.player_vehicle.0];
        let local = tug.orientation().conjugate() * cart.rotation * Vec3::X;
        tug.baggage_links.push(Link {
            yaw: local.y.atan2(local.x),
            spin: cart.spin,
        });
        true
    }

    pub fn baggage_cart_instances(&self) -> impl Iterator<Item = (usize, usize, Mat4, bool)> + '_ {
        let center = self.player_position();
        let radius = crate::performance::draw_radius(super::parked::RENDER_DISTANCE);
        let moving = self
            .vehicles
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.baggage_links.is_empty())
            .flat_map(move |(i, v)| {
                v.carts().enumerate().map(move |(index, cart)| {
                    (
                        cart,
                        i == self.player_vehicle.0,
                        self.baggage_cargo_visible(i, index),
                    )
                })
            });
        let carts = moving.chain(
            self.parked_carts
                .iter()
                .copied()
                .map(|cart| (cart, false, self.baggage_shift.is_none())),
        );
        let (id, definition) = super::catalog::definitions()
            .iter()
            .enumerate()
            .find(|(_, d)| d.config.research_id == MODEL_ID)
            .expect("bundled baggage tractor");
        carts
            .filter(move |(c, _, _)| c.axle().distance_squared(center) < radius * radius)
            .flat_map(move |(c, player, cargo)| {
                let world = Mat4::from_scale_rotation_translation(
                    Vec3::splat(1. / METERS_PER_UNIT),
                    c.rotation,
                    c.eye / METERS_PER_UNIT,
                );
                definition
                    .cart_parts(c.spin, cargo)
                    .map(move |(i, local)| (id, i, world * local, player))
            })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{Controls, DynamicWorld, SpawnPoint, WalkControls, METERS_PER_UNIT};
    use crate::hermes::Hermes;
    use glam::Vec2;

    #[test]
    fn enter_the_airport_tug_drive_a_turn_and_reset_with_the_cart() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let map = Hermes::load_with_game_data(&path, None).unwrap();
        let surface = map
            .city()
            .surface_at([-4.335, -0.34], f32::INFINITY)
            .unwrap();
        assert_eq!(surface.kind, opencity::runtime::SurfaceKind::Sidewalk);
        assert!(!map
            .city()
            .building_collision([-4.335, -0.369], 0.009, 0.123, 0.146));
        assert!(map
            .city()
            .building_collision([-4.42, -0.369], 0.009, 0.123, 0.146));
        let start = SpawnPoint {
            point: [-4.36, -0.53],
            heading: Some(0.),
        };
        let mut world = DynamicWorld::with_start(&map, Some(start)).unwrap();
        let tug = world.vehicles.iter().find(|v| v.is_baggage_tug()).unwrap();
        let parked = tug.position;
        let point = (parked.truncate() + Vec2::Y * 1.7) / METERS_PER_UNIT;
        world.pedestrian = Some(
            super::super::player::Pedestrian::at(
                map.city(),
                SpawnPoint {
                    point: point.to_array(),
                    heading: Some(0.),
                },
            )
            .unwrap(),
        );
        assert!(world.toggle_vehicle(&map));
        assert!(world.player().is_baggage_tug());
        assert_eq!(world.baggage_hitch_status(), Some((4, false, true)));
        assert_eq!(world.baggage_cart_instances().count(), 24);
        assert!(!world.attach_baggage_cart());
        for remaining in (0..4).rev() {
            assert!(world.detach_baggage_cart());
            assert_eq!(world.player().baggage_links.len(), remaining);
        }
        assert!(!world.detach_baggage_cart());
        let slot = world.player_vehicle.0;
        world.vehicles[slot].position -= Vec3::X * 2.;
        assert!(!world.attach_baggage_cart());
        assert_eq!(world.baggage_cart_instances().filter(|c| !c.3).count(), 24);
        let reverse = Controls {
            throttle: -1.,
            ..Default::default()
        };
        for _ in 0..240 {
            world.step(&map, reverse, WalkControls::default(), 1. / 60.);
            if world.player().position.distance(parked) < 0.65 {
                break;
            }
        }
        assert!(world.player().position.distance(parked) < 0.65);
        assert!(!world.attach_baggage_cart());
        for _ in 0..60 {
            world.step(
                &map,
                Controls {
                    brake: true,
                    handbrake: true,
                    ..Default::default()
                },
                WalkControls::default(),
                1. / 60.,
            );
        }
        for _ in 0..4 {
            assert!(world.attach_baggage_cart());
        }
        assert!(world.parked_carts.is_empty());
        let mut swung = false;
        for i in 0..180 {
            world.step(
                &map,
                Controls {
                    throttle: 1.,
                    steering: if i > 75 { 0.65 } else { 0. },
                    ..Default::default()
                },
                WalkControls::default(),
                1. / 60.,
            );
            swung |= world.player().baggage_links[0].yaw.abs() > 0.06;
        }
        assert!(world.player().position.distance(parked) > 4.);
        assert!(swung);
        for (front, back) in world.player().carts().zip(world.player().carts().skip(1)) {
            assert!(front.pin().distance(back.eye) < 0.001);
        }
        assert!(!world.detach_baggage_cart());
        world.vehicles[slot].velocity = Vec3::ZERO;
        assert!(world.detach_baggage_cart());
        let dropped = world.parked_carts[0].eye;
        world.reset_player(&map);
        assert!(world.player().position.distance(parked) < 0.002);
        assert_eq!(world.player().baggage_links.len(), 3);
        assert_eq!(world.player().baggage_links[0].yaw, 0.);
        assert_eq!(world.parked_carts[0].eye, dropped);
    }
}
