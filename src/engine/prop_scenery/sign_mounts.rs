//! Validate authored support feet against the compiled surfaces, including junctions.
//! When requested, emit a reviewable source repair plan; never mutate the map here.
use crate::{
    hermes::Hermes,
    props::{Entry, Kind, Placement, Shape, SignDesign},
};
use glam::{Vec2, Vec3};
use opencity::{
    runtime::{City, SurfaceKind},
    RoadElementKind,
};

// Elevated verges are deliberately rendered as asphalt. Their authored verge
// bands are shoulders, not travel lanes; do not flag them merely by material.
fn highway_shoulders(program: &opencity::Program) -> Vec<[Vec3; 4]> {
    let projection = opencity::map::project(program);
    let sections =
        opencity::road_geometry::RoadGeometry::placement_cross_sections_all(program, &projection);
    let mut result = Vec::new();
    for (segment, rows) in projection.segments.iter().zip(sections) {
        let definition = program.statements.iter().find_map(|s| match s {
            opencity::Statement::Road(r) if r.name == segment.road_name => Some(r),
            _ => None,
        });
        let Some(road) = definition else { continue };
        if !matches!(
            road.role,
            opencity::RoadRole::Highway | opencity::RoadRole::Ramp | opencity::RoadRole::Merge
        ) {
            continue;
        }
        for (band, element) in road.elements.iter().enumerate() {
            if element.kind != RoadElementKind::Verge {
                continue;
            }
            for pair in rows.windows(2) {
                result.push(
                    [(0, band), (0, band + 1), (1, band + 1), (1, band)].map(|(r, b)| {
                        let p = pair[r].boundaries[b];
                        Vec3::new(p.point.x, p.point.y, p.height)
                    }),
                );
            }
        }
    }
    result
}
fn on_paved_shoulder(point: Vec3, shoulders: &[[Vec3; 4]]) -> bool {
    shoulders.iter().any(|q| {
        [[q[0], q[1], q[2]], [q[0], q[2], q[3]]].iter().any(|t| {
            let a = t[0].truncate();
            let b = t[1].truncate();
            let c = t[2].truncate();
            let p = point.truncate();
            let area = (b - a).perp_dot(c - a);
            if area.abs() < 1e-12 {
                return false;
            }
            let u = (p - a).perp_dot(c - a) / area;
            let v = (b - a).perp_dot(p - a) / area;
            u >= -1e-5
                && v >= -1e-5
                && u + v <= 1. + 1e-5
                && (t[0].z + u * (t[1].z - t[0].z) + v * (t[2].z - t[0].z) - point.z).abs() < 0.006
        })
    })
}

fn supports_clear(city: &City, p: &Placement, shoulders: &[[Vec3; 4]]) -> bool {
    let design = p
        .sign
        .clone()
        .or_else(|| SignDesign::for_kind(p.kind))
        .unwrap();
    design
        .profiles()
        .iter()
        .filter(|profile| profile.pole)
        .all(|profile| {
            let foot = p
                .transform()
                .transform_point3(Vec3::new(profile.center.x, profile.center.y, 0.) / 100.);
            let radius = profile.half.x.max(profile.half.y) / 100. + 0.0015;
            [
                [0., 0.],
                [-radius, 0.],
                [radius, 0.],
                [0., -radius],
                [0., radius],
            ]
            .iter()
            .all(|offset| {
                city.surface_at([foot.x + offset[0], foot.y + offset[1]], foot.z + 0.008)
                    .is_some_and(|s| {
                        s.kind != SurfaceKind::Water
                            && (s.kind != SurfaceKind::Asphalt
                                || on_paved_shoulder(
                                    Vec3::new(foot.x + offset[0], foot.y + offset[1], foot.z),
                                    shoulders,
                                ))
                            && (s.height - foot.z).abs() < 0.006
                    })
            })
        })
}
struct Candidate {
    point: Vec3,
    center: Vec2,
    direction: Vec2,
    road: String,
}
fn curb_candidates(program: &opencity::Program, city: &City) -> Vec<Candidate> {
    let projection = opencity::map::project(program);
    let sections =
        opencity::road_geometry::RoadGeometry::placement_cross_sections_all(program, &projection);
    let mut result = Vec::new();
    for (segment, rows) in projection.segments.iter().zip(sections) {
        let definition = program.statements.iter().find_map(|s| match s {
            opencity::Statement::Road(r) if r.name == segment.road_name => Some(r),
            _ => None,
        });
        let Some(definition) = definition else {
            continue;
        };
        for sample in 0..=40 {
            let t = 0.025 + 0.95 * sample as f32 / 40.;
            let Some(pair) = rows.windows(2).find(|r| r[0].t <= t && r[1].t >= t) else {
                continue;
            };
            let blend = (t - pair[0].t) / (pair[1].t - pair[0].t).max(1e-9);
            let point = segment.point_at(t);
            let tangent = segment.tangent_at(t);
            let length = tangent.x.hypot(tangent.y);
            if length < 1e-8 {
                continue;
            }
            for (band, element) in definition.elements.iter().enumerate() {
                if !matches!(
                    element.kind,
                    RoadElementKind::Sidewalk | RoadElementKind::Verge
                ) {
                    continue;
                }
                for across in [0.3, 0.5, 0.7] {
                    let edge = |row: usize, index: usize| {
                        let v = pair[row].boundaries[index];
                        Vec3::new(v.point.x, v.point.y, v.height)
                    };
                    let a = edge(0, band).lerp(edge(1, band), blend);
                    let b = edge(0, band + 1).lerp(edge(1, band + 1), blend);
                    let mut p = a.lerp(b, across);
                    let Some(ground) = city.surface_at(p.truncate().to_array(), p.z + 0.008) else {
                        continue;
                    };
                    if matches!(ground.kind, SurfaceKind::Asphalt | SurfaceKind::Water)
                        || (ground.height - p.z).abs() > 0.006
                    {
                        continue;
                    }
                    p.z = ground.height;
                    result.push(Candidate {
                        point: p,
                        center: Vec2::new(point.x, point.y),
                        direction: Vec2::new(tangent.x / length, tangent.y / length),
                        road: segment
                            .name
                            .clone()
                            .unwrap_or_else(|| segment.road_name.clone()),
                    });
                }
            }
        }
    }
    result
}
fn owner(name: &str) -> &str {
    name.strip_prefix("sign_stop_")
        .or_else(|| name.strip_prefix("sign_speed_"))
        .unwrap_or(name)
}
fn replacement(
    entry: &Entry,
    original: &Placement,
    candidates: &[Candidate],
    city: &City,
    occupied: &[Vec3],
    shoulders: &[[Vec3; 4]],
) -> Option<(Entry, String)> {
    if let Some(surface) = city.surface_at(
        original.position.truncate().to_array(),
        original.position.z + 0.008,
    ) {
        if !matches!(surface.kind, SurfaceKind::Asphalt | SurfaceKind::Water) {
            let mut p = original.clone();
            p.position.z = surface.height;
            if supports_clear(city, &p, shoulders) && !p.sign_overlaps_building(city) {
                let mut updated = entry.clone();
                updated.shape = Shape::Single(p);
                return Some((updated, "existing supporting surface".into()));
            }
        }
    }
    let front = original.transform().transform_vector3(-Vec3::Y).truncate();
    candidates
        .iter()
        .filter_map(|c| {
            let distance = c
                .point
                .truncate()
                .distance_squared(original.position.truncate());
            if distance > 0.8 * 0.8 || (c.point.z - original.position.z).abs() > 0.06 {
                return None;
            }
            if occupied
                .iter()
                .any(|p| p.truncate().distance_squared(c.point.truncate()) < 0.008 * 0.008)
            {
                return None;
            }
            let mut p = original.clone();
            p.position = c.point;
            let facing = if p.kind == Kind::ParkingSign {
                c.center - c.point.truncate()
            } else if front.dot(c.direction) < 0. {
                -c.direction
            } else {
                c.direction
            };
            p.heading = facing.x.atan2(-facing.y).to_degrees();
            if !supports_clear(city, &p, shoulders) || p.sign_overlaps_building(city) {
                return None;
            }
            let score = distance
                + if owner(&entry.name) == c.road {
                    0.
                } else {
                    0.0025
                }
                + if original.kind == Kind::ParkingSign {
                    0.
                } else {
                    (1. - front.dot(c.direction).abs()) * 0.0025
                };
            let mut changed = entry.clone();
            changed.shape = Shape::Single(p);
            Some((score, changed, c.road.clone()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, entry, road)| (entry, road))
}

#[test]
fn authored_sign_supports_clear_drivable_surfaces() {
    let source = std::fs::read_to_string("data/maps/mtl/map.road").unwrap();
    let program = opencity::parse(&source).unwrap();
    let entries = crate::props::read(&program).unwrap();
    let hermes =
        Hermes::load_with_game_data(std::path::Path::new("data/maps/mtl/map.road"), None).unwrap();
    let city = hermes.city();
    let shoulders = highway_shoulders(&program);
    let candidates = curb_candidates(&program, city);
    let mut occupied: Vec<_> = entries
        .iter()
        .filter(|e| e.kind().is_road_sign())
        .flat_map(|e| e.placements(city).unwrap_or_default())
        .filter(|(_, p)| !p.sign_overlaps_building(city) && supports_clear(city, p, &shoulders))
        .map(|(_, p)| p.position)
        .collect();
    let mut issues = Vec::new();
    let mut inspected = 0;
    for entry in &entries {
        if !entry.kind().is_road_sign() {
            continue;
        }
        for (_, p) in entry.placements(city).unwrap() {
            if p.sign_overlaps_building(city) {
                continue;
            }
            inspected += 1;
            if supports_clear(city, &p, &shoulders) {
                continue;
            }
            let changed = replacement(entry, &p, &candidates, city, &occupied, &shoulders);
            if let Some((e, _)) = &changed {
                if let Shape::Single(p) = &e.shape {
                    occupied.push(p.position);
                }
            }
            let statement = changed.as_ref().map(|(e, _)| {
                opencity::Program {
                    statements: vec![opencity::Statement::Call(crate::props::write(e))],
                }
                .to_road_source()
            });
            issues.push(serde_json::json!({"name":entry.name,"position":p.position.to_array(),"surface":format!("{:?}",city.surface_at(p.position.truncate().to_array(),p.position.z+0.008).map(|s|s.kind)),"replacement":statement,"road":changed.map(|(_,road)|road)}));
        }
    }
    if let Some(path) = std::env::var_os("ROAD_SIGN_REPAIR_PLAN") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({"inspected":inspected,"issues":issues}))
                .unwrap(),
        )
        .unwrap();
    }
    eprintln!("Checked {inspected} sign assemblies against compiled road and junction surfaces; {} need relocation",issues.len());
    assert!(
        issues.is_empty(),
        "{}",
        serde_json::to_string_pretty(&issues).unwrap()
    );
}
