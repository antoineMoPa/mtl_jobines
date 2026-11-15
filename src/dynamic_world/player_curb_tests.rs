use super::*;
use opencity::runtime::SurfaceKind;

#[test]
fn walk_and_run_cross_sidewalk_curbs_without_getting_stuck() {
    let city = City::from_program(&opencity::parse(
        "ground=land(-2,-2,2,-2,2,2,-2,2); elevation(ground,0.02); road street: sidewalk carlane carlane sidewalk; end street(-1,0,1,0);"
    ).unwrap(), None).unwrap();
    let sidewalk_y = (1..200)
        .map(|i| i as f32 * 0.05)
        .find(|&y| {
            city.surface_at([0., y / METERS_PER_UNIT], 1.)
                .is_some_and(|surface| surface.kind == SurfaceKind::Sidewalk)
        })
        .expect("sidewalk in cross-section")
        + 0.4;
    let sidewalk = city
        .surface_at([0., sidewalk_y / METERS_PER_UNIT], 1.)
        .unwrap();
    let road = city.surface_at([0., 0.], 1.).unwrap();
    let grass = city.surface_at([0., 0.2], 1.).unwrap();
    assert_eq!(grass.kind, SurfaceKind::Grass);
    assert!(
        (grass.height - road.height).abs() < 1e-6,
        "fixture must have a curb, not a cliff"
    );
    assert_eq!(sidewalk.kind, SurfaceKind::Sidewalk);
    assert!((sidewalk.height - road.height) * METERS_PER_UNIT > 0.02);
    for run in [false, true] {
        for sign in [-1., 1.] {
            for hz in [30, 60, 120] {
                let start = Vec3::new(
                    0.,
                    sign * sidewalk_y,
                    sidewalk.height * METERS_PER_UNIT + HALF_HEIGHT,
                );
                let mut player = Pedestrian::new(&city, start, -sign * std::f32::consts::FRAC_PI_2);
                let mut stopped_frames = 0;
                // Cover both curbs at the configured travel speed, plus settling time.
                let speed = if run { RUN_SPEED } else { WALK_SPEED };
                let frames = ((2. * sidewalk_y + 1.) / speed + 2.).ceil() as i32 * hz;
                for frame in 0..frames {
                    let before = player.position;
                    player.step(
                        &city,
                        WalkControls {
                            forward: 1.,
                            run,
                            ..Default::default()
                        },
                        1. / hz as f32,
                        |_| false,
                    );
                    if frame > hz / 2 && (player.position - before).truncate().length() < 0.0001 {
                        stopped_frames += 1;
                    } else {
                        stopped_frames = 0;
                    }
                    assert!(
                        stopped_frames < hz / 2,
                        "stuck crossing curb: run={run}, hz={hz}, position={:?}, speed={}",
                        player.position,
                        player.speed
                    );
                    assert!(
                        player.gait.reach_error < 0.06,
                        "curb strained legs: {}, run={run}, sign={sign}, hz={hz}, frame={frame}, position={:?}",
                        player.gait.reach_error, player.position
                    );
                }
                assert!(
                    -sign * player.position.y > sidewalk_y + 0.5,
                    "failed to cross both curbs: {:?}, speed={}, start={sidewalk_y}, run={run}, hz={hz}",
                    player.position, player.speed
                );
            }
        }
    }
}
