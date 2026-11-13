use super::*;

#[test]
fn distant_npcs_use_lane_motion_while_player_physics_matches_full_simulation() {
    let hermes =
        Hermes::test_map_from_source(include_str!("../../tests/fixtures/traffic_lod.road"));
    let mut lod = DynamicWorld::new(&hermes);
    let mut full = DynamicWorld::new(&hermes);
    full.use_full_traffic_physics();
    let origin = lod.player().position;
    for tick in 0..120 {
        let input = if tick < 60 {
            Controls {
                throttle: 0.7,
                steering: 0.15,
                ..Controls::default()
            }
        } else {
            Controls {
                brake: true,
                steering: -0.1,
                ..Controls::default()
            }
        };
        lod.step(&hermes, input, WalkControls::default(), 1. / 120.);
        full.step(&hermes, input, WalkControls::default(), 1. / 120.);
        assert_eq!(lod.player().position, full.player().position);
        assert_eq!(lod.player().velocity, full.player().velocity);
        assert_eq!(
            lod.player().angular_momentum,
            full.player().angular_momentum
        );
        assert_eq!(lod.player().yaw, full.player().yaw);
    }
    assert!(
        lod.player().position.distance(origin) > 0.1,
        "player preservation check never moved"
    );
    let counts = lod.traffic_physics_counts();
    assert!(
        counts[0] > 0 && counts[1] > 0,
        "physics LOD never engaged: {counts:?}"
    );
    assert_eq!(full.traffic_physics_counts()[1], 0);
    assert_eq!(lod.traffic.count(), full.traffic.count());
}
