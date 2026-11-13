use super::nearby::Planner;
use super::*;

fn wall(x: f32, height: f32, kind: SurfaceKind, metadata: f32) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.quad(
        [
            [x, 0., 0.],
            [x + 0.08, 0., 0.],
            [x + 0.08, 0., height],
            [x, 0., height],
        ],
        [0.4; 3],
        kind,
    );
    for v in &mut mesh.vertices {
        v.surface[3] = metadata;
    }
    mesh
}
#[test]
fn sparse_placement_is_stable_spaced_and_uses_only_concrete() {
    let mut world = Mesh::default();
    for i in 0..500 {
        world
            .vertices
            .extend(wall(i as f32 * 0.1, 0.011, SurfaceKind::Prop, -2.).vertices);
    }
    let paint = Planner::new(&world).mesh([1.5, 0.1]);
    assert!(!paint.vertices.is_empty());
    assert!(paint.vertices.len() / 6 <= MAX_VISIBLE);
    let signature = |m: &Mesh| {
        m.vertices
            .iter()
            .flat_map(|v| {
                v.position
                    .into_iter()
                    .chain(v.color)
                    .chain(v.surface)
                    .map(f32::to_bits)
            })
            .collect::<Vec<_>>()
    };
    let mut triangles: Vec<_> = world
        .vertices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    triangles.reverse();
    let reversed = Mesh {
        vertices: triangles.into_iter().flatten().collect(),
    };
    assert_eq!(
        signature(&paint),
        signature(&Planner::new(&reversed).mesh([1.5, 0.1]))
    );
    let positions: Vec<_> = paint
        .vertices
        .chunks_exact(6)
        .map(|t| {
            t.iter()
                .fold(Vec3::ZERO, |p, v| p + Vec3::from_array(v.position))
                / 6.
        })
        .collect();
    for (i, p) in positions.iter().enumerate() {
        for q in &positions[..i] {
            assert!(p.truncate().distance(q.truncate()) >= SPACING - 1e-6);
        }
    }
    for mesh in [
        wall(0., 0.003, SurfaceKind::Sidewalk, -2.),
        wall(0., 0.0065, SurfaceKind::Prop, -2.),
        wall(0., 0.03, SurfaceKind::Window, 0.),
        wall(0., 0.03, SurfaceKind::Building, 3.),
        wall(0., 0.03, SurfaceKind::Prop, 0.),
    ] {
        assert!(Planner::new(&mesh).mesh([0., 0.]).vertices.is_empty());
    }
}
#[test]
fn nearby_budget_is_bounded_but_travel_has_no_citywide_limit() {
    let mut world = Mesh::default();
    for i in 0..5000 {
        world
            .vertices
            .extend(wall(i as f32 * 0.1, 0.011, SurfaceKind::Prop, -2.).vertices);
    }
    let mut planner = Planner::new(&world);
    let mut visited = std::collections::BTreeSet::new();
    let signature = |mesh: &Mesh| {
        mesh.vertices
            .iter()
            .map(|v| v.position.map(f32::to_bits))
            .collect::<Vec<_>>()
    };
    let first = signature(&planner.mesh([1.5, 0.1]));
    for i in 0..100 {
        let center = [1.5 + i as f32 * 5., 0.1];
        let mesh = planner.mesh(center);
        assert!(mesh.vertices.len() / 6 <= MAX_VISIBLE);
        assert!(planner.cached_tiles() <= 169);
        for mark in mesh.vertices.chunks_exact(6) {
            visited.insert(mark[0].position.map(f32::to_bits));
        }
    }
    assert!(
        visited.len() > 96,
        "travel still exhausts a citywide allowance: {}",
        visited.len()
    );
    assert_eq!(
        first,
        signature(&planner.mesh([1.5, 0.1])),
        "returning to a wall changed its paint"
    );
}
#[test]
fn sloped_retaining_quad_pairs_by_shared_edge_and_scales_larger_art() {
    let mut world = wall(0., 0.08, SurfaceKind::Prop, -2.);
    for v in &mut world.vertices {
        v.position[2] += (0.08 - v.position[0]) * 0.12;
    }
    let receivers = panels(&world);
    assert_eq!(
        receivers.len(),
        1,
        "sloped triangle bounds lost their shared wall receiver"
    );
    let receiver = receivers.values().next().unwrap();
    let paint = paint_panel(receiver, 42).unwrap();
    let lo = paint
        .vertices
        .iter()
        .map(|v| v.position[2])
        .fold(f32::INFINITY, f32::min);
    let hi = paint
        .vertices
        .iter()
        .map(|v| v.position[2])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        hi - lo > 0.012,
        "large retaining wall retained a tiny barrier-sized mark"
    );
}
#[test]
fn opposite_wall_faces_do_not_consume_each_others_placement() {
    let mut world = Mesh::default();
    for i in -30..30 {
        let front = wall(i as f32 * 0.1, 0.08, SurfaceKind::Prop, -2.);
        let mut back = front.clone();
        for triangle in back.vertices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
            for v in triangle {
                v.position[1] += 0.004;
                v.normal = [0., 1., 0.];
            }
        }
        world.vertices.extend(front.vertices);
        world.vertices.extend(back.vertices);
    }
    let paint = Planner::new(&world).mesh([0., 0.1]);
    assert!(paint.vertices.iter().any(|v| v.normal[1] < 0.));
    assert!(paint.vertices.iter().any(|v| v.normal[1] > 0.));
    assert!(paint.vertices.len() / 6 <= MAX_VISIBLE);
}
#[test]
fn rotated_marks_fit_the_receiver_and_records_keep_fractional_variation() {
    let mesh = wall(0., 0.011, SurfaceKind::Prop, -2.);
    let triangles: Vec<_> = mesh
        .vertices
        .chunks_exact(3)
        .map(|t| [t[0], t[1], t[2]])
        .collect();
    let mut opacity = std::collections::BTreeSet::new();
    let mut wash = std::collections::BTreeSet::new();
    for seed in 0..128 {
        let paint = paint_panel(&triangles, seed).unwrap();
        for v in paint.vertices {
            assert!((0.00079..=0.07921).contains(&v.position[0]));
            assert!((0.00079..=0.01021).contains(&v.position[2]));
            assert!((v.position[1] + OFFSET).abs() < 1e-7);
            assert_eq!(v.surface[0], MATERIAL);
            let bits = v.surface[3] as u32;
            assert_eq!(bits as f32, v.surface[3]);
            assert_eq!(bits & 1, seed & 1);
            assert_eq!((bits >> 17) & 127, seed);
            opacity.insert((bits >> 1) & 255);
            wash.insert((bits >> 9) & 255);
        }
    }
    assert!(opacity.len() > 60 && wash.len() > 60);
}
#[test]
fn masks_have_padding_highlights_and_alpha_weighted_mips() {
    for bytes in ART {
        let pixels = decode(bytes);
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 200 && p[1] > 100));
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 100 && p[0] < 90));
        for y in 0..256 {
            for x in 0..512 {
                if x < 8 || x >= 504 || y < 8 || y >= 248 {
                    assert_eq!(pixels[(y * 512 + x) * 4 + 3], 0);
                }
            }
        }
    }
    let source = [180, 220, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(next_mip(&source, 2, 2), [180, 220, 0, 64]);
}
#[cfg(not(target_arch = "wasm32"))]
mod native;
