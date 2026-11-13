use super::*;
use crate::{
    engine::render::Renderer,
    hermes::{Hermes, MinimapView, RouteStatus},
};
use std::path::Path;

fn navigation() -> MinimapView {
    MinimapView {
        uv_center: [0.5; 2],
        uv_span: 1.,
        heading: 0.,
        player_uv: [0.5; 2],
        destination_uv: [0.5; 2],
        route_status: RouteStatus::Inactive,
    }
}
fn target(r: &Renderer) -> wgpu::Texture {
    r.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("graffiti regression capture"),
        size: wgpu::Extent3d {
            width: r.config.width,
            height: r.config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: r.config.format.add_srgb_suffix(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}
fn save(r: &Renderer, pixels: &[u8], name: &str) {
    if let Some(directory) = std::env::var_os("GRAFFITI_CAPTURE") {
        std::fs::create_dir_all(&directory).unwrap();
        let mut encoder = png::Encoder::new(
            std::fs::File::create(Path::new(&directory).join(name)).unwrap(),
            r.config.width,
            r.config.height,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(pixels)
            .unwrap();
    }
}
fn delta(a: &[u8], b: &[u8]) -> u64 {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .map(|(a, b)| (0..3).map(|i| u64::from(a[i].abs_diff(b[i]))).sum::<u64>())
        .sum()
}
#[test]
#[ignore = "requires a native GPU adapter"]
fn native_paint_keeps_soft_alpha_and_supports_wash_and_color_override() {
    let program =
        opencity::parse(include_str!("../../../../tests/fixtures/graffiti.road")).unwrap();
    let city = opencity::runtime::City::from_program(&program, None).unwrap();
    let world = city
        .mesh(opencity::runtime::Region::WholeMap, 0.04)
        .unwrap();
    let mut panels = BTreeMap::<PanelKey, Vec<[Vertex; 3]>>::new();
    for t in world.vertices.chunks_exact(3) {
        if let Some(key) = panel_key(t).filter(|_| t[0].normal[1] < -0.99) {
            panels.entry(key).or_default().push([t[0], t[1], t[2]]);
        }
    }
    let panel = panels
        .values()
        .find(|t| paint_panel(t, 42).is_some())
        .expect("concrete fixture face");
    let mut painted = world.clone();
    for (seed, x) in [(42, -0.018), (43, 0.018)] {
        let mut mark = paint_panel(panel, seed).unwrap();
        let center = mark
            .vertices
            .iter()
            .fold(Vec3::ZERO, |s, v| s + Vec3::from_array(v.position))
            / 6.;
        for v in &mut mark.vertices {
            v.position[0] += x - center.x;
            v.position[2] += 0.005 - center.z;
        }
        painted.vertices.extend(mark.vertices);
    }
    let mut r = pollster::block_on(Renderer::new(None, 1024, 640)).unwrap();
    eprintln!("graffiti adapter: {}", r.adapter_name);
    r.daylight = Some(1.);
    r.ambient_occlusion = false;
    let bare = r.upload(&world);
    let paint = r.upload(&painted);
    let output = target(&r);
    let view = output.create_view(&Default::default());
    let eye = Vec3::new(0., -0.055, 0.011);
    let look = Vec3::new(0., 0., 0.006);
    let capture = |settings: Settings, mesh: &crate::engine::render::GpuMesh, name: &str| {
        r.configure_graffiti(settings);
        r.render(&view, &[mesh], eye, look, 0., navigation());
        let pixels = r.rgba_pixels(&output).unwrap();
        save(&r, &pixels, name);
        pixels
    };
    let bare_pixels = capture(Settings::default(), &bare, "fixture-bare.png");
    let zero = capture(
        Settings {
            ranges: [0., 0., 0., 0.],
            ..Settings::default()
        },
        &paint,
        "fixture-zero.png",
    );
    assert_eq!(
        bare_pixels, zero,
        "zero-opacity paint altered depth/shadows or background"
    );
    let gone = capture(
        Settings {
            ranges: [1., 1., 1., 1.],
            ..Settings::default()
        },
        &paint,
        "fixture-fully-washed.png",
    );
    assert_eq!(bare_pixels, gone, "fully washed paint must disappear");
    let full = capture(
        Settings {
            ranges: [1., 1., 0., 0.],
            ..Settings::default()
        },
        &paint,
        "fixture-full.png",
    );
    let faint = capture(
        Settings {
            ranges: [0.03, 0.03, 0., 0.],
            ..Settings::default()
        },
        &paint,
        "fixture-faint.png",
    );
    let washed = capture(
        Settings {
            ranges: [1., 1., 0.88, 0.88],
            ..Settings::default()
        },
        &paint,
        "fixture-washed.png",
    );
    assert!(delta(&bare_pixels, &full) > 5000, "paint not visible");
    assert!(
        delta(&bare_pixels, &faint) > 0
            && delta(&bare_pixels, &faint) < delta(&bare_pixels, &full) / 4
    );
    assert!(delta(&bare_pixels, &washed) < delta(&bare_pixels, &full));
    let recolored = capture(
        Settings {
            color_override: [0.015, 0.15, 0.65, 1.],
            ranges: [1., 1., 0., 0.],
        },
        &paint,
        "fixture-recolor.png",
    );
    assert!(
        delta(&full, &recolored) > 2000,
        "runtime color override failed"
    );
    let default = capture(Settings::default(), &paint, "fixture-default.png");
    assert!(
        delta(&bare_pixels, &default) > 0
            && delta(&bare_pixels, &default) < delta(&bare_pixels, &full)
    );
    r.configure_graffiti(Settings {
        ranges: [1., 1., 0., 0.],
        ..Settings::default()
    });
    let stats = r.visibility_stats(&[&paint], eye, look, 0.);
    assert_eq!(
        stats.shadow_vertices,
        world.vertices.len() as u64,
        "paint reached shadow draw ranges"
    );
}

#[test]
#[ignore = "requires a native GPU adapter and the canonical local map"]
fn native_sparse_graffiti_in_canonical_scenery() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut hermes =
        Hermes::load_with_game_data(&root.join("data/maps/mtl/map.road"), None).unwrap();
    let (world, details) = hermes.take_geometry();
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("bug-reports/43/report.json")).unwrap())
            .unwrap();
    let point = |field: &str| {
        Vec3::new(
            report[field][0].as_f64().unwrap() as f32,
            report[field][1].as_f64().unwrap() as f32,
            report[field][2].as_f64().unwrap() as f32,
        )
    };
    let at = point("map_coordinates");
    let mut planner = super::super::nearby::Planner::new(&world);
    let paint = planner.mesh([at.x, at.y]);
    assert!(
        !paint.vertices.is_empty(),
        "canonical concrete walls received no paint"
    );
    assert!(paint.vertices.len() / 6 <= MAX_VISIBLE);
    eprintln!(
        "report 43 nearby graffiti count: {}",
        paint.vertices.len() / 6
    );
    let mut r = pollster::block_on(Renderer::new(None, 1280, 753)).unwrap();
    r.daylight = Some(1.);
    for mark in paint.vertices.chunks_exact(6) {
        let center = mark
            .iter()
            .fold(Vec3::ZERO, |s, v| s + Vec3::from_array(v.position))
            / 6.;
        let normal = Vec3::from_array(mark[0].normal);
        let clip = r.view_projection(point("camera_position"), point("camera_look_at"))
            * center.extend(1.);
        eprintln!(
            "report candidate center={center:?},normal={normal:?},facing={},clip={:?},record={}",
            (point("camera_position") - center).dot(normal),
            clip.truncate() / clip.w,
            mark[0].surface[3]
        );
    }
    let mut nearby = Nearby::new(&r, &world);
    nearby.update(&r, [at.x, at.y]);
    let paint_stats = r.visibility_stats(
        &[&nearby.gpu],
        point("camera_position"),
        point("camera_look_at"),
        0.,
    );
    assert!(paint_stats.candidate_vertices <= MAX_VISIBLE as u64 * 6);
    assert_eq!(paint_stats.shadow_vertices, 0, "nearby paint casts shadows");
    let mut geometry = world;
    geometry.vertices.extend(details.vertices);
    let gpu = r.upload(&geometry);
    let output = target(&r);
    let view = output.create_view(&Default::default());
    let mut report_images = Vec::new();
    for (settings, name) in [
        (
            Settings {
                ranges: [0., 0., 0., 0.],
                ..Settings::default()
            },
            "report-43-before.png",
        ),
        (Settings::default(), "report-43-after.png"),
    ] {
        r.configure_graffiti(settings);
        r.render(
            &view,
            &[&gpu, &nearby.gpu],
            point("camera_position"),
            point("camera_look_at"),
            0.,
            navigation(),
        );
        let image = r.rgba_pixels(&output).unwrap();
        save(&r, &image, name);
        report_images.push(image);
    }
    eprintln!(
        "report43 default visible delta={}",
        delta(&report_images[0], &report_images[1])
    );
    r.configure_graffiti(Settings {
        ranges: [1., 1., 0., 0.],
        ..Settings::default()
    });
    r.render(
        &view,
        &[&gpu, &nearby.gpu],
        point("camera_position"),
        point("camera_look_at"),
        0.,
        navigation(),
    );
    let strong = r.rgba_pixels(&output).unwrap();
    save(&r, &strong, "report-43-strong.png");
    eprintln!(
        "report43 full paint delta={}",
        delta(&report_images[0], &strong)
    );
    let start = at.truncate();
    let mut marks: Vec<_> = paint.vertices.chunks_exact(6).collect();
    marks.sort_by(|a, b| {
        glam::Vec2::new(a[0].position[0], a[0].position[1])
            .distance_squared(start)
            .total_cmp(&glam::Vec2::new(b[0].position[0], b[0].position[1]).distance_squared(start))
    });
    let mut captured = 0;
    for mark in marks.into_iter().take(12) {
        let center = mark
            .iter()
            .fold(Vec3::ZERO, |sum, v| sum + Vec3::from_array(v.position))
            / 6.;
        let normal = Vec3::from_array(mark[0].normal);
        let tangent = Vec3::new(normal.y, -normal.x, 0.);
        let eye = center + normal * 0.05 + tangent * 0.006 + Vec3::Z * 0.006;
        nearby.update(&r, [center.x, center.y]);
        r.configure_graffiti(Settings {
            ranges: [0., 0., 0., 0.],
            ..Settings::default()
        });
        r.render(&view, &[&gpu, &nearby.gpu], eye, center, 0., navigation());
        let before = r.rgba_pixels(&output).unwrap();
        r.configure_graffiti(Settings::default());
        r.render(&view, &[&gpu, &nearby.gpu], eye, center, 0., navigation());
        let after = r.rgba_pixels(&output).unwrap();
        let difference = delta(&before, &after);
        eprintln!(
            "canonical paint: center={center:?},normal={normal:?},visible delta={difference}"
        );
        if difference < 500 {
            continue;
        }
        save(
            &r,
            &before,
            &format!("canonical-wall-{captured}-before.png"),
        );
        save(&r, &after, &format!("canonical-wall-{captured}.png"));
        captured += 1;
        if captured == 2 {
            break;
        }
    }
    assert_eq!(
        captured, 2,
        "could not inspect two visible canonical graffiti receivers"
    );
    assert!(
        delta(&report_images[0], &strong) > 1000,
        "report 43 retaining wall paint is absent even at full opacity"
    );
    // Seeded faint paint is allowed to be barely visible; compare it against the
    // exact empty frame rather than imposing the full-strength pixel threshold.
    assert!(
        delta(&report_images[0], &report_images[1]) > 100,
        "default paint disappeared at the saved report camera"
    );
}
