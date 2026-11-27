use super::*;
fn map() -> City {
    City::from_program(
        &opencity::parse("road street: sidewalk carlane sidewalk; end street(-25,3,-20,3);")
            .unwrap(),
        None,
    )
    .unwrap()
}
fn planar_energy(vehicle: &Vehicle) -> f32 {
    vehicle.velocity.truncate().length_squared()
        + CHASSIS_YAW_INERTIA * vehicle.angular_velocity().z.powi(2)
}

#[test]
fn reverse_launch_is_prompt_and_retains_speed_limit() {
    let city = map();
    for definition in [None, Some(1)] {
        let mut car = Vehicle::spawn(&city);
        car.set_catalog_model(definition);
        car.yaw = 0.;
        let controls = Controls {
            throttle: -1.,
            ..Default::default()
        };
        for _ in 0..60 {
            car.step(&city, controls, 1. / 120.);
        }
        assert!(car.velocity.x < -2.8, "reverse launch: {:?}", car.velocity);
        for _ in 0..1200 {
            car.step(&city, controls, 1. / 120.);
            assert!(car.velocity.x >= -10., "reverse speed: {:?}", car.velocity);
        }
    }
}

#[test]
fn road_barriers_stop_fast_cars_but_leave_underpasses_clear() {
    for height in [0., 0.08] {
        let city = City::from_program(
            &opencity::parse(&format!(
                "road r: verge carlane verge; role highway; end s=r(-2,0,2,0); \
                    elevation(s,{height});"
            ))
            .unwrap(),
            None,
        )
        .unwrap();
        let edge = city
            .road_barriers
            .iter()
            .map(|w| w.start.point.y)
            .fold(0_f32, f32::max)
            * METERS_PER_UNIT;
        for sign in [-1., 1.] {
            let mut car = Vehicle::spawn(&city);
            car.position = Vec3::new(0., 0., height * METERS_PER_UNIT + RIDE_HEIGHT);
            car.yaw = 0.;
            car.velocity = Vec3::new(0., sign * 80., 0.);
            car.move_with_building_contacts(&city, 0.2);
            assert!(car.collisions > 0);
            assert!(car.position.y.abs() < edge);
            // The broadside hit can transfer some yaw back to translation as
            // the opposite corner contacts the barrier during the sweep.
            assert!(
                (-3. ..0.).contains(&(car.velocity.y * sign)),
                "barrier rebound at height={height}, sign={sign}: {:?}",
                car.velocity
            );
        }
        if height > 0. {
            let mut car = Vehicle::spawn(&city);
            car.position = Vec3::new(0., 0., RIDE_HEIGHT);
            car.velocity = Vec3::new(0., 80., 0.);
            car.move_with_building_contacts(&city, 0.2);
            assert_eq!(car.collisions, 0);
            assert!(car.position.y > edge);
        }
    }
}

#[test]
fn suspension_squats_dives_and_settles_without_moving_wheels() {
    let city = map();
    let mut car = Vehicle::spawn(&city);
    car.yaw = 0.;
    for _ in 0..90 {
        car.step(
            &city,
            Controls {
                throttle: 1.,
                ..Default::default()
            },
            1. / 120.,
        );
    }
    assert!(
        car.body_lean.x > 0.025,
        "acceleration should raise the nose"
    );
    assert!(car.body_lean.y.abs() < 0.001);
    let wheels = car.wheel_contacts(&city).map(|(p, _)| p);
    let leaning = car.mesh();
    car.body_lean = Vec2::ZERO;
    let level = car.mesh();
    assert_ne!(leaning.vertices[0].position, level.vertices[0].position);
    // The three chassis cuboids use 108 vertices; tires follow them.
    for (a, b) in leaning.vertices[108..].iter().zip(&level.vertices[108..]) {
        assert_eq!(a.position, b.position);
    }
    assert_eq!(wheels, car.wheel_contacts(&city).map(|(p, _)| p));
    for _ in 0..40 {
        car.step(
            &city,
            Controls {
                brake: true,
                ..Default::default()
            },
            1. / 120.,
        );
    }
    assert!(car.body_lean.x < -0.02, "braking should lower the nose");
    for _ in 0..600 {
        car.step(
            &city,
            Controls {
                brake: true,
                ..Default::default()
            },
            1. / 120.,
        );
    }
    assert!(car.body_lean.length() < 0.001);
    assert!(car.body_lean_rate.length() < 0.001);
    assert!(car.grounded);
}

#[test]
fn suspension_rolls_outside_both_turns_and_releases_in_air() {
    let city = map();
    for direction in [-1., 1.] {
        let mut car = Vehicle::spawn(&city);
        car.yaw = 0.;
        car.velocity = Vec3::X * 12.;
        for _ in 0..45 {
            car.step(
                &city,
                Controls {
                    steering: direction,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        assert!(car.body_lean.y * direction > 0.01);
        assert!(car.body_lean.length() < 0.25);
        car.position.z += 100.;
        for _ in 0..120 {
            car.step(
                &city,
                Controls {
                    steering: direction,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        assert!(!car.grounded);
        assert!(car.body_lean.length() < 0.001);
    }
}

#[test]
fn turns_build_yaw_and_bend_travel_within_surface_grip() {
    let city = map();
    for speed in [-30.0_f32, -20., -10., 10., 20., 30.] {
        for steering in [-1.0_f32, -0.1, 0.1, 1.] {
            let mut vehicle = Vehicle::spawn(&city);
            vehicle.yaw = 0.;
            vehicle.velocity = Vec3::X * speed;
            vehicle.steering = steering * 0.52 / (1. + speed.abs() * STEERING_SPEED_TAPER);
            let start = vehicle.position;
            for _ in 0..30 {
                let energy = planar_energy(&vehicle);
                vehicle.step(
                    &city,
                    Controls {
                        steering,
                        ..Default::default()
                    },
                    1. / 120.,
                );
                assert!(vehicle.grounded);
                assert!(vehicle.position.is_finite() && vehicle.velocity.is_finite());
                assert!(planar_energy(&vehicle) <= energy + 1e-4);
            }
            let travel = vehicle.velocity.truncate() * speed.signum();
            let travel_heading = travel.y.atan2(travel.x);
            let chord = vehicle.position.truncate().distance(start.truncate());
            let radius = chord / (2. * (travel_heading.abs() * 0.5).sin());
            assert_eq!(vehicle.yaw.signum(), (speed * steering).signum());
            if speed > 0. {
                assert_eq!(travel_heading.signum(), steering.signum());
            }
            // In reverse the front contact initially deflects the tail's
            // travel outward while its torque builds the opposite body yaw.
            // Curvature follows actual travel, including transient sideslip.
            let grip_radius = speed.powi(2) / (0.95 * 9.81 * LATERAL_GRIP_SCALE);
            let geometric_radius = WHEELBASE / vehicle.steering.tan().abs();
            assert!(
                radius > grip_radius * 0.75,
                "speed={speed}, radius={radius}"
            );
            if speed > 0. {
                assert!(radius < geometric_radius.max(grip_radius) * 8.);
            }
            assert!(vehicle.yaw.abs() > 0.001 * steering.abs());
        }
    }
}

#[test]
fn asphalt_recovers_from_sideways_slip_without_losing_forward_motion() {
    let road = format!(
        "road street: {}; end street(-25,3,-20,3);",
        "carlane ".repeat(30)
    );
    let city = City::from_program(&opencity::parse(&road).unwrap(), None).unwrap();
    for direction in [-1., 1.] {
        let mut car = Vehicle::spawn(&city);
        car.position.y = 300.;
        car.yaw = 0.;
        car.velocity = Vec3::new(12., direction * 8., 0.);
        let initial_energy = planar_energy(&car);
        for _ in 0..30 {
            car.step(&city, Controls::default(), 1. / 120.);
            assert!(car.grounded);
            assert_eq!(car.surface, SurfaceKind::Asphalt);
            assert!(planar_energy(&car) <= initial_energy);
        }
        assert!(
            car.velocity.y.abs() < 3.85,
            "sideways velocity: {}",
            car.velocity.y
        );
        assert!(car.velocity.x > 11.5);
    }
}

#[test]
fn city_speed_turns_keep_side_slip_moderate() {
    let road = format!(
        "road street: {}; end street(-25,3,-20,3);",
        "carlane ".repeat(30)
    );
    let city = City::from_program(&opencity::parse(&road).unwrap(), None).unwrap();
    for direction in [-1., 1.] {
        let mut car = Vehicle::spawn(&city);
        car.position.y = 300.;
        car.yaw = 0.;
        car.velocity = Vec3::X * 10.;
        let mut max_slip = 0.0_f32;
        for _ in 0..120 {
            car.step(
                &city,
                Controls {
                    steering: direction,
                    ..Default::default()
                },
                1. / 120.,
            );
            let forward = Vec2::new(car.yaw.cos(), car.yaw.sin());
            let velocity = car.velocity.truncate();
            max_slip = max_slip.max(
                forward
                    .perp_dot(velocity)
                    .abs()
                    .atan2(forward.dot(velocity).abs()),
            );
            assert_eq!(car.surface, SurfaceKind::Asphalt);
        }
        assert!(
            max_slip.to_degrees() < 10.,
            "slip: {} degrees",
            max_slip.to_degrees()
        );
        assert!(car.yaw.abs() > 0.8, "car must still turn readily");
    }
}

#[test]
fn strong_sideways_slide_settles_quickly_on_asphalt() {
    let road = format!(
        "road street: {}; end street(-25,3,-20,3);",
        "carlane ".repeat(40)
    );
    let city = City::from_program(&opencity::parse(&road).unwrap(), None).unwrap();
    for direction in [-1., 1.] {
        for throttle in [0., 1.] {
            let mut car = Vehicle::spawn(&city);
            car.position.y = 300.;
            car.yaw = 0.;
            car.velocity = Vec3::new(20., direction * 10., 0.);
            for _ in 0..48 {
                car.step(
                    &city,
                    Controls {
                        throttle,
                        ..Default::default()
                    },
                    1. / 120.,
                );
                assert!(car.grounded);
            }
            assert!(
                car.velocity.y.abs() < if throttle > 0. { 3.5 } else { 2.5 },
                "throttle={throttle}, sideways velocity: {}",
                car.velocity.y
            );
            assert!(car.velocity.x > 19.);
        }
    }
}

#[test]
fn steering_accelerates_yaw_without_snapping_momentum_to_the_body() {
    let city = map();
    for speed in [-5.0_f32, 5., 20.] {
        for direction in [-1.0, 1.0] {
            let mut vehicle = Vehicle::spawn(&city);
            vehicle.yaw = 0.;
            vehicle.velocity = Vec3::X * speed;
            vehicle.steering = direction * 0.4;
            let energy = planar_energy(&vehicle);
            vehicle.step(
                &city,
                Controls {
                    steering: direction,
                    ..Default::default()
                },
                1. / 120.,
            );
            assert_eq!(
                vehicle.angular_velocity().z.signum(),
                (speed * direction).signum()
            );
            let geometric_rate = speed / WHEELBASE * vehicle.steering.tan();
            assert!(vehicle.angular_velocity().z.abs() < geometric_rate.abs());
            let travel = vehicle.velocity.truncate() * speed.signum();
            let heading = travel.y.atan2(travel.x);
            assert!((heading - vehicle.yaw).abs() > 1e-5);
            assert!(planar_energy(&vehicle) <= energy);
            // Releasing steering does not discard angular momentum.
            let rate = vehicle.angular_velocity().z;
            vehicle.step(&city, Controls::default(), 1. / 120.);
            assert_eq!(vehicle.angular_velocity().z.signum(), rate.signum());
        }
    }
}
#[test]
fn braking_respects_surface_grip_and_never_reverses() {
    let city = City::from_program(
        &opencity::parse(
            "road street: carlane; end street(-25,3,-20,3); ground=land(-26,2,\
                -19,2,-19,5,-26,5); elevation(ground,0.02);",
        )
        .unwrap(),
        None,
    )
    .unwrap();
    let mut asphalt = Vehicle::spawn(&city);
    let mut grass = Vehicle::spawn(&city);
    grass.position.y += 20.;
    asphalt.velocity = Vec3::X * 20.;
    grass.velocity = Vec3::X * 20.;
    let input = Controls {
        brake: true,
        throttle: 1.,
        ..Default::default()
    };
    let dt = 1. / 120.;
    asphalt.step(&city, input, dt);
    grass.step(&city, input, dt);
    assert!(asphalt.velocity.x < grass.velocity.x);
    // Exclude rolling/air resistance when checking the tire braking budget.
    let deceleration = (20. - asphalt.velocity.x) / dt;
    assert!(
        deceleration - 0.015 * 9.81 - AIR_DRAG * 20_f32.powi(2)
            < 0.95 * 9.81 * SERVICE_BRAKE_GRIP_SCALE
    );
    for _ in 0..600 {
        asphalt.step(&city, input, dt);
        grass.step(&city, input, dt);
        assert!(asphalt.velocity.x >= 0.);
        assert!(grass.velocity.x >= 0.);
    }
    assert!(asphalt.velocity.length() < 0.01);
    assert!(grass.velocity.length() < 0.01);
}

#[test]
fn service_brake_stops_promptly_in_forward_and_reverse() {
    let city = City::from_program(
        &opencity::parse(
            "road street: carlane; end street(-25,3,-20,3); ground=land(-26,2,\
                -19,2,-19,5,-26,5); elevation(ground,0.02);",
        )
        .unwrap(),
        None,
    )
    .unwrap();
    for speed in [-20., 20.] {
        for on_grass in [false, true] {
            let mut vehicle = Vehicle::spawn(&city);
            if on_grass {
                vehicle.position.y += 20.;
            }
            vehicle.velocity = Vec3::X * speed;
            let start = vehicle.position;
            let mut elapsed = 0.;
            while vehicle.velocity.truncate().length() > 0.01 && elapsed < 4. {
                vehicle.step(
                    &city,
                    Controls {
                        brake: true,
                        throttle: speed.signum(),
                        ..Default::default()
                    },
                    1. / 120.,
                );
                elapsed += 1. / 120.;
                assert!(vehicle.velocity.x * speed >= 0., "braking reversed travel");
            }
            let distance = vehicle.position.truncate().distance(start.truncate());
            eprintln!(
                "service stop speed={speed}, grass={on_grass}: {elapsed:.2}s, {distance:.2}m"
            );
            assert!(elapsed < if on_grass { 2.8 } else { 1.4 });
            assert!(distance < if on_grass { 28. } else { 14. });
        }
    }
}

fn flat_tires() -> [Option<TireContact>; 4] {
    [
        Vec2::new(-WHEELBASE * 0.5, -HALF_TRACK),
        Vec2::new(-WHEELBASE * 0.5, HALF_TRACK),
        Vec2::new(WHEELBASE * 0.5, -HALF_TRACK),
        Vec2::new(WHEELBASE * 0.5, HALF_TRACK),
    ]
    .map(|arm| {
        Some(TireContact {
            arm,
            forward: Vec2::X,
            grip: 0.95 * 9.81 * 0.25,
            rolling: 0.,
            rear: arm.x < 0.,
        })
    })
}

#[test]
fn acceleration_and_slide_recovery_share_each_tires_force_budget() {
    let city = map();
    let mut coasting = Vehicle::spawn(&city);
    let mut powered = Vehicle::spawn(&city);
    let mut coast_velocity = Vec2::new(10., 3.);
    let mut power_velocity = coast_velocity;
    coasting.apply_tire_forces(
        &flat_tires(),
        Controls::default(),
        &mut coast_velocity,
        1. / 120.,
    );
    powered.apply_tire_forces(
        &flat_tires(),
        Controls {
            throttle: 1.,
            ..Default::default()
        },
        &mut power_velocity,
        1. / 120.,
    );
    assert!(coast_velocity.y < power_velocity.y);
    assert!(power_velocity.y < 3. && power_velocity.y > 0.);
    assert!(power_velocity.x > coast_velocity.x);

    // One isolated contact exposes the actual force and its moment arm.
    for brake in [false, true] {
        let mut vehicle = Vehicle::spawn(&city);
        let tire = flat_tires()[0].unwrap();
        let mut velocity = Vec2::new(10., 3.);
        let initial = velocity;
        let dt = 1. / 120.;
        vehicle.apply_tire_forces(
            &[Some(tire), None, None, None],
            Controls {
                throttle: 1.,
                brake,
                ..Default::default()
            },
            &mut velocity,
            dt,
        );
        let impulse = velocity - initial;
        let longitudinal_axis = tire.grip * if brake { SERVICE_BRAKE_GRIP_SCALE } else { 1. } * dt;
        let lateral_axis = tire.grip * LATERAL_GRIP_SCALE * dt;
        let usage = (impulse.x / longitudinal_axis).powi(2) + (impulse.y / lateral_axis).powi(2);
        assert!(usage <= 1.0001, "ellipse usage={usage}");
        assert!(
            (vehicle.angular_velocity().z * CHASSIS_YAW_INERTIA - tire.arm.perp_dot(impulse)).abs()
                < 1e-5
        );
    }
}

#[test]
fn tires_damp_spin_but_steering_cannot_rotate_a_stationary_vehicle() {
    let city = map();
    let mut vehicle = Vehicle::spawn(&city);
    for _ in 0..120 {
        vehicle.step(
            &city,
            Controls {
                steering: 1.,
                ..Default::default()
            },
            1. / 120.,
        );
    }
    assert_eq!(vehicle.angular_velocity().z, 0.);
    assert_eq!(vehicle.velocity.truncate(), Vec2::ZERO);
    vehicle.angular_momentum.z = vehicle.inertia();
    let mut velocity = Vec2::ZERO;
    for _ in 0..120 {
        vehicle.apply_tire_forces(&flat_tires(), Controls::default(), &mut velocity, 1. / 120.);
    }
    assert!(vehicle.angular_velocity().z.abs() < 0.001);
    assert!(velocity.length() < 0.001);
}

#[test]
fn split_grip_braking_turns_toward_the_grippier_side() {
    let city = map();
    for grippy_side in [-1_f32, 1.] {
        let mut vehicle = Vehicle::spawn(&city);
        let mut tires = flat_tires();
        for tire in tires.iter_mut().flatten() {
            if tire.arm.y * grippy_side < 0. {
                tire.grip *= 0.2;
            }
        }
        let mut velocity = Vec2::X * 20.;
        vehicle.apply_tire_forces(
            &tires,
            Controls {
                brake: true,
                ..Default::default()
            },
            &mut velocity,
            1. / 120.,
        );
        assert!(vehicle.angular_velocity().z * grippy_side > 0.);
        assert!(velocity.x < 20.);
    }
}

#[test]
fn rear_handbrake_reduces_rear_lateral_authority_and_increases_rotation() {
    let city = map();
    let mut coasting = Vehicle::spawn(&city);
    let mut handbraking = Vehicle::spawn(&city);
    let mut coast_velocity = Vec2::new(10., 4.);
    let mut brake_velocity = coast_velocity;
    coasting.apply_tire_forces(
        &flat_tires(),
        Controls::default(),
        &mut coast_velocity,
        1. / 120.,
    );
    handbraking.apply_tire_forces(
        &flat_tires(),
        Controls {
            handbrake: true,
            ..Default::default()
        },
        &mut brake_velocity,
        1. / 120.,
    );
    assert!(handbraking.angular_velocity().z < coasting.angular_velocity().z - 0.01);
    assert!(brake_velocity.y > coast_velocity.y);
    assert!(brake_velocity.x < coast_velocity.x);
    assert!(
        brake_velocity.length_squared()
            + CHASSIS_YAW_INERTIA * handbraking.angular_velocity().z.powi(2)
            < 116.
    );
}

#[test]
fn passive_tires_dissipate_translation_and_spin_for_varied_slip_and_support() {
    let city = map();
    for mask in 0..16 {
        for speed in [-30., -0.01, 0., 0.01, 30.] {
            for slip in [-5., 0., 5.] {
                for spin in [-2_f32, 0., 2.] {
                    for (brake, handbrake) in [(false, false), (true, false), (false, true)] {
                        let mut vehicle = Vehicle::spawn(&city);
                        vehicle.angular_momentum.z = spin * vehicle.inertia();
                        let mut tires = flat_tires();
                        for (i, tire) in tires.iter_mut().enumerate() {
                            if mask & (1 << i) == 0 {
                                *tire = None;
                            } else if i >= 2 {
                                tire.as_mut().unwrap().forward =
                                    Vec2::new(0.4_f32.cos(), 0.4_f32.sin());
                            }
                        }
                        let mut velocity = Vec2::new(speed, slip);
                        let initial =
                            velocity.length_squared() + CHASSIS_YAW_INERTIA * spin.powi(2);
                        vehicle.apply_tire_forces(
                            &tires,
                            Controls {
                                brake,
                                handbrake,
                                ..Default::default()
                            },
                            &mut velocity,
                            1. / 120.,
                        );
                        let energy = velocity.length_squared()
                            + CHASSIS_YAW_INERTIA * vehicle.angular_velocity().z.powi(2);
                        assert!(
                            energy <= initial + 0.0001,
                            "mask={mask}, speed={speed}, slip={slip}, spin={spin}, \
                                brake={brake}, handbrake={handbrake}: {initial} -> {energy}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn airborne_rotation_preserves_angular_momentum_without_tire_authority() {
    let city = map();
    let mut vehicle = Vehicle::spawn(&city);
    vehicle.position.z += 20.;
    vehicle.velocity = Vec3::X * 10.;
    vehicle.angular_momentum.z = 0.8 * vehicle.inertia();
    let momentum = vehicle.angular_momentum;
    let yaw = vehicle.yaw;
    for _ in 0..60 {
        vehicle.step(
            &city,
            Controls {
                boost: false,
                steering: 1.,
                throttle: 1.,
                brake: true,
                handbrake: true,
                ..Controls::default()
            },
            1. / 120.,
        );
        assert!(!vehicle.grounded);
        assert_eq!(vehicle.angular_momentum, momentum);
        assert_eq!(vehicle.velocity.y, 0.);
    }
    assert!((vehicle.yaw - yaw - 0.4).abs() < 1e-5);
}

#[test]
fn only_supported_front_wheels_supply_steering_force() {
    let city = City::from_program(
        &opencity::parse("road street: verge verge verge carlane; end street(-25,3,-20,3);")
            .unwrap(),
        None,
    )
    .unwrap();
    let road = &city.roads[0];
    let grass = opencity::map::element_position(&road.segment, &road.definition, 1, 0.5).unwrap();
    for direction in [-1., 1.] {
        for on_grass in [false, true] {
            let mut vehicle = Vehicle::spawn(&city);
            if on_grass {
                vehicle.position.x = grass.x * METERS_PER_UNIT;
                vehicle.position.y = grass.y * METERS_PER_UNIT;
            } else {
                // Front wheels over the road's open end: half the normal load.
                vehicle.position.x = -2000.;
            }
            vehicle.yaw = 0.;
            vehicle.velocity = Vec3::X * 5.;
            vehicle.steering = direction * 0.5;
            vehicle.step(
                &city,
                Controls {
                    throttle: 1.,
                    steering: direction,
                    ..Default::default()
                },
                1. / 120.,
            );
            assert!(vehicle.grounded);
            assert_eq!(
                vehicle.surface,
                if on_grass {
                    SurfaceKind::Grass
                } else {
                    SurfaceKind::Asphalt
                }
            );
            assert!(
                vehicle.velocity.x > 5.,
                "turning must still allow acceleration"
            );
            if on_grass {
                assert!(
                    vehicle.angular_velocity().z * direction > 0.,
                    "grass lost steering"
                );
            } else {
                // With both steered wheels unsupported, steering cannot turn
                // the chassis or bend momentum through the rear tires.
                assert!(vehicle.angular_velocity().z.abs() < 1e-6);
                assert!(vehicle.velocity.y.abs() < 1e-6);
            }
        }
    }
}

#[test]
fn crossing_curbs_keeps_each_tire_above_the_surface_in_both_directions() {
    let city = City::from_program(
        &opencity::parse(
            "road street: verge verge verge sidewalk carlane sidewalk verge \
                verge verge; end street(-25,3,-20,3);",
        )
        .unwrap(),
        None,
    )
    .unwrap();
    let road = &city.roads[0];
    for band in [1, 7] {
        for speed in [5., 20.] {
            let start = opencity::map::element_position(&road.segment, &road.definition, band, 0.5)
                .unwrap();
            let end =
                opencity::map::element_position(&road.segment, &road.definition, 8 - band, 0.5)
                    .unwrap();
            let direction = (end.y - start.y).signum();
            let mut vehicle = Vehicle::spawn(&city);
            let height = city
                .surface_at([start.x, start.y], f32::INFINITY)
                .unwrap()
                .height;
            vehicle.position = Vec3::new(
                start.x * METERS_PER_UNIT,
                start.y * METERS_PER_UNIT,
                height * METERS_PER_UNIT + RIDE_HEIGHT,
            );
            vehicle.yaw = direction * std::f32::consts::FRAC_PI_2;
            vehicle.velocity = Vec3::Y * direction * speed;
            let mut saw_curb = false;
            let mut reached_other_side = false;
            for _ in 0..240 {
                vehicle.step(
                    &city,
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    1. / 30.,
                );
                for (wheel, sample) in vehicle.wheel_contacts(&city) {
                    let surface = sample.unwrap_or_else(|| {
                        panic!(
                            "wheel left road profile: band={band}, speed={speed}, \
                                position={:?}, wheel={wheel:?}",
                            vehicle.position
                        )
                    });
                    saw_curb |= surface.kind == SurfaceKind::Sidewalk;
                    assert!(
                        wheel.z >= surface.height * METERS_PER_UNIT - 1e-4,
                        "tire below curb: {wheel:?}, surface={surface:?}"
                    );
                }
                if (vehicle.position.y - end.y * METERS_PER_UNIT) * direction >= 0. {
                    reached_other_side = true;
                    break;
                }
            }
            assert!(saw_curb && reached_other_side);
        }
    }
}

#[test]
fn larger_frame_matches_fixed_substeps_and_airborne_has_no_tire_authority() {
    let city = map();
    let mut framed = Vehicle::spawn(&city);
    let mut fixed = Vehicle::spawn(&city);
    let input = Controls {
        throttle: 1.,
        steering: 0.3,
        ..Default::default()
    };
    for _ in 0..30 {
        framed.step(&city, input, 1. / 30.);
        for _ in 0..4 {
            fixed.step(&city, input, 1. / 120.);
        }
    }
    assert!(framed.position.distance(fixed.position) < 0.001);
    assert!(framed.velocity.distance(fixed.velocity) < 0.001);
    let mut airborne = Vehicle::spawn(&city);
    airborne.position.z += 10.;
    airborne.velocity = Vec3::X * 20.;
    let yaw = airborne.yaw;
    airborne.step(&city, input, 1. / 30.);
    assert!(!airborne.grounded);
    assert_eq!(airborne.yaw, yaw);
    assert!(airborne.velocity.x < 20.);
    assert!(airborne.velocity.z < 0.);
}

#[test]
fn wheel_clearance_stays_on_the_lower_deck_under_a_bridge() {
    let city = City::from_program(
        &opencity::parse(
            "road street: carlane; end street(-25,3,-20,3); upper=street(-25,\
                3,-20,3); elevation(upper,0.05);",
        )
        .unwrap(),
        None,
    )
    .unwrap();
    let mut vehicle = Vehicle::spawn(&city);
    let floor = city
        .surface_at(
            (vehicle.position.truncate() / METERS_PER_UNIT).to_array(),
            0.04,
        )
        .unwrap()
        .height
        * METERS_PER_UNIT;
    vehicle.position.z = floor + RIDE_HEIGHT;
    for _ in 0..120 {
        vehicle.step(&city, Controls::default(), 1. / 120.);
        assert!(vehicle.grounded);
        assert!((vehicle.position.z - floor - RIDE_HEIGHT).abs() < 0.001);
    }
}

fn building_map() -> City {
    City::from_program(
        &opencity::parse(
            "road street: carlane; end street(-25,3,-20,3); \
             building(0.2, -22.4,2.8, -22.35,2.8, -22.35,3.2, -22.4,3.2);",
        )
        .unwrap(),
        None,
    )
    .unwrap()
}

#[test]
fn collision_normal_follows_rotated_facade_in_either_polygon_winding() {
    let anchor = Vec2::new(-2240., 300.);
    let angle = std::f32::consts::FRAC_PI_4;
    let forward = Vec2::new(angle.cos(), angle.sin());
    let left = Vec2::new(-forward.y, forward.x);
    for reverse in [false, true] {
        let mut city = building_map();
        for building in &mut city.landscape {
            if building.kind != opencity::LandscapeKind::Building {
                continue;
            }
            for point in &mut building.points {
                let local = Vec2::from_array(*point) * METERS_PER_UNIT - anchor;
                *point =
                    ((anchor + forward * local.x + left * local.y) / METERS_PER_UNIT).to_array();
            }
            if reverse {
                building.points.reverse();
            }
        }
        let mut vehicle = Vehicle::spawn(&city);
        vehicle.yaw = angle + (5_f32 / 30.).atan();
        let initial_yaw = vehicle.yaw;
        let start = anchor - forward * 3.;
        vehicle.position.x = start.x;
        vehicle.position.y = start.y;
        let incoming = forward * 30. + left * 5.;
        vehicle.velocity = incoming.extend(0.);
        vehicle.move_with_building_contacts(&city, 0.1);
        assert_eq!(vehicle.collisions, 1);
        assert!((vehicle.velocity.truncate().dot(forward) + 1.).abs() < 0.02);
        assert!((vehicle.velocity.truncate().dot(left) - 5.).abs() < 0.02);
        assert!(vehicle.yaw > initial_yaw);
        let chassis_forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
        assert!(vehicle.building_contact(&city, chassis_forward).is_none());
    }
}

#[test]
fn building_impact_absorbs_inward_speed_and_keeps_the_slide() {
    let city = building_map();
    let mut vehicle = Vehicle::spawn(&city);
    vehicle.yaw = 0.;
    vehicle.position.x = -2243.;
    vehicle.position.y = 300.;
    vehicle.velocity = Vec3::new(30., 5., 0.5);
    let initial_energy = vehicle.velocity.length_squared();
    let initial_z = vehicle.position.z;
    vehicle.move_with_building_contacts(&city, 0.1);
    assert_eq!(vehicle.collisions, 1);
    assert!((vehicle.velocity.x + 1.).abs() < 0.001);
    assert!((vehicle.velocity.y - 5.).abs() < 0.001);
    assert!(vehicle.velocity.length_squared() < initial_energy);
    assert!(vehicle.position.x < -2242.12);
    assert!((vehicle.position.z - initial_z - 0.05).abs() < 0.001);
    assert!(vehicle.building_contact(&city, Vec2::X).is_none());
}

#[test]
fn overlapping_vehicle_separates_and_can_reverse_away() {
    let city = building_map();
    for starting_x in [-2241.8, -2241.2, -2239.] {
        let mut vehicle = Vehicle::spawn(&city);
        vehicle.yaw = 0.;
        vehicle.position.x = starting_x;
        vehicle.position.y = 300.;
        vehicle.move_with_building_contacts(&city, 0.);
        assert!(vehicle.building_contact(&city, Vec2::X).is_none());
        assert_eq!(vehicle.velocity, Vec3::ZERO);
        let separated_x = vehicle.position.x;
        let collisions = vehicle.collisions;
        for _ in 0..240 {
            vehicle.step(
                &city,
                Controls {
                    throttle: -1.,
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        assert!(vehicle.position.x < separated_x - 2.);
        assert!(vehicle.velocity.x < -1.);
        assert_eq!(vehicle.collisions, collisions);
    }
}

#[test]
fn fast_vehicle_cannot_tunnel_through_building() {
    let city = building_map();
    let mut vehicle = Vehicle::spawn(&city);
    vehicle.yaw = 0.;
    vehicle.position.x = -2250.;
    vehicle.position.y = 300.;
    vehicle.velocity = Vec3::X * 200.;
    vehicle.move_with_building_contacts(&city, 0.2);
    assert_eq!(vehicle.collisions, 1);
    assert!(vehicle.position.x < -2242.12);
    assert!((vehicle.velocity.x + 1.).abs() < 0.001);
    assert!(vehicle.velocity.length() <= 1.);
}

#[test]
fn oblique_impacts_turn_chassis_smoothly_in_forward_and_reverse() {
    let city = building_map();
    for direction in [-1_f32, 1.] {
        for angle in [-0.6_f32, -0.15, 0., 0.15, 0.6] {
            let mut vehicle = Vehicle::spawn(&city);
            // Reverse travel has the same incoming ray but the tail meets
            // the wall. Both must turn consistently with the contact torque.
            vehicle.yaw = angle
                + if direction < 0. {
                    std::f32::consts::PI
                } else {
                    0.
                };
            let initial_yaw = vehicle.yaw;
            vehicle.position.x = -2243.;
            vehicle.position.y = 300.;
            vehicle.velocity = Vec3::new(angle.cos() * 30., angle.sin() * 30., 0.);
            let initial_energy = vehicle.velocity.length_squared();
            vehicle.move_with_building_contacts(&city, 0.1);
            assert_eq!(vehicle.collisions, 1);
            assert!((-1.001..0.).contains(&vehicle.velocity.x));
            assert!((vehicle.velocity.y - angle.sin() * 30.).abs() < 0.001);
            let yaw_delta = vehicle.yaw - initial_yaw;
            if angle == 0. {
                assert!(yaw_delta.abs() < 1e-5);
                assert!(vehicle.angular_velocity().z.abs() < 1e-4);
            } else {
                assert!(
                    yaw_delta * angle > 0.,
                    "angle={angle}, reverse={direction}, yaw={yaw_delta}"
                );
                assert!(vehicle.angular_velocity().z.abs() > 0.1);
            }
            let outgoing = vehicle.velocity;
            let outgoing_rate = vehicle.angular_velocity().z;
            let rebound_yaw = vehicle.yaw;
            for _ in 0..120 {
                let previous_yaw = vehicle.yaw;
                vehicle.move_with_building_contacts(&city, 1. / 120.);
                assert!((vehicle.yaw - previous_yaw).abs() <= 2.8 / 120. + 1e-5);
                assert!(vehicle.position.is_finite() && vehicle.velocity.is_finite());
                let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
                assert!(vehicle.building_contact(&city, forward).is_none());
                assert!(vehicle.velocity.length_squared() <= initial_energy);
            }
            assert!(vehicle.velocity.distance(outgoing) < 0.001);
            assert_eq!(vehicle.collisions, 1);
            // Free motion conserves spin; tire forces recover it in step().
            assert_eq!(vehicle.angular_velocity().z, outgoing_rate);
            assert!((vehicle.yaw - rebound_yaw - outgoing_rate).abs() < 0.0001);
        }
    }
}

#[test]
fn grazing_impacts_reseparate_rotating_body_without_energy_gain() {
    let city = building_map();
    for speed in [0.1_f32, 3., 30., 200.] {
        for angle in [-1.45_f32, 1.45] {
            let mut vehicle = Vehicle::spawn(&city);
            vehicle.yaw = angle;
            // Begin just inside contact to exercise even almost-resting hits.
            vehicle.position.x = -2240. - 0.92 - 1.2 * angle.cos() + 0.01;
            vehicle.position.y = 300.;
            vehicle.velocity = Vec3::new(angle.cos() * speed, angle.sin() * speed, 0.5);
            let initial_planar_energy = vehicle.velocity.truncate().length_squared();
            let initial_tangent_speed = vehicle.velocity.y.abs();
            for frame in 0..120 {
                vehicle.move_with_building_contacts(&city, 1. / 120.);
                if frame == 0 {
                    assert!(vehicle.velocity.x < 0.);
                    assert!(
                        (vehicle.velocity.x + (0.05 * angle.cos() * speed).min(1.)).abs() < 0.001
                    );
                    assert!((vehicle.velocity.y.abs() - initial_tangent_speed).abs() < 0.001);
                }
                let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
                assert!(vehicle.building_contact(&city, forward).is_none());
                assert!(vehicle.position.is_finite() && vehicle.yaw.is_finite());
                let energy = vehicle.velocity.truncate().length_squared()
                    + CHASSIS_YAW_INERTIA * vehicle.angular_velocity().z.powi(2);
                assert!(energy <= initial_planar_energy + 0.001);
                assert_eq!(vehicle.velocity.z, 0.5);
            }
        }
    }
}

#[test]
fn tires_damp_spin_after_an_oblique_wall_slide_without_penetration() {
    // Leave room for the slide to settle on tires, away from an open road edge.
    let city = City::from_program(
        &opencity::parse(
            "road street: carlane carlane carlane carlane carlane; end street(-25,3,-20,3); \
             building(0.2, -22.4,2.8, -22.35,2.8, -22.35,3.2, -22.4,3.2);",
        )
        .unwrap(),
        None,
    )
    .unwrap();
    for reverse in [false, true] {
        for angle in [-0.4_f32, 0.4] {
            let mut vehicle = Vehicle::spawn(&city);
            vehicle.yaw = angle + if reverse { std::f32::consts::PI } else { 0. };
            vehicle.position.x = -2243.;
            vehicle.position.y = 300.;
            vehicle.velocity = Vec3::new(angle.cos() * 20., angle.sin() * 20., 0.);
            vehicle.step(&city, Controls::default(), 0.1);
            assert_eq!(vehicle.collisions, 1);
            assert!((-1.001..0.).contains(&vehicle.velocity.x));
            let impact_y = vehicle.position.y;
            let impact_spin = vehicle.angular_velocity().z.abs();
            let impact_energy = vehicle.velocity.truncate().length_squared()
                + CHASSIS_YAW_INERTIA * impact_spin.powi(2);
            for _ in 0..240 {
                vehicle.step(&city, Controls::default(), 1. / 120.);
                assert!(vehicle.position.is_finite() && vehicle.velocity.is_finite());
                let forward = Vec2::new(vehicle.yaw.cos(), vehicle.yaw.sin());
                assert!(vehicle.building_contact(&city, forward).is_none());
                let energy = vehicle.velocity.truncate().length_squared()
                    + CHASSIS_YAW_INERTIA * vehicle.angular_velocity().z.powi(2);
                assert!(energy <= impact_energy + 0.001);
            }
            assert!((vehicle.position.y - impact_y) * angle.signum() > 0.5);
            assert!(
                vehicle.angular_velocity().z.abs() < impact_spin * 0.1,
                "spin {:?} after impact, reverse={reverse}, angle={angle}, position={:?}, \
                 pitch={}, roll={}, grounded={}",
                vehicle.angular_velocity(),
                vehicle.position,
                vehicle.pitch,
                vehicle.roll,
                vehicle.grounded
            );
        }
    }
}

#[test]
fn throttle_brake_and_fixed_step_remain_finite() {
    let city = map();
    let mut v = Vehicle::spawn(&city);
    for _ in 0..600 {
        v.step(
            &city,
            Controls {
                throttle: 1.,
                ..Default::default()
            },
            1. / 120.,
        );
    }
    assert!(v.velocity.length() > 10.);
    for _ in 0..360 {
        v.step(
            &city,
            Controls {
                brake: true,
                ..Default::default()
            },
            1. / 120.,
        );
    }
    assert!(v.velocity.length() < 0.1);
    assert!(v.position.is_finite());
}
#[test]
fn grass_has_lower_acceleration_and_firmer_slide_recovery() {
    let city = City::from_program(
        &opencity::parse(
            "road street: carlane; end street(-25,3,-20,3); ground=land(-26,2,\
                -19,2,-19,5,-26,5); elevation(ground,0.02);",
        )
        .unwrap(),
        None,
    )
    .unwrap();
    let mut asphalt = Vehicle::spawn(&city);
    let mut grass = Vehicle::spawn(&city);
    grass.position.y += 20.;
    for _ in 0..360 {
        let input = Controls {
            throttle: 1.,
            ..Default::default()
        };
        asphalt.step(&city, input, 1. / 120.);
        grass.step(&city, input, 1. / 120.);
    }
    assert_eq!(grass.surface, SurfaceKind::Grass);
    assert!(asphalt.velocity.length() > grass.velocity.length() * 1.2);
    asphalt.velocity = Vec3::X * 10.;
    grass.velocity = Vec3::X * 10.;
    for _ in 0..120 {
        asphalt.step(&city, Controls::default(), 1. / 120.);
        grass.step(&city, Controls::default(), 1. / 120.);
    }
    assert!(asphalt.velocity.x > grass.velocity.x);

    // A car that starts sliding sideways at 8 m/s should shed at least
    // 3 m/s of that slip in a quarter second on dry grass.
    grass.velocity = Vec3::new(12., 8., 0.);
    for _ in 0..30 {
        grass.step(&city, Controls::default(), 1. / 120.);
        assert_eq!(grass.surface, SurfaceKind::Grass);
    }
    assert!(
        grass.velocity.y.abs() < 5.,
        "sideways speed {}",
        grass.velocity.y
    );
}
#[test]
fn uphill_gravity_rolls_unpowered_vehicle_downhill() {
    let city = City::from_program(
        &opencity::parse("road street: carlane; end s=street(-25,3,-20,3); grade(s,0,0.5);")
            .unwrap(),
        None,
    )
    .unwrap();
    let mut v = Vehicle::spawn(&city);
    for _ in 0..240 {
        v.step(&city, Controls::default(), 1. / 120.);
    }
    assert!(v.velocity.x < -0.1);
}
#[test]
fn building_grid_contacts_match_the_full_map_scan_at_cell_boundaries() {
    let city = building_map();
    let grid = crate::dynamic_world::buildings::Buildings::new(&city);
    let mut candidates = Vec::new();
    let mut vehicle = Vehicle::spawn(&city);
    for x in -2260..=-2220 {
        for y in [290., 299.9, 300., 300.1, 310.] {
            for yaw in [0., 0.7, 1.5] {
                vehicle.position.x = x as f32;
                vehicle.position.y = y;
                vehicle.yaw = yaw;
                let forward = Vec2::new(yaw.cos(), yaw.sin());
                grid.nearby(
                    vehicle.position.truncate(),
                    vehicle.length().hypot(vehicle.width()),
                    &mut candidates,
                );
                let full = vehicle.building_contact(&city, forward);
                let local = vehicle.building_contact_candidates(&city, forward, Some(&candidates));
                assert_eq!(local, full, "missed building at {:?}", vehicle.position);
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn report_45_heritage_facade_stops_vehicles_and_follow_camera() {
    let map = crate::hermes::Hermes::load_with_game_data(
        &std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road"),
        None,
    )
    .unwrap();
    let city = map.city();
    // The recorded vehicle position was already inside this facade.
    let reported = Vec2::new(-0.70686495, -2.9790354);
    assert!(city.building_collision(reported.to_array(), 0.003, 0.103, 0.12));
    let building = city
        .landscape
        .iter()
        .find(|f| f.name.as_deref() == Some("placed_building_2"))
        .unwrap();
    let a = Vec2::from_array(building.points[0]);
    let edge = Vec2::from_array(building.points[1]) - a;
    let front = a + edge * ((reported - a).dot(edge) / edge.length_squared()).clamp(0., 1.);
    let depth = Vec2::from_array(building.points[3]) - a;
    let inward = depth / depth.length();
    let start = front - inward * 0.1;
    let ground = city
        .surface_at(start.to_array(), f32::INFINITY)
        .unwrap()
        .height;
    let grid = crate::dynamic_world::buildings::Buildings::new(city);
    let truck = super::super::catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-j05")
        .unwrap();
    for model in [None, Some(truck)] {
        for indexed in [false, true] {
            let mut vehicle = Vehicle::spawn(city);
            vehicle.set_catalog_model(model);
            vehicle.position =
                (start * METERS_PER_UNIT).extend(ground * METERS_PER_UNIT + vehicle.ride_height());
            vehicle.yaw = inward.y.atan2(inward.x);
            vehicle.velocity = (inward * 25.).extend(0.);
            let mut candidates = Vec::new();
            grid.nearby(vehicle.position.truncate(), 40., &mut candidates);
            vehicle.move_with_building_candidates(city, 1., indexed.then_some(&candidates));
            assert!(vehicle.collisions > 0, "model={model:?}, indexed={indexed}");
            let distance = (vehicle.position.truncate() - front * METERS_PER_UNIT).dot(inward);
            assert!(distance < 0., "crossed facade: {distance}");
            assert!(vehicle.velocity.truncate().dot(inward) < 0., "no rebound");
        }
    }
    let anchor = start.extend(ground + 0.02);
    let desired = (front + inward * 0.1).extend(anchor.z);
    let eye = grid.camera_eye(city, anchor, desired);
    assert!((eye.truncate() - front).dot(inward) < 0.);
    assert!(!city.building_collision(
        eye.truncate().to_array(),
        0.0059,
        eye.z - 0.0059,
        eye.z + 0.0059
    ));
}

#[test]
fn report_94_drive_from_grass_over_excavation_edge() {
    use crate::dynamic_world::{catalog, SpawnPoint};
    let program = opencity::parse(include_str!("../../../data/maps/mtl/map.road")).unwrap();
    let city = City::from_program(
        &program,
        Some(&std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl")),
    )
    .unwrap();
    for model in [
        Some(0),
        catalog::definitions().iter().position(|d| d.is_bicycle()),
    ] {
        let mut car = Vehicle::spawn_at(
            &city,
            SpawnPoint {
                point: [-0.65, -3.654],
                heading: Some(0.),
            },
        )
        .unwrap();
        car.set_catalog_model(model);
        let upper = city
            .surface_at([-0.65, -3.654], f32::INFINITY)
            .unwrap()
            .height
            * METERS_PER_UNIT;
        car.position.z = upper + car.ride_height();
        car.velocity = Vec3::X * 11.;
        for _ in 0..110 {
            car.step(
                &city,
                Controls {
                    throttle: 1.,
                    ..Default::default()
                },
                1. / 60.,
            );
        }
        assert!(
            car.position.x > -51.,
            "blocked at {:?} model {model:?}",
            car.position
        );
        assert!(car.position.z - car.ride_height() < upper - 0.2);
    }
}
