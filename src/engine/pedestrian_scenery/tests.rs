use super::*;
use crate::{
    dynamic_world::{Controls, WalkControls},
    hermes::Hermes,
};
use glam::Vec3;

pub(super) fn save(label: &str, png: &[u8]) {
    if let Some(folder) = std::env::var_os("PEDESTRIAN_CAPTURE") {
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            std::path::Path::new(&folder).join(format!("{label}.png")),
            png,
        )
        .unwrap();
    }
}
fn pixels(png: &[u8]) -> Vec<u8> {
    let mut reader = png::Decoder::new(png).read_info().unwrap();
    let mut data = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut data).unwrap();
    data
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_player_uses_shared_skin_and_extra_pose_slot() {
    let hermes = Hermes::test_map_from_source(
        "land(-2,-2,2,-2,2,2,-2,2); road street:sidewalk carlane carlane sidewalk;end
        street(-0.4,0,0.4,0); street(-0.4,0.2,0.4,0.2); street(-0.4,-0.2,0.4,-0.2);",
    );
    let mut world = DynamicWorld::new(&hermes);
    for _ in 0..60 {
        world.step(
            &hermes,
            Controls {
                handbrake: true,
                ..Default::default()
            },
            WalkControls::default(),
            1. / 120.,
        );
    }
    assert!(world.toggle_vehicle(&hermes));
    let count = world.pedestrian_instances().count();
    assert!(count > 1 && count <= crate::dynamic_world::PEDESTRIAN_CAPACITY);
    let mut renderer = pollster::block_on(Renderer::new(None, 960, 720)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let mut scenery = PedestrianScenery::new(&renderer);
    let mut images = Vec::new();
    for (label, steps) in [
        ("player-idle", 0),
        ("player-walk", 90),
        ("player-turn", 60),
        ("player-turn-full", 200),
        ("player-sprint", 120),
        ("player-jump-crouch", 17),
        ("player-jump-extension", 7),
        ("player-jump-takeoff", 4),
        ("player-jump", 64),
        ("player-jump-land", 120),
        ("player-moving-warmup", 120),
        ("player-moving-jump-extension", 24),
        ("player-moving-jump-takeoff", 8),
        ("player-moving-jump-flight", 30),
        ("player-jump-land", 120),
        ("player-turning-warmup", 30),
        ("player-turning-jump-extension", 24),
        ("player-turning-jump-takeoff", 8),
        ("player-turning-jump-flight", 30),
    ] {
        for _ in 0..steps {
            world.step(
                &hermes,
                Controls::default(),
                WalkControls {
                    forward: if matches!(label, "player-walk" | "player-sprint")
                        || label.starts_with("player-moving")
                    {
                        1.
                    } else {
                        0.
                    },
                    turn: if label.starts_with("player-turn") {
                        1.
                    } else {
                        0.
                    },
                    run: label == "player-sprint" || label.starts_with("player-moving"),
                    jump: label.contains("-jump"),
                    ..Default::default()
                },
                1. / 120.,
            );
        }
        scenery.update(&renderer, &world);
        assert!(scenery.poses.len() <= POSE_CAPACITY);
        let actor = world.pedestrian_instances().next().unwrap();
        assert!(matches!(
            actor.model,
            pedestrian_catalog::PLAYER_SUMMER_MODEL | pedestrian_catalog::PLAYER_WINTER_MODEL
        ));
        assert_eq!(actor.opacity, 1.);
        let root = actor.transform.w_axis.truncate();
        assert!((world.player_position().z / 100. - root.z - 0.009).abs() < 1e-5);
        // Isolate the player for a close view; the update above exercised all 33 palettes.
        for (index, model) in scenery.models.iter_mut().enumerate() {
            if index == actor.model {
                model.transforms.truncate(1);
            } else {
                model.transforms.clear();
            }
            renderer.update_instances(&mut model.mesh, &model.transforms);
        }
        let png = renderer
            .report_screenshot(
                &scenery.visible().collect::<Vec<_>>(),
                root + Vec3::new(0.035, -0.055, 0.023),
                root + Vec3::Z * 0.009,
                0.,
                hermes.minimap_view([root.x, root.y], world.player_yaw()),
            )
            .unwrap();
        save(label, &png);
        if label == "player-idle" {
            let yaw = world.player_yaw();
            let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
            let front = renderer
                .report_screenshot(
                    &scenery.visible().collect::<Vec<_>>(),
                    root + forward * 0.026 + Vec3::Z * 0.018,
                    root + Vec3::Z * 0.008,
                    0.,
                    hermes.minimap_view([root.x, root.y], yaw),
                )
                .unwrap();
            save("player-feet-front", &front);
        }
        if label == "player-sprint" {
            let yaw = world.player_yaw();
            let side = Vec3::new(-yaw.sin(), yaw.cos(), 0.);
            let side_png = renderer
                .report_screenshot(
                    &scenery.visible().collect::<Vec<_>>(),
                    root + side * 0.030 + Vec3::Z * 0.012,
                    root + Vec3::Z * 0.009,
                    0.,
                    hermes.minimap_view([root.x, root.y], yaw),
                )
                .unwrap();
            save("player-sprint-side", &side_png);
        }
        images.push(pixels(&png));
    }
    let changed = images[0]
        .chunks_exact(4)
        .zip(images[1].chunks_exact(4))
        .filter(|(a, b)| {
            a[0].abs_diff(b[0]) > 10 || a[1].abs_diff(b[1]) > 10 || a[2].abs_diff(b[2]) > 10
        })
        .count();
    assert!(
        changed > 100,
        "player gait did not change visible pixels: {changed}"
    );
    // The faster sprint travels beyond the car's interaction radius. Return for re-entry QA.
    world.reset_player(&hermes);
    assert!(world.toggle_vehicle(&hermes));
    assert!(world.toggle_vehicle(&hermes));
    scenery.update(&renderer, &world);
    assert!(world.is_driving());
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_vehicle_hit_ragdoll_get_up_and_walk_again() {
    capture_ragdoll_recovery(false);
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_player_bump_ragdoll_get_up_and_walk_again() {
    capture_ragdoll_recovery(true);
}
fn capture_ragdoll_recovery(on_foot: bool) {
    let mut hermes = Hermes::test_map();
    let mut world = DynamicWorld::new(&hermes);
    let target = if on_foot {
        world.prepare_player_pedestrian_collision(&hermes)
    } else {
        world.prepare_pedestrian_collision()
    } / 100.;
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let scenery = super::super::scenery::Scenery::new(&renderer, &mut hermes);
    let mut actors = super::super::vehicle_scenery::VehicleScenery::new(&renderer);
    let mut seen_fall = false;
    let mut seen_rise = false;
    for frame in 0..=510 {
        world.step(
            &hermes,
            Controls {
                throttle: if frame < 35 { 0.3 } else { 0. },
                brake: frame >= 35,
                ..Default::default()
            },
            WalkControls {
                forward: if on_foot && frame < 35 { 1. } else { 0. },
                ..Default::default()
            },
            1. / 60.,
        );
        let counts = world.pedestrian_reactions();
        seen_fall |= counts[1] > 0;
        seen_rise |= counts[2] > 0;
        let label = match frame {
            0 => Some("ragdoll-approach"),
            40 => Some("ragdoll-impact"),
            100 => Some("ragdoll-fallen"),
            230 => Some("ragdoll-get-up"),
            510 => Some("ragdoll-walking-again"),
            _ => None,
        };
        if let Some(label) = label {
            actors.update(&renderer, &world);
            let mut meshes = scenery.visible([target.x, target.y]);
            meshes.extend(actors.visible());
            let png = renderer
                .report_screenshot(
                    &meshes,
                    target + Vec3::new(-0.04, -0.075, 0.045),
                    target + Vec3::new(0.025, 0., 0.009),
                    0.,
                    hermes.minimap_view([target.x, target.y], 0.),
                )
                .unwrap();
            let label = if on_foot {
                format!("player-bump-{label}")
            } else {
                label.to_string()
            };
            save(&label, &png);
            eprintln!("{label}: walking/falling/rising {counts:?}");
        }
    }
    assert!(
        seen_fall && seen_rise,
        "contact never triggered the full reaction cycle"
    );
    assert_eq!(world.pedestrian_reactions(), [1, 0, 0]);
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_pedestrian_skins_textures_and_independent_poses() {
    let hermes = Hermes::test_map();
    let mut renderer = pollster::block_on(Renderer::new(None, 1440, 900)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let mut scenery = PedestrianScenery::new(&renderer);
    let mut frames = Vec::new();
    for (label, time) in [
        ("pedestrians-walk-a", 0.1),
        ("pedestrians-walk-b", 0.35),
        ("pedestrians-idle", 0.35),
    ] {
        scenery.poses.clear();
        let center = (scenery.models.len() - 1) as f32 * 0.5;
        for (i, (model, definition)) in scenery
            .models
            .iter_mut()
            .zip(pedestrian_catalog::definitions())
            .enumerate()
        {
            model.transforms.clear();
            let mut columns = pedestrian_catalog::instance_transform(
                Vec3::new((i as f32 - center) * 1.5, 0., 0.),
                -std::f32::consts::FRAC_PI_2,
            )
            .to_cols_array_2d();
            columns[0][3] = i as f32;
            columns[2][3] = scenery.poses.len() as f32;
            let amount = if label.ends_with("idle") { 0. } else { 1. };
            scenery.poses.extend(
                definition
                    .pose(time, time + i as f32 * 0.17, amount)
                    .map(|m| m.to_cols_array_2d()),
            );
            model.transforms.push(columns);
            renderer.update_instances(&mut model.mesh, &model.transforms);
        }
        renderer.queue.write_buffer(
            &renderer.pedestrians.poses,
            0,
            bytemuck::cast_slice(&scenery.poses),
        );
        let meshes: Vec<_> = scenery.visible().collect();
        assert_eq!(meshes.len(), pedestrian_catalog::definitions().len());
        let png = renderer
            .report_screenshot(
                &meshes,
                Vec3::new(0., -0.15, 0.045),
                Vec3::new(0., 0., 0.009),
                0.,
                hermes.minimap_view([0., 0.], 0.),
            )
            .unwrap();
        save(label, &png);
        frames.push(pixels(&png));
    }
    for (a, b) in [(&frames[0], &frames[1]), (&frames[1], &frames[2])] {
        let changed = a
            .chunks_exact(4)
            .zip(b.chunks_exact(4))
            .filter(|(a, b)| {
                a[0].abs_diff(b[0]) > 10 || a[1].abs_diff(b[1]) > 10 || a[2].abs_diff(b[2]) > 10
            })
            .count();
        assert!(
            changed > 500,
            "animated skins did not change enough visible pixels: {changed}"
        );
    }
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_pedestrian_population_on_sidewalks() {
    let mut hermes = if let Some(path) = std::env::var_os("PEDESTRIAN_MAP") {
        Hermes::load_with_game_data(std::path::Path::new(&path), None).unwrap()
    } else {
        Hermes::test_map()
    };
    let mut world = DynamicWorld::new(&hermes);
    for _ in 0..120 {
        world.step(
            &hermes,
            Controls {
                handbrake: true,
                ..Default::default()
            },
            WalkControls::default(),
            1. / 60.,
        );
    }
    let actor = world
        .pedestrian_instances()
        .next()
        .expect("sidewalk population");
    let foot = actor.transform.w_axis.truncate();
    let forward = actor.transform.transform_vector3(Vec3::Z) * 100.;
    let side = Vec3::new(-forward.y, forward.x, 0.);
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let scenery = super::super::scenery::Scenery::new(&renderer, &mut hermes);
    let mut actors = super::super::vehicle_scenery::VehicleScenery::new(&renderer);
    actors.update(&renderer, &world);
    let mut meshes = scenery.visible([foot.x, foot.y]);
    meshes.extend(actors.visible());
    let png = renderer
        .report_screenshot(
            &meshes,
            foot + forward * 0.05 + side * 0.035 + Vec3::Z * 0.027,
            foot + Vec3::Z * 0.009,
            0.,
            hermes.minimap_view([foot.x, foot.y], 0.),
        )
        .unwrap();
    save("pedestrians-game-sidewalk", &png);
    let rear = renderer
        .report_screenshot(
            &meshes,
            foot - forward * 0.045 + Vec3::Z * 0.016,
            foot + Vec3::Z * 0.009,
            0.,
            hermes.minimap_view([foot.x, foot.y], 0.),
        )
        .unwrap();
    save("pedestrians-game-rear-ik", &rear);
    eprintln!(
        "Pedestrian game snapshot: {} nearby actors, {} walking paths ({} connector paths)",
        world.pedestrian_instances().count(),
        hermes.walking_paths().paths.len(),
        hermes
            .walking_paths()
            .paths
            .iter()
            .filter(|p| p.connector)
            .count()
    );
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_ik_gait_six_rigs_and_rear_foot_placement() {
    use crate::dynamic_world::pedestrian_gait::{Gait, Ground};
    let hermes = Hermes::test_map();
    let mut renderer = pollster::block_on(Renderer::new(None, 1440, 900)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    let mut scenery = PedestrianScenery::new(&renderer);
    let sample = |p: Vec3| {
        Some(Ground {
            point: p.truncate().extend(0.),
            gradient: [0., 0.],
        })
    };
    let mut gaits: Vec<_> = pedestrian_catalog::definitions()
        .iter()
        .enumerate()
        .map(|(i, def)| {
            Gait::new(
                def,
                Vec3::new((i as f32 - 2.5) * 2., 0., 0.),
                -std::f32::consts::FRAC_PI_2,
                i as u32,
                sample,
            )
        })
        .collect();
    let mut images = Vec::new();
    let mut frame = 0;
    for (label, end, rear) in [
        ("ik-walk-contact", 40, false),
        ("ik-walk-transfer", 75, false),
        ("ik-walk-rear", 110, true),
        ("ik-turn-quarter", 170, false),
        ("ik-turn-half", 250, false),
        ("ik-turn-full", 398, false),
    ] {
        for _ in frame..end {
            frame += 1;
            for (i, (gait, def)) in gaits
                .iter_mut()
                .zip(pedestrian_catalog::definitions())
                .enumerate()
            {
                let p = Vec3::new(
                    (i as f32 - 2.5) * 2.,
                    -frame.min(110) as f32 * 1.05 / 120.,
                    0.,
                );
                let yaw =
                    -std::f32::consts::FRAC_PI_2 + (frame - frame.min(110)) as f32 * 2.62 / 120.;
                gait.advance(
                    def,
                    p,
                    yaw,
                    if frame <= 110 { 1.05 } else { 0. },
                    p + Vec3::new(yaw.cos(), yaw.sin(), 0.),
                    1. / 120.,
                    sample,
                );
                assert!(gait.reach_error < 0.02);
            }
        }
        scenery.poses.clear();
        for (i, (model, gait)) in scenery.models.iter_mut().zip(&gaits).enumerate() {
            let position = Vec3::new(
                (i as f32 - 2.5) * 2.,
                -frame.min(110) as f32 * 1.05 / 120.,
                0.,
            );
            let yaw = -std::f32::consts::FRAC_PI_2 + (frame - frame.min(110)) as f32 * 2.62 / 120.;
            let mut transform =
                pedestrian_catalog::instance_transform(position, yaw).to_cols_array_2d();
            transform[0][3] = i as f32;
            transform[2][3] = scenery.poses.len() as f32;
            model.transforms.clear();
            model.transforms.push(transform);
            scenery
                .poses
                .extend(gait.palette.map(|m| m.to_cols_array_2d()));
            renderer.update_instances(&mut model.mesh, &model.transforms);
        }
        renderer.queue.write_buffer(
            &renderer.pedestrians.poses,
            0,
            bytemuck::cast_slice(&scenery.poses),
        );
        let eye = Vec3::new(0., if rear { 0.15 } else { -0.15 }, 0.035);
        let png = renderer
            .report_screenshot(
                &scenery.visible().collect::<Vec<_>>(),
                eye,
                Vec3::new(0., 0., 0.009),
                0.,
                hermes.minimap_view([0., 0.], 0.),
            )
            .unwrap();
        save(label, &png);
        images.push(pixels(&png));
    }
    let changed = images[0]
        .chunks_exact(4)
        .zip(images[1].chunks_exact(4))
        .filter(|(a, b)| {
            a[0].abs_diff(b[0]) > 10 || a[1].abs_diff(b[1]) > 10 || a[2].abs_diff(b[2]) > 10
        })
        .count();
    assert!(
        changed > 500,
        "IK palette did not visibly animate: {changed} pixels"
    );
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn fabula_staging_reuses_models_and_restores_original_pose() {
    use crate::engine::fabula::{Anchor, Scene, Stage};
    let hermes = Hermes::test_map();
    let mut world = DynamicWorld::new(&hermes);
    if world.is_driving() {
        assert!(world.toggle_vehicle(&hermes));
    }
    let renderer = pollster::block_on(Renderer::new(None, 640, 480)).unwrap();
    let mut scenery = PedestrianScenery::new(&renderer);
    scenery.update(&renderer, &world);
    let transforms: Vec<_> = scenery
        .models
        .iter()
        .map(|m| m.transforms.clone())
        .collect();
    let poses = scenery.poses.clone();
    let position = world.player_position();
    let mut json: serde_json::Value =
        serde_json::from_str(include_str!("../../../assets/cutscenes/opening/scene.json")).unwrap();
    json["characters"][0]["offset"] = serde_json::json!([2., 0., 0.]);
    json["characters"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "source": "model", "index": 0, "offset": [-2., 0., -0.9], "yaw": 90., "pose": "walk"
        }));
    let scene = Scene::load(&json.to_string(), None).unwrap();
    scenery.update_with_stage(
        &renderer,
        &world,
        Some(Stage {
            definition: &scene.definition,
            anchor: Anchor {
                position,
                yaw: world.player_yaw(),
            },
            elapsed: 1.5,
        }),
    );
    assert_eq!(scenery.poses.len(), poses.len() + JOINT_LIMIT);
    assert_ne!(
        scenery.models[pedestrian_catalog::player_model(world.season())].transforms,
        transforms[pedestrian_catalog::player_model(world.season())]
    );
    assert_eq!(world.player_position(), position);
    scenery.update(&renderer, &world);
    assert_eq!(scenery.poses, poses);
    assert_eq!(
        scenery
            .models
            .iter()
            .map(|m| m.transforms.clone())
            .collect::<Vec<_>>(),
        transforms
    );
}
