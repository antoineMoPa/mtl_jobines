use super::*;
use crate::hermes::external_models::{instances::ModelInstances, textures};
use glam::Mat4;
use opencity::runtime::{Mesh, SurfaceKind};
use std::sync::Arc;

fn quad(mesh: &mut Mesh, p: [Vec3; 4], layer: usize, tile: bool) {
    let n = (p[1] - p[0]).cross(p[2] - p[0]);
    let n = n / n.length().max(0.000001);
    let u = if tile {
        p[0].distance(p[1]) / 0.012
    } else {
        1.
    };
    let v = if tile {
        p[0].distance(p[3]) / 0.012
    } else {
        1.
    };
    let uv = [[0., v], [u, v], [u, 0.], [0., 0.]];
    for i in [0, 1, 2, 0, 2, 3] {
        mesh.vertices.push(opencity::runtime::Vertex {
            position: p[i].to_array(),
            normal: n.to_array(),
            color: [1.; 3],
            surface: [42., uv[i][0], uv[i][1], layer as f32],
        });
    }
}
fn concrete() -> Result<usize, String> {
    let size = textures::SIZE as usize;
    let mut colour = vec![0; size * size * 4];
    let mut normal = colour.clone();
    let bump = |x: i32, y: i32| -> f32 {
        let x = x.rem_euclid(512) as u32;
        let y = y.rem_euclid(512) as u32;
        let h = x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663);
        (h % 97) as f32 / 97.
    };
    for y in 0..size {
        for x in 0..size {
            let i = (y * size + x) * 4;
            let pore = bump(x as i32, y as i32);
            let mottling = bump((x / 16) as i32, (y / 16) as i32);
            for (c, base) in [0.235, 0.245, 0.25].iter().enumerate() {
                colour[i + c] =
                    ((base + (mottling - 0.5) * 0.035 + (pore - 0.5) * 0.025) * 255.) as u8;
            }
            colour[i + 3] = 255;
            let dx = (bump(x as i32 - 1, y as i32) - bump(x as i32 + 1, y as i32)) * 0.13;
            let dy = (bump(x as i32, y as i32 + 1) - bump(x as i32, y as i32 - 1)) * 0.13;
            let len = dx.hypot(dy).hypot(1.);
            for (c, v) in [dx / len, dy / len, 1. / len].iter().enumerate() {
                normal[i + c] = ((v * 0.5 + 0.5) * 255.) as u8;
            }
            normal[i + 3] = 235;
        }
    }
    textures::register_pixels(colour, normal)
}
fn png_layer(bytes: &[u8]) -> Result<usize, String> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut data = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut data).map_err(|e| e.to_string())?;
    let mut colour = vec![0; 512 * 512 * 4];
    for y in 0..512 {
        for x in 0..512 {
            let i = ((y * info.height as usize / 512 * info.width as usize
                + x * info.width as usize / 512)
                * info.color_type.samples()) as usize;
            let o = (y * 512 + x) * 4;
            colour[o..o + 3].copy_from_slice(&data[i..i + 3]);
            colour[o + 3] = 255;
        }
    }
    textures::register_pixels(colour, [128, 128, 255, 200].repeat(512 * 512))
}
pub(super) fn build(map: &MetroMap) -> Result<Option<ModelInstances>, String> {
    if map.stations.is_empty() {
        return Ok(None);
    }
    let wall_layer = concrete()?;
    let sign = png_layer(include_bytes!("../../../data/metro/entrance-sign.png"))?;
    let mut mesh = Mesh::default();
    // Enclosed tunnel follows the same sampled path as the train. No station end walls cross the track.
    for pair in map.samples.windows(2) {
        let a = pair[0].1;
        let b = pair[1].1;
        let d = (b - a).truncate();
        if d.length() < 0.000001 {
            continue;
        }
        let n = Vec3::new(-d.y, d.x, 0.) / d.length();
        // Continuous concrete bed and two solid running rails. There are no sleepers.
        let bed = Vec3::Z * 0.0008;
        mesh.quad(
            [
                (a - n * 0.029 - bed).to_array(),
                (b - n * 0.029 - bed).to_array(),
                (b + n * 0.029 - bed).to_array(),
                (a + n * 0.029 - bed).to_array(),
            ],
            [0.42, 0.44, 0.43],
            SurfaceKind::Sidewalk,
        );
        for offset in [-0.01105, 0.01105] {
            let lo = offset - 0.0011;
            let hi = offset + 0.0011;
            let vertices = [
                a + n * lo - bed,
                b + n * lo - bed,
                b + n * hi - bed,
                a + n * hi - bed,
                a + n * lo,
                b + n * lo,
                b + n * hi,
                a + n * hi,
            ];
            for face in [
                [0, 1, 5, 4],
                [1, 2, 6, 5],
                [2, 3, 7, 6],
                [3, 0, 4, 7],
                [4, 5, 6, 7],
                [3, 2, 1, 0],
            ] {
                mesh.quad(
                    face.map(|i| vertices[i].to_array()),
                    [0.44, 0.48, 0.50],
                    SurfaceKind::Prop,
                );
            }
        }
        if map
            .stations
            .iter()
            .any(|s| ((a.truncate() + b.truncate()) * 0.5).distance(s.center) < 0.325)
        {
            continue;
        }
        for side in [-1., 1.] {
            let mut face = [
                a + n * 0.029 * side - Vec3::Z * 0.001,
                b + n * 0.029 * side - Vec3::Z * 0.001,
                b + n * 0.029 * side + Vec3::Z * 0.023,
                a + n * 0.029 * side + Vec3::Z * 0.023,
            ];
            if side < 0. {
                face.reverse();
            }
            quad(&mut mesh, face, wall_layer, true);
        }
        // Elliptical vault: 2.3 m vertical walls, 4.8 m crown, 5.8 m clear width.
        for i in 0..16 {
            let t0 = i as f32 * std::f32::consts::PI / 16.;
            let t1 = (i + 1) as f32 * std::f32::consts::PI / 16.;
            let c0 = n * (0.029 * t0.cos()) + Vec3::Z * (0.023 + 0.025 * t0.sin());
            let c1 = n * (0.029 * t1.cos()) + Vec3::Z * (0.023 + 0.025 * t1.sin());
            quad(
                &mut mesh,
                [a + c0, b + c0, b + c1, a + c1],
                wall_layer,
                true,
            );
        }
    }
    for p in map.tunnel_fixtures() {
        mesh.cuboid(
            (p - Vec3::new(0.012, 0.003, 0.001)).to_array(),
            (p + Vec3::new(0.012, 0.003, 0.001)).to_array(),
            [0.85, 0.91, 1.],
            SurfaceKind::Light,
        );
    }
    for s in &map.stations {
        let p = |a, b, h| s.point(a, b, h);
        // Raised concrete platform and sealed gutter hide the excavation's grass surface.
        mesh.quad(
            [
                p(-0.32, 0.0151, FLOOR_HEIGHT + 0.00015).to_array(),
                p(0.32, 0.0151, FLOOR_HEIGHT + 0.00015).to_array(),
                p(0.32, 0.1175, FLOOR_HEIGHT + 0.00015).to_array(),
                p(-0.32, 0.1175, FLOOR_HEIGHT + 0.00015).to_array(),
            ],
            [0.52, 0.53, 0.50],
            SurfaceKind::Sidewalk,
        );
        mesh.quad(
            [
                p(-0.32, -0.0375, TRACK_HEIGHT - 0.0008).to_array(),
                p(0.32, -0.0375, TRACK_HEIGHT - 0.0008).to_array(),
                p(0.32, -0.029, TRACK_HEIGHT - 0.0008).to_array(),
                p(-0.32, -0.029, TRACK_HEIGHT - 0.0008).to_array(),
            ],
            [0.42, 0.44, 0.43],
            SurfaceKind::Sidewalk,
        );
        mesh.quad(
            [
                p(-0.32, 0.0151, TRACK_HEIGHT - 0.0008).to_array(),
                p(0.32, 0.0151, TRACK_HEIGHT - 0.0008).to_array(),
                p(0.32, 0.0151, FLOOR_HEIGHT + 0.00015).to_array(),
                p(-0.32, 0.0151, FLOOR_HEIGHT + 0.00015).to_array(),
            ],
            [0.42, 0.44, 0.43],
            SurfaceKind::Sidewalk,
        );
        // Large concrete chamber, 64×16 m, with a raised passenger dock.
        for side in [-0.0375, 0.1175] {
            quad(
                &mut mesh,
                [
                    p(-0.32, side, TRACK_HEIGHT - 0.001),
                    p(0.32, side, TRACK_HEIGHT - 0.001),
                    p(0.32, side, TRACK_HEIGHT + 0.068),
                    p(-0.32, side, TRACK_HEIGHT + 0.068),
                ],
                wall_layer,
                true,
            );
        }
        // End bulkheads frame the tunnel; leave the 5.8 m rail opening clear.
        for (along, b, d) in [
            (-0.3175, 0.029, 0.04),
            (-0.3175, 0.08, 0.1175),
            (0.3175, 0.029, 0.1175),
        ] {
            quad(
                &mut mesh,
                [
                    p(along, b, TRACK_HEIGHT),
                    p(along, d, TRACK_HEIGHT),
                    p(along, d, TRACK_HEIGHT + 0.068),
                    p(along, b, TRACK_HEIGHT + 0.068),
                ],
                wall_layer,
                true,
            );
        }
        // Arched station mouths join the vault without an open gap above its crown.
        for end in [-0.3175, 0.3175] {
            for i in 0..16 {
                let t0 = i as f32 * std::f32::consts::PI / 16.;
                let t1 = (i + 1) as f32 * std::f32::consts::PI / 16.;
                let x0 = 0.029 * t0.cos();
                let x1 = 0.029 * t1.cos();
                let z0 = TRACK_HEIGHT + 0.023 + 0.025 * t0.sin();
                let z1 = TRACK_HEIGHT + 0.023 + 0.025 * t1.sin();
                let mut face = [
                    p(end, x0, z0),
                    p(end, x1, z1),
                    p(end, x1, TRACK_HEIGHT + 0.068),
                    p(end, x0, TRACK_HEIGHT + 0.068),
                ];
                if end > 0. {
                    face.reverse();
                }
                quad(&mut mesh, face, wall_layer, true);
            }
            quad(
                &mut mesh,
                [
                    p(end, -0.0375, TRACK_HEIGHT - 0.001),
                    p(end, -0.029, TRACK_HEIGHT - 0.001),
                    p(end, -0.029, TRACK_HEIGHT + 0.068),
                    p(end, -0.0375, TRACK_HEIGHT + 0.068),
                ],
                wall_layer,
                true,
            );
        }
        for side in [0.0425, 0.0775] {
            quad(
                &mut mesh,
                [
                    p(-0.50, side, TRACK_HEIGHT - 0.001),
                    p(-0.30, side, TRACK_HEIGHT - 0.001),
                    p(-0.30, side, 0.115),
                    p(-0.50, side, 0.115),
                ],
                wall_layer,
                true,
            );
            mesh.quad(
                [
                    p(-0.50, side - 0.002, 0.115).to_array(),
                    p(-0.30, side - 0.002, 0.115).to_array(),
                    p(-0.30, side + 0.002, 0.115).to_array(),
                    p(-0.50, side + 0.002, 0.115).to_array(),
                ],
                [0.32, 0.23, 0.23],
                SurfaceKind::Prop,
            );
        }
        // Concrete roof covers the excavation, with a genuine opening over the stairs.
        for (a, b, c, d) in [
            (-0.28, -0.04, 0.32, 0.12),
            (-0.32, -0.04, -0.28, 0.04),
            (-0.32, 0.08, -0.28, 0.12),
        ] {
            quad(
                &mut mesh,
                [
                    p(a, d, TRACK_HEIGHT + 0.068),
                    p(c, d, TRACK_HEIGHT + 0.068),
                    p(c, b, TRACK_HEIGHT + 0.068),
                    p(a, b, TRACK_HEIGHT + 0.068),
                ],
                wall_layer,
                true,
            );
        }
        // Stair treads sit on the collision ramp; 17.7 cm risers, 55 cm treads.
        for i in 0..72 {
            let a = -0.50 + i as f32 * 0.20 / 72.;
            let b = a + 0.20 / 72.;
            let top = 0.102 - i as f32 * (0.102 - FLOOR_HEIGHT) / 72.;
            mesh.quad(
                [
                    p(a, 0.045, top).to_array(),
                    p(b, 0.045, top).to_array(),
                    p(b, 0.075, top).to_array(),
                    p(a, 0.075, top).to_array(),
                ],
                [0.55, 0.56, 0.53],
                SurfaceKind::Prop,
            );
            mesh.quad(
                [
                    p(b, 0.045, top).to_array(),
                    p(b, 0.045, top - (0.102 - FLOOR_HEIGHT) / 72.).to_array(),
                    p(b, 0.075, top - (0.102 - FLOOR_HEIGHT) / 72.).to_array(),
                    p(b, 0.075, top).to_array(),
                ],
                [0.30, 0.31, 0.29],
                SurfaceKind::Prop,
            );
        }
        // Entrance sign is the supplied artwork, with matching physical proportions.
        let pole = p(-0.505, 0.022, 0.102);
        mesh.cuboid(
            (pole - Vec3::new(0.001, 0.001, 0.)).to_array(),
            (pole + Vec3::new(0.001, 0.001, 0.041)).to_array(),
            [0.12, 0.13, 0.14],
            SurfaceKind::Prop,
        );
        // Cantilever stays above the artwork; the post clears both faces and the nameboard.
        let arm_a = p(-0.506, 0.022, 0.142);
        let arm_b = p(-0.504, 0.042, 0.143);
        mesh.cuboid(
            arm_a.min(arm_b).to_array(),
            arm_a.max(arm_b).to_array(),
            [0.12, 0.13, 0.14],
            SurfaceKind::Prop,
        );
        quad(
            &mut mesh,
            [
                p(-0.505, 0.029, 0.126),
                p(-0.505, 0.041, 0.126),
                p(-0.505, 0.041, 0.14075),
                p(-0.505, 0.029, 0.14075),
            ],
            sign,
            false,
        );
        quad(
            &mut mesh,
            [
                p(-0.5051, 0.041, 0.126),
                p(-0.5051, 0.029, 0.126),
                p(-0.5051, 0.029, 0.14075),
                p(-0.5051, 0.041, 0.14075),
            ],
            sign,
            false,
        );
        let name = png_layer(s.id.label_png())?;
        quad(
            &mut mesh,
            [
                p(-0.5051, 0.045, 0.119),
                p(-0.5051, 0.027, 0.119),
                p(-0.5051, 0.027, 0.124),
                p(-0.5051, 0.045, 0.124),
            ],
            name,
            false,
        );
        quad(
            &mut mesh,
            [
                p(-0.505, 0.027, 0.119),
                p(-0.505, 0.045, 0.119),
                p(-0.505, 0.045, 0.124),
                p(-0.505, 0.027, 0.124),
            ],
            name,
            false,
        );
        for a in [-0.13, 0.13] {
            quad(
                &mut mesh,
                [
                    p(a - 0.05, 0.1170, TRACK_HEIGHT + 0.023),
                    p(a + 0.05, 0.1170, TRACK_HEIGHT + 0.023),
                    p(a + 0.05, 0.1170, TRACK_HEIGHT + 0.051),
                    p(a - 0.05, 0.1170, TRACK_HEIGHT + 0.051),
                ],
                name,
                false,
            );
        }
        // Tactile yellow edge and overhead fluorescent housings.
        mesh.quad(
            [
                p(-0.32, 0.0151, FLOOR_HEIGHT + 0.0002).to_array(),
                p(0.32, 0.0151, FLOOR_HEIGHT + 0.0002).to_array(),
                p(0.32, 0.019, FLOOR_HEIGHT + 0.0002).to_array(),
                p(-0.32, 0.019, FLOOR_HEIGHT + 0.0002).to_array(),
            ],
            [0.9, 0.68, 0.06],
            SurfaceKind::Prop,
        );
        for a in [-0.28, -0.09, 0., 0.09, 0.18] {
            mesh.quad(
                [
                    p(a - 0.025, 0.04, TRACK_HEIGHT + 0.064).to_array(),
                    p(a + 0.025, 0.04, TRACK_HEIGHT + 0.064).to_array(),
                    p(a + 0.025, 0.046, TRACK_HEIGHT + 0.064).to_array(),
                    p(a - 0.025, 0.046, TRACK_HEIGHT + 0.064).to_array(),
                ],
                [1., 0.93, 0.76],
                SurfaceKind::Light,
            );
        }
    }
    Ok(Some(ModelInstances {
        mesh: Arc::new(mesh),
        transforms: vec![Mat4::IDENTITY],
    }))
}
