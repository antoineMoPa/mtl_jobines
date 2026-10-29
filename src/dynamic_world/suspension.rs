//! Arcade suspension profiles: long bodies need smaller angles and quicker settling.
use super::Vehicle;
use glam::Vec2;

#[derive(Clone, Copy)]
pub(super) struct Suspension {
    pub heave_spring: f32,
    pub heave_damping: f32,
    pub lean_response: Vec2,
    pub lean_limit: Vec2,
    pub lean_spring: f32,
    pub lean_damping: f32,
}
const PROTOTYPE: Suspension = Suspension {
    heave_spring: 90.,
    heave_damping: 14.,
    lean_response: Vec2::new(0.012, 0.009),
    lean_limit: Vec2::new(0.12, 0.15),
    lean_spring: 64.,
    lean_damping: 10.,
};
const ROAD: Suspension = Suspension {
    heave_spring: 196.,
    heave_damping: 28.,
    lean_response: Vec2::new(0.007, 0.006),
    lean_limit: Vec2::new(0.08, 0.10),
    lean_spring: 144.,
    lean_damping: 24.,
};
const HEAVY: Suspension = Suspension {
    heave_spring: 324.,
    heave_damping: 36.,
    lean_response: Vec2::new(0.003, 0.003),
    lean_limit: Vec2::new(0.03, 0.04),
    lean_spring: 225.,
    lean_damping: 30.,
};
pub(super) fn profile(vehicle: &Vehicle) -> Suspension {
    match vehicle.definition() {
        Some(def) if def.config.physics.mass >= 5000. => HEAVY,
        Some(_) => ROAD,
        None => PROTOTYPE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{catalog, Controls};
    use glam::Vec3;
    use opencity::runtime::City;

    fn flat_city() -> City {
        City::from_program(
            &opencity::parse("road r: carlane; end r(-30,0,30,0); land(-31,-1,31,-1,31,1,-31,1);")
                .unwrap(),
            None,
        )
        .unwrap()
    }
    #[test]
    fn heavy_vehicles_launch_quickly_and_keep_the_nose_controlled() {
        let city = flat_city();
        for (id, def) in catalog::definitions()
            .iter()
            .enumerate()
            .filter(|(_, d)| d.config.road_vehicle && d.config.physics.mass >= 5000.)
        {
            for dt in [1. / 120., 1. / 30.] {
                let mut car = Vehicle::spawn(&city);
                car.definition = Some(id);
                car.position = Vec3::new(-2500., 0., car.ride_height());
                car.yaw = 0.;
                for _ in 0..(4. / dt) as usize {
                    car.step(
                        &city,
                        Controls {
                            throttle: 1.,
                            ..Default::default()
                        },
                        dt,
                    );
                    let nose_travel = car.body_lean.x.abs().sin() * car.length() * 0.5;
                    assert!(
                        nose_travel < 0.2,
                        "{} nose travel: {nose_travel}",
                        car.name()
                    );
                }
                assert!(
                    car.velocity.x > 12.,
                    "{} launch: {} m/s",
                    car.name(),
                    car.velocity.x
                );
                for _ in 0..(16. / dt) as usize {
                    car.step(
                        &city,
                        Controls {
                            throttle: 1.,
                            ..Default::default()
                        },
                        dt,
                    );
                }
                assert!(
                    car.velocity.x > 23.,
                    "{} cruising: {} m/s",
                    car.name(),
                    car.velocity.x
                );
            }
        }
    }
    #[test]
    fn heavy_suspension_settles_without_a_slow_rebound() {
        let city = flat_city();
        for dt in [1. / 120., 1. / 30.] {
            let mut car = Vehicle::spawn(&city);
            car.definition = Some(9); // Bus: the longest road body.
            car.position = Vec3::new(-2500., 0., car.ride_height() + 0.15);
            car.yaw = 0.;
            car.body_lean = Vec2::new(0.03, 0.04);
            for _ in 0..(0.5 / dt) as usize {
                car.step(&city, Controls::default(), dt);
                assert!(car.body_lean.min_element() >= 0., "suspension rebounded");
            }
            assert!(car.body_lean.length() < 0.001);
            assert!((car.position.z - car.ride_height()).abs() < 0.005);
            assert!(car.velocity.z.abs() < 0.05);
        }
    }
}
