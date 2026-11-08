use super::*;
const SOURCE: &str = "# Keep this comment\nroad street: carlane carlane; direction forward; end
    parent = street(0,0,1,0); grade(parent,0.1,0.12);\n";
fn city(source: &str) -> City {
    City::from_program(&opencity::parse(source).unwrap(), None).unwrap()
}
fn selection(end: End) -> BTreeSet<LaneEnd> {
    [0, 1]
        .map(|lane| LaneEnd { road: 0, end, lane })
        .into_iter()
        .collect()
}
fn pose(x: f32, y: f32, yaw: f32) -> Pose {
    Pose {
        position: Vec3::new(x, y, 0.12),
        yaw,
    }
}
#[test]
fn driving_generates_attached_parallel_smooth_curves_with_grades() {
    let original = city(SOURCE);
    let mut drawing = Drawing::new(&original, &selection(End::Finish), pose(1., 0., 0.));
    assert!(!drawing.sample(pose(1.001, 0., 0.01)));
    for i in 1..=12 {
        let angle = i as f32 * 0.06;
        assert!(drawing.sample(pose(1. + angle.sin(), 1. - angle.cos(), angle)));
    }
    let source = format!("{SOURCE}{}", drawing.source("driven_0001").unwrap());
    let generated = city(&source);
    assert_eq!(generated.roads.len(), 25);
    for lane in 0..2 {
        let first = &generated.roads[1 + lane * 12];
        let original_end = opencity::map::element_position(
            &original.roads[0].segment,
            &original.roads[0].definition,
            lane,
            1.,
        )
        .unwrap();
        assert!((first.segment.start.x - original_end.x).abs() < 1e-6);
        assert!((first.segment.start.y - original_end.y).abs() < 1e-6);
        assert!((first.segment.start_z - 0.12).abs() < 1e-6);
        assert_eq!(first.definition.direction, RoadDirection::Forward);
        for pair in generated.roads[1 + lane * 12..1 + (lane + 1) * 12].windows(2) {
            assert_eq!(pair[0].segment.end_ref, pair[1].segment.start_ref);
            let a = pair[0].segment.tangent_at(1.);
            let b = pair[1].segment.tangent_at(0.);
            assert!((a.x * b.y - a.y * b.x).abs() < 1e-6);
        }
    }
    let left = &generated.roads[12].segment;
    let right = &generated.roads[24].segment;
    assert!(((left.end.x - right.end.x).hypot(left.end.y - right.end.y) - 0.045).abs() < 1e-5);
}
#[test]
fn start_end_reverses_lane_direction_and_wraps_vehicle_heading_smoothly() {
    let original = city(SOURCE);
    let mut drawing = Drawing::new(&original, &selection(End::Start), pose(0., 0., 3.13));
    drawing.sample(pose(-0.06, 0., -3.13));
    assert!(drawing.knots[1].angle.abs() < 0.03);
    let generated = city(&format!(
        "{SOURCE}{}",
        drawing.source("driven_0001").unwrap()
    ));
    assert_eq!(
        generated.roads[1].definition.direction,
        RoadDirection::Reverse
    );
    assert!(generated.roads[1].segment.tangent_at(0.).x < 0.);
    assert!(!drawing.sample(pose(50., 50., 0.)));
}
#[test]
fn extension_follows_world_trajectory_and_retains_implicit_lane_directions() {
    let source = SOURCE.replace(" direction forward;", "");
    let original = city(&source);
    let mut drawing = Drawing::new(
        &original,
        &selection(End::Finish),
        pose(1., 0., std::f32::consts::FRAC_PI_2),
    );
    drawing.sample(pose(1., 0.06, std::f32::consts::FRAC_PI_2));
    let generated = city(&format!(
        "{source}{}",
        drawing.source("driven_0001").unwrap()
    ));
    assert_eq!(
        generated.roads[1].definition.direction,
        RoadDirection::Reverse
    );
    assert_eq!(
        generated.roads[2].definition.direction,
        RoadDirection::Forward
    );
    assert!((generated.roads[1].segment.end.x - 1.).abs() < 1e-6);
    assert!(generated.roads[1].segment.end.y > 0.06);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn continuous_saves_preserve_comments_name_anonymous_roads_and_reject_external_edits() {
    use file::Recording;
    use web_time::{Duration, Instant};
    let source = SOURCE
        .replace("parent = ", "")
        .replace("grade(parent,0.1,0.12);", "");
    let city = city(&source);
    let directory = std::env::temp_dir().join(format!("road-recording-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("map.road");
    std::fs::write(&path, &source).unwrap();
    let mut recording =
        Recording::new(&path, &city, &selection(End::Finish), pose(1., 0., 0.)).unwrap();
    assert!(!recording.save(Instant::now(), true).unwrap());
    recording.drawing.sample(pose(1.06, 0., 0.));
    assert!(recording.save(Instant::now(), true).unwrap());
    let first = std::fs::read_to_string(&path).unwrap();
    assert!(first.starts_with("# Keep this comment\n"));
    assert!(first.contains("driven_0001_parent_0 = street"));
    assert!(recording.owns_source(&first));
    assert_eq!(
        opencity::map::project(&opencity::parse(&first).unwrap()).unresolved,
        0
    );
    recording.drawing.sample(pose(1.12, 0., 0.));
    assert!(recording
        .save(Instant::now() + Duration::from_secs(2), false)
        .unwrap());
    let second = std::fs::read_to_string(&path).unwrap();
    assert_eq!(second.matches("# Road extension recorded").count(), 1);
    assert_eq!(second.matches("road driven_0001_lane_0:").count(), 1);
    let external = format!("{second}\n# external edit\n");
    std::fs::write(&path, &external).unwrap();
    recording.drawing.sample(pose(1.18, 0., 0.));
    assert!(recording.save(Instant::now(), true).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), external);
    std::fs::remove_dir_all(directory).unwrap();
}
