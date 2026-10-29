//! Small game-owned shop props. Keep this category separate so a future winter
//! layout can omit sidewalk boards without changing permanent architecture.
use super::commercial::seed;
use opencity::runtime::{BuildingFrontage, City, Mesh, SurfaceKind};

pub(super) fn sandwich_board(city: &City, f: BuildingFrontage, name: &str) -> Option<Mesh> {
    let id = seed(&format!("{name}:sandwich-board"));
    if id % 8 != 0 {
        return None;
    }
    let x = f.width * 0.22;
    let y = 0.012;
    let mut heights = Vec::new();
    for (dx, dy) in [
        (-0.0045, -0.0035),
        (0.0045, -0.0035),
        (-0.0045, 0.0035),
        (0.0045, 0.0035),
        (0., 0.),
    ] {
        let p = f.point(x + dx, y + dy, 0.);
        let s = city.surface_at([p[0], p[1]], f.base + 0.015)?;
        if s.kind != SurfaceKind::Sidewalk
            || (s.height - f.base).abs() > 0.012
            || city.building_collision([p[0], p[1]], 0.001, s.height, s.height + 0.012)
        {
            return None;
        }
        heights.push(s.height);
    }
    let low = heights.iter().copied().fold(f32::INFINITY, f32::min);
    let high = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if high - low > 0.0005 {
        return None;
    }
    let p = f.point(x, y, 0.);
    let frame = BuildingFrontage {
        origin: [p[0], p[1]],
        base: high + 0.0001,
        ..f
    };
    Some(board_mesh(frame, id))
}

fn board_mesh(f: BuildingFrontage, id: u64) -> Mesh {
    let mut mesh = Mesh::default();
    let paint = match (id >> 16) % 3 {
        0 => [0.045, 0.13, 0.12],
        1 => [0.24, 0.075, 0.13],
        _ => [0.10, 0.12, 0.22],
    };
    for side in [-1., 1.] {
        let mut panel = Mesh::default();
        panel.cuboid(
            [-0.004, -0.00035, 0.0003],
            [0.004, 0.00035, 0.011],
            [0.20, 0.14, 0.09],
            SurfaceKind::Prop,
        );
        panel.cuboid(
            [-0.0035, -0.0004, 0.001],
            [0.0035, 0.0004, 0.0104],
            paint,
            SurfaceKind::Prop,
        );
        // A few bold painted strokes, without writing, logos or seasonal art.
        for face in [-1., 1.] {
            for (z, width, color) in [
                (0.008, 0.005, [0.78, 0.60, 0.28]),
                (0.0058, 0.004, [0.63, 0.72, 0.60]),
                (0.0036, 0.0055, [0.78, 0.60, 0.28]),
            ] {
                let y = face * 0.00041;
                let mut points = [
                    [-width / 2., y, z],
                    [width / 2., y, z],
                    [width / 2., y, z + 0.0009],
                    [-width / 2., y, z + 0.0009],
                ];
                if face > 0. {
                    points.reverse();
                }
                panel.quad(points, color, SurfaceKind::Prop);
            }
        }
        let (sin, cos) = (side * 0.29_f32).sin_cos();
        for mut v in panel.vertices {
            let [x, y, z] = v.position;
            v.position = f.point(x, side * 0.003 + y * cos - z * sin, y * sin + z * cos);
            let [nx, ny, nz] = v.normal;
            let ny = ny * cos - nz * sin;
            v.normal = [
                f.along[0] * nx + f.outward[0] * ny,
                f.along[1] * nx + f.outward[1] * ny,
                v.normal[1] * sin + nz * cos,
            ];
            mesh.vertices.push(v);
        }
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_boards_require_pavement_and_keep_seeded_art_without_seasonal_objects() {
        let name = (0..128)
            .map(|i| format!("shop_{i}"))
            .find(|name| seed(&format!("{name}:sandwich-board")) % 8 == 0)
            .unwrap();
        let f = BuildingFrontage {
            origin: [-0.04, 0.12],
            along: [1., 0.],
            outward: [0., -1.],
            width: 0.08,
            depth: 0.1,
            base: 0.,
        };
        let pavement = "road r: carlane; end r(-1,0,1,0);
            p=concrete(-1,0.05,1,0.05,1,0.12,-1,0.12);elevation(p,0.02);";
        let city = |s: &str| City::from_program(&opencity::parse(s).unwrap(), None).unwrap();
        let first = sandwich_board(&city(pavement), f, &name).unwrap();
        let second = sandwich_board(&city(pavement), f, &name).unwrap();
        assert_eq!(
            first
                .vertices
                .iter()
                .map(|v| (v.position, v.normal, v.color, v.surface))
                .collect::<Vec<_>>(),
            second
                .vertices
                .iter()
                .map(|v| (v.position, v.normal, v.color, v.surface))
                .collect::<Vec<_>>()
        );
        for material in ["land", "lake"] {
            assert!(
                sandwich_board(&city(&pavement.replace("concrete", material)), f, &name).is_none()
            );
        }
        assert!(first
            .vertices
            .iter()
            .all(|v| v.surface[0] == SurfaceKind::Prop as u32 as f32));
        let count = (0..1000)
            .filter(|i| seed(&format!("shop_{i}:sandwich-board")) % 8 == 0)
            .count();
        assert!(
            (100..150).contains(&count),
            "{count}/1000, before pavement filtering"
        );
    }
}
