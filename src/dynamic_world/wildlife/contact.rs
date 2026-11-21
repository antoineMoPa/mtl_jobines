use super::Home;
use crate::dynamic_world::{player, DynamicWorld};
use glam::{Quat, Vec3};
use rapier3d::{
    na::{Isometry3, Quaternion, Translation3, UnitQuaternion, Vector3},
    parry::query::{cast_shapes, ShapeCastOptions},
    prelude::{Cuboid, Cylinder, Shape},
};

fn pose(center: Vec3, rotation: Quat) -> Isometry3<f32> {
    Isometry3::from_parts(
        Translation3::new(center.x, center.y, center.z),
        UnitQuaternion::new_unchecked(Quaternion::new(
            rotation.w, rotation.x, rotation.y, rotation.z,
        )),
    )
}

#[derive(Clone, Copy)]
enum Mover {
    Pedestrian,
    Vehicle,
}

fn contact_on_travel(
    homes: &mut [Home],
    time: f32,
    mover: Mover,
    shape: &dyn Shape,
    rotation: Quat,
    start: Vec3,
    travel: Vec3,
    dt: f32,
) {
    let speed = travel.truncate().length() / dt;
    if speed < 0.25 {
        return;
    }
    let actor = pose(start, rotation);
    let displacement = Vector3::new(travel.x, travel.y, travel.z);
    let reach = shape.compute_local_bounding_sphere().radius + travel.length();
    let knocks_over = matches!(mover, Mover::Vehicle) && speed > 8.;
    for home in homes.iter_mut().filter(|home| {
        if knocks_over {
            !home.is_downed()
        } else {
            home.is_active()
        }
    }) {
        let animal = home.sighting(time);
        let radii = animal.species.body_radii();
        let center = animal.position + Vec3::Z * radii.z;
        let relative = center - start;
        if relative.length_squared() > (reach + radii.length()).powi(2)
            || relative.truncate().dot(travel.truncate()) < 0.
        {
            continue;
        }
        let body = Cuboid::new(Vector3::new(radii.x, radii.y, radii.z));
        let animal_pose = pose(center, Quat::from_rotation_z(animal.heading));
        let options = ShapeCastOptions {
            max_time_of_impact: 1.,
            compute_impact_geometry_on_penetration: false,
            ..Default::default()
        };
        if let Ok(Some(hit)) = cast_shapes(
            &actor,
            &displacement,
            shape,
            &animal_pose,
            &Vector3::zeros(),
            &body,
            options,
        ) {
            if knocks_over {
                home.knock_over(time);
                continue;
            }
            let away = (relative - travel * hit.time_of_impact).truncate();
            let direction = if away.length_squared() > 0.0001 {
                away / away.length()
            } else {
                travel.truncate() / (speed * dt)
            };
            home.bump(direction, speed);
        }
    }
}

impl DynamicWorld {
    pub(in crate::dynamic_world) fn capture_wildlife_vehicle_starts(
        &mut self,
        eye: Vec3,
        velocity: Vec3,
        dt: f32,
    ) {
        self.wildlife_vehicle_starts.clear();
        // Include approaching homes just outside draw range. Capture after traffic
        // spawns so a recycled vehicle never sweeps across its old and new locations.
        let approach = velocity.length() * dt + 16.;
        if self.wildlife_homes.iter().any(|home| {
            !home.is_downed()
                && home.ground[0].distance_squared(eye)
                    <= (home.species.view_distance() + approach).powi(2)
        }) {
            self.wildlife_vehicle_starts
                .extend(self.vehicles.iter().map(|v| v.position));
        }
    }

    pub(in crate::dynamic_world) fn check_wildlife_walking_contacts(
        &mut self,
        before: Vec3,
        after: Vec3,
        dt: f32,
    ) {
        // Parry's cylinder is Y-up, while simulation positions are Z-up.
        let body = Cylinder::new(player::HALF_HEIGHT, player::RADIUS);
        contact_on_travel(
            &mut self.wildlife_homes,
            self.wildlife_time,
            Mover::Pedestrian,
            &body,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            before,
            after - before,
            dt,
        );
    }

    pub(in crate::dynamic_world) fn check_wildlife_vehicle_contacts(&mut self, dt: f32) {
        for (car, &before) in self.vehicles.iter().zip(&self.wildlife_vehicle_starts) {
            let travel = car.position - before;
            if travel.truncate().length_squared() < (0.25 * dt).powi(2) {
                continue;
            }
            let rotation = car.orientation();
            let center = before + rotation * Vec3::Z * (car.height() * 0.5 - car.ride_height());
            let body = Cuboid::new(Vector3::new(car.length(), car.width(), car.height()) * 0.5);
            contact_on_travel(
                &mut self.wildlife_homes,
                self.wildlife_time,
                Mover::Vehicle,
                &body,
                rotation,
                center,
                travel,
                dt,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::response::Reaction;
    use super::*;
    use crate::{
        dynamic_world::{player::Pedestrian, Controls, WalkControls},
        hermes::Hermes,
    };

    fn habitat() -> Hermes {
        Hermes::test_map_from_source(
            "road street: sidewalk carlane carlane sidewalk; end street(-1,0,1,0);
             land(-2,-2,2,-2,2,2,-2,2); wildlife_crow_lawn = map_point(0.5,0.5);",
        )
    }

    #[test]
    fn walking_into_an_animal_startles_it_in_the_world_step() {
        let map = habitat();
        let mut world = DynamicWorld::new(&map);
        let ground = world.wildlife_homes[0].sighting(0.).position;
        world.pedestrian = Some(Pedestrian::new(
            map.city(),
            ground - Vec3::X + Vec3::Z * player::HALF_HEIGHT,
            0.,
        ));
        for _ in 0..60 {
            world.step(
                &map,
                Controls::default(),
                WalkControls {
                    forward: 1.,
                    ..Default::default()
                },
                1. / 60.,
            );
            if matches!(world.wildlife_homes[0].reaction, Reaction::Startled { .. }) {
                return;
            }
        }
        panic!("walking across the crow did not startle it");
    }

    #[test]
    fn vehicle_contacts_respect_speed_deck_and_the_full_swept_path() {
        let map = habitat();
        for (speed, height, distance, fleeing) in [
            (40., 0., 4., false),
            (40., 0., 4., true),
            (40., 4., 4., false),
            (2., 0., 2.4, false),
        ] {
            let mut world = DynamicWorld::new(&map);
            let ground = world.wildlife_homes[0].sighting(0.).position;
            if fleeing {
                world.wildlife_homes[0].bump(glam::Vec2::Y, 1.);
            }
            let car = &mut world.vehicles[world.player_vehicle.0];
            car.set_catalog_model(None);
            car.yaw = 0.;
            car.position = ground - Vec3::X * distance + Vec3::Z * (car.ride_height() + height);
            car.velocity = Vec3::X * speed;
            world.step(&map, Controls::default(), WalkControls::default(), 0.24);
            let reaction = world.wildlife_homes[0].reaction;
            if speed > 8. {
                assert!(world.player().position.x - ground.x > world.player().length() * 0.5 + 0.4);
                assert_eq!(matches!(reaction, Reaction::Down { .. }), height == 0.);
                assert_eq!(matches!(reaction, Reaction::Roaming), height > 0.);
            } else {
                assert!(matches!(reaction, Reaction::Startled { .. }));
            }
        }
    }
}
