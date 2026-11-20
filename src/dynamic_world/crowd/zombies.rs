//! Optional street-crowd behaviour. Shot queries are disabled in ordinary play.
use super::*;
use crate::dynamic_world::{
    melee::{Attack, ZOMBIE_HEALTH},
    player::HALF_HEIGHT,
};
use rapier3d::{
    na::{Point3, Vector3},
    prelude::{Capsule, Ray, RayCast},
};

impl Crowd {
    pub(in crate::dynamic_world) fn cancel_bites(&mut self) {
        for w in &mut self.walkers {
            w.gait.melee = Default::default();
        }
    }

    pub(in crate::dynamic_world) fn bite_contacts(
        &mut self,
        target: Option<Vec3>,
        dt: f32,
    ) -> Vec<(u32, Vec3)> {
        let mut contacts = Vec::new();
        for w in &mut self.walkers {
            let Some(center) = target else {
                w.gait.melee = Default::default();
                continue;
            };
            if w.health == 0 || w.reaction.is_some() {
                continue;
            }
            let to_player = center - (w.position + Vec3::Z * HALF_HEIGHT);
            let delta = to_player.truncate();
            let distance = delta.length();
            let facing = glam::Vec2::from_angle(w.yaw).dot(delta);
            let in_reach =
                distance <= 0.98 && to_player.z.abs() <= 0.43 && facing >= distance * 0.55;
            if in_reach && distance <= 0.86 && w.stagger <= 0. {
                w.gait.melee.start(Attack::Bite);
            }
            if w.gait.melee.advance(dt) && in_reach {
                contacts.push((w.serial, w.position + Vec3::Z * 1.22));
            }
        }
        contacts
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(in crate::dynamic_world) fn health_meters(
        &self,
        center: Vec3,
    ) -> impl Iterator<Item = (Vec3, f32)> + '_ {
        self.walkers
            .iter()
            .filter(move |w| {
                w.health > 0
                    && w.reaction.is_none()
                    && w.position.distance_squared(center) < 8. * 8.
            })
            .map(|w| {
                (
                    w.position + Vec3::Z * 1.90,
                    w.health as f32 / ZOMBIE_HEALTH as f32,
                )
            })
    }

    pub(in crate::dynamic_world) fn toggle_zombies(
        &mut self,
        city: &City,
        vehicles: &[Vehicle],
        props: &Props,
    ) {
        self.zombies = !self.zombies;
        for w in &mut self.walkers {
            w.gait.zombie_reach = self.zombies;
            w.health = ZOMBIE_HEALTH;
            w.gait.melee = Default::default();
            w.stagger = 0.;
            w.deferred_dt = 0.;
            if !self.zombies && w.reaction.is_none() {
                w.returning = Some(return_point(city, &self.graph, w, vehicles, props));
            }
        }
    }
    pub(in crate::dynamic_world) fn zombie_hit(
        &self,
        from: Vec3,
        dir: Vec3,
        limit: f32,
    ) -> Option<(u32, f32, Vec3)> {
        if !self.zombies {
            return None;
        }
        let shape = Capsule::new_z(0.61, 0.29);
        let mut nearest = limit;
        let mut hit = None;
        for w in &self.walkers {
            if w.health == 0 || w.reaction.is_some() {
                continue;
            }
            let local = from - w.position - Vec3::Z * 0.91;
            let ray = Ray::new(
                Point3::new(local.x, local.y, local.z),
                Vector3::new(dir.x, dir.y, dir.z),
            );
            if let Some(r) = shape.cast_local_ray_and_get_normal(&ray, nearest, true) {
                nearest = r.time_of_impact;
                hit = Some((
                    w.serial,
                    nearest,
                    Vec3::new(r.normal.x, r.normal.y, r.normal.z),
                ));
            }
        }
        hit
    }
    pub(in crate::dynamic_world) fn damage_zombie(
        &mut self,
        id: u32,
        damage: u8,
        dir: Vec3,
        city: &City,
        vehicles: &[Vehicle],
    ) {
        if !self.zombies {
            return;
        }
        if let Some(w) = self
            .walkers
            .iter_mut()
            .find(|w| w.serial == id && w.health > 0)
        {
            w.health = w.health.saturating_sub(damage);
            w.stagger = 0.32;
            w.gait.melee.hurt();
            if w.health == 0 {
                w.fall(city, vehicles, dir * 2.4);
            }
        }
    }
}

pub(super) fn pursue(
    w: &mut Walker,
    city: &City,
    center: Vec3,
    vehicles: &[Vehicle],
    props: &Props,
    neighbors: &[(u32, Vec3, f32)],
    dt: f32,
) {
    w.gait.zombie_reach = true;
    let delta = (center - w.position).truncate();
    let distance = delta.length();
    w.deferred_dt += dt;
    let interval = if distance > 55. {
        0.1
    } else if distance > 24. {
        1. / 30.
    } else {
        0.
    };
    if w.deferred_dt < interval {
        return;
    }
    let dt = std::mem::take(&mut w.deferred_dt);

    let mut desired = delta / distance.max(0.001);
    for &(id, other, _) in neighbors {
        if id == w.serial {
            continue;
        }
        let away = (w.position - other).truncate();
        let span = away.length();
        if (0.001..0.82).contains(&span) {
            desired += away / span * (0.82 - span) * 1.4;
        }
    }
    w.stagger = (w.stagger - dt).max(0.);
    let target_yaw = desired.y.atan2(desired.x);
    let speed = if distance > 0.83 && w.stagger == 0. && !w.gait.melee.active() {
        w.preferred_speed * 0.89
    } else {
        0.
    };
    let stride = (speed * dt).min((distance - 0.82).max(0.));
    let mut moved = 0.;
    let mut facing = target_yaw;
    if stride > 0. {
        let side = if w.serial % 2 == 0 { 1. } else { -1. };
        for offset in [0., side * 0.58, -side * 0.58, side * 1.32, -side * 1.32] {
            let a = target_yaw + offset;
            let next = w.position + Vec3::new(a.cos(), a.sin(), 0.) * stride;
            let Some(g) = footing(city, next, false) else {
                continue;
            };
            if props.blocked(g.point + Vec3::Z * 0.8) || blocked_by_vehicle(g.point, vehicles) {
                continue;
            }
            facing = a;
            moved = stride;
            w.position = g.point;
            break;
        }
    }
    w.yaw += angle_delta(w.yaw, facing).clamp(-2.6 * dt, 2.6 * dt);
    w.speed = if dt > 0. { moved / dt } else { 0. };
    w.movement = (w.speed / w.preferred_speed).clamp(0., 1.);
    w.walk_time += moved;
    w.idle_time += dt;
    let def = &pedestrian_catalog::definitions()[w.model];
    let forward = Vec3::new(w.yaw.cos(), w.yaw.sin(), 0.);
    w.gait.advance(
        def,
        w.position,
        w.yaw,
        w.speed,
        w.position + forward,
        dt,
        |p| footing(city, p, false),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zombie_mode_gates_hits_pursuit_and_knockout() {
        let h = crate::hermes::Hermes::test_map();
        let city = h.city();
        let props = Props::new(Vec::new());
        let mut crowd = Crowd::new(h.walking_paths());
        crowd.populate(city, Vec3::ZERO, &[], &props);
        let (start, yaw) = crowd.prepare_collision();
        let id = crowd.walkers[0].serial;
        let target = start + Vec3::new(yaw.cos(), yaw.sin(), 0.) * 3.;
        let ray = start - Vec3::X * 2. + Vec3::Z;
        assert!(crowd.zombie_hit(ray, Vec3::X, 5.).is_none());
        crowd.damage_zombie(id, 3, Vec3::X, city, &[]);
        assert!(crowd.walkers[0].reaction.is_none());
        crowd.toggle_zombies(city, &[], &props);
        assert!(crowd.zombie_hit(ray, Vec3::X, 1.).is_none());
        assert_eq!(crowd.zombie_hit(ray, Vec3::X, 5.).unwrap().0, id);
        for _ in 0..35 {
            crowd.step(city, target, true, &[], &props, 1. / 60.);
        }
        assert!(crowd.walkers[0].position.distance(target) < start.distance(target) - 0.2);
        crowd.damage_zombie(id, 2, Vec3::X, city, &[]);
        assert_eq!(crowd.walkers[0].health, 1);
        crowd.damage_zombie(id, 1, Vec3::X, city, &[]);
        for _ in 0..300 {
            crowd.step(city, target, true, &[], &props, 1. / 60.);
        }
        assert!(matches!(
            crowd.walkers[0].reaction.as_deref(),
            Some(Reaction::Falling(_))
        ));
        assert!(crowd.zombie_hit(ray, Vec3::X, 5.).is_none());
        crowd.toggle_zombies(city, &[], &props);
        for _ in 0..150 {
            crowd.step(city, target, true, &[], &props, 1. / 60.);
        }
        assert!(crowd.walkers[0].reaction.is_none());
        assert!(!crowd.instances(target).next().unwrap().zombie);
    }
}
