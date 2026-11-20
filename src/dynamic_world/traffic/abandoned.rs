//! Reuse traffic slots for unoccupied cars left across lanes during the zombie jobine.
use super::{SpeedClass, RETAIN_RADIUS, SPAWN_RADIUS};
use crate::dynamic_world::{
    catalog, doors::Door, DynamicWorld, SpawnPoint, Vehicle, METERS_PER_UNIT,
};
use glam::Vec2;
use opencity::runtime::City;

const COUNT: usize = 10;

impl DynamicWorld {
    pub(in crate::dynamic_world) fn refresh_abandoned_cars(&mut self, city: &City, enabled: bool) {
        let center = self.player_position();
        self.traffic.abandoned.retain(|&id| {
            let car = &mut self.vehicles[id];
            if enabled && car.position.distance_squared(center) <= RETAIN_RADIUS.powi(2) {
                return true;
            }
            car.position.z = -1000.;
            car.velocity = glam::Vec3::ZERO;
            self.traffic.interacted.remove(&id);
            self.traffic.inactive.push(id);
            false
        });
        let budget = crate::performance::npc_budget(COUNT);
        if !enabled || self.traffic.abandoned.len() >= budget {
            return;
        }

        let mut sites = Vec::new();
        for path in &self.traffic.graph.paths {
            if !matches!(path.speed, SpeedClass::Local | SpeedClass::Arterial)
                || !path.near(center, SPAWN_RADIUS)
            {
                continue;
            }
            let mut distance = 7.;
            while distance < path.length - 6. {
                let (point, heading) = path.sample(distance);
                if (16. ..SPAWN_RADIUS).contains(&point.distance(center)) {
                    sites.push((point, heading));
                }
                distance += 18.;
            }
        }
        let models: Vec<_> = catalog::definitions()
            .iter()
            .enumerate()
            .filter_map(|(id, d)| {
                matches!(
                    d.config.category.as_str(),
                    "compact" | "suv" | "pickup" | "taxi"
                )
                .then_some(id)
            })
            .collect();
        for _ in 0..budget * 16 {
            if sites.is_empty() || self.traffic.abandoned.len() >= budget {
                break;
            }
            let choice = self.traffic.random(sites.len());
            let (point, heading) = sites.swap_remove(choice);
            let offset = self.traffic.random(7) as f32 * 0.32 - 0.96;
            let xy = point.truncate() + heading.truncate().perp() * offset;
            if self.traffic.abandoned.iter().any(|&id| {
                self.vehicles[id].position.truncate().distance_squared(xy) < 13_f32.powi(2)
            }) {
                continue;
            }
            let yaw = Vec2::X.angle_to(heading.truncate())
                + (self.traffic.random(101) as f32 - 50.) * 0.014;
            let spawn = SpawnPoint {
                point: (xy / METERS_PER_UNIT).to_array(),
                heading: Some(yaw),
            };
            let Ok(mut car) = Vehicle::spawn_at(city, spawn) else {
                continue;
            };
            car.set_catalog_model(Some(models[self.traffic.random(models.len())]));
            if (car.position.z - car.ride_height() - point.z).abs() > 0.5
                || !self.clear_vehicle_position(city, &car, None)
                || self.crowd.overlaps_vehicle(&car)
            {
                continue;
            }
            let door = Door::ALL[self.traffic.random(4)];
            car.doors.abandon(door);
            if self.traffic.random(3) == 0 {
                car.doors.abandon(Door::FrontLeft);
            }
            let id = if let Some(id) = self.traffic.inactive.pop() {
                self.vehicles[id] = car;
                self.vehicle_spawns[id] = spawn;
                id
            } else {
                let id = self.vehicles.len();
                self.vehicles.push(car);
                self.vehicle_spawns.push(spawn);
                id
            };
            self.traffic.abandoned.push(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hermes::Hermes;

    #[test]
    fn zombie_cars_are_scattered_claimable_and_recycle_across_mission_restarts() {
        let map = Hermes::test_map_from_source("road street: sidewalk carlane middle carlane sidewalk; end street(-2,0,2,0); street(-2,0.45,2,0.45); land(-3,-1,3,-1,3,1,-3,1); player_start(0,-0.3,0);");
        let mut world = DynamicWorld::new(&map);
        let drivers = world.traffic.driven_slots().count();
        assert!(drivers > 0);
        let initial_cars = world.vehicles.len();
        world.set_zombie_mode(map.city(), true);
        let abandoned = world.traffic.abandoned.clone();
        assert!(abandoned.len() >= 3);
        assert_eq!(world.vehicles.len(), initial_cars + abandoned.len());
        assert_eq!(world.traffic.driven_slots().count(), drivers);

        for &id in &abandoned {
            let car = &world.vehicles[id];
            assert!(world.clear_vehicle_position(map.city(), car, Some(id)));
            assert!(Door::ALL.into_iter().any(|d| car.doors.fraction(d) == 1.));
            assert!(!world.traffic.driven_slots().any(|(i, _)| i == id));
        }
        world.set_zombie_mode(map.city(), true);
        assert_eq!(world.traffic.abandoned, abandoned);
        assert!(world.toggle_vehicle(&map));
        let id = abandoned[0];
        let xy = world.vehicles[id].access_point(Door::FrontLeft).unwrap();
        let foot = world.pedestrian.as_mut().unwrap();
        foot.position = xy.extend(world.vehicles[id].position.z + 0.4);
        assert!(world.toggle_vehicle(&map));
        assert_eq!(world.player_vehicle.0, id);
        let avatar = crate::dynamic_world::pedestrian_catalog::player_model(world.season());
        assert_eq!(
            world
                .seated_drivers()
                .filter(|actor| actor.model == avatar)
                .count(),
            1
        );
        let spot = world.player().position;
        world.set_zombie_mode(map.city(), false);
        assert_eq!(world.player().position, spot);
        assert!(world.traffic.abandoned.is_empty());
        for &slot in &abandoned[1..] {
            assert!(world.traffic.inactive(slot));
        }
        assert!(!world.traffic.inactive(id));
        world.reset_player(&map);
        assert!(world.player().position.distance(spot) < 0.01);
        world.set_zombie_mode(map.city(), true);
        assert!(!world.traffic.abandoned.contains(&id));
        assert_eq!(world.vehicle_spawns.len(), world.vehicles.len());
        assert!(world.vehicles.len() <= initial_cars + COUNT + 1);
    }
}
