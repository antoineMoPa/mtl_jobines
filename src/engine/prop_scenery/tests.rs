//! Compare cached signal draws with the former combined dynamic geometry path.
use super::*;
use crate::{
    dynamic_world::{Controls, WalkControls, METERS_PER_UNIT},
    engine::{scenery::Scenery, vehicle_scenery::VehicleScenery},
    hermes::Hermes,
};
use glam::Vec3;

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_road_sign_designs_share_uploads_and_render_distinct_artwork() {
    let source = include_str!("../../../tests/fixtures/road_signs.road");
    let mut hermes = Hermes::test_map_from_source(source);
    let world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 1600, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.render_lod = crate::engine::render::RenderLod::Full;
    let mut props = PropScenery::default();
    props.update(&renderer, &world);
    let stops: Vec<_> = props
        .models
        .iter()
        .filter(|(key, _)| key.0 == crate::props::Kind::StopSign)
        .collect();
    assert_eq!(
        stops.len(),
        4,
        "two identical four-part clusters share four uploads"
    );
    assert!(stops.iter().all(|(_, m)| m.transforms.len() == 2));
    let highways: Vec<_> = props
        .models
        .iter()
        .filter(|(key, _)| key.0 == crate::props::Kind::HighwaySign)
        .collect();
    assert_eq!(
        highways.len(),
        6,
        "different text must never reuse another sign's panel"
    );
    let models = props.models.len();
    props.update(&renderer, &world);
    assert_eq!(props.models.len(), models);
    let scenery = Scenery::new(&renderer, &mut hermes);
    let mut meshes = scenery.visible([0., 0.]);
    meshes.extend(props.visible());
    let png = renderer
        .report_screenshot(
            &meshes,
            Vec3::new(0.05, -0.4, 0.07),
            Vec3::new(0.05, -0.04, 0.025),
            0.,
            hermes.minimap_view([0., 0.], 0.),
        )
        .unwrap();
    let mut decoder = png::Decoder::new(png.as_slice()).read_info().unwrap();
    let mut pixels = vec![0; decoder.output_buffer_size()];
    decoder.next_frame(&mut pixels).unwrap();
    assert!(
        pixels
            .chunks_exact(4)
            .any(|p| p[0] as u16 > p[1] as u16 * 2 && p[0] > 80),
        "stop faces must be visible"
    );
    if let Some(path) = std::env::var_os("ROAD_SIGN_CAPTURE") {
        let path = std::path::PathBuf::from(path);
        std::fs::write(&path, png).unwrap();
        for (name, eye, look) in [
            (
                "game-stop-cluster.png",
                Vec3::new(-0.12, -0.14, 0.035),
                Vec3::new(-0.12, -0.04, 0.026),
            ),
            (
                "game-autoroute-sign.png",
                Vec3::new(0.14, -0.2, 0.05),
                Vec3::new(0.14, -0.04, 0.04),
            ),
        ] {
            let png = renderer
                .report_screenshot(&meshes, eye, look, 0., hermes.minimap_view([0., 0.], 0.))
                .unwrap();
            std::fs::write(path.with_file_name(name), png).unwrap();
        }
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_cached_signals_match_combined_mesh_across_phases_and_views() {
    let mut hermes =
        Hermes::test_map_from_source(include_str!("../../../tests/fixtures/signal_cache.road"));
    let mut world = DynamicWorld::new(&hermes);
    let mut renderer = pollster::block_on(Renderer::new(None, 800, 600)).unwrap();
    // This regression isolates signal caching; render LOD has separate image checks.
    renderer.render_lod = crate::engine::render::RenderLod::Full;
    let scenery = Scenery::new(&renderer, &mut hermes);
    let mut props = PropScenery::default();
    let mut vehicles = VehicleScenery::new(&renderer);
    let mut initial = Vec::new();
    for (stage, ticks) in [("initial", 0), ("changed", 660)] {
        for _ in 0..ticks {
            world.step(
                &hermes,
                Controls {
                    handbrake: true,
                    ..Controls::default()
                },
                WalkControls::default(),
                1. / 60.,
            );
        }
        let selections: Vec<_> = world.standing_signal_batches().collect();
        assert!(!selections.is_empty());
        if ticks == 0 {
            initial = selections.clone();
        } else {
            assert_ne!(initial, selections, "signal phase never changed");
        }
        props.update(&renderer, &world);
        vehicles.update(&renderer, &world);
        let mut moving = Mesh::default();
        world.write_moving_mesh(&mut moving);
        let transient_vertices = moving.vertices.len();
        let dynamic = renderer.upload_dynamic(&moving);
        let mut batch_mesh = Mesh::default();
        for selection in &selections {
            world.write_signal_batch(selection, &mut batch_mesh);
            moving.vertices.extend_from_slice(&batch_mesh.vertices);
        }
        let combined = renderer.upload_dynamic(&moving);
        assert!(moving.vertices.len() > transient_vertices);
        eprintln!(
            "signal upload {stage}: {} transient vertices vs {} combined",
            transient_vertices,
            moving.vertices.len()
        );
        let center = world.player_position() / METERS_PER_UNIT;
        if let Some(pixels) = hermes.update_minimap([center.x, center.y]) {
            renderer.update_minimap(pixels);
        }
        let mut expected = scenery.visible([center.x, center.y]);
        expected.push(&combined);
        expected.extend(
            props
                .visible
                .iter()
                .filter_map(|key| props.batches.get(key).map(|b| &b.mesh)),
        );
        expected.extend(vehicles.visible());
        let mut actual = scenery.visible([center.x, center.y]);
        actual.push(&dynamic);
        actual.extend(props.visible());
        actual.extend(vehicles.visible());
        for (view, span, eye) in [
            ("street", None, Vec3::new(0.1, -0.22, 0.12)),
            ("iso", Some(1.4), Vec3::new(0.5, -0.6, 0.6)),
        ] {
            renderer.construction_span = span;
            for (lighting, daylight) in [("day", 1.), ("night", 0.)] {
                renderer.daylight = Some(daylight);
                let navigation = hermes.minimap_view([center.x, center.y], world.player_yaw());
                let before = renderer
                    .report_screenshot(&expected, eye, Vec3::ZERO, 0., navigation)
                    .unwrap();
                let after = renderer
                    .report_screenshot(&actual, eye, Vec3::ZERO, 0., navigation)
                    .unwrap();
                let decode = |png: &[u8]| {
                    let mut reader = png::Decoder::new(png).read_info().unwrap();
                    let mut pixels = vec![0; reader.output_buffer_size()];
                    reader.next_frame(&mut pixels).unwrap();
                    pixels
                };
                let a = decode(&before);
                let b = decode(&after);
                let differences = a.iter().zip(&b).filter(|(a, b)| a != b).count();
                assert_eq!(
                    differences, 0,
                    "cached signals changed pixels: {stage}/{view}/{lighting}"
                );
                if let Some(directory) = std::env::var_os("SIGNAL_CACHE_CAPTURE") {
                    std::fs::create_dir_all(&directory).unwrap();
                    std::fs::write(
                        std::path::Path::new(&directory)
                            .join(format!("{stage}-{view}-{lighting}.png")),
                        after,
                    )
                    .unwrap();
                }
            }
        }
    }
}

#[test]
fn tree_lod_crossfades_cover_ten_meters_without_endpoint_holes() {
    for (distance, tier) in [(0., 0), (50., 0), (60., 1), (100., 1), (110., 2), (350., 2)] {
        assert_eq!(tree_lods(distance), [(tier, 0.), (3, 0.)]);
    }
    for (start, tier) in [(50., 0), (100., 1)] {
        let mut previous = 0.;
        for step in 1..100 {
            let [(outgoing, fade), (incoming, other)] = tree_lods(start + step as f32 * 0.1);
            assert_eq!((outgoing, incoming), (tier, tier + 1));
            assert_eq!(fade, -other);
            assert!(fade > previous && fade < 1.);
            for pixel in 0..256 {
                let threshold = (pixel as f32 + 0.5) / 256.;
                assert_ne!(
                    threshold >= fade,
                    threshold < -other,
                    "overlapping silhouettes must have exactly one surviving tier"
                );
            }
            previous = fade;
        }
        assert_eq!(tree_lods(start + 5.), [(tier, 0.5), (tier + 1, -0.5)]);
    }
}
