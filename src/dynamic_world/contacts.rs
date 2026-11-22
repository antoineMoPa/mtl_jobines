//! Rigid body impulses between drivable and parked catalog vehicles.
use super::Vehicle;
use glam::Vec3;
use rapier3d::{
    parry::query::{contact, ContactManifold, DefaultQueryDispatcher, PersistentQueryDispatcher},
    prelude::*,
};
fn contact_parts<'a>(
    v: &'a Vehicle,
    chassis: &'a Cuboid,
) -> impl Iterator<Item = (Isometry<f32>, &'a dyn Shape)> + 'a {
    let orientation = v.orientation();
    let pose = move |local: Vec3, rotation: glam::Quat| {
        let p = v.position + orientation * local;
        let q: glam::Quat = orientation * rotation;
        Isometry::from_parts(
            Translation::new(p.x, p.y, p.z),
            Rotation::new_unchecked(rapier3d::na::Quaternion::new(q.w, q.x, q.y, q.z)),
        )
    };
    let aircraft = v
        .definition()
        .filter(|d| d.config.vehicle_kind == super::catalog::VehicleKind::Aircraft);
    let road = aircraft.is_none().then(|| {
        (
            pose(
                Vec3::Z * (v.height() * 0.5 - v.ride_height()),
                glam::Quat::IDENTITY,
            ),
            chassis as &dyn Shape,
        )
    });
    road.into_iter()
        .chain(aircraft.into_iter().flat_map(move |d| {
            d.aircraft_hulls(v)
                .map(move |(pivot, rotation, shape)| (pose(pivot, rotation), shape))
        }))
}

fn contact_patch(
    a: &Vehicle,
    b: &Vehicle,
) -> Option<(
    Isometry<f32>,
    Isometry<f32>,
    rapier3d::parry::query::Contact,
    ContactManifold<(), ()>,
)> {
    let chassis =
        |v: &Vehicle| Cuboid::new(vector![v.length() * 0.5, v.width() * 0.5, v.height() * 0.5]);
    let box_a = chassis(a);
    let box_b = chassis(b);
    let mut deepest = 0.;
    let mut patch = None;
    for (pose_a, shape_a) in contact_parts(a, &box_a) {
        for (pose_b, shape_b) in contact_parts(b, &box_b) {
            if let Ok(Some(hit)) = contact(&pose_a, shape_a, &pose_b, shape_b, 0.) {
                if hit.dist < deepest {
                    deepest = hit.dist;
                    patch = Some((pose_a, shape_a, pose_b, shape_b, hit));
                }
            }
        }
    }
    let (pose_a, shape_a, pose_b, shape_b, hit) = patch?;
    let mut manifold = ContactManifold::new();
    DefaultQueryDispatcher
        .contact_manifold_convex_convex(
            &pose_a.inv_mul(&pose_b),
            shape_a,
            shape_b,
            None,
            None,
            0.,
            &mut manifold,
        )
        .ok()?;
    Some((pose_a, pose_b, hit, manifold))
}

pub(super) struct VehicleContact {
    pub vehicles: (usize, usize),
    // Each vehicle's horizontal speed toward the other along the contact normal, before impulses.
    pub approach: [f32; 2],
}

impl VehicleContact {
    pub(super) fn struck_by(&self, driver: usize) -> Option<usize> {
        const MIN_BUMP_SPEED: f32 = 0.1;
        let (a, b) = self.vehicles;
        let [from_a, from_b] = self.approach;
        if from_a + from_b <= MIN_BUMP_SPEED {
            return None;
        }
        if driver == a && from_a > MIN_BUMP_SPEED {
            Some(b)
        } else if driver == b && from_b > MIN_BUMP_SPEED {
            Some(a)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(super) fn resolve(vehicles: &mut [Vehicle]) {
    let _ = resolve_with_contacts(vehicles);
}
pub(super) fn resolve_with_contacts(vehicles: &mut [Vehicle]) -> Vec<VehicleContact> {
    let mut contacts = Vec::new();
    for i in 0..vehicles.len() {
        let (before, after) = vehicles.split_at_mut(i);
        let a = &mut after[0];
        for (j, b) in before.iter_mut().enumerate() {
            if a.position.distance_squared(b.position) > (a.length() + b.length()).powi(2) {
                continue;
            }
            let Some((pose_a, pose_b, hit, manifold)) = contact_patch(a, b) else {
                continue;
            };
            let n = Vec3::new(hit.normal1.x, hit.normal1.y, hit.normal1.z);
            contacts.push(VehicleContact {
                vehicles: (i, j),
                approach: [
                    a.velocity.truncate().dot(n.truncate()),
                    -b.velocity.truncate().dot(n.truncate()),
                ],
            });
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
                let center =
                    constraints.iter().map(|c| c.0).sum::<Vec3>() / constraints.len() as f32;
                a.crash_damage(center, n, impact * inv_a / sum);
                b.crash_damage(a.position + center - b.position, -n, impact * inv_b / sum);
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

    #[test]
    fn aircraft_clear_a_bike_in_open_space_but_contact_the_fuselage_and_gear() {
        use super::super::{aircraft::Model, catalog};
        let hermes = crate::hermes::Hermes::test_map();
        let bike_id = catalog::definitions()
            .iter()
            .position(|d| d.is_bicycle())
            .unwrap();
        for model in Model::ALL {
            let vehicle = |id| {
                let mut v = Vehicle::spawn(hermes.city());
                v.set_catalog_model(Some(id));
                v.yaw = 0.72;
                v
            };
            let plane = vehicle(model.catalog_index());
            let bike = vehicle(bike_id);
            // The top of the bike overlaps the deployed tire by 25 cm.
            let wheel_height = -plane.ride_height() - bike.height() + bike.ride_height() + 0.25;
            for &(x, y, height, gear, should_hit) in &[
                (0.30, 0.36, 0., 0., false),    // beside the nose
                (0., 0., 0., 0., true),         // fuselage
                (-0.11, 0.33, -0.62, 0., true), // wing
                (-0.11, 0.33, -3., 0., false),  // below the wing
                (0.32, 0., wheel_height, 0., true),
                (0.32, 0., wheel_height, 1., false),
            ] {
                let mut plane = vehicle(model.catalog_index());
                let mut bike = vehicle(bike_id);
                plane.position = Vec3::new(9., 13., 20.);
                plane.roll = 0.14;
                plane.aircraft_gear = gear;
                let local = Vec3::new(x * plane.length(), y * plane.width(), height);
                bike.position = plane.position + plane.orientation() * local;
                let mut vehicles = [plane, bike];
                assert_eq!(
                    !resolve_with_contacts(&mut vehicles).is_empty(),
                    should_hit,
                    "{model:?} at {local:?}, gear {gear}"
                );
            }
        }
    }

    #[test]
    fn repeated_vehicle_impacts_loosen_then_release_only_the_struck_panels() {
        use super::super::{
            catalog,
            vehicle::damage::{Piece, Zone},
        };
        let hermes = crate::hermes::Hermes::test_map();
        let model = catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-12")
            .unwrap();
        let mut a = Vehicle::spawn(hermes.city());
        a.set_catalog_model(Some(model));
        a.yaw = 0.;
        let mut cars = [a.clone(), a];
        for hit in 1..=3 {
            for v in &mut cars {
                v.angular_momentum = Vec3::ZERO;
                if let Some(d) = &mut v.damage {
                    d.tick(1.);
                }
            }
            cars[0].position = Vec3::Z * 2.;
            // Nose of the moving car meets the parked SUV's left front quarter.
            cars[1].yaw = std::f32::consts::FRAC_PI_2;
            cars[1].position = cars[0].position
                + Vec3::new((cars[0].length() + cars[1].width()) * 0.5 - 0.05, -1.4, 0.);
            cars[0].velocity = Vec3::X * 22.;
            cars[1].velocity = Vec3::ZERO;
            resolve(&mut cars);
            for (v, zone) in cars.iter().zip([Zone::Nose, Zone::LeftFront]) {
                let d = v.damage.as_ref().unwrap();
                assert!(d.strain(zone) > 0.);
            }
            let panel = catalog::definitions()[model]
                .crash_pieces()
                .find(|(_, p, _)| *p == Piece::Panel(Zone::Nose))
                .unwrap()
                .0;
            assert_eq!(
                cars[0]
                    .damage
                    .as_ref()
                    .unwrap()
                    .loose
                    .iter()
                    .any(|p| p.part == panel),
                hit == 3
            );
            assert!(cars[1].damage.as_ref().unwrap().loose.is_empty());
        }
    }

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
