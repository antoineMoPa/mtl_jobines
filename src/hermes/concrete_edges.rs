//! Clip weathered shoulder strips against the finished pavement triangles.
//! Preserve road openings and existing triangulation for arbitrary authored footprints.
use glam::Vec2;
use opencity::runtime::{City, Mesh, SurfaceKind};

#[derive(Clone, Copy, PartialEq)]
enum Contact {
    Turf,
    Mineral,
}

// Airport slab and shoulder tags are decoded in fs_opaque_game.
const AIRPORT_SLAB: f32 = -3.25;
const AIRPORT_TURF_EDGE: f32 = -3.5;
const AIRPORT_MINERAL_EDGE: f32 = -3.75;

impl Contact {
    fn marker(self, airport: bool) -> f32 {
        match (self, airport) {
            (Self::Turf, true) => AIRPORT_TURF_EDGE,
            (Self::Mineral, true) => AIRPORT_MINERAL_EDGE,
            (Self::Turf, false) => -1.5,
            (Self::Mineral, false) => -1.75,
        }
    }
}

struct StripEdge {
    polygon: [Vec2; 4],
    contact: Contact,
    width_scale: f32,
    origin: Vec2,
    outward: Vec2,
}

fn clip(poly: &[Vec2], a: Vec2, normal: Vec2) -> (Vec<Vec2>, Vec<Vec2>) {
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    let Some(mut prev) = poly.last().copied() else {
        return (inside, outside);
    };
    let mut dp = (prev - a).dot(normal);
    for &p in poly {
        let d = (p - a).dot(normal);
        if (dp > 0. && d < 0.) || (dp < 0. && d > 0.) {
            let crossing = prev.lerp(p, dp / (dp - d));
            inside.push(crossing);
            outside.push(crossing);
        }
        if d >= 0. {
            inside.push(p);
        }
        if d <= 0. {
            outside.push(p);
        }
        prev = p;
        dp = d;
    }
    (inside, outside)
}

fn exposed_strips(city: &City, corners: &[Vec2], z: f32) -> Vec<StripEdge> {
    let n = corners.len();
    let ends = |i: usize| (corners[i], corners[(i + 1) % n]);
    let winding = (0..n)
        .map(|i| {
            let (a, b) = ends(i);
            (a - corners[0]).perp_dot(b - corners[0])
        })
        .sum::<f32>()
        .signum();
    let mut normals = Vec::with_capacity(n);
    let mut width = 0.012f32;
    for i in 0..n {
        let (a, b) = ends(i);
        let length = a.distance(b);
        if length < 1e-6 {
            return Vec::new();
        }
        normals.push((a - b).perp() * (winding / length));
        width = width.min(length * 0.2);
    }
    // Offset the incident lines to meet at each corner, including re-entrant corners.
    let inset: Vec<_> = (0..n)
        .map(|i| {
            let before = normals[(i + n - 1) % n];
            let after = normals[i];
            corners[i] - (before + after) * (width / (1. + before.dot(after)).max(0.25))
        })
        .collect();
    let mut strips = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b) = ends(i);
        let edge = b - a;
        let outward = normals[i];
        let span = corners
            .iter()
            .map(|p| (p - a).dot(outward).abs())
            .fold(0., f32::max);
        let probe = 0.0003 + span * 0.0001;
        let steps = (edge.length() / 0.005).ceil().max(1.) as usize;
        let mut previous = None;
        let mut first = 0;
        for step in 0..=steps {
            let point = a + edge * ((step as f32 + 0.5) / steps as f32) + outward * probe;
            let contact = if step == steps {
                None
            } else {
                match city.surface_at(point.to_array(), z + 0.003) {
                    Some(s) if s.kind == SurfaceKind::Sidewalk && (s.height - z).abs() < 0.0005 => {
                        None
                    }
                    Some(s) if s.kind == SurfaceKind::Grass && (s.height - z).abs() < 0.003 => {
                        Some(Contact::Turf)
                    }
                    _ => Some(Contact::Mineral),
                }
            };
            if contact == previous {
                continue;
            }
            let begin = first;
            first = step;
            let last = std::mem::replace(&mut previous, contact);
            let Some(material) = last else { continue };
            let outer = |k: usize| a + edge * (k as f32 / steps as f32);
            let inner = |k: usize| {
                if k == 0 {
                    return inset[i];
                }
                if k == steps {
                    return inset[j];
                }
                let span = inset[j] - inset[i];
                inset[i]
                    + span * ((outer(k) - inset[i]).dot(span) / span.length_squared()).clamp(0., 1.)
            };
            strips.push(StripEdge {
                polygon: [outer(begin), outer(step), inner(step), inner(begin)],
                contact: material,
                width_scale: width / 0.012,
                origin: a,
                outward,
            });
        }
    }
    strips
}

pub(super) fn add_edges(city: &City, mesh: &mut Mesh) {
    let mut additions = Mesh::default();
    // Look up the slab faces by centroid once. A city contains millions of other faces.
    let mut slabs: Vec<_> = mesh
        .vertices
        .chunks_exact(3)
        .enumerate()
        .filter(|(_, t)| {
            t[0].surface[0] == SurfaceKind::Sidewalk as u32 as f32
                && t[0].surface[1] == t[0].position[2]
        })
        .map(|(i, t)| {
            let center = t
                .iter()
                .map(|v| Vec2::new(v.position[0], v.position[1]))
                .sum::<Vec2>()
                / 3.;
            (center, i)
        })
        .collect();
    slabs.sort_unstable_by(|a, b| a.0.x.total_cmp(&b.0.x));
    for f in city
        .landscape
        .iter()
        .filter(|f| f.kind == opencity::LandscapeKind::Concrete && f.points.len() >= 3)
    {
        let mut corners: Vec<_> = f.points.iter().copied().map(Vec2::from_array).collect();
        corners.dedup();
        if corners.first() == corners.last() {
            corners.pop();
        }
        if corners.len() < 3 {
            continue;
        }
        let lo = corners.iter().copied().reduce(Vec2::min).unwrap() - 0.00001;
        let hi = corners.iter().copied().reduce(Vec2::max).unwrap() + 0.00001;
        let z = f.elevation - 0.02;
        let left = slabs.partition_point(|s| s.0.x < lo.x);
        let right = slabs.partition_point(|s| s.0.x <= hi.x);
        if left == right {
            continue;
        }
        let airport = f
            .name
            .as_deref()
            .is_some_and(|name| name.starts_with("airport_"));
        let strips = exposed_strips(city, &corners, z);
        for &(center, index) in &slabs[left..right] {
            if center.y < lo.y || center.y > hi.y {
                continue;
            }
            // Overlapping bounds of rotated or concave polygons can contain a different slab.
            let mut inside = false;
            for (a, b) in corners.iter().zip(corners.iter().cycle().skip(1)) {
                if (a.y > center.y) != (b.y > center.y)
                    && center.x < a.x + (center.y - a.y) * (b.x - a.x) / (b.y - a.y)
                {
                    inside = !inside;
                }
            }
            if !inside {
                continue;
            }
            let tri = &mut mesh.vertices[index * 3..index * 3 + 3];
            if tri[0].surface[0] != SurfaceKind::Sidewalk as u32 as f32
                || !tri.iter().all(|v| {
                    let p = Vec2::new(v.position[0], v.position[1]);
                    (v.position[2] - z).abs() < 0.00004 && p.cmpge(lo).all() && p.cmple(hi).all()
                })
            {
                continue;
            }
            if airport {
                for v in tri.iter_mut() {
                    v.surface[3] = AIRPORT_SLAB;
                }
            }
            let mut pieces = vec![tri
                .iter()
                .map(|v| Vec2::new(v.position[0], v.position[1]))
                .collect::<Vec<_>>()];
            let mut replaced = false;
            let start = additions.vertices.len();
            let mut emit = |poly: &[Vec2], edge: Option<&StripEdge>| {
                for i in 1..poly.len().saturating_sub(1) {
                    let face = [poly[0], poly[i], poly[i + 1]];
                    if (face[1] - face[0]).perp_dot(face[2] - face[0]).abs() < 1e-12 {
                        continue;
                    }
                    for p in face {
                        let mut v = tri[0];
                        v.position = p.extend(z).to_array();
                        if let Some(s) = edge {
                            v.surface[1] = (s.origin - p).dot(s.outward);
                            v.surface[2] = s.width_scale;
                            v.surface[3] = s.contact.marker(airport);
                        }
                        additions.vertices.push(v);
                    }
                }
            };
            for s in &strips {
                let mut remaining = Vec::new();
                for piece in pieces {
                    let mut inner = piece.clone();
                    let mut offcuts = Vec::new();
                    let middle = s.polygon.iter().sum::<Vec2>() * 0.25;
                    for k in 0..4 {
                        let a = s.polygon[k];
                        let mut n = (s.polygon[(k + 1) % 4] - a).perp();
                        if n.dot(middle - a) < 0. {
                            n = -n;
                        }
                        let (keep, rest) = clip(&inner, a, n);
                        inner = keep;
                        if rest.len() >= 3 {
                            offcuts.push(rest);
                        }
                        if inner.len() < 3 {
                            break;
                        }
                    }
                    if inner.len() < 3 {
                        remaining.push(piece);
                    } else {
                        replaced = true;
                        emit(&inner, Some(s));
                        remaining.extend(offcuts);
                    }
                }
                pieces = remaining;
            }
            if replaced {
                for piece in pieces {
                    emit(&piece, None);
                }
                // Reuse the original face slot and append its remaining fragments.
                let len = additions.vertices.len();
                if len >= start + 3 {
                    tri.copy_from_slice(&additions.vertices[len - 3..]);
                    additions.vertices.truncate(len - 3);
                }
            }
        }
    }
    mesh.vertices.append(&mut additions.vertices);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_89_east_apron_has_a_grass_shoulder() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/mtl/map.road");
        let hermes = crate::hermes::Hermes::load_with_game_data(&path, None).unwrap();
        let city = hermes.city();
        let sample_y = -0.89;
        let mut mesh = city
            .mesh(
                opencity::runtime::Region::radius([-4.76, 0.], 1.8).unwrap(),
                0.04,
            )
            .unwrap();
        add_edges(city, &mut mesh);
        assert!(
            mesh.vertices.chunks_exact(3).any(|t| {
                let lip: Vec<_> = t
                    .iter()
                    .filter(|v| (v.position[0] + 3.97).abs() < 0.00001)
                    .collect();
                t[0].surface[3] == AIRPORT_TURF_EDGE
                    && lip.len() == 2
                    && (lip[0].position[1] - sample_y) * (lip[1].position[1] - sample_y) <= 0.
            }),
            "the east apron at the report must carry an exposed shoulder"
        );
    }
    #[test]
    fn concave_courtyard_keeps_its_area_and_matches_neighboring_materials() {
        let hermes = crate::hermes::Hermes::test_map_from_source(
            "lawn=land(-1,-1,3,-1,3,3,-1,3); elevation(lawn,0.02);
             yard=concrete(0,0,2,0,2,1,1,1,1,2,0,2); elevation(yard,0.021);
             sand=beach(2,0,3,0,3,1,2,1); elevation(sand,0.026);",
        );
        let city = hermes.city();
        let mut mesh = city
            .mesh(opencity::runtime::Region::WholeMap, 0.04)
            .unwrap();
        add_edges(city, &mut mesh);
        let mut area = 0.;
        let mut edges = [false; 6];
        let footprint =
            [[0., 0.], [2., 0.], [2., 1.], [1., 1.], [1., 2.], [0., 2.]].map(Vec2::from_array);
        for t in mesh
            .vertices
            .chunks_exact(3)
            .filter(|t| t[0].surface[0] == SurfaceKind::Sidewalk as u32 as f32)
        {
            let p: [Vec2; 3] =
                std::array::from_fn(|i| Vec2::new(t[i].position[0], t[i].position[1]));
            area += (p[1] - p[0]).perp_dot(p[2] - p[0]).abs() * 0.5;
            for (i, &a) in footprint.iter().enumerate() {
                let dir = footprint[(i + 1) % 6] - a;
                if p.iter()
                    .filter(|&&q| (q - a).perp_dot(dir).abs() < 1e-6)
                    .count()
                    >= 2
                {
                    edges[i] |= t[0].surface[3] == if i == 1 { -1.75 } else { -1.5 };
                }
            }
        }
        assert!((area - 3.0).abs() < 1e-4);
        assert_eq!(edges, [true; 6]);
    }

    #[test]
    fn taxiway_opening_keeps_its_pavement() {
        let hermes = crate::hermes::Hermes::test_map_from_source(
            "turf=land(-1,-1,3,-1,3,2,-1,2);
             airport_apron=concrete(0,0,1,0,1,1,0,1); elevation(airport_apron,0.001);
             airport_link=concrete(1,0.25,2,0.25,2,0.75,1,0.75); elevation(airport_link,0.001);",
        );
        let city = hermes.city();
        let mut mesh = city
            .mesh(opencity::runtime::Region::WholeMap, 0.04)
            .unwrap();
        add_edges(city, &mut mesh);
        let mut exposed = 0.;
        let mut joined = 0.;
        for tri in mesh
            .vertices
            .chunks_exact(3)
            .filter(|t| t[0].surface[0] == SurfaceKind::Sidewalk as u32 as f32)
        {
            assert!(tri.iter().all(|v| matches!(
                v.surface[3],
                AIRPORT_SLAB | AIRPORT_TURF_EDGE | AIRPORT_MINERAL_EDGE
            )));
            let on_edge: Vec<_> = tri.iter().filter(|v| v.position[0] == 1.).collect();
            if on_edge.len() != 2 {
                continue;
            }
            let a = on_edge[0].position[1];
            let b = on_edge[1].position[1];
            if tri[0].surface[3] == AIRPORT_TURF_EDGE {
                assert!((a + b) * 0.5 <= 0.25 || (a + b) * 0.5 >= 0.75);
                exposed += (a - b).abs();
            } else {
                joined += (a - b).abs();
            }
        }
        assert!((exposed - 0.5).abs() < 0.001);
        assert!((joined - 1.).abs() < 0.001);
    }
}
