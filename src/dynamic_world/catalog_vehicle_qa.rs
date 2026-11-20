use super::*;
use std::collections::{HashMap, HashSet};

pub(super) fn vented_bus_rim(steel: gltf::Primitive<'_>, blob: &[u8], left: bool) {
    let reader = steel.reader(|_| Some(blob));
    let positions: Vec<_> = reader.read_positions().unwrap().collect();
    let indices: Vec<_> = reader.read_indices().unwrap().into_u32().collect();
    let mut lip = f32::NEG_INFINITY;
    let mut dish = f32::NEG_INFINITY;
    let mut hub = f32::NEG_INFINITY;
    for p in &positions {
        let radius = p[0].hypot(p[1]);
        let outward = if left { -p[2] } else { p[2] };
        if radius > 0.37 {
            lip = lip.max(outward);
        }
        if (0.18..0.23).contains(&radius) {
            dish = dish.max(outward);
        }
        if radius < 0.12 {
            hub = hub.max(outward);
        }
        assert!(radius < 0.401, "rim extends beyond the tire bead");
        assert!(p[2].abs() < 0.199, "rim protrudes beyond the tire sidewall");
    }
    assert!(lip > dish + 0.045, "wheel face lacks a recessed dish");
    assert!(hub > dish + 0.035, "wheel hub is still flat");
    // A ray through each vent center must pass through the steel completely.
    // This rejects a solid cylinder with dark dots painted on its front face.
    for vent in 0..10 {
        let angle = std::f32::consts::TAU * vent as f32 / 10.;
        let center = glam::Vec2::new(angle.cos(), angle.sin()) * 0.27;
        let blocked = indices.chunks_exact(3).any(|ix| {
            let [a, b, c] = [ix[0], ix[1], ix[2]].map(|i| {
                let p = positions[i as usize];
                glam::Vec2::new(p[0], p[1])
            });
            let ab = b - a;
            let ac = c - a;
            let area = ab.perp_dot(ac);
            if area.abs() < 1e-10 {
                return false;
            }
            let q = center - a;
            let u = q.perp_dot(ac) / area;
            let v = ab.perp_dot(q) / area;
            u >= -1e-5 && v >= -1e-5 && u + v <= 1.00001
        });
        assert!(!blocked, "wheel vent {vent} is filled by a steel face");
    }
}

pub(super) fn textured_truck(
    bytes: &[u8],
    id: &str,
    wheel_count: usize,
    cabin_count: usize,
    texture_bytes: usize,
) {
    let source = gltf::Gltf::from_slice(bytes).unwrap();
    let blob = source.blob.as_deref().unwrap();
    let definition = definitions()
        .iter()
        .find(|d| d.config.research_id == id)
        .unwrap();
    assert_eq!(
        definition.config.sound.engine,
        crate::audio::EngineKind::Diesel
    );
    assert_eq!(source.images().count(), 1);
    assert_eq!(
        definition.config.qa["symmetry"],
        "negative-z-half-reflected"
    );
    assert_eq!(definition.config.qa["wheelStyle"], "recessed-dished-steel");
    let texture = match source.images().next().unwrap().source() {
        gltf::image::Source::View { view, mime_type } => {
            assert_eq!(mime_type, "image/png");
            &blob[view.offset()..view.offset() + view.length()]
        }
        _ => panic!("vehicle texture must be embedded"),
    };
    assert_eq!(
        texture.len(),
        texture_bytes,
        "original source atlas was replaced"
    );
    assert_eq!(definition.texture_png.as_deref(), Some(texture));
    let image = png::Decoder::new(texture).read_info().unwrap();
    assert_eq!((image.info().width, image.info().height), (1024, 1024));
    let names: HashSet<_> = source.nodes().filter_map(|n| n.name()).collect();
    assert_eq!(names.len(), source.nodes().count());
    assert!(names.contains("body_shell"));
    let cabin = source
        .nodes()
        .filter(|n| {
            let name = n.name().unwrap();
            name.starts_with("interior_")
                || name.starts_with("seat_")
                || name.starts_with("steering_")
                || name == "plow_control_console"
        })
        .map(|n| n.mesh().unwrap().primitives().count())
        .sum::<usize>();
    assert_eq!(cabin, cabin_count);
    assert_eq!(definition.config.qa["interiorPrimitives"], cabin);
    let wheels: Vec<_> = source
        .nodes()
        .filter(|n| n.name().unwrap().starts_with("wheel_"))
        .collect();
    assert_eq!(wheels.len(), wheel_count);
    for wheel in wheels
        .iter()
        .filter(|n| n.name().unwrap().contains("front"))
    {
        let mut metal_depth = 0f32;
        let mut tire_depth = 0f32;
        let mut metal_triangles = 0;
        for primitive in wheel.mesh().unwrap().primitives() {
            let metal = primitive
                .material()
                .pbr_metallic_roughness()
                .metallic_factor()
                > 0.5;
            let reader = primitive.reader(|_| Some(blob));
            if metal {
                metal_triangles += reader.read_indices().unwrap().into_u32().count() / 3;
            }
            for p in reader.read_positions().unwrap() {
                if metal {
                    metal_depth = metal_depth.max(p[2].abs());
                } else {
                    tire_depth = tire_depth.max(p[2].abs());
                }
            }
        }
        assert!(
            metal_triangles > 500,
            "recessed rim lacks hub/lug/dish geometry"
        );
        assert!(
            metal_depth + 0.02 < tire_depth,
            "steel face protrudes past tire shoulder"
        );
    }
    let glass: Vec<_> = source
        .nodes()
        .filter(|n| n.name().unwrap().starts_with("glass_") || n.name() == Some("windshield"))
        .collect();
    assert_eq!(glass.len(), 3);
    for node in glass {
        for primitive in node.mesh().unwrap().primitives() {
            assert_eq!(
                primitive.material().alpha_mode(),
                gltf::material::AlphaMode::Blend
            );
            assert_eq!(
                primitive
                    .material()
                    .pbr_metallic_roughness()
                    .base_color_factor()[3],
                0.7
            );
        }
    }
    assert_eq!(definition.headlights.len(), 2);
    assert_eq!(definition.rear_emitters.len(), 2);
    // The final six vertices are the separate shared registration-plate atlas.
    let body = &definition.parts[0].vertices;
    let textured: Vec<_> = body[..body.len() - 6]
        .iter()
        .chain(
            definition
                .parts
                .iter()
                .filter(|p| matches!(p.role, PartRole::Door(..)))
                .flat_map(|p| &p.vertices),
        )
        .filter(|v| v.surface[0] < 0.)
        .collect();
    assert!(textured.len() > 10_000);
    assert!(textured
        .iter()
        .all(|v| (-v.surface[0]).floor() == (-textured[0].surface[0]).floor()));
    let mut triangles = Vec::new();
    for node in source.nodes() {
        for primitive in node.mesh().unwrap().primitives() {
            if primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_texture()
                .is_none()
            {
                continue;
            }
            let reader = primitive.reader(|_| Some(blob));
            let positions: Vec<_> = reader.read_positions().unwrap().collect();
            let uvs: Vec<_> = reader.read_tex_coords(0).unwrap().into_f32().collect();
            let factor = primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_factor();
            for i in (0..positions.len()).step_by((positions.len() / 10).max(1)) {
                let expected =
                    game_vector(positions[i]) - Vec3::Z * definition.config.physics.height_offset;
                assert!(
                    textured
                        .iter()
                        .any(|v| Vec3::from_array(v.position).distance(expected) < 1e-5
                            && v.surface[2..] == uvs[i]
                            && v.color == factor[..3]),
                    "source UV or material lost at {expected:?}"
                );
            }
            let indices: Vec<_> = reader.read_indices().unwrap().into_u32().collect();
            for ix in indices.chunks_exact(3) {
                let ix: [u32; 3] = ix.try_into().unwrap();
                triangles.push((
                    ix.map(|i| Vec3::from(positions[i as usize])),
                    ix.map(|i| glam::Vec2::from(uvs[i as usize])),
                ));
            }
        }
    }
    // Check reflected points against opposite-side triangles, including UV correspondence.
    let mut grid: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
    for (index, (p, _)) in triangles.iter().enumerate() {
        let lo = (p[0].min(p[1]).min(p[2]) - Vec3::splat(0.0001))
            .to_array()
            .map(|v| (v * 4.).floor() as i32);
        let hi = (p[0].max(p[1]).max(p[2]) + Vec3::splat(0.0001))
            .to_array()
            .map(|v| (v * 4.).floor() as i32);
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    grid.entry([x, y, z]).or_default().push(index);
                }
            }
        }
    }
    for (p, uv) in triangles.iter().step_by(16) {
        let mut point = (p[0] + p[1] + p[2]) / 3.;
        point.z = -point.z;
        let expected = (uv[0] + uv[1] + uv[2]) / 3.;
        let key = point.to_array().map(|v| (v * 4.).floor() as i32);
        assert!(
            grid.get(&key)
                .is_some_and(|candidates| candidates.iter().any(|&i| {
                    let (p, uv) = triangles[i];
                    let a = p[1] - p[0];
                    let b = p[2] - p[0];
                    let n = a.cross(b);
                    let area = n.length_squared();
                    if area < 1e-16 {
                        return false;
                    }
                    let q = point - p[0];
                    let u = q.cross(b).dot(n) / area;
                    let v = a.cross(q).dot(n) / area;
                    u >= -0.001
                        && v >= -0.001
                        && u + v <= 1.001
                        && q.dot(n).abs() / area.sqrt() < 0.00003
                        && (uv[0] * (1. - u - v) + uv[1] * u + uv[2] * v).distance(expected)
                            < 0.00005
                })),
            "asymmetric textured surface at {point:?}"
        );
    }
}
