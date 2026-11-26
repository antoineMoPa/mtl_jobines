//! Third-person picking and shot obstruction in world metres.
use super::{vehicle::tire::Wheel, DynamicWorld, METERS_PER_UNIT};
use glam::Vec3;
use opencity::runtime::City;
use rapier3d::na as nalgebra;
use rapier3d::{
    na::{point, vector},
    prelude::{Cuboid, Ray, RayCast},
};

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
pub enum ShotSurface {
    Wood,
    Concrete,
    Brick([u8; 3]),
    Glass,
    Earth,
    Asphalt,
    Metal,
    Rubber,
    Water,
    Spectral,
    Wildlife,
}
impl ShotSurface {
    pub(super) fn facade(material: opencity::BuildingMaterial) -> Self {
        use opencity::BuildingMaterial::*;
        match material {
            Concrete | Limestone => Self::Concrete,
            Glass => Self::Glass,
            Brick | RedBrick | BrownBrick | BuffBrick | PaleBrick => {
                Self::Brick(material.wall_rgb())
            }
        }
    }
    pub fn sound(self) -> crate::audio::Material {
        use crate::audio::Material;
        match self {
            Self::Wood => Material::Timber,
            Self::Concrete | Self::Brick(_) => Material::Hard,
            Self::Glass => Material::Glass,
            Self::Metal => Material::Lamp,
            Self::Asphalt => Material::Asphalt,
            Self::Rubber => Material::Rubber,
            Self::Earth | Self::Water | Self::Spectral | Self::Wildlife => Material::Body,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ShotHit {
    pub point: Vec3,
    pub normal: Vec3,
    pub surface: ShotSurface,
    pub stationary: bool,
}
#[derive(Clone, Copy)]
enum Target {
    Fixed,
    Car(usize),
    Tire(usize, Wheel),
    Prop(usize),
    Zombie(u32),
    Wildlife(usize),
}
struct Contact {
    hit: ShotHit,
    target: Target,
}

/// Intersect walking support surfaces along a unit ray in world metres.
pub fn ground_shot_contact(city: &City, from: Vec3, dir: Vec3, limit: f32) -> Option<ShotHit> {
    for i in 0..=(limit / 0.25).ceil() as usize {
        let t = (i as f32 * 0.25).min(limit);
        let p = from + dir * t;
        let Some(s) = city.surface_at(
            [p.x / METERS_PER_UNIT, p.y / METERS_PER_UNIT],
            (p.z + 0.26) / METERS_PER_UNIT,
        ) else {
            continue;
        };
        let height = s.height * METERS_PER_UNIT;
        let n = Vec3::new(-s.gradient[0], -s.gradient[1], 1.);
        let denom = dir.dot(n);
        if p.z > height || denom >= -0.00001 {
            continue;
        }
        let distance = t + (height - p.z) / denom;
        if !(0.0..=limit).contains(&distance) {
            continue;
        }
        let point = from + dir * distance;
        let Some(face) = city.surface_at(
            [point.x / METERS_PER_UNIT, point.y / METERS_PER_UNIT],
            (point.z + 0.01) / METERS_PER_UNIT,
        ) else {
            continue;
        };
        if (face.height * METERS_PER_UNIT - point.z).abs() > 0.01 {
            continue;
        }
        use opencity::runtime::SurfaceKind;
        let surface = match face.kind {
            SurfaceKind::Grass | SurfaceKind::Sand => ShotSurface::Earth,
            SurfaceKind::Asphalt | SurfaceKind::Marking => ShotSurface::Asphalt,
            SurfaceKind::Water => ShotSurface::Water,
            _ => ShotSurface::Concrete,
        };
        return Some(ShotHit {
            point,
            normal: n / n.length(),
            surface,
            stationary: surface != ShotSurface::Water,
        });
    }
    None
}

impl DynamicWorld {
    pub fn can_equip_weapon(&self) -> bool {
        if self.is_wearing_skis() {
            return false;
        }
        self.cat_grip().is_none()
            && self.health > 0
            && self.pedestrian.as_ref().is_some_and(|p| !p.swimming)
            && !self.metro.aboard
            && !self.riding_chairlift()
    }

    pub(super) fn first_zombie_on_segment(&self, city: &City, from: Vec3, to: Vec3) -> Option<u32> {
        match self.weapon_hit(city, from, to)?.target {
            Target::Zombie(id) => Some(id),
            _ => None,
        }
    }

    pub fn aim_weapon(&mut self, target: Option<Vec3>, two_handed: bool) {
        let cat = self.cat_grip();
        if let Some(player) = self.pedestrian.as_mut() {
            if let Some((position, crouch)) = cat {
                player.cradle_cat(self.season, position, crouch);
            } else {
                player.aim_weapon(self.season, target, two_handed);
            }
        }
    }

    pub fn weapon_hand(&self) -> Option<Vec3> {
        let instance = self.pedestrian.as_ref()?.instance(self.season);
        Self::right_hand(instance)
    }

    pub(super) fn officer_gunshot(&mut self, position: Vec3) {
        crate::audio::Impact {
            position,
            speed: 10.,
            material: crate::audio::Material::Gunshot,
            ..Default::default()
        }
        .publish(&mut self.sound_impacts, &mut self.sound_serial);
    }

    pub(super) fn clear_officer_shot(&self, city: &City, hand: Vec3, target: Vec3) -> bool {
        match self.weapon_hit(city, hand, target) {
            None => true,
            Some(hit) => {
                matches!(hit.target, Target::Car(id) | Target::Tire(id, _) if self.is_driving() && id == self.player_vehicle.0)
            }
        }
    }

    pub(super) fn right_hand(instance: super::crowd::Instance<'_>) -> Option<Vec3> {
        let model = &super::pedestrian_catalog::definitions()[instance.model];
        let joint = model.joint("Hand_R");
        let wrist =
            instance.transform * instance.pose?[joint] * model.inverse_bind[joint].inverse();
        Some(wrist.w_axis.truncate() * METERS_PER_UNIT)
    }

    fn weapon_hit(&self, city: &City, from: Vec3, to: Vec3) -> Option<Contact> {
        let delta = to - from;
        let length = delta.length();
        if length < 0.0001 {
            return None;
        }
        let dir = delta / length;
        let mut limit = length;
        let mut hit = None;
        if let Some((t, normal, surface)) = self.buildings.ray_face(city, from, dir, length) {
            limit = t;
            hit = Some(Contact {
                hit: ShotHit {
                    point: from + dir * t,
                    normal,
                    surface,
                    stationary: true,
                },
                target: Target::Fixed,
            });
        }
        for (index, car) in self.vehicles.iter().enumerate() {
            if self.traffic.inactive(index) || self.cyclists.inactive(index) {
                continue;
            }
            let reach = car.length() + car.height();
            if from.distance_squared(car.position) > (limit + reach).powi(2) {
                continue;
            }
            let pose = car.orientation();
            let o = pose.conjugate() * (from - car.position);
            let d = pose.conjugate() * dir;
            // Reject off-axis vehicles before visiting any articulated mesh.
            if (o + d * (-o.dot(d)).clamp(0., limit)).length_squared() > reach * reach {
                continue;
            }
            let contact = if let Some(model) = car.definition().filter(|m| m.road_wheels.is_some())
            {
                model.road_shot_hit(car, o, d, limit)
            } else {
                let ray = Ray::new(point![o.x, o.y, o.z], vector![d.x, d.y, d.z]);
                let bounds = Cuboid::new(vector![car.length(), car.width(), car.height()] * 0.5);
                bounds
                    .cast_local_ray_and_get_normal(&ray, limit, true)
                    .map(|v| {
                        (
                            v.time_of_impact,
                            Vec3::new(v.normal.x, v.normal.y, v.normal.z),
                            None,
                        )
                    })
            };
            if let Some((distance, normal, wheel)) = contact {
                limit = distance;
                hit = Some(Contact {
                    target: wheel.map_or(Target::Car(index), |w| Target::Tire(index, w)),
                    hit: ShotHit {
                        point: from + dir * limit,
                        normal: pose * normal,
                        surface: if wheel.is_some() {
                            ShotSurface::Rubber
                        } else {
                            ShotSurface::Metal
                        },
                        stationary: false,
                    },
                });
            }
        }
        if let Some((id, t, n)) = self.props.ray_hit(from, dir, limit) {
            limit = t;
            hit = Some(Contact {
                target: Target::Prop(id),
                hit: ShotHit {
                    point: from + dir * t,
                    normal: n,
                    surface: self.props.shot_surface(id),
                    stationary: false,
                },
            });
        }
        if let Some((id, t, n)) = self.crowd.zombie_hit(from, dir, limit) {
            limit = t;
            hit = Some(Contact {
                target: Target::Zombie(id),
                hit: ShotHit {
                    point: from + dir * t,
                    normal: n,
                    surface: ShotSurface::Spectral,
                    stationary: false,
                },
            });
        }
        if let Some((id, t)) = self.wildlife_shot_candidate(from, dir, limit) {
            limit = t;
            hit = Some(Contact {
                target: Target::Wildlife(id),
                hit: ShotHit {
                    point: from + dir * t,
                    normal: -dir,
                    surface: ShotSurface::Wildlife,
                    stationary: false,
                },
            });
        }
        if let Some(ground) = ground_shot_contact(city, from, dir, limit) {
            return Some(Contact {
                hit: ground,
                target: Target::Fixed,
            });
        }
        hit
    }
    pub fn weapon_aim_point(&self, city: &City, eye: Vec3, end: Vec3) -> Vec3 {
        self.weapon_hit(city, eye, end).map_or(end, |c| c.hit.point)
    }
    pub fn set_zombie_mode(&mut self, city: &City, enabled: bool) {
        if self.crowd.zombies != enabled {
            if enabled {
                self.clear_chase();
                self.cyclists.clear_population(&mut self.vehicles);
                self.update_riders();
            }
            self.refresh_abandoned_cars(city, enabled);
            self.crowd.toggle_zombies(city, &self.vehicles, &self.props);
            self.health = super::melee::PLAYER_HEALTH;
            if let Some(p) = &mut self.pedestrian {
                if p.fallen.is_some() {
                    *p = super::player::Pedestrian::new(city, p.position, p.yaw);
                } else {
                    p.gait.melee = Default::default();
                }
            }
        }
    }
    pub fn zombie_mode(&self) -> bool {
        self.crowd.zombies
    }
    pub fn fire_at_scenery(&mut self, city: &City, hand: Vec3, target: Vec3) -> Vec3 {
        self.fire_weapon_at(city, hand, target, 2)
            .map_or(target, |hit| hit.point)
    }
    pub fn drain_player_tire_pops(&mut self) -> impl Iterator<Item = ShotHit> + '_ {
        self.player_tire_pops.drain(..)
    }
    pub fn fire_weapon_at(
        &mut self,
        city: &City,
        hand: Vec3,
        target: Vec3,
        damage: u8,
    ) -> Option<ShotHit> {
        let delta = target - hand;
        if delta.length_squared() < 1e-8 {
            return None;
        }
        let dir = delta / delta.length();
        let mut contact = self.weapon_hit(city, hand, target + dir * 0.04)?;
        if contact.hit.normal.length_squared() < 0.5 {
            contact.hit.normal = -dir;
        }

        let force = if damage > 1 { 1. } else { 0.73 };
        let mut broken = false;
        match contact.target {
            Target::Tire(index, wheel) => {
                broken = self.vehicles[index].pop_tire(wheel);
                if broken {
                    self.traffic.wake_after_tire_hit(index);
                    if self.is_driving() && index == self.player_vehicle.0 {
                        self.player_tire_pops.push(contact.hit);
                    }
                }
            }
            Target::Prop(id) => self
                .props
                .strike_prop(id, contact.hit.point, dir, force, city),
            Target::Zombie(id) => self
                .crowd
                .damage_zombie(id, damage, dir, city, &self.vehicles),
            Target::Wildlife(id) => self.hit_wildlife(id),
            _ => {}
        }
        crate::audio::Impact {
            position: contact.hit.point,
            speed: 7. * force,
            material: contact.hit.surface.sound(),
            broken,
            ..Default::default()
        }
        .publish(&mut self.sound_impacts, &mut self.sound_serial);
        Some(contact.hit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hermes::Hermes;
    use opencity::runtime::{Mesh, SurfaceKind};

    #[test]
    fn wildlife_shot_uses_camera_aim_and_respects_cover() {
        let map = Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane sidewalk; end
             street(-1,0,1,0); land(-2,-2,2,-2,2,2,-2,2);
             wall=building(0.05,{ material: red_brick },0.18,0.1,0.2,0.1,0.2,0.22,0.18,0.22);
             wildlife_crow_garden=map_point(0.25,0.16);",
        );
        let mut world = DynamicWorld::with_start(&map, Some("0.25,0,0".parse().unwrap())).unwrap();
        let animal = world.wildlife().next().unwrap();
        let eye = animal.position + 4. * Vec3::Z;
        let end = animal.position - Vec3::Z;
        let aim = world.weapon_aim_point(map.city(), eye, end);
        assert!(aim.z > animal.position.z);

        let blocked = world
            .fire_weapon_at(map.city(), Vec3::new(12., 16., 1.5), aim, 1)
            .unwrap();
        assert!(matches!(blocked.surface, ShotSurface::Brick(_)));
        assert_eq!(world.weapon_aim_point(map.city(), eye, end), aim);

        let hit = world.fire_weapon_at(map.city(), eye, aim, 1).unwrap();
        assert_eq!(hit.surface, ShotSurface::Wildlife);
        assert!(hit.point.abs_diff_eq(aim, 0.001));
        assert!(!hit.stationary);
        let next = world.fire_weapon_at(map.city(), eye, end, 2).unwrap();
        assert!(next.stationary);
        assert!(next.point.z < aim.z);
    }

    #[test]
    fn weapon_shot_breaks_the_first_prop_and_stops_at_cover() {
        let h=Hermes::test_map_from_source("road street: sidewalk carlane carlane sidewalk; end street(-1,0,1,0); land(-2,-2,2,-2,2,2,-2,2); wall=building(0.05,{ material: red_brick },0.18,0.1,0.2,0.1,0.2,0.22,0.18,0.22);");
        let mut world = DynamicWorld::new(&h);
        let mut mesh = Mesh::default();
        mesh.cuboid(
            [0.21, 0.14, 0.01],
            [0.22, 0.16, 0.03],
            [0.5; 3],
            SurfaceKind::Prop,
        );
        world.props = super::super::props::Props::new(vec![mesh]);
        let target = Vec3::new(22., 15., 2.);
        let blocked = world
            .fire_weapon_at(h.city(), Vec3::new(12., 15., 2.), target, 2)
            .unwrap();
        assert!(blocked.point.x < 20.);
        assert_eq!(blocked.normal, -Vec3::X);
        assert_eq!(
            blocked.surface,
            ShotSurface::Brick(opencity::BuildingMaterial::RedBrick.wall_rgb())
        );
        assert!(world.props.group_intact(0));
        let hit = world
            .fire_weapon_at(
                h.city(),
                Vec3::new(28., 15., 2.),
                Vec3::new(20., 15., 2.),
                2,
            )
            .unwrap()
            .point;
        assert!((hit.x - 22.).abs() < 0.02);
        assert!(!world.props.group_intact(0));
    }
}
