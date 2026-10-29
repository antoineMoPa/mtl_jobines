use super::*;

#[test]
fn forward_performance_reaches_highway_speed_and_settles_near_200_kmh() {
    // Six kilometres of flat road keeps the full run supported and collision free.
    let city = City::from_program(
        &opencity::parse("road r: carlane; end r(-30,0,30,0);").unwrap(),
        None,
    )
    .unwrap();
    for dt in [1. / 120., 1. / 30.] {
        let mut car = Vehicle::spawn(&city);
        car.position = Vec3::new(-2500., 0., RIDE_HEIGHT);
        car.yaw = 0.;
        let input = Controls {
            throttle: 1.,
            ..Default::default()
        };
        let mut speed_at_55_seconds = 0.;
        for frame in 0..(60. / dt) as usize {
            car.step(&city, input, dt);
            let elapsed = (frame + 1) as f32 * dt;
            if (elapsed - 5.).abs() < dt * 0.5 {
                assert!(
                    car.velocity.x * 3.6 > 100.,
                    "0–100 km/h must take under 5 s: {:?}",
                    car.velocity
                );
            }
            if (elapsed - 55.).abs() < dt * 0.5 {
                speed_at_55_seconds = car.velocity.x;
            }
        }
        let kmh = car.velocity.x * 3.6;
        assert!(
            (195. ..=210.).contains(&kmh),
            "top speed: {kmh} km/h at dt={dt}"
        );
        assert!((car.velocity.x - speed_at_55_seconds).abs() < 0.1);
        assert!(car.grounded);
        assert_eq!(car.collisions, 0);
        // Service brakes must still stop the car from its higher cruising speed.
        for _ in 0..(4. / dt) as usize {
            car.step(
                &city,
                Controls {
                    brake: true,
                    ..Default::default()
                },
                dt,
            );
            assert!(car.velocity.x >= 0.);
        }
        assert!(
            car.velocity.length() < 0.1,
            "high-speed braking: {:?}",
            car.velocity
        );
    }
}

#[test]
fn small_catalog_vehicles_have_brisk_arcade_launches() {
    let city = City::from_program(
        &opencity::parse("road r: carlane; end r(-30,0,30,0);").unwrap(),
        None,
    )
    .unwrap();
    for (id, def) in crate::dynamic_world::catalog::definitions()
        .iter()
        .enumerate()
        .filter(|(_, d)| d.config.road_vehicle && d.config.physics.mass < 5000.)
    {
        for dt in [1. / 120., 1. / 30.] {
            let mut car = Vehicle::spawn(&city);
            car.definition = Some(id);
            car.position = Vec3::new(-2500., 0., RIDE_HEIGHT);
            car.yaw = 0.;
            let mut to_50 = None;
            let mut to_100 = None;
            for frame in 0..(6. / dt) as usize {
                car.step(
                    &city,
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    dt,
                );
                let elapsed = (frame + 1) as f32 * dt;
                if car.velocity.x * 3.6 >= 50. {
                    to_50.get_or_insert(elapsed);
                }
                if car.velocity.x * 3.6 >= 100. {
                    to_100 = Some(elapsed);
                    break;
                }
            }
            let to_50 = to_50.expect("small vehicle must reach 50 km/h");
            let to_100 = to_100.expect("small vehicle must reach 100 km/h within 6 seconds");
            assert!(to_50 <= 2.3, "{}: 0–50 takes {to_50}s", car.name());
            assert!(to_100 <= 5.5, "{}: 0–100 takes {to_100}s", car.name());
            println!(
                "{} dt={dt}: 0–50 {to_50:.2}s, 0–100 {to_100:.2}s",
                car.name()
            );
        }
    }
}
