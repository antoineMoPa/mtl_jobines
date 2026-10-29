use super::{Controls, Vehicle};
use glam::Vec3;
use opencity::runtime::City;

#[test]
fn space_braking_stops_promptly_forward_and_backward_on_asphalt_and_grass() {
    let city = City::from_program(
        &opencity::parse(
            "road street: carlane; end street(-25,3,-20,3); ground=land(-26,2,\
             -19,2,-19,5,-26,5); elevation(ground,0.02);",
        )
        .unwrap(),
        None,
    )
    .unwrap();
    for speed in [-20.0_f32, 20.] {
        for grass in [false, true] {
            let mut car = Vehicle::spawn(&city);
            if grass {
                car.position.y += 20.;
            }
            car.yaw = 0.;
            car.velocity = Vec3::X * speed;
            let start = car.position;
            let mut elapsed = 0.;
            while car.velocity.truncate().length() > 0.01 && elapsed < 4. {
                car.step(
                    &city,
                    Controls {
                        handbrake: true,
                        throttle: speed.signum(),
                        ..Default::default()
                    },
                    1. / 120.,
                );
                elapsed += 1. / 120.;
                assert!(car.velocity.x * speed >= 0., "braking reversed the car");
            }
            let distance = car.position.truncate().distance(start.truncate());
            assert!(
                elapsed < if grass { 2.8 } else { 1.4 },
                "{speed}, grass={grass}: {elapsed}s"
            );
            assert!(
                distance < if grass { 28. } else { 14. },
                "{speed}, grass={grass}: {distance}m"
            );
        }
    }
}
