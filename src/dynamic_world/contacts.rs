//! Rigid body impulses between drivable and parked catalog vehicles.
use super::Vehicle;
use glam::Vec3;
use rapier3d::{
    parry::query::{contact, ContactManifold, DefaultQueryDispatcher, PersistentQueryDispatcher},
    prelude::*,
};
#[cfg(test)]
pub(super) fn resolve(vehicles: &mut [Vehicle]) {
    let _ = resolve_with_contacts(vehicles);
}
pub(super) fn resolve_with_contacts(vehicles: &mut [Vehicle]) -> Vec<(usize, usize)> {
    let mut contacts = Vec::new();
    for i in 0..vehicles.len() {
        let (before, after) = vehicles.split_at_mut(i);
        let a = &mut after[0];
        for (j, b) in before.iter_mut().enumerate() {
            if a.position.distance_squared(b.position) > (a.length() + b.length()).powi(2) {
                continue;
            }
            let center = |v: &Vehicle| {
                v.position + v.orientation() * Vec3::Z * (v.height() * 0.5 - v.ride_height())
            };
            let pose = |v: &Vehicle| {
                let p = center(v);
                let q = v.orientation();
                Isometry::from_parts(
                    Translation::new(p.x, p.y, p.z),
                    Rotation::new_unchecked(rapier3d::na::Quaternion::new(q.w, q.x, q.y, q.z)),
                )
            };
            let shape = |v: &Vehicle| {
                Cuboid::new(vector![v.length() * 0.5, v.width() * 0.5, v.height() * 0.5])
            };
            let pose_a = pose(a);
            let pose_b = pose(b);
            let shape_a = shape(a);
            let shape_b = shape(b);
            let Ok(Some(hit)) = contact(&pose_a, &shape_a, &pose_b, &shape_b, 0.) else {
                continue;
            };
            if hit.dist >= 0. {
                continue;
            }
            let n = Vec3::new(hit.normal1.x, hit.normal1.y, hit.normal1.z);
            let mut manifold = ContactManifold::<(), ()>::new();
            if DefaultQueryDispatcher
                .contact_manifold_convex_convex(
                    &pose_a.inv_mul(&pose_b),
                    &shape_a,
                    &shape_b,
                    None,
                    None,
                    0.,
                    &mut manifold,
                )
                .is_err()
            {
                continue;
            }
            contacts.push((i, j));
            let inv_a = 1. / a.mass();
            let inv_b = 1. / b.mass();
            let sum = inv_a + inv_b;
            let separation = n * (-hit.dist + 0.001) / sum;
            a.position -= separation * inv_a;
            b.position += separation * inv_b;
            let mut impact = 0_f32;
            let mut constraints = manifold
                .points
                .iter()
                .map(|hit| {
                    // Shared world points conserve angular momentum. Resolving
                    // the whole patch avoids artificial torque at face centres.
                    let point =
                        ((pose_a * hit.local_p1).coords + (pose_b * hit.local_p2).coords) * 0.5;
                    let point = Vec3::new(point.x, point.y, point.z);
                    let arm_a = point - a.position;
                    let arm_b = point - b.position;
                    let closing = (b.velocity_at(arm_b) - a.velocity_at(arm_a)).dot(n);
                    impact = impact.max(-closing);
                    let response =
                        inv_a * a.impulse_response(arm_a, n) + inv_b * b.impulse_response(arm_b, n);
                    (arm_a, arm_b, response, (-closing * 0.15).max(0.), 0_f32)
                })
                .collect::<Vec<_>>();
            for _ in 0..12 {
                for &mut (arm_a, arm_b, response, rebound, ref mut accumulated) in &mut constraints
                {
                    let closing = (b.velocity_at(arm_b) - a.velocity_at(arm_a)).dot(n);
                    let next = (*accumulated + (rebound - closing) / response).max(0.);
                    let impulse = n * (next - *accumulated);
                    *accumulated = next;
                    a.apply_impulse(arm_a, -impulse * inv_a);
                    b.apply_impulse(arm_b, impulse * inv_b);
                }
            }
            if impact > 0. {
                a.collisions += 1;
                b.collisions += 1;
                // Report the physical pair once, at one participant's chassis.
                a.sound_impact(impact, crate::audio::Material::Body, false);
            }
        }
    }
    contacts
}
#[cfg(test)]
mod tests {
    use super::*;

    fn kinetic_energy(cars: &[Vehicle]) -> f32 {
        cars.iter()
            .map(|car| {
                car.mass()
                    * (car.velocity.length_squared()
                        + car.angular_momentum.dot(car.angular_velocity()))
            })
            .sum()
    }

    #[test]
    fn offset_impacts_transfer_spin_and_conserve_pair_momentum_without_adding_energy() {
        let hermes = crate::hermes::Hermes::test_map();
        for offset in [Vec3::Y, Vec3::Z * 0.8] {
            let mut a = Vehicle::spawn(hermes.city());
            let mut b = Vehicle::spawn(hermes.city());
            a.position = Vec3::new(0., 0., 20.);
            b.position = a.position + Vec3::X * 4.1 + offset;
            a.yaw = 0.;
            b.yaw = 0.;
            a.velocity = Vec3::X * 12.;
            let momentum = a.velocity * a.mass();
            let mut cars = [a, b];
            let initial_energy = kinetic_energy(&cars);
            resolve(&mut cars);
            let linear = cars
                .iter()
                .map(|car| car.velocity * car.mass())
                .sum::<Vec3>();
            assert!(linear.distance(momentum) < 0.01);
            let angular = cars
                .iter()
                .map(|car| car.mass() * (car.position.cross(car.velocity) + car.angular_momentum))
                .sum::<Vec3>();
            let initial_angular = cars[0].position.cross(momentum);
            assert!(
                angular.distance(initial_angular) < 0.1,
                "angular: {angular:?}"
            );
            assert!(kinetic_energy(&cars) < initial_energy);
            let spin = cars[0].angular_velocity() + cars[1].angular_velocity();
            if offset.y > 0. {
                assert!(spin.z > 0.1, "offset hit did not yaw: {spin:?}");
            } else {
                assert!(spin.y < -0.1, "offset hit did not pitch: {spin:?}");
            }
        }
    }

    #[test]
    fn centred_face_impact_does_not_create_corner_spin() {
        let hermes = crate::hermes::Hermes::test_map();
        let mut a = Vehicle::spawn(hermes.city());
        let mut b = Vehicle::spawn(hermes.city());
        a.yaw = 0.;
        b.yaw = 0.;
        b.position = a.position + Vec3::X * 4.1;
        a.velocity = Vec3::X * 10.;
        let mut cars = [a, b];
        resolve(&mut cars);
        for car in &cars {
            assert!(
                car.angular_velocity().length() < 0.03,
                "spin {:?}",
                car.angular_velocity()
            );
        }
    }
    #[test]
    fn a_heavy_parked_vehicle_blocks_and_receives_less_velocity_than_a_compact() {
        let hermes = crate::hermes::Hermes::test_map();
        let mut a = Vehicle::spawn(hermes.city());
        a.definition = super::super::catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-v2-09");
        assert!(a.definition.is_some());
        a.yaw = 0.;
        let mut b = Vehicle::spawn(hermes.city());
        b.definition = super::super::catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-j02");
        assert!(b.definition.is_some());
        b.yaw = 0.;
        b.position = a.position + Vec3::X * ((a.length() + b.length()) * 0.5 - 0.1);
        a.velocity = Vec3::X * 5.;
        let mut vehicles = vec![a, b];
        resolve(&mut vehicles);
        assert!(vehicles[0].velocity.x < 1.);
        assert!(vehicles[1].velocity.x > 0. && vehicles[1].velocity.x < 1.);
        assert!(vehicles[0].collisions > 0);
        assert!(vehicles[0].position.distance(vehicles[1].position) > 8.);
    }
}
