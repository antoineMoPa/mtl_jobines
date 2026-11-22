//! Subtract the runway footprint from the rendered height map and reclaimed land.
use super::{Mesh, Runway, SurfaceKind, Vec2, Vertex};

fn interpolate(a: Vertex, b: Vertex, t: f32) -> Vertex {
    fn mix<const N: usize>(a: [f32; N], b: [f32; N], t: f32) -> [f32; N] {
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
    }
    Vertex {
        position: mix(a.position, b.position, t),
        normal: mix(a.normal, b.normal, t),
        color: mix(a.color, b.color, t),
        surface: mix(a.surface, b.surface, t),
    }
}

fn xy(v: &Vertex) -> Vec2 {
    Vec2::new(v.position[0], v.position[1])
}

pub(super) fn cut_runway(mesh: &mut Mesh, runway: &Runway) {
    let end = runway.point(runway.size);
    let along = runway.across.perp();
    let edges = [
        (runway.origin, runway.across),
        (end, -runway.across),
        (runway.origin, along),
        (end, -along),
    ];
    let mut write = 0;
    let mut fragments = Vec::new();
    for read in (0..mesh.vertices.len()).step_by(3) {
        let tri = &mesh.vertices[read..read + 3];
        if tri[0].surface[0] != SurfaceKind::Grass as u32 as f32
            || edges
                .iter()
                .any(|&(origin, inward)| tri.iter().all(|v| (xy(v) - origin).dot(inward) <= 0.))
        {
            if read != write {
                mesh.vertices.copy_within(read..read + 3, write);
            }
            write += 3;
            continue;
        }

        // Emit the part outside each successive half-plane. The polygon left
        // after all four edges lies under the asphalt and can be dropped.
        let mut inside = tri.to_vec();
        for (origin, inward) in edges {
            let Some(mut a) = inside.last().copied() else {
                break;
            };
            let mut remaining = Vec::new();
            let mut outside = Vec::new();
            let mut da = (xy(&a) - origin).dot(inward);
            for b in inside {
                let db = (xy(&b) - origin).dot(inward);
                if (da > 0. && db < 0.) || (da < 0. && db > 0.) {
                    let crossing = interpolate(a, b, da / (da - db));
                    remaining.push(crossing);
                    outside.push(crossing);
                }
                if db >= 0. {
                    remaining.push(b);
                }
                if db <= 0. {
                    outside.push(b);
                }
                a = b;
                da = db;
            }
            for i in 1..outside.len().saturating_sub(1) {
                let a = xy(&outside[0]);
                if (xy(&outside[i]) - a).perp_dot(xy(&outside[i + 1]) - a) != 0. {
                    fragments.extend([outside[0], outside[i], outside[i + 1]]);
                }
            }
            inside = remaining;
        }
    }
    mesh.vertices.truncate(write);
    mesh.vertices.append(&mut fragments);
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencity::runtime::Region;

    fn covers(tri: &[Vertex], p: Vec2) -> bool {
        let sides: [f32; 3] = std::array::from_fn(|i| {
            let a = xy(&tri[i]);
            (xy(&tri[(i + 1) % 3]) - a).perp_dot(p - a)
        });
        sides.iter().all(|&s| s >= 0.) || sides.iter().all(|&s| s <= 0.)
    }

    #[test]
    fn montreal_runways_cover_the_ground_from_threshold_to_threshold() {
        use super::super::{append_scenery, Strip};
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/mtl/map.road");
        let hermes = crate::hermes::Hermes::load_with_game_data(&path, None).unwrap();
        let city = hermes.city();
        for strip in [Strip::A, Strip::B] {
            let r = Runway::find(city, strip).unwrap();
            let region = Region::radius(r.center().to_array(), r.size.y * 0.55).unwrap();
            let mut mesh = city.mesh(region, 0.04).unwrap();
            append_scenery(city, &mut mesh);
            for fraction in [-0.001, 0.001, 0.23, 0.57, 0.999, 1.001] {
                for across in [
                    -0.002,
                    0.002,
                    r.size.x * 0.41,
                    r.size.x - 0.002,
                    r.size.x + 0.002,
                ] {
                    let p = r.point(Vec2::new(across, r.size.y * fraction));
                    let on_runway =
                        (0.0..r.size.x).contains(&across) && (0.0..1.).contains(&fraction);
                    for (kind, expected) in [
                        (SurfaceKind::Grass, !on_runway),
                        (SurfaceKind::Asphalt, on_runway),
                    ] {
                        assert_eq!(
                            mesh.vertices.chunks_exact(3).any(|tri| tri[0].surface[0]
                                == kind as u32 as f32
                                && covers(tri, p)),
                            expected,
                            "{kind:?} at {p:?}"
                        );
                    }
                }
            }
        }
    }
}
