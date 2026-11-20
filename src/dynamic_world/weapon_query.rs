//! Third-person picking and shot obstruction in world metres.
use super::{DynamicWorld, METERS_PER_UNIT};
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
            Self::Earth | Self::Water | Self::Spectral => Material::Body,
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
    Car,
    Prop(usize),
    Zombie(u32),
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
        self.health > 0
            && self.pedestrian.as_ref().is_some_and(|p| !p.swimming)
            && !self.metro.aboard
    }

    pub fn start_punch(&mut self) -> bool {
        if !self.can_equip_weapon() {
            return false;
        }
        self.pedestrian
            .as_mut()
            .unwrap()
            .gait
            .melee
            .start(super::melee::Attack::Punch)
    }

    pub fn cancel_punch(&mut self) {
        if let Some(p) = &mut self.pedestrian {
            if p.gait.melee.active() {
                p.gait.melee = Default::default();
            }
        }
    }

    pub fn punching(&self) -> bool {
        self.pedestrian
            .as_ref()
            .is_some_and(|p| p.gait.melee.active())
    }

    pub fn zombie_health_meters(&self) -> impl Iterator<Item = (Vec3, f32)> + '_ {
        self.crowd.health_meters(self.player_position())
    }

    pub fn player_health(&self) -> u8 {
        self.health
    }

    pub(super) fn step_melee(&mut self, city: &City, dt: f32) {
        use super::{
            melee::BITE_DAMAGE, pedestrian_catalog, player::HALF_HEIGHT, ragdoll::Ragdoll,
        };
        let can_fight = self.can_equip_weapon();
        if let Some(p) = &mut self.pedestrian {
            if let Some(doll) = &mut p.fallen {
                if !doll.ready_to_rise() {
                    let model = pedestrian_catalog::player_model(self.season);
                    doll.step(
                        &pedestrian_catalog::definitions()[model],
                        city,
                        &self.vehicles,
                        dt,
                    );
                    p.position = doll.anchor + Vec3::Z * HALF_HEIGHT;
                }
                return;
            }
            if !can_fight {
                p.gait.melee = Default::default();
            }
            if p.gait.melee.advance(dt) && self.crowd.zombies {
                let forward = glam::Vec2::from_angle(p.yaw).extend(0.);
                let from = p.position + Vec3::Z * 0.32;
                if let Some(Contact {
                    target: Target::Zombie(id),
                    ..
                }) = self.weapon_hit(city, from, from + forward * 0.82)
                {
                    self.crowd
                        .damage_zombie(id, 1, forward, city, &self.vehicles);
                }
            }
        }
        if !self.crowd.zombies {
            return;
        }
        let target = can_fight.then(|| self.player_position());
        for (id, shoulder) in self.crowd.bite_contacts(target, dt) {
            let from = self.player_position() + Vec3::Z * 0.32;
            // Use the same obstruction query as the punch, from outside the attacker's capsule.
            if !matches!(self.weapon_hit(city, from, shoulder), Some(Contact { target: Target::Zombie(hit), .. }) if hit == id)
            {
                continue;
            }
            self.health = self.health.saturating_sub(BITE_DAMAGE);
            let player = self.pedestrian.as_mut().unwrap();
            player.gait.melee.hurt();
            if self.health == 0 {
                let model = pedestrian_catalog::player_model(self.season);
                let pose = *player.instance(self.season).pose.unwrap();
                player.fallen = Some(Box::new(Ragdoll::new(
                    &pedestrian_catalog::definitions()[model],
                    pose,
                    player.position - Vec3::Z * HALF_HEIGHT,
                    player.yaw,
                    (from - shoulder) * 1.5,
                    city,
                    &self.vehicles,
                )));
                self.crowd.cancel_bites();
                break;
            }
        }
    }

    pub fn aim_weapon(&mut self, target: Option<Vec3>, two_handed: bool) {
        if let Some(player) = self.pedestrian.as_mut() {
            player.aim_weapon(self.season, target, two_handed);
        }
    }

    pub fn weapon_hand(&self) -> Option<Vec3> {
        let instance = self.pedestrian.as_ref()?.instance(self.season);
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
        for car in &self.vehicles {
            if from.distance(car.position) > limit + car.length() + car.height() {
                continue;
            }
            let pose = car.orientation();
            let o = pose.conjugate() * (from - car.position);
            let d = pose.conjugate() * dir;
            let ray = Ray::new(point![o.x, o.y, o.z], vector![d.x, d.y, d.z]);
            let bounds = Cuboid::new(vector![car.length(), car.width(), car.height()] * 0.5);
            if let Some(v) = bounds.cast_local_ray_and_get_normal(&ray, limit, true) {
                limit = v.time_of_impact;
                hit = Some(Contact {
                    target: Target::Car,
                    hit: ShotHit {
                        point: from + dir * limit,
                        normal: pose * Vec3::new(v.normal.x, v.normal.y, v.normal.z),
                        surface: ShotSurface::Metal,
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
        match contact.target {
            Target::Prop(id) => self
                .props
                .strike_prop(id, contact.hit.point, dir, force, city),
            Target::Zombie(id) => self
                .crowd
                .damage_zombie(id, damage, dir, city, &self.vehicles),
            _ => {}
        }
        self.sound_serial = self.sound_serial.wrapping_add(1).max(1);
        let slot = (self.sound_serial as usize - 1) % self.sound_impacts.len();
        self.sound_impacts[slot] = crate::audio::Impact {
            serial: self.sound_serial,
            position: contact.hit.point,
            speed: 7. * force,
            material: contact.hit.surface.sound(),
            broken: false,
            age: 0.,
        };
        Some(contact.hit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hermes::Hermes;
    use opencity::runtime::{Mesh, SurfaceKind};
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
