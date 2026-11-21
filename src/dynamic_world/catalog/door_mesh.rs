//! Separate door skins, glass and hardware from the fixed shell before GPU batching.
use super::{Config, Part, PartRole, Vertex};
use crate::dynamic_world::doors::{opening, Door, Opening};
use glam::Vec3;

fn authored_panel(name: &str) -> Option<(Door, f32)> {
    use Door::*;
    let panel = match name {
        "door_front_left"
        | "glass_front_left"
        | "mirror_left"
        | "handle_front_left"
        | "glass_door_front_left_forward"
        | "glass_door_front_left_rearward"
        | "door_left"
        | "glass_left"
        | "interior_door_card_left"
        | "cab_window_left"
        | "cab_mirror_left"
        | "Boarding door skin"
        | "Boarding door handle"
        | "door_left_0"
        | "door_split_left_0" => FrontLeft,
        "door_front_right"
        | "glass_front_right"
        | "mirror_right"
        | "handle_front_right"
        | "glass_door_front_right_forward"
        | "glass_door_front_right_rearward"
        | "door_right"
        | "glass_right"
        | "interior_door_card_right"
        | "cab_window_right"
        | "cab_mirror_right"
        | "door_right_0"
        | "door_split_right_0" => FrontRight,
        "door_rear_left"
        | "glass_rear_left"
        | "handle_rear_left"
        | "door_left_1"
        | "door_split_left_1"
        | "glass_door_rear_left_forward"
        | "glass_door_rear_left_rearward" => RearLeft,
        "door_rear_right"
        | "glass_rear_right"
        | "handle_rear_right"
        | "door_right_1"
        | "door_split_right_1"
        | "glass_door_rear_right_forward"
        | "glass_door_rear_right_rearward" => RearRight,
        "metro_door_left_front_forward" => return Some((FrontLeft, 0.5)),
        "metro_door_left_front_backward" => return Some((FrontLeft, -0.5)),
        "metro_door_left_rear_forward" => return Some((RearLeft, 0.5)),
        "metro_door_left_rear_backward" => return Some((RearLeft, -0.5)),
        "metro_door_right_front_forward" => return Some((FrontRight, 0.5)),
        "metro_door_right_front_backward" => return Some((FrontRight, -0.5)),
        "metro_door_right_rear_forward" => return Some((RearRight, 0.5)),
        "metro_door_right_rear_backward" => return Some((RearRight, -0.5)),
        _ => return None,
    };
    Some((panel, -1.))
}

fn shared_panel(name: &str) -> bool {
    matches!(
        name,
        "body_shell"
            | "cab"
            | "Fuselage"
            | "Nose hemisphere"
            | "Livery -1"
            | "side_windows"
            | "window_frames"
            | "window_frame"
            | "window_frame_10"
            | "window_frame_11"
            | "door_reveal_lines"
            | "door_seam_front"
            | "door_seam_center"
            | "door_seam_rear"
            | "police_blue_side_stripe"
            | "lower_trim"
            | "front_blue_panels"
            | "lower_trim_left"
            | "lower_trim_right"
            | "formed_side_left"
            | "formed_side_right"
            | "waist_band_left"
            | "waist_band_right"
    ) || name.starts_with("interior_door_card_")
        || name.starts_with("interior_armrest_")
}

fn interpolate(a: Vertex, b: Vertex, t: f32) -> Vertex {
    let blend = |a: [f32; 3], b: [f32; 3]| Vec3::from(a).lerp(b.into(), t).to_array();
    let mut v = a;
    v.position = blend(a.position, b.position);
    v.normal = blend(a.normal, b.normal);
    v.color = blend(a.color, b.color);
    // The material ID and packed finish are constant per primitive.
    if a.surface[0] < 0. {
        for i in 2..4 {
            v.surface[i] += (b.surface[i] - a.surface[i]) * t;
        }
    }
    v.surface[1] += (b.surface[1] - a.surface[1]) * t;
    v
}
fn triangles(poly: &[Vertex], out: &mut Vec<Vertex>) {
    for edge in poly[1..].windows(2) {
        out.extend([poly[0], edge[0], edge[1]]);
    }
}

// Clip only boundary triangles. Interior and exterior triangles keep their source vertices.
fn partition(
    source: Vec<Vertex>,
    planes: &[(usize, f32, f32)],
    inside: &mut Vec<Vertex>,
) -> Vec<Vertex> {
    let mut outside = Vec::new();
    for tri in source.chunks_exact(3) {
        if planes
            .iter()
            .any(|&(axis, sign, limit)| tri.iter().all(|v| sign * v.position[axis] < limit))
        {
            outside.extend_from_slice(tri);
            continue;
        }
        if planes
            .iter()
            .all(|&(axis, sign, limit)| tri.iter().all(|v| sign * v.position[axis] >= limit))
        {
            inside.extend_from_slice(tri);
            continue;
        }
        let mut polygon = tri.to_vec();
        for &(axis, sign, limit) in planes {
            let mut keep = Vec::new();
            let mut cut = Vec::new();
            let mut a = *polygon.last().unwrap();
            for &b in &polygon {
                let da = sign * a.position[axis] - limit;
                let db = sign * b.position[axis] - limit;
                if da * db < 0. {
                    let crossing = interpolate(a, b, da / (da - db));
                    keep.push(crossing);
                    cut.push(crossing);
                }
                if db >= 0. {
                    keep.push(b);
                }
                if db <= 0. {
                    cut.push(b);
                }
                a = b;
            }
            if cut.len() >= 3 && polygon.iter().any(|v| sign * v.position[axis] < limit) {
                triangles(&cut, &mut outside);
            }
            polygon = keep;
            if polygon.len() < 3 {
                break;
            }
        }
        if polygon.len() >= 3 {
            triangles(&polygon, inside);
        }
    }
    outside
}

fn planes(o: Opening, door: Door, ride: f32) -> [(usize, f32, f32); 5] {
    [
        (0, 1., o.rear),
        (0, -1., -o.front),
        (1, door.side(), o.inner),
        (2, 1., o.sill - ride),
        (2, -1., ride - o.top),
    ]
}

// Older cabs use dark decals over a solid shell. Give the underlying windscreen
// the same coverage as the inset glazing so the driver is visible through both.
fn cab_glazing(config: &Config, name: &str, vertices: &mut Vec<Vertex>) {
    let id = config.research_id.as_str();
    let decal = matches!(
        (id, name),
        (
            "vehicle-j07",
            "cab_windscreen" | "cab_window_left" | "cab_window_right"
        ) | ("vehicle-r02", "front_windshield")
    );
    if decal {
        for v in vertices {
            v.surface[1] = 0.44;
        }
        return;
    }
    // Min/max X, absolute Y and height from the road, in local metres.
    let windows: &[[f32; 6]] = match (id, name) {
        ("vehicle-j07", "cab") => &[
            [4.03, 4.16, 0., 1.02, 1.62, 2.28],
            [1.62, 3.81, 1.1, 1.3, 1.63, 2.28],
        ],
        ("vehicle-r02", "front_bulkhead") => &[[8.1, 8.3, 0., 1.18, 1.98, 2.93]],
        _ => return,
    };
    let ride = config.physics.height_offset;
    for &[rear, front, inner, outer, sill, top] in windows {
        let mut glass = Vec::new();
        for side in [-1., 1.] {
            *vertices = partition(
                std::mem::take(vertices),
                &[
                    (0, 1., rear),
                    (0, -1., -front),
                    (1, side, inner),
                    (1, -side, -outer),
                    (2, 1., sill - ride),
                    (2, -1., ride - top),
                ],
                &mut glass,
            );
        }
        for v in &mut glass {
            v.surface[1] = 0.2;
            v.color = [0.05, 0.11, 0.14];
        }
        vertices.extend(glass);
    }
}

pub(super) fn separate(config: &Config, name: &str, mut part: Part, parts: &mut Vec<Part>) {
    if part.role == PartRole::Body {
        cab_glazing(config, name, &mut part.vertices);
        if let Some((door, leaf)) = authored_panel(name) {
            if opening(&config.research_id, door).is_some() {
                part.role = PartRole::Door(door, leaf);
                parts.push(part);
                return;
            }
        }
        if name == "mirror_housings" {
            // The two housings share a mesh and extend forward of the door seam.
            let mut left = Vec::new();
            let right = partition(part.vertices, &[(1, 1., 0.)], &mut left);
            for (door, vertices) in [(Door::FrontLeft, left), (Door::FrontRight, right)] {
                parts.push(Part {
                    role: PartRole::Door(door, -1.),
                    pivot: Vec3::ZERO,
                    vertices,
                    shadow_vertices: None,
                });
            }
            return;
        }
        if shared_panel(name) {
            for door in Door::ALL {
                let Some(o) = opening(&config.research_id, door) else {
                    continue;
                };
                let mut panel = Vec::new();
                part.vertices = partition(
                    part.vertices,
                    &planes(o, door, config.physics.height_offset),
                    &mut panel,
                );
                if !panel.is_empty() {
                    parts.push(Part {
                        role: PartRole::Door(door, -1.),
                        pivot: Vec3::ZERO,
                        vertices: panel,
                        shadow_vertices: part.shadow_vertices.as_ref().map(|_| vec![]),
                    });
                }
            }
        }
    }
    parts.push(part);
}
