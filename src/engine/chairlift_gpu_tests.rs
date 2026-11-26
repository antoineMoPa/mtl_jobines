//! Real-map proof of instanced chairlift geometry, seated rider, and summit unload.
use super::{render::Renderer, vehicle_scenery::VehicleScenery};
use crate::{
    dynamic_world::{Controls, DynamicWorld, WalkControls, METERS_PER_UNIT},
    hermes::Hermes,
    season::Season,
};
use glam::Vec3;
#[test]
#[ignore = "requires native GPU; CHAIRLIFT_CAPTURE selects output directory"]
fn native_chairlift_departure_and_summit() {
    let output = std::env::var_os("CHAIRLIFT_CAPTURE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("chairlift-captures"));
    std::fs::create_dir_all(&output).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/map.road");
    let h = Hermes::load_with_game_data(&path, None).unwrap();
    let mut world = DynamicWorld::new(&h);
    world.set_season(&h, Season::Winter);
    world.prepare_ski_set_pickup(&h);
    assert!(world.toggle_skis());
    world.prepare_chairlift_boarding(&h);
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(12.);
    renderer.season = Season::Winter;
    renderer.winter_weather = false;
    let terrain = h
        .city()
        .mesh_with_external_models(
            opencity::runtime::Region::Radius {
                center: [0.8, -84.],
                radius: 10.,
            },
            0.04,
        )
        .unwrap();
    let terrain = renderer.upload(&terrain);
    let mut scenery = VehicleScenery::new(&renderer);
    // Include the nearby chalet with the same immutable model path used in the game.
    let mut buildings = Vec::new();
    for group in &h.external_models {
        let matrices: Vec<_> = group
            .transforms
            .iter()
            .filter(|t| {
                t.w_axis
                    .truncate()
                    .truncate()
                    .distance(glam::Vec2::new(4.1, -82.6))
                    < 1.
            })
            .map(|t| t.to_cols_array_2d())
            .collect();
        if !matrices.is_empty() {
            let mut gpu = renderer.upload_equipment(&group.mesh.vertices);
            renderer.update_instances(&mut gpu, &matrices);
            buildings.push(gpu);
        }
    }
    for (label, frames) in [
        ("base", 0),
        ("foundation", 0),
        ("departure", 180),
        ("side", 0),
        ("front", 0),
        ("ride-view", 0),
        ("arrival", 240),
        ("unloaded", 70),
    ] {
        if label == "departure" {
            for _ in 0..720 {
                world.step(&h, Controls::default(), WalkControls::default(), 1. / 60.);
                if world.riding_chairlift() {
                    break;
                }
            }
            assert!(world.riding_chairlift());
            assert!(world.is_wearing_skis());
        }
        for _ in 0..frames {
            world.step(&h, Controls::default(), WalkControls::default(), 1. / 60.);
        }
        scenery.update(&renderer, &world);
        let mut equipment = opencity::runtime::Mesh::default();
        world.write_moving_mesh(&mut equipment);
        let equipment = renderer.upload_dynamic(&equipment);
        let mut meshes = vec![&terrain, &equipment];
        meshes.extend(buildings.iter());
        meshes.extend(scenery.visible());
        let p = world.player_position() / METERS_PER_UNIT;
        let (eye, look) = match label {
            "base" => (
                p + Vec3::new(0.32, 0.25, 0.2),
                p + Vec3::new(-0.10, -0.02, 0.015),
            ),
            "foundation" => {
                let f = Vec3::new(world.player_yaw().cos(), world.player_yaw().sin(), 0.);
                let r = Vec3::new(f.y, -f.x, 0.);
                (
                    p - f * 0.12 + r * 0.10 - Vec3::Z * 0.008,
                    p - Vec3::Z * 0.010,
                )
            }
            "departure" => {
                let f = Vec3::new(world.player_yaw().cos(), world.player_yaw().sin(), 0.);
                let r = Vec3::new(f.y, -f.x, 0.);
                (
                    p + f * 0.05 + r * 0.06 + Vec3::Z * 0.025,
                    p - Vec3::Z * 0.004,
                )
            }
            "side" | "front" => {
                let f = Vec3::new(world.player_yaw().cos(), world.player_yaw().sin(), 0.);
                let r = Vec3::new(f.y, -f.x, 0.);
                let focus = p - Vec3::Z * 0.004;
                let offset = if label == "side" {
                    r * 0.042 + f * 0.003 + Vec3::Z * 0.003
                } else {
                    f * 0.05 + r * 0.005 + Vec3::Z * 0.005
                };
                (focus + offset, focus)
            }
            "ride-view" => world.chairlift_camera().unwrap(),
            _ => (p + Vec3::new(0.17, 0.14, 0.11), p - Vec3::Z * 0.003),
        };
        let png = renderer
            .report_screenshot(&meshes, eye, look, 0., h.minimap_view([p.x, p.y], 0.))
            .unwrap();
        std::fs::write(output.join(format!("chairlift-{label}.png")), png).unwrap();
    }
    assert!(!world.riding_chairlift());
}
