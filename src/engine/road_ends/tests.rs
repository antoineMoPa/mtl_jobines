use super::*;
fn city(source: &str) -> City {
    City::from_program(&opencity::parse(source).unwrap(), None).unwrap()
}
#[test]
fn road_end_cubes_are_two_metres_and_selection_survives_shift_release() {
    let city = city(
        "road street: sidewalk carlane bikelane buslane sidewalk; end
        edge = street(0,0,1,0); grade(edge,0.1,0.2);",
    );
    let mut ends = RoadEnds::new(&city);
    assert_eq!(ends.handles.len(), 6);
    let h = &ends.handles[4];
    assert_eq!(h.id.end, End::Finish);
    assert!((h.position.z - 0.2004).abs() < 1e-6);
    let (min, max) = h.bounds();
    assert!(((max - min) * METERS_PER_UNIT - Vec3::splat(2.)).length() < 1e-4);
    assert!(ends.mesh(false).vertices.is_empty());
    let a = ends.handles[0].id;
    let b = ends.handles[1].id;
    ends.toggle(a);
    ends.toggle(b);
    assert_eq!(ends.selected.len(), 2);
    assert_eq!(ends.mesh(false).vertices.len(), 72);
    ends.toggle(a);
    assert!(ends.selected.contains(&b));
    assert_eq!(ends.mesh(true).vertices.len(), 216);
}
#[test]
fn picking_uses_camera_projection_and_nearest_cube_at_stacked_ends() {
    let mut ends = RoadEnds::new(&city(
        "road lane: carlane; end
        lower = lane(0,0,1,0); upper = lane(0,0,1,0); elevation(upper,0.1);",
    ));
    let eye = Vec3::new(1., 0., 0.4);
    let view = Mat4::perspective_rh(58_f32.to_radians(), 1.5, 0.003, 10.)
        * Mat4::look_at_rh(eye, Vec3::new(1., 0., 0.), Vec3::Y);
    let picked = ends.pick(view, [900., 600.], [450., 300.]).unwrap();
    assert_eq!(picked.road, 1);
    assert_eq!(picked.end, End::Finish);
    ends.toggle(picked);
    assert!(ends.pick(view, [900., 600.], [0., 0.]).is_none());
    let away = Mat4::perspective_rh(58_f32.to_radians(), 1.5, 0.003, 10.)
        * Mat4::look_at_rh(eye, eye + Vec3::Z, Vec3::Y);
    assert!(ends.pick(away, [900., 600.], [450., 300.]).is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[ignore = "requires native GPU"]
fn road_end_gpu_snapshot() {
    use crate::{
        engine::{
            render::Renderer,
            road_drawing::{Drawing, Pose},
        },
        hermes::Hermes,
    };
    let source = "road street: sidewalk carlane carlane sidewalk; end
         edge = street(0,0,0.6,0); land(-1,-1,2,-1,2,2,-1,2);";
    let mut hermes = Hermes::test_map_from_source(source);
    let mut renderer = pollster::block_on(Renderer::new(None, 960, 640)).unwrap();
    renderer.daylight = Some(1.);
    let mut ends = RoadEnds::new(hermes.city());
    ends.toggle(ends.handles[2].id);
    let road = renderer.upload_dynamic(
        &hermes
            .city()
            .mesh(opencity::runtime::Region::WholeMap, 0.02)
            .unwrap(),
    );
    let handles = renderer.upload_dynamic(&ends.mesh(true));
    hermes.update_minimap([0.55, 0.]);
    let bytes = renderer
        .report_screenshot(
            &[&road, &handles],
            Vec3::new(0.78, -0.22, 0.16),
            Vec3::new(0.58, 0., 0.01),
            0.,
            hermes.minimap_view([0.55, 0.], 0.),
        )
        .unwrap();
    std::fs::write(std::env::temp_dir().join("road-end-selection.png"), bytes).unwrap();

    ends.toggle(ends.handles[3].id);
    let mut drawing = Drawing::new(
        hermes.city(),
        &ends.selected,
        Pose {
            position: Vec3::new(0.6, 0., 0.),
            yaw: 0.,
        },
    );
    for i in 1..=16 {
        let angle = i as f32 * 0.1;
        drawing.sample(Pose {
            position: Vec3::new(0.6 + 0.6 * angle.sin(), 0.6 * (1. - angle.cos()), 0.),
            yaw: angle,
        });
    }
    let mut curved = Hermes::test_map_from_source(&format!(
        "{source}{}",
        drawing.source("driven_0001").unwrap()
    ));
    let roads = renderer.upload_dynamic(
        &curved
            .city()
            .mesh(opencity::runtime::Region::WholeMap, 0.02)
            .unwrap(),
    );
    curved.update_minimap([0.8, 0.3]);
    let bytes = renderer
        .report_screenshot(
            &[&roads],
            Vec3::new(1.7, -0.6, 0.9),
            Vec3::new(0.7, 0.3, 0.),
            0.,
            curved.minimap_view([0.8, 0.3], 0.),
        )
        .unwrap();
    std::fs::write(
        std::env::temp_dir().join("road-driving-extension.png"),
        bytes,
    )
    .unwrap();
}
