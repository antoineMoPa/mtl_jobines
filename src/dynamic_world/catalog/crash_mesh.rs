//! Breakaway geometry stays in chassis coordinates so dent fields line up at seams.
use super::{Part, PartRole};
use crate::dynamic_world::vehicle::damage::{Piece, Zone};
use glam::Vec3;

pub(super) fn assign(
    model: &str,
    name: &str,
    mut part: Part,
    parts: &mut Vec<Part>,
) -> Option<Part> {
    if !matches!(model, "vehicle-v2-09" | "vehicle-v2-01" | "vehicle-12") {
        return Some(part);
    }
    let zone = match (model, name) {
        ("vehicle-v2-09", "grille_fine_mesh") | (_, "front_bumper") => Zone::Nose,
        (_, "rear_bumper") => Zone::Tail,
        ("vehicle-v2-01", "bed_side_right" | "bed_inner_1" | "bed_rail_1" | "bed_tie_down_1") => {
            Zone::RightRear
        }
        ("vehicle-12", "front_arch") => {
            let mut flare = Vec::new();
            let mut fixed = Vec::new();
            for tri in part.vertices.chunks_exact(3) {
                if tri[0].position[1] < 0. {
                    flare.extend_from_slice(tri);
                } else {
                    fixed.extend_from_slice(tri);
                }
            }
            parts.push(Part {
                role: PartRole::Crash(Piece::Panel(Zone::RightFront)),
                pivot: Vec3::ZERO,
                vertices: flare,
                shadow_vertices: None,
            });
            part.vertices = fixed;
            return Some(part);
        }
        (_, "windshield" | "rear_window" | "rear_glass") => {
            let end = if name == "windshield" {
                Zone::Nose
            } else {
                Zone::Tail
            };
            let mut shards: [Vec<_>; 4] = Default::default();
            for tri in part.vertices.chunks_exact(3) {
                let center = tri.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / 3.;
                let lateral = center.y + 0.23 * center.z;
                let shard = [-0.33, 0., 0.38].partition_point(|cut| lateral > *cut);
                shards[shard].extend_from_slice(tri);
            }
            for (i, vertices) in shards
                .into_iter()
                .enumerate()
                .filter(|(_, v)| !v.is_empty())
            {
                parts.push(Part {
                    role: PartRole::Crash(Piece::Glass(end, i as u8)),
                    pivot: Vec3::ZERO,
                    vertices,
                    shadow_vertices: Some(Vec::new()),
                });
            }
            return None;
        }
        _ => return Some(part),
    };
    part.role = PartRole::Crash(Piece::Panel(zone));
    parts.push(part);
    None
}
