use super::*;

fn flat_city() -> City {
    City::from_program(
        &opencity::parse("road r: carlane; end r(-2,0,2,0); land(-3,-3,3,-3,3,3,-3,3);").unwrap(),
        None,
    )
    .unwrap()
}

fn energy(car: &Vehicle) -> f32 {
    car.velocity.length_squared() + car.angular_momentum.dot(car.angular_velocity())
}

#[test]
fn free_tumble_conserves_world_momentum_and_energy_at_both_frame_rates() {
    let city = flat_city();
    for dt in [1. / 120., 1. / 30.] {
        let mut car = Vehicle::spawn(&city);
        car.position = Vec3::new(0., 0., 100.);
        car.yaw = 0.4;
        car.pitch = 0.6;
        car.roll = -0.3;
        car.angular_momentum = Vec3::new(0.7, -1.3, 0.8);
        let momentum = car.angular_momentum;
        let rotation = car.orientation();
        let spin_energy = momentum.dot(car.angular_velocity());
        for _ in 0..(2. / dt) as usize {
            car.step(
                &city,
                Controls {
                    steering: 1.,
                    throttle: 1.,
                    brake: true,
                    handbrake: true,
                },
                dt,
            );
            assert!(!car.grounded);
            assert_eq!(car.angular_momentum, momentum);
            let error = (momentum.dot(car.angular_velocity()) / spin_energy - 1.).abs();
            assert!(error < 0.002, "rotational energy drift: {error}");
            assert!(car.position.is_finite() && car.orientation().is_finite());
        }
        assert!(car.orientation().dot(rotation).abs() < 0.9);
        assert_eq!(car.velocity.truncate(), Vec2::ZERO);
    }
}

#[test]
fn pitch_and_roll_pass_through_a_full_turn_without_stopping_in_air() {
    let city = flat_city();
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        let mut car = Vehicle::spawn(&city);
        car.position = Vec3::new(0., 0., 100.);
        car.yaw = 0.;
        let inertia = Vec3::new(
            (car.width().powi(2) + car.height().powi(2)) / 12. + 0.175_f32.powi(2),
            (car.length().powi(2) + car.height().powi(2)) / 12. + 0.175_f32.powi(2),
            car.inertia(),
        );
        car.angular_momentum = axis * inertia * std::f32::consts::TAU;
        for frame in 0..120 {
            car.step(&city, Controls::default(), 1. / 120.);
            let expected =
                Quat::from_axis_angle(axis, std::f32::consts::TAU * (frame + 1) as f32 / 120.);
            assert!(car.orientation().dot(expected).abs() > 0.9999);
            if axis != Vec3::Z {
                assert!(
                    car.yaw.abs() < 0.001,
                    "heading flipped during a tumble: {}",
                    car.yaw
                );
            }
        }
    }
}

#[test]
fn losing_front_wheel_support_pitches_the_nose_down() {
    let city = City::from_program(
        &opencity::parse("road r: carlane; end r(-2,0,0,0);").unwrap(),
        None,
    )
    .unwrap();
    let mut car = Vehicle::spawn(&city);
    car.position = Vec3::new(0., 0., car.ride_height());
    car.yaw = 0.;
    car.step(&city, Controls::default(), 1. / 30.);
    assert!(car.pitch < -0.001, "nose stayed level: {}", car.pitch);
    assert!(car.angular_velocity().y > 0.1);
    assert!(car.velocity.z < 0.);
}

#[test]
fn fast_falls_cannot_skip_the_ground_between_substeps() {
    let city = flat_city();
    for roll in [0., std::f32::consts::FRAC_PI_2, std::f32::consts::PI] {
        let mut car = Vehicle::spawn(&city);
        car.position = Vec3::new(0., 0., 2.);
        car.yaw = 0.;
        car.roll = roll;
        car.velocity.z = -200.;
        car.step(&city, Controls::default(), 1. / 30.);
        assert!(
            car.position.z > 0.3,
            "fall skipped floor at roll={roll}: {:?}",
            car.position
        );
        assert!(car.velocity.z > -1.);
        assert!(car.angular_velocity().is_finite());
    }
}

#[test]
fn rotating_corners_strike_walls_even_with_a_stationary_centre() {
    let city = City::from_program(
        &opencity::parse("road r: carlane; end r(-2,0,2,0); building(0.2,0,-1,1,-1,1,1,0,1);")
            .unwrap(),
        None,
    )
    .unwrap();
    let mut car = Vehicle::spawn(&city);
    car.yaw = 0.7;
    car.position = Vec3::new(-1.7, 0., 5.);
    car.angular_momentum.z = -car.inertia();
    let initial = energy(&car);
    car.move_with_building_contacts(&city, 1. / 120.);
    assert!(
        car.velocity.x < 0.,
        "spinning corner had no collision response"
    );
    assert!(energy(&car) <= initial + 0.001);
}

#[test]
fn tilted_wall_impacts_transfer_torque_without_creating_energy() {
    let city = City::from_program(
        &opencity::parse("road r: carlane; end r(-2,0,2,0); building(0.2,0,-1,1,-1,1,1,0,1);")
            .unwrap(),
        None,
    )
    .unwrap();
    for roll in [-1., 0.5, 2.5] {
        let mut car = Vehicle::spawn(&city);
        car.yaw = 0.3;
        car.pitch = 0.4;
        car.roll = roll;
        car.position = Vec3::new(-3., 0., 5.);
        car.velocity = Vec3::new(20., 3., -1.);
        let initial = energy(&car);
        for _ in 0..60 {
            car.move_with_building_contacts(&city, 1. / 120.);
            assert!(energy(&car) <= initial * 1.002);
            let forward = Vec2::new(car.yaw.cos(), car.yaw.sin());
            assert!(car.building_contact(&city, forward).is_none());
        }
        assert!(car.velocity.x < 0.);
        assert!(car.angular_momentum.truncate().length() > 0.1);
    }
}

#[test]
fn tilted_landing_transfers_vertical_motion_into_rotation_and_settles() {
    let city = flat_city();
    for dt in [1. / 120., 1. / 30.] {
        let mut car = Vehicle::spawn(&city);
        car.position = Vec3::new(0., 0., 5.);
        car.yaw = 0.;
        car.pitch = 0.25;
        car.roll = 0.35;
        car.velocity.z = -8.;
        let initial_energy = energy(&car) + 2. * 9.81 * car.position.z;
        let mut landed = false;
        let mut saw_torque = false;
        for _ in 0..(5. / dt) as usize {
            car.step(&city, Controls::default(), dt);
            landed |= car.grounded;
            saw_torque |= car.angular_momentum.truncate().length() > 0.05;
            assert!(car.position.z > -0.01);
            assert!(energy(&car) + 2. * 9.81 * car.position.z < initial_energy + 1.);
        }
        assert!(landed && saw_torque);
        assert!(
            car.velocity.length() < 0.1,
            "landing velocity {:?}",
            car.velocity
        );
        assert!(car.angular_velocity().length() < 0.1);
        assert!(car.pitch.abs() < 0.01 && car.roll.abs() < 0.01);
    }
}

#[test]
fn overturned_car_lands_on_its_roof_and_cannot_drive_or_automatically_right_itself() {
    let city = flat_city();
    for dt in [1. / 120., 1. / 30.] {
        let mut car = Vehicle::spawn(&city);
        car.position = Vec3::new(0., 0., 5.);
        car.yaw = 0.;
        car.roll = std::f32::consts::PI;
        for _ in 0..(5. / dt) as usize {
            car.step(
                &city,
                Controls {
                    throttle: 1.,
                    steering: 1.,
                    ..Default::default()
                },
                dt,
            );
            let q = car.orientation();
            for x in [-car.length() * 0.5, car.length() * 0.5] {
                for y in [-car.width() * 0.5, car.width() * 0.5] {
                    let roof = car.position + q * Vec3::new(x, y, car.height() - car.ride_height());
                    assert!(roof.z >= -0.001, "roof passed through ground: {roof:?}");
                }
            }
        }
        assert!(!car.grounded);
        assert!((car.orientation() * Vec3::Z).z < -0.99);
        assert!(
            car.velocity.length() < 0.1,
            "roof velocity {:?}",
            car.velocity
        );
        assert!(car.angular_velocity().length() < 0.1);
    }
}
