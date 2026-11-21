//! Split rectangular airport slabs into an interior and narrow edge strips.
//! Shared taxiway and apron entrances keep their uninterrupted paving.
use super::*;

pub(super) fn add_edges(city: &City, mesh: &mut Mesh) {
    for f in &city.landscape {
        if f.kind != opencity::LandscapeKind::Concrete
            || !f
                .name
                .as_deref()
                .is_some_and(|n| n.starts_with("airport_") && !n.ends_with("_runway"))
        {
            continue;
        }
        let Ok(points): Result<[[f32; 2]; 4], _> = f.points.as_slice().try_into() else {
            continue;
        };
        let corners = points.map(Vec2::from_array);
        let z = f.elevation - 0.02;
        let mut faces = Vec::new();
        for (i, tri) in mesh.vertices.chunks_exact(3).enumerate() {
            if tri[0].surface[0] == SurfaceKind::Sidewalk as u32 as f32
                && tri.iter().all(|v| {
                    (v.position[2] - z).abs() < 0.00004
                        && points.contains(&[v.position[0], v.position[1]])
                })
            {
                faces.push(i);
            }
        }
        if faces.len() != 2 {
            continue;
        }
        let center = corners.iter().sum::<Vec2>() * 0.25;
        let color = mesh.vertices[faces[0] * 3].color;
        let data: [f32; 3] = mesh.vertices[faces[0] * 3].surface[1..].try_into().unwrap();
        let inset = corners.map(|p| {
            let i = corners.iter().position(|&q| q == p).unwrap();
            let a = corners[(i + 1) % 4] - p;
            let b = corners[(i + 3) % 4] - p;
            p + a * (0.012 / a.length()).min(0.25) + b * (0.012 / b.length()).min(0.25)
        });
        // Keep the original two faces for the inset, without a coincident overlay.
        for face in faces {
            for v in &mut mesh.vertices[face * 3..face * 3 + 3] {
                let i = points
                    .iter()
                    .position(|p| *p == [v.position[0], v.position[1]])
                    .unwrap();
                v.position = inset[i].extend(z).to_array();
            }
        }
        for i in 0..4 {
            let j = (i + 1) % 4;
            let edge = corners[j] - corners[i];
            let length = edge.length();
            let mut outward = edge.perp() / length;
            if outward.dot(center - corners[i]) > 0. {
                outward = -outward;
            }
            let steps = (length / 0.005).ceil().max(1.) as usize;
            let exposed = |step: usize| {
                let p = corners[i] + edge * ((step as f32 + 0.5) / steps as f32) + outward * 0.0003;
                city.surface_at(p.to_array(), f32::INFINITY)
                    .is_some_and(|s| s.kind == SurfaceKind::Grass)
            };
            let mut start = 0;
            let mut turf = exposed(0);
            for step in 1..=steps {
                let next = step < steps && exposed(step);
                if step < steps && next == turf {
                    continue;
                }
                let t0 = start as f32 / steps as f32;
                let t1 = step as f32 / steps as f32;
                let outer = |t| corners[i].lerp(corners[j], t);
                // Keep the cross-section perpendicular at interior split points.
                let inner = |t: f32| {
                    if t == 0. {
                        inset[i]
                    } else if t == 1. {
                        inset[j]
                    } else {
                        let span = inset[j] - inset[i];
                        inset[i]
                            + span
                                * ((outer(t) - inset[i]).dot(span) / span.length_squared())
                                    .clamp(0., 1.)
                    }
                };
                let a = outer(t0);
                let b = outer(t1);
                let c = inner(t1);
                let d = inner(t0);
                for tri in [[a, b, c], [a, c, d]] {
                    let base = mesh.vertices.len();
                    mesh.triangle(
                        tri.map(|p| p.extend(z).to_array()),
                        color,
                        SurfaceKind::Sidewalk,
                        data,
                    );
                    if turf {
                        for v in &mut mesh.vertices[base..] {
                            let point = Vec2::new(v.position[0], v.position[1]);
                            v.surface[1] = (corners[i] - point).dot(outward);
                            v.surface[3] = -1.5;
                        }
                    }
                }
                start = step;
                turf = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            let on_edge: Vec<_> = tri.iter().filter(|v| v.position[0] == 1.).collect();
            if on_edge.len() != 2 {
                continue;
            }
            let a = on_edge[0].position[1];
            let b = on_edge[1].position[1];
            if tri[0].surface[3] == -1.5 {
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
