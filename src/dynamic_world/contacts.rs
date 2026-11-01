//! Low-speed rigid body contacts between drivable and parked catalog vehicles.
use super::Vehicle;
use glam::Vec3;
use rapier3d::{parry::query::contact, prelude::*};
pub(super) fn resolve(vehicles: &mut [Vehicle]) {
    for i in 0..vehicles.len() {
        let (before, after) = vehicles.split_at_mut(i);
        let a = &mut after[0];
        for b in before {
            if a.position.distance_squared(b.position) > (a.length() + b.length()).powi(2) {
                continue;
            }
            let center = |v: &Vehicle| v.position + Vec3::Z * (v.height() * 0.5 - v.ride_height());
            let pose = |v: &Vehicle| {
                let p = center(v);
                Isometry::new(vector![p.x, p.y, p.z], vector![0., 0., v.yaw])
            };
            let shape = |v: &Vehicle| {
                Cuboid::new(vector![v.length() * 0.5, v.width() * 0.5, v.height() * 0.5])
            };
            let Ok(Some(hit)) = contact(&pose(a), &shape(a), &pose(b), &shape(b), 0.) else {
                continue;
            };
            if hit.dist >= 0. {
                continue;
            }
            let n = Vec3::new(hit.normal1.x, hit.normal1.y, hit.normal1.z);
            let inv_a = 1. / a.mass();
            let inv_b = 1. / b.mass();
            let sum = inv_a + inv_b;
            let separation = n * (-hit.dist + 0.001) / sum;
            a.position -= separation * inv_a;
            b.position += separation * inv_b;
            let closing = (b.velocity - a.velocity).dot(n);
            if closing < 0. {
                let impulse = n * (-closing * 1.15 / sum);
                a.velocity -= impulse * inv_a;
                b.velocity += impulse * inv_b;
                a.collisions += 1;
                b.collisions += 1;
                // Report the physical pair once, at one participant's chassis.
                a.sound_impact(-closing, crate::audio::Material::Body, false);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
