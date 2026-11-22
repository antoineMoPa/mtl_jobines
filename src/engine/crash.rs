//! Bounded, seekable billboards using the prelit density atlas from the crash study.
use super::{impact::quad, render::ImpactVertex};
use crate::dynamic_world::{Crash, METERS_PER_UNIT};
use glam::{Quat, Vec3};

#[derive(Default, Clone, Copy)]
struct Puff {
    center: Vec3,
    width: f32,
    height: f32,
    roll: f32,
    depth: f32,
    params: [f32; 4],
}
pub(super) fn sprites(crash: Crash, camera: Quat, out: &mut Vec<ImpactVertex>) {
    let mut puffs = [Puff::default(); 80];
    let mut count = 0;
    let forward = camera * Vec3::X;
    let mut seed = 24377_u32;
    let mut unit = || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / 16777216.
    };
    for i in 0..puffs.len() {
        let (a, b, c, d) = (unit(), unit(), unit(), unit());
        let fire = (36..58).contains(&i);
        let spark = i >= 58;
        let birth = if spark {
            a * 0.1
        } else if i == 0 || i == 36 {
            0.
        } else {
            a * a * if fire { 2.5 } else { 3.5 }
        };
        let t = crash.age - birth;
        let life = if spark {
            0.45 + d * 0.65
        } else if fire {
            0.45 + d * 0.55
        } else {
            2.5 + d * 1.5
        };
        if t < 0. || t >= life {
            continue;
        }
        let age = t / life;
        let spread = Vec3::new(b - 0.5, c - 0.5, 0.);
        let (offset, size, aspect, kind, alpha) = if spark {
            let off = spread * (9. * t) + Vec3::Z * ((2. + 6. * d) * t - 5. * t * t);
            if off.z < 0. {
                continue;
            }
            (off, 0.045, 2.8, 11., 1. - age)
        } else {
            let growth = 1. - (-4. * t).exp();
            let off = spread * (1.2 + 3.8 * (-2. * birth).exp()) * growth
                + Vec3::new(0.65 * t * t, 0.14 * t * t, t * (1.3 + 1.7 * b));
            let size = if fire {
                (0.55 + growth) * (1. - 0.35 * age)
            } else {
                0.42 + growth * (0.9 + d) + t * 0.6
            };
            let alpha = if i == 36 {
                1. - age
            } else {
                (t * 9.).min(1.) * (1. - age * age)
            };
            (off, size, 0.95, if fire { 10. } else { 9. }, alpha)
        };
        let offset = offset * crash.scale;
        puffs[count] = Puff {
            center: crash.origin + offset,
            width: size * crash.scale,
            height: size * aspect * crash.scale,
            roll: (c - 0.5) * 1.2 + age * (d - 0.5),
            depth: offset.dot(forward),
            params: [kind, (d * 4.).floor(), alpha, age],
        };
        count += 1;
    }
    puffs[..count].sort_unstable_by(|a, b| b.depth.total_cmp(&a.depth));
    out.reserve(6 * count);
    for p in &puffs[..count] {
        let roll = camera * Quat::from_rotation_x(p.roll);
        let right = roll * Vec3::Y * p.width;
        let up = roll * Vec3::Z * p.height;
        quad(out, |uv| {
            ImpactVertex::new(
                (p.center + right * uv.x + up * uv.y) / METERS_PER_UNIT,
                uv.into(),
                Vec3::ONE,
                p.params,
            )
        });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
#[ignore = "requires native GPU and the playable map"]
fn smoky_crash_composites_over_the_game_scene() {
    use super::{render::Renderer, scenery::Scenery, vehicle_scenery::VehicleScenery};
    use crate::{
        dynamic_world::{catalog, DynamicWorld},
        hermes::Hermes,
    };
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/map.road");
    let mut hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let mut world = DynamicWorld::new(&hermes);
    let model = catalog::definitions()
        .iter()
        .position(|d| d.config.research_id == "vehicle-v2-01")
        .unwrap();
    assert!(world.switch_vehicle(model));
    let mut renderer = pollster::block_on(Renderer::new(None, 800, 450)).unwrap();
    renderer.daylight = Some(1.);
    renderer.sky_settings.solar_hour = Some(15.);
    let scenery = Scenery::new(&renderer, &mut hermes);
    let mut actors = VehicleScenery::new(&renderer);
    actors.update(&renderer, &world);
    let car = world.player();
    let forward = Vec3::new(car.yaw.cos(), car.yaw.sin(), 0.);
    let origin = car.position + forward * car.length() * 0.46;
    let look = (car.position + Vec3::Z * 1.9) / METERS_PER_UNIT;
    let eye = look + (Vec3::Z.cross(forward) * 9. - forward * 5. + Vec3::Z * 2.) / METERS_PER_UNIT;
    let rotation = Quat::from_mat4(&glam::Mat4::look_at_rh(eye, look, Vec3::Z).inverse())
        * Quat::from_mat3(&glam::Mat3::from_cols(-Vec3::Z, -Vec3::X, Vec3::Y));
    let mut meshes = scenery.visible([look.x, look.y]);
    meshes.extend(actors.visible());
    let mut baseline = Vec::new();
    for (label, age) in [("clear", -1.), ("flash", 0.27), ("plume", 1.45)] {
        let mut vertices = Vec::new();
        sprites(
            Crash {
                origin,
                age,
                scale: 1.,
            },
            rotation,
            &mut vertices,
        );
        renderer.update_impacts(&vertices);
        let frame = renderer
            .capture_png(|view| renderer.render_scene(view, &meshes, eye, look, 0., None))
            .unwrap();
        if let Some(dir) = std::env::var_os("MTL_CRASH_CAPTURE") {
            std::fs::write(
                std::path::Path::new(&dir).join(format!("crash-{label}.png")),
                &frame,
            )
            .unwrap();
        }
        let mut reader = png::Decoder::new(frame.as_slice()).read_info().unwrap();
        let mut rgba = vec![0; reader.output_buffer_size()];
        reader.next_frame(&mut rgba).unwrap();
        if age < 0. {
            baseline = rgba;
        } else {
            assert!(
                rgba.iter()
                    .zip(&baseline)
                    .filter(|(a, b)| a.abs_diff(**b) > 15)
                    .count()
                    > 3000,
                "{label} should be clearly visible over the vehicle"
            );
        }
    }
}
