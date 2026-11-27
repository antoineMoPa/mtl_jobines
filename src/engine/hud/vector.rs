//! Tessellate the navigation SVG once; camera changes only update the GPU projection.
use opencity::runtime::City;
use resvg::{tiny_skia as sk, usvg};

pub(super) fn build(city: &City) -> Result<(Vec<[f32; 5]>, Vec<u8>), String> {
    let tree = usvg::Tree::from_str(
        &crate::hermes::metro::navigation_svg(city)?,
        &Default::default(),
    )
    .map_err(|e| e.to_string())?;
    let span = (city.bounds[2] - city.bounds[0]).max(city.bounds[3] - city.bounds[1]);
    let scale = (city.bounds[2] - city.bounds[0]) / span / tree.size().width();
    let transform = sk::Transform::from_row(
        scale,
        0.,
        0.,
        scale,
        (1. - tree.size().width() * scale) * 0.5,
        (1. - tree.size().height() * scale) * 0.5,
    );
    let mut vertices = Vec::new();
    // ViewBox transforms may wrap the ocean rectangle in a group. Walk the
    // entire tree and exclude that background by identity, never by nesting.
    collect(tree.root(), transform, &mut vertices)?;
    let background = usvg::Tree::from_str(
        &city.minimap_relief_svg_with_palette(&crate::hermes::NAVIGATION_PALETTE)?,
        &Default::default(),
    )
    .map_err(|e| e.to_string())?;
    let size = crate::hermes::TEXTURE_SIZE;
    let mut pixels = sk::Pixmap::new(size, size).ok_or("map background allocation")?;
    pixels.fill(sk::Color::from_rgba8(23, 62, 85, 255));
    resvg::render(
        &background,
        sk::Transform::from_scale(size as f32, size as f32).pre_concat(transform),
        &mut pixels.as_mut(),
    );
    Ok((vertices, pixels.take()))
}
fn collect(
    group: &usvg::Group,
    transform: sk::Transform,
    out: &mut Vec<[f32; 5]>,
) -> Result<(), String> {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect(g, transform, out)?,
            usvg::Node::Path(p) if p.id() == "navigation-ocean" => {}
            usvg::Node::Path(p) => {
                let t = transform.pre_concat(p.abs_transform());
                if let Some(fill) = p.fill() {
                    append(p.data(), t, fill.paint(), out)?;
                }
                if let Some(stroke) = p.stroke() {
                    if let Some(path) = p.data().stroke(&stroke.to_tiny_skia(), 1024.) {
                        append(&path, t, stroke.paint(), out)?;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}
fn append(
    path: &sk::Path,
    transform: sk::Transform,
    paint: &usvg::Paint,
    out: &mut Vec<[f32; 5]>,
) -> Result<(), String> {
    let usvg::Paint::Color(color) = paint else {
        return Err("unsupported navigation paint".into());
    };
    let rgb = [color.red, color.green, color.blue].map(|c| {
        let c = c as f32 / 255.;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    });
    let path = path
        .clone()
        .transform(transform)
        .ok_or("invalid map transform")?;
    let mut contour = Vec::<sk::Point>::new();
    let mut current = sk::Point::from_xy(0., 0.);
    for segment in path.segments() {
        match segment {
            sk::PathSegment::MoveTo(p) => {
                polygon(&mut contour, rgb, out)?;
                contour.push(p);
                current = p;
            }
            sk::PathSegment::LineTo(p) => {
                contour.push(p);
                current = p;
            }
            sk::PathSegment::QuadTo(b, c) => {
                let a = current;
                cubic(
                    a,
                    sk::Point::from_xy(a.x + (b.x - a.x) * 2. / 3., a.y + (b.y - a.y) * 2. / 3.),
                    sk::Point::from_xy(c.x + (b.x - c.x) * 2. / 3., c.y + (b.y - c.y) * 2. / 3.),
                    c,
                    0,
                    &mut contour,
                );
                current = c;
            }
            sk::PathSegment::CubicTo(b, c, d) => {
                cubic(current, b, c, d, 0, &mut contour);
                current = d;
            }
            sk::PathSegment::Close => {
                polygon(&mut contour, rgb, out)?;
            }
        }
    }
    polygon(&mut contour, rgb, out)
}
fn cubic(
    a: sk::Point,
    b: sk::Point,
    c: sk::Point,
    d: sk::Point,
    depth: u8,
    out: &mut Vec<sk::Point>,
) {
    // Subpixel error even at 16x zoom on a 4K display (UV tolerance 1e-6).
    let error = (b.x - (2. * a.x + d.x) / 3.)
        .abs()
        .max((b.y - (2. * a.y + d.y) / 3.).abs())
        .max((c.x - (a.x + 2. * d.x) / 3.).abs())
        .max((c.y - (a.y + 2. * d.y) / 3.).abs());
    if error < 0.000001 || depth == 18 {
        out.push(d);
        return;
    }
    let mid = |p: sk::Point, q: sk::Point| sk::Point::from_xy((p.x + q.x) * 0.5, (p.y + q.y) * 0.5);
    let ab = mid(a, b);
    let bc = mid(b, c);
    let cd = mid(c, d);
    let abc = mid(ab, bc);
    let bcd = mid(bc, cd);
    let center = mid(abc, bcd);
    cubic(a, ab, abc, center, depth + 1, out);
    cubic(center, bcd, cd, d, depth + 1, out);
}
fn polygon(
    points: &mut Vec<sk::Point>,
    rgb: [f32; 3],
    out: &mut Vec<[f32; 5]>,
) -> Result<(), String> {
    if points.len() >= 3 {
        let coords: Vec<f64> = points
            .iter()
            .flat_map(|p| [p.x as f64, p.y as f64])
            .collect();
        for i in earcutr::earcut(&coords, &[], 2).map_err(|e| e.to_string())? {
            out.push([points[i].x, points[i].y, rgb[0], rgb[1], rgb[2]]);
        }
    }
    points.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn terrain_city() -> City {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 1024, 1024);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![128; 1024 * 1024])
                .unwrap();
        }
        let program = opencity::parse("elevation_map(\"terrain.png\", -4, -2, 8, 4, -1, 1); water_level(0); road street: sidewalk carlane carlane sidewalk; end street(-2,0,2,0);").unwrap();
        City::from_program_with_elevation_loader(&program, |spec| {
            opencity::heightmap::ElevationMap::from_png(spec.clone(), &bytes)
        })
        .unwrap()
    }
    #[test]
    fn terrain_background_is_dark_gray_and_vector_layer_does_not_cover_it_with_ocean() {
        let (vertices, pixels) = build(&terrain_city()).unwrap();
        let size = crate::hermes::TEXTURE_SIZE as usize;
        let rgb = &pixels[(size / 2 * size + size / 2) * 4..][..3];
        assert_eq!(rgb, &[46; 3]);
        assert!(
            !vertices.iter().any(|v| v[4] > v[3]),
            "ocean background must not be drawn over the terrain image"
        );
    }
    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn gpu_vector_map_preserves_roads_at_overview_and_max_zoom() {
        use crate::engine::{
            hud::{FullMapView, Overlay},
            render::Renderer,
        };
        let city = std::sync::Arc::new(terrain_city());
        let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
        renderer.update_vector_map(&city).unwrap();
        let navigation = crate::hermes::MinimapView {
            texture_region: [0., 0., 1., 1.],
            uv_center: [0.5; 2],
            uv_span: 1.,
            heading: 0.,
            player_uv: [0.5; 2],
            destination_uv: [0.; 2],
            route_status: crate::hermes::RouteStatus::Inactive,
        };
        let capture = |renderer: &Renderer| {
            renderer
                .report_screenshot(
                    &[],
                    glam::Vec3::new(0., -1., 0.5),
                    glam::Vec3::ZERO,
                    0.,
                    navigation,
                )
                .unwrap()
        };
        let decode = |png: &[u8]| {
            let mut reader = png::Decoder::new(png).read_info().unwrap();
            let mut pixels = vec![0; reader.output_buffer_size()];
            reader.next_frame(&mut pixels).unwrap();
            pixels
        };
        renderer.cinematic = true;
        let scene = decode(&capture(&renderer));
        renderer.cinematic = false;
        for (name, overlay, zoom) in [
            ("overview", Overlay::Map, 1.),
            ("zoom", Overlay::Map, 16.),
            ("minimap", Overlay::Driving, 1.),
        ] {
            renderer.overlay = overlay;
            renderer.full_map = FullMapView {
                zoom,
                ..Default::default()
            };
            let png = capture(&renderer);
            if let Some(dir) = std::env::var_os("VECTOR_MAP_CAPTURE") {
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), &png)
                    .unwrap();
            }
            let pixels = decode(&png);
            let ((x, y), (ground_x, ground_y)) = if overlay == Overlay::Map {
                ((680, 430), (640, 270))
            } else {
                let [left, top, side, _] = super::super::minimap_rect(1280., 800.);
                let x = (left + side * 0.5) as usize;
                let y = (top + side * 0.5) as usize;
                ((x, y - 40), (x + 46, y))
            };
            let linear = |value: u8| {
                let value = f32::from(value) / 255.;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            for (x, y, map_color) in [(x, y, 80), (ground_x, ground_y, 46)] {
                let index = (y * 1280 + x) * 4;
                let rgb = &pixels[index..][..3];
                for (&channel, &background) in rgb.iter().zip(&scene[index..][..3]) {
                    let expected = if overlay == Overlay::Driving {
                        linear(map_color) * 0.75 + linear(background) * 0.25
                    } else {
                        linear(map_color)
                    };
                    assert!(
                        (linear(channel) - expected).abs() < 0.006,
                        "{name} ({x}, {y}): {rgb:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn vector_roads_align_with_navigation_and_background_has_no_baked_roads() {
        let city = City::from_program(
            &opencity::parse(
                "road street: sidewalk carlane carlane sidewalk; end street(-2,0,2,0);",
            )
            .unwrap(),
            None,
        )
        .unwrap();
        let (vertices, pixels) = build(&city).unwrap();
        assert!(!vertices.is_empty());
        assert_eq!(vertices.len() % 3, 0);
        assert!(vertices.iter().all(|v| v.iter().all(|c| c.is_finite())));
        let streets: Vec<_> = vertices.iter().filter(|v| v[2] > 0.07).collect();
        assert!(!streets.is_empty());
        assert!(streets.iter().any(|v| v[0] < 0.1));
        assert!(streets.iter().any(|v| v[0] > 0.9));
        assert!(streets.iter().all(|v| (v[1] - 0.5).abs() < 0.1));
        let size = crate::hermes::TEXTURE_SIZE as usize;
        assert_eq!(
            &pixels[(size / 2 * size + size / 2) * 4..][..4],
            &[23, 62, 85, 255]
        );
    }
    #[test]
    fn curved_roads_and_junction_subpaths_tessellate() {
        let city = City::from_program(
            &opencity::parse("road street: carlane carlane; end street(-2,0,-1,2,1,-2,2,0);")
                .unwrap(),
            None,
        )
        .unwrap();
        let (vertices, _) = build(&city).unwrap();
        assert!(vertices.len() > 100);
        assert!(vertices.iter().any(|v| v[1] < 0.45));
        assert!(vertices.iter().any(|v| v[1] > 0.55));
        let mut path = sk::PathBuilder::new();
        path.move_to(0., 0.);
        path.line_to(1., 0.);
        path.line_to(0., 1.);
        path.close();
        path.move_to(2., 2.);
        path.line_to(3., 2.);
        path.line_to(2., 3.);
        path.close();
        let mut triangles = Vec::new();
        append(
            &path.finish().unwrap(),
            sk::Transform::identity(),
            &usvg::Paint::Color(usvg::Color::new_rgb(226, 229, 210)),
            &mut triangles,
        )
        .unwrap();
        assert_eq!(triangles.len(), 6);
    }
}
