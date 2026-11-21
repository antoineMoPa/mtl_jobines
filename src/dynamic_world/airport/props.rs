//! Breakable airfield furniture. Model dimensions are metres, placement is in map units.
use super::{Runway, Strip, METERS_PER_UNIT};
use glam::Vec2;
use opencity::runtime::{City, Mesh, SurfaceKind};

const PROP: SurfaceKind = SurfaceKind::Prop;
const IRON: [f32; 3] = [0.085, 0.105, 0.115];
const YELLOW: [f32; 3] = [0.98, 0.63, 0.022];
const RED: [f32; 3] = [0.75, 0.045, 0.035];
const WHITE: [f32; 3] = [0.9, 0.91, 0.85];

fn on_grass(
    city: &City,
    groups: &mut Vec<Vec<Mesh>>,
    parts: &[Mesh],
    runway: &Runway,
    local: Vec2,
) {
    let point = runway.point(local);
    let Some(surface) = city.surface_at(point.to_array(), f32::INFINITY) else {
        return;
    };
    if surface.kind == SurfaceKind::Grass {
        let base = point.extend(surface.height);
        let mut placed = parts.to_vec();
        for vertex in placed.iter_mut().flat_map(|part| &mut part.vertices) {
            let orient = |v: [f32; 3]| runway.across.rotate(Vec2::new(v[0], v[1])).extend(v[2]);
            vertex.position = (base + orient(vertex.position) / METERS_PER_UNIT).to_array();
            vertex.normal = orient(vertex.normal).to_array();
        }
        groups.push(placed);
    }
}

pub(crate) fn groups(city: &City) -> Vec<Vec<Mesh>> {
    let mut groups = Vec::new();
    for strip in [Strip::A, Strip::B] {
        if let Some(runway) = Runway::find(city, strip) {
            place_runway_props(city, &mut groups, &runway, strip);
        }
    }
    groups
}

fn place_runway_props(city: &City, groups: &mut Vec<Vec<Mesh>>, r: &Runway, strip: Strip) {
    let mut base = Mesh::default();
    base.cuboid([-0.22, -0.22, 0.], [0.22, 0.22, 0.12], IRON, PROP);
    base.cuboid([-0.09, -0.09, 0.12], [0.09, 0.09, 0.41], YELLOW, PROP);
    let mut lens = Mesh::default();
    lens.cylinder(
        [0., 0., 0.41],
        0.18,
        0.19,
        [1., 0.96, 0.82],
        SurfaceKind::Light,
    );
    let lamp = [base, lens];
    let mut y = 0.10;
    while y < r.size.y {
        for x in [-0.025, r.size.x + 0.025] {
            on_grass(city, groups, &lamp, r, Vec2::new(x, y));
        }
        y += 0.55;
    }

    let is_b = strip == Strip::B;
    let sign = direction_sign(is_b);
    let x = if is_b { -0.18 } else { r.size.x + 0.18 };
    for y in [1.02, r.size.y - 1.02] {
        on_grass(city, groups, &sign, r, Vec2::new(x, y));
    }
    if is_b {
        on_grass(
            city,
            groups,
            &station(),
            r,
            Vec2::new(-2.03, r.size.y - 0.12),
        );
    }
}

fn direction_sign(is_b: bool) -> [Mesh; 3] {
    let [left, right] = [-1.1, 1.1].map(|x| {
        let mut post = Mesh::default();
        post.cuboid([x - 0.08, -0.09, 0.], [x + 0.08, 0.09, 0.52], IRON, PROP);
        post
    });
    let mut mesh = Mesh::default();
    mesh.cuboid([-1.6, -0.14, 0.52], [1.6, 0.14, 1.75], IRON, PROP);
    // Keep the taxiway letter readable on both faces. The arrow always points
    // toward its runway in world coordinates.
    for face in [-1., 1.] {
        let point = |x: f32, z, depth| [-face * x, face * depth, z];
        mesh.quad(
            [(-1.53, 0.59), (1.53, 0.59), (1.53, 1.68), (-1.53, 1.68)]
                .map(|(x, z)| point(x, z, 0.145)),
            YELLOW,
            PROP,
        );
        let direction = if is_b { -face } else { face };
        let ink = |x, z| point(x, z, 0.15);
        // Rotate the arrow in its plane to preserve its face normal.
        let arrow = |x, z: f32| ink(direction * x, 1.13 + direction * (z - 1.13));
        mesh.quad(
            [
                arrow(0.05, 1.06),
                arrow(0.82, 1.06),
                arrow(0.82, 1.20),
                arrow(0.05, 1.20),
            ],
            IRON,
            PROP,
        );
        mesh.triangle(
            [arrow(0.8, 0.81), arrow(1.25, 1.13), arrow(0.8, 1.45)],
            IRON,
            PROP,
            [0.; 3],
        );

        let letter = |x, z| ink(x - direction * 0.78, z);
        let mut stroke = |points: [(f32, f32); 4]| {
            mesh.quad(points.map(|(x, z)| letter(x, z)), IRON, PROP);
        };
        if is_b {
            // B, with chamfered bowls.
            stroke([(-0.30, 0.74), (-0.16, 0.74), (-0.16, 1.52), (-0.30, 1.52)]);
            for bottom in [0.74, 1.08] {
                stroke([
                    (-0.16, bottom),
                    (0.15, bottom),
                    (0.27, bottom + 0.12),
                    (-0.16, bottom + 0.12),
                ]);
                stroke([
                    (0.13, bottom + 0.12),
                    (0.27, bottom + 0.12),
                    (0.27, bottom + 0.32),
                    (0.13, bottom + 0.32),
                ]);
                stroke([
                    (-0.16, bottom + 0.32),
                    (0.27, bottom + 0.32),
                    (0.15, bottom + 0.44),
                    (-0.16, bottom + 0.44),
                ]);
            }
        } else {
            stroke([(-0.36, 0.74), (-0.21, 0.74), (0.07, 1.52), (-0.07, 1.52)]);
            stroke([(0.21, 0.74), (0.36, 0.74), (0.07, 1.52), (-0.07, 1.52)]);
            stroke([(-0.18, 0.98), (0.18, 0.98), (0.14, 1.10), (-0.14, 1.10)]);
        }
    }
    [left, right, mesh]
}

fn station() -> [Mesh; 4] {
    let mut slab = Mesh::default();
    slab.cuboid([-3.2, -2.2, 0.], [3.2, 2.2, 0.2], IRON, PROP);
    let mut mesh = Mesh::default();
    let corners = [
        Vec2::new(-3., -2.),
        Vec2::new(3., -2.),
        Vec2::new(3., 2.),
        Vec2::new(-3., 2.),
    ];
    for (a, b) in corners.into_iter().zip(corners.into_iter().cycle().skip(1)) {
        let length = a.distance(b) as usize;
        let step = (b - a) / length as f32;
        for col in 0..length {
            let left = a + step * col as f32;
            let right = left + step;
            for row in 0..3 {
                let z = 0.2 + row as f32;
                let color = if (col + row) % 2 == 0 { RED } else { WHITE };
                mesh.quad(
                    [
                        left.extend(z),
                        right.extend(z),
                        right.extend(z + 1.),
                        left.extend(z + 1.),
                    ]
                    .map(|p| p.to_array()),
                    color,
                    PROP,
                );
            }
        }
    }
    let mut roof = Mesh::default();
    roof.cuboid([-3.23, -2.23, 3.2], [3.23, 2.23, 3.46], IRON, PROP);
    mesh.cuboid(
        [-2.35, 1.98, 0.2],
        [-1.28, 2.04, 2.40],
        [0.21, 0.24, 0.25],
        PROP,
    );
    mesh.cuboid([0.0, 1.98, 1.25], [1.92, 2.06, 2.45], IRON, PROP);
    mesh.cuboid(
        [0.10, 2.06, 1.35],
        [1.82, 2.08, 2.35],
        [0.18, 0.36, 0.42],
        PROP,
    );
    let mut mast = Mesh::default();
    mast.cylinder([0., 0., 3.46], 0.13, 2.65, WHITE, PROP);
    mast.cylinder(
        [0., 0., 6.11],
        0.24,
        0.32,
        [1., 0.045, 0.02],
        SurfaceKind::Light,
    );
    [slab, mesh, roof, mast]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{props::Props, SpawnPoint, Vehicle};
    use crate::hermes::Hermes;
    use glam::Vec3;

    #[test]
    fn airport_furniture_detaches_one_assembly_per_impact() {
        let hermes = Hermes::test_map_from_source(
            "field=land(-5,-22,5,-22,5,-4,-5,-4); elevation(field,0.121);
             airport_east_runway=concrete(3.675,-21.15,4.325,-21.15,4.325,-5.15,3.675,-5.15);
             elevation(airport_east_runway,0.122001); player_start(3,-10,0);",
        );
        let city = hermes.city();
        let groups = hermes.prop_parts();
        for count in [2, 3, 4] {
            let index = groups
                .iter()
                .position(|parts| parts.len() == count)
                .unwrap();
            let (lo, hi) = crate::hermes::external_models::bounds(&groups[index][0].mesh);
            let center = (lo + hi) * (METERS_PER_UNIT / 2.);
            let mut props = Props::from_parts_near(groups.clone(), center);
            assert!(props.blocked(center));
            let mut vehicle = Vehicle::spawn_at(
                city,
                SpawnPoint {
                    point: (center.truncate() / METERS_PER_UNIT).to_array(),
                    heading: Some(0.),
                },
            )
            .unwrap();
            vehicle.velocity = Vec3::X * 16.;
            vehicle.position = center + Vec3::X * 8.;
            props.vehicle_move(&mut vehicle, center - Vec3::X * 8., city);
            assert_eq!(props.damaged_groups().collect::<Vec<_>>(), [index]);
            assert!(!props.group_intact(index));
            assert!(!props.blocked(center));
            let mut debris = Mesh::default();
            props.append_moving(&mut debris, center);
            assert_eq!(
                debris.vertices.len(),
                groups[index]
                    .iter()
                    .map(|part| part.mesh.vertices.len())
                    .sum::<usize>()
            );
            props.update_focus(center + Vec3::Y * 1000.);
            props.update_focus(center);
            assert!(props.group_intact(index) && props.blocked(center));
        }
    }
}
