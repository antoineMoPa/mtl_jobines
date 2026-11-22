//! Screen-sized glows for distant airfield lenses. Positions are in map units.
use super::{impact::quad, render::ImpactVertex};
use glam::{Quat, Vec3};

pub(super) fn append(
    out: &mut Vec<ImpactVertex>,
    lights: impl Iterator<Item = (Vec3, Vec3)>,
    eye: Vec3,
    rotation: Quat,
    height: u32,
    far: f32,
    daylight: f32,
) {
    let night = 1. - (daylight / 0.65).clamp(0., 1.);
    if night <= 0. {
        return;
    }
    let forward = rotation * Vec3::X;
    let right = rotation * -Vec3::Y;
    let up = rotation * Vec3::Z;
    // Keep the core about two pixels wide at the gameplay camera's 58 degree FOV.
    let radius_per_depth = 6. * 29_f32.to_radians().tan() / height as f32;
    for (position, color) in lights {
        let depth = (position - eye).dot(forward);
        if depth <= 0. || depth >= far {
            continue;
        }
        let near_fade = ((depth - 0.15) / 0.6).clamp(0., 1.);
        let far_fade = ((far - depth) / (far * 0.2)).min(1.);
        let alpha = night * near_fade * far_fade;
        if alpha > 0. {
            let radius = (radius_per_depth * depth).max(0.003);
            // Put the glow on the visible face of the small cylindrical lens.
            let center = position - forward * 0.002;
            quad(out, |uv| {
                ImpactVertex::new(
                    center + (right * uv.x + up * uv.y) * radius,
                    uv.into(),
                    color,
                    [-1., 0., alpha, 0.],
                )
            });
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
#[ignore = "requires a native GPU adapter"]
fn distant_lens_is_visible_at_night_and_occluded_by_scenery() {
    use super::render::Renderer;
    use opencity::runtime::{Mesh, SurfaceKind};

    let mut renderer = pollster::block_on(Renderer::new(None, 320, 240)).unwrap();
    renderer.flight_visibility = 4.;
    let eye = Vec3::Z;
    let lens = eye + Vec3::X * 25.; // 2.5 km down the approach.
    let mut wall = Mesh::default();
    wall.cuboid([4., -1., 0.], [5., 1., 2.], [0.3; 3], SurfaceKind::Prop);
    let wall = renderer.upload(&wall);
    for (daylight, blocked) in [(0., false), (1., false), (0., true)] {
        renderer.daylight = Some(daylight);
        let meshes = if blocked { vec![&wall] } else { Vec::new() };
        let mut sprites = Vec::new();
        let mut baseline = Vec::new();
        for with_lamp in [false, true] {
            if with_lamp {
                let height = renderer.scene_height();
                append(
                    &mut sprites,
                    [(lens, Vec3::ONE)].into_iter(),
                    eye,
                    Quat::IDENTITY,
                    height,
                    40.,
                    daylight,
                );
            }
            renderer.update_impacts(&sprites);
            let png = renderer
                .capture_png(|view| renderer.render_scene(view, &meshes, eye, lens, 0., None))
                .unwrap();
            let mut reader = png::Decoder::new(png.as_slice()).read_info().unwrap();
            let mut pixels = vec![0; reader.output_buffer_size()];
            reader.next_frame(&mut pixels).unwrap();
            if !with_lamp {
                baseline = pixels;
            } else {
                let changed = pixels
                    .chunks_exact(4)
                    .zip(baseline.chunks_exact(4))
                    .filter(|(a, b)| a[0].saturating_sub(b[0]) > 24)
                    .count();
                if daylight == 0. && !blocked {
                    assert!((2..32).contains(&changed), "lit pixels: {changed}");
                } else {
                    assert_eq!(changed, 0);
                }
            }
        }
    }
}
