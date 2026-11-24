use super::*;
use crate::hermes::Hermes;
use glam::Vec3;

// Fixed gestures, stride phase, cameras and noon lighting keep asset revisions comparable.
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_player_outfits_front_and_back() {
    let hermes = Hermes::test_map();
    let mut renderer = pollster::block_on(Renderer::new(None, 720, 900)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    for (outfit, index) in [
        ("summer", pedestrian_catalog::PLAYER_SUMMER_MODEL),
        ("winter", pedestrian_catalog::PLAYER_WINTER_MODEL),
    ] {
        for (gesture, time) in [("pocket", 0.), ("wrist", 5.), ("walk", 0.9)] {
            let mut scenery = PedestrianScenery::new(&renderer);
            let definition = &pedestrian_catalog::definitions()[index];
            let mut columns =
                pedestrian_catalog::instance_transform(Vec3::ZERO, 0.).to_cols_array_2d();
            columns[0][3] = index as f32;
            columns[2][3] = 0.;
            let mut gait = crate::dynamic_world::pedestrian_gait::Gait::new_player(
                definition,
                Vec3::ZERO,
                0.,
                |_| None,
            );
            let speed = if gesture == "walk" { 1.5 } else { 0. };
            for frame in 1..=(time * 60.) as usize {
                let position = Vec3::X * speed * frame as f32 / 60.;
                gait.advance(
                    definition,
                    position,
                    0.,
                    speed,
                    position + Vec3::X,
                    1. / 60.,
                    |p| {
                        Some(crate::dynamic_world::pedestrian_gait::Ground {
                            point: p.truncate().extend(0.),
                            gradient: [0., 0.],
                        })
                    },
                );
            }
            scenery
                .poses
                .extend(gait.palette.map(|m| m.to_cols_array_2d()));
            scenery.models[index].transforms.push(columns);
            let model = &mut scenery.models[index];
            renderer.update_instances(&mut model.mesh, &model.transforms);
            renderer
                .pedestrians
                .upload_poses(&renderer.queue, &scenery.poses);
            for (view, direction) in [("front", 1.), ("back", -1.)] {
                let png = renderer
                    .report_screenshot(
                        &scenery.visible().collect::<Vec<_>>(),
                        Vec3::new(direction * 0.036, -0.008, 0.017),
                        Vec3::new(0., 0., 0.009),
                        0.,
                        hermes.minimap_view([0., 0.], 0.),
                    )
                    .unwrap();
                let label = if gesture == "pocket" {
                    format!("{outfit}-{view}")
                } else {
                    format!("{outfit}-{gesture}-{view}")
                };
                super::tests::save(&label, &png);
                // The walking pose carries the torso lower in this fixed camera.
                // A white shirt in noon shade must not become the old near-black silhouette.
                if outfit == "summer" && view == "back" {
                    let mut reader = png::Decoder::new(png.as_slice()).read_info().unwrap();
                    let mut pixels = vec![0; reader.output_buffer_size()];
                    let info = reader.next_frame(&mut pixels).unwrap();
                    let channels = info.color_type.samples();
                    let mut total = 0_u64;
                    let shirt_top = if gesture == "walk" { 410 } else { 365 };
                    for y in shirt_top..shirt_top + 35 {
                        for x in 335..375 {
                            let offset = (y * info.width as usize + x) * channels;
                            total += pixels[offset..offset + 3]
                                .iter()
                                .map(|v| *v as u64)
                                .sum::<u64>();
                        }
                    }
                    let brightness = total as f32 / (35. * 40. * 3.);
                    assert!(
                        brightness > 80.,
                        "rear white shirt is too dark: {brightness}"
                    );
                }
            }
        }
    }
}

#[test]
fn summer_shorts_cover_front_back_and_upper_legs() {
    let definition = &pedestrian_catalog::definitions()[pedestrian_catalog::PLAYER_SUMMER_MODEL];
    let mut decoder = png::Decoder::new(definition.texture_png.as_slice());
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels).unwrap();
    let channels = info.color_type.samples();
    let mut covered = [0; 2];
    for vertex in &definition.vertices {
        // Interior of the v2 shorts, between the modeled hem and waistband.
        if !(0.62..0.90).contains(&vertex.position[1]) {
            continue;
        }
        let x = (vertex.surface[2] * info.width as f32) as usize;
        let y = (vertex.surface[3] * info.height as f32) as usize;
        let offset = (y * info.width as usize + x) * channels;
        assert!(
            pixels[offset..offset + 3].iter().all(|value| *value < 80),
            "skin-colored gap in shorts at {:?}",
            vertex.position
        );
        covered[usize::from(vertex.position[2] > 0.)] += 1;
    }
    assert!(covered.iter().all(|count| *count > 100));
}
