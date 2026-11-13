use super::*;
use crate::{
    dynamic_world::{catalog, DynamicWorld},
    engine::scenery::Scenery,
    hermes::Hermes,
};

fn pixels(png: &[u8]) -> Vec<u8> {
    let mut reader = png::Decoder::new(png).read_info().unwrap();
    let mut result = vec![0; reader.output_buffer_size()];
    reader.next_frame(&mut result).unwrap();
    result
}
fn save(bytes: &[u8], name: &str) {
    if let Some(directory) = std::env::var_os("RENDER_LOD_CAPTURE") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(std::path::Path::new(&directory).join(name), bytes).unwrap();
    }
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_render_lod_preserves_player_and_reduces_distant_car_with_bounded_silhouette_error() {
    let mut hermes =
        Hermes::test_map_from_source(include_str!("../../../../tests/fixtures/render_lod.road"));
    let world = DynamicWorld::new(&hermes);
    let mut r = pollster::block_on(Renderer::new(None, 1000, 650)).unwrap();
    r.ambient_occlusion = false;
    let scenery = Scenery::new(&r, &mut hermes);
    let at = world.player().position / crate::dynamic_world::METERS_PER_UNIT;
    let definition = &catalog::definitions()[0];
    let mut parts: Vec<_> = definition
        .part_vertices()
        .zip(definition.part_shadow_vertices())
        .map(|(full, shadow)| r.upload_vehicle_lod(full, shadow))
        .collect();
    let transforms = definition.part_transforms(world.player());
    if let Some(pixels) = hermes.update_minimap([at.x, at.y]) {
        r.update_minimap(pixels);
    }
    let navigation = hermes.minimap_view([at.x, at.y], world.player_yaw());
    for (name, distance, player) in [
        ("player", 0.12, true),
        ("middle", 0.6, false),
        ("far", 1.5, false),
    ] {
        for (mesh, transform) in parts.iter_mut().zip(&transforms) {
            let mut columns = transform.to_cols_array_2d();
            columns[3][3] = 1.;
            columns[0][3] = 1.;
            columns[1][3] = if player { 1. } else { 0. };
            r.update_instances(mesh, &[columns]);
        }
        let eye = at + Vec3::new(-distance, -distance * 0.65, distance * 0.4);
        let look = at + Vec3::Z * 0.007;
        let base = scenery.visible([at.x, at.y]);
        let mut meshes = base.clone();
        meshes.extend(parts.iter());
        for (light, daylight) in [("day", 1.), ("night", 0.)] {
            r.daylight = Some(daylight);
            r.render_lod = RenderLod::Full;
            let background = pixels(
                &r.report_screenshot(&base, eye, look, 0., navigation)
                    .unwrap(),
            );
            let before_png = r
                .report_screenshot(&meshes, eye, look, 0., navigation)
                .unwrap();
            let before = pixels(&before_png);
            let full = r.visibility_stats(&meshes, eye, look, 0.);
            r.render_lod = RenderLod::Adaptive;
            let after_png = r
                .report_screenshot(&meshes, eye, look, 0., navigation)
                .unwrap();
            let after = pixels(&after_png);
            let selected = r.visibility_stats(&meshes, eye, look, 0.);
            if player {
                assert_eq!(before, after, "near player detail changed: {light}");
            } else {
                assert!(
                    selected.camera_vertices < full.camera_vertices,
                    "distant camera LOD did not reduce geometry"
                );
                assert!(
                    selected.shadow_vertices < full.shadow_vertices,
                    "distant shadow LOD did not reduce geometry"
                );
                let mut overlap = 0;
                let mut union = 0;
                for ((a, b), bg) in before
                    .chunks_exact(4)
                    .zip(after.chunks_exact(4))
                    .zip(background.chunks_exact(4))
                {
                    let mask = |p: &[u8]| (0..3).any(|i| p[i].abs_diff(bg[i]) > 12);
                    let a = mask(a);
                    let b = mask(b);
                    overlap += usize::from(a && b);
                    union += usize::from(a || b);
                }
                assert!(union > 20, "vehicle missing from silhouette test");
                let agreement = overlap as f32 / union as f32;
                assert!(
                    agreement >= 0.70,
                    "silhouette agreement {name}/{light}: {agreement}"
                );
                eprintln!(
                    "LOD {name}/{light}: camera {} -> {}, shadow {} -> {}, silhouette {:.3}",
                    full.camera_vertices,
                    selected.camera_vertices,
                    full.shadow_vertices,
                    selected.shadow_vertices,
                    agreement
                );
            }
            save(&before_png, &format!("{name}-{light}-full.png"));
            save(&after_png, &format!("{name}-{light}-lod.png"));
        }
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn native_scenery_lod_preserves_near_detail_paint_ranges_and_offscreen_shadow_casters() {
    let mut r = pollster::block_on(Renderer::new(None, 800, 600)).unwrap();
    r.daylight = Some(1.);
    let mut source = Mesh::default();
    for x in 0..30 {
        for y in 0..30 {
            let x = x as f32 * 0.0003;
            let y = y as f32 * 0.0003;
            source.cuboid(
                [x, y, 0.],
                [x + 0.00025, y + 0.00025, 0.00025],
                [0.4, 0.4, 0.4],
                SurfaceKind::Prop,
            );
        }
    }
    let mut art = source.vertices[..3].to_vec();
    for v in &mut art {
        v.surface = [42., 0., 0., 0.];
        v.position[2] += 0.0005;
    }
    source.vertices.extend(art);
    let mesh = r.upload_scenery(&source);
    assert!(mesh.lod.is_some());
    let full = mesh.geometry(Choice::Full);
    let lod = mesh.geometry(Choice::Coarse);
    assert_eq!(
        full.count - full.paint,
        lod.count - lod.paint,
        "paint range was reduced"
    );
    let eye = Vec3::new(0., -0.01, 0.01);
    let look = Vec3::new(0.004, 0.004, 0.);
    let view = r.detail_view(eye, look);
    assert_eq!(
        mesh.static_draw(&view, RenderLod::Adaptive, false).count,
        full.count,
        "near scenery selected a reduced mesh"
    );
    let camera = Frustum::from_matrix(
        Mat4::perspective_rh(58_f32.to_radians(), 1., 0.003, 10.)
            * Mat4::look_at_rh(Vec3::new(0., 1., 0.2), Vec3::new(0., 2., 0.), Vec3::Z),
    );
    let shadow = Frustum::from_matrix(
        Mat4::orthographic_rh(-1.6, 1.6, -1.6, 1.6, 0.01, 10.)
            * Mat4::look_at_rh(Vec3::new(2., -2., 4.), Vec3::ZERO, Vec3::Z),
    );
    assert!(!camera.includes(mesh.bounds));
    assert!(shadow.includes(mesh.bounds));
    let far = r.detail_view(Vec3::new(0., -2., 1.), Vec3::ZERO);
    let caster = mesh.static_draw(&far, RenderLod::Adaptive, true);
    assert!(caster.paint > 0);
    assert_eq!(caster.paint, caster.count, "paint entered shadow geometry");
}
