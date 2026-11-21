//! Lane-centre paths and permitted component movements, in metres.
use crate::dynamic_world::METERS_PER_UNIT;
use glam::Vec3;
use opencity::{runtime::City, RoadDirection, RoadElementKind, RoadRole};
use std::collections::HashMap;
#[path = "junction_lanes.rs"]
mod junction_lanes;
#[path = "signal_placement.rs"]
mod signal_placement;
#[cfg(test)]
#[path = "traffic_path_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedClass {
    Local,
    Arterial,
    Highway,
    Ramp,
    Merge,
    Junction,
}
impl SpeedClass {
    pub fn meters_per_second(self) -> f32 {
        let kmh = match self {
            Self::Local => 30.,
            Self::Arterial => 50.,
            Self::Highway => 100.,
            Self::Ramp => 50.,
            Self::Merge => 70.,
            Self::Junction => 20.,
        };
        kmh / 3.6
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Turn {
    #[default]
    Straight,
    Left,
    Right,
}
#[derive(Clone)]
pub struct TrafficPath {
    pub points: Vec<Vec3>,
    pub length: f32,
    bounds: [Vec3; 2],
    pub speed: SpeedClass,
    pub turn: Turn,
    pub next: Vec<usize>,
    pub signal: Option<usize>,
    pub junction: Option<usize>,
    start_ref: Option<String>,
    end_ref: Option<String>,
}
impl TrafficPath {
    fn new(points: Vec<Vec3>, speed: SpeedClass) -> Self {
        let length = points.windows(2).map(|p| p[0].distance(p[1])).sum();
        let bounds = points.iter().fold(
            [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)],
            |b, p| [b[0].min(*p), b[1].max(*p)],
        );
        Self {
            points,
            bounds,
            length,
            speed,
            turn: Turn::Straight,
            next: Vec::new(),
            signal: None,
            junction: None,
            start_ref: None,
            end_ref: None,
        }
    }
    pub fn near(&self, point: Vec3, radius: f32) -> bool {
        point.distance(point.clamp(self.bounds[0], self.bounds[1])) <= radius
    }
    pub fn sample(&self, distance: f32) -> (Vec3, Vec3) {
        let mut remaining = distance.max(0.);
        for pair in self.points.windows(2) {
            let delta = pair[1] - pair[0];
            let length = delta.length();
            if length < 1e-5 {
                continue;
            }
            if remaining <= length {
                return (pair[0] + delta * (remaining / length), delta / length);
            }
            remaining -= length;
        }
        let end = self.points.len() - 1;
        let delta = self.points[end] - self.points[end - 1];
        (self.points[end], delta / delta.length().max(1e-5))
    }
    pub fn project(&self, point: Vec3) -> (f32, f32) {
        let mut best = (0., f32::INFINITY);
        let mut along = 0.;
        for pair in self.points.windows(2) {
            let delta = pair[1] - pair[0];
            let length = delta.length();
            let t = ((point - pair[0]).dot(delta) / length.powi(2).max(1e-10)).clamp(0., 1.);
            let error = point.distance(pair[0] + delta * t);
            if error < best.1 {
                best = (along + length * t, error);
            }
            along += length;
        }
        best
    }
}
#[derive(Clone)]
pub struct SignalApproach {
    pub position: Vec3,
    pub mast_base: Option<Vec3>,
    pub forward: Vec3,
    pub junction: usize,
    pub phase: usize,
}
#[derive(Default, Clone)]
pub struct TrafficPaths {
    pub paths: Vec<TrafficPath>,
    pub signals: Vec<SignalApproach>,
}
impl TrafficPaths {
    fn push(&mut self, points: Vec<Vec3>, speed: SpeedClass) -> usize {
        let id = self.paths.len();
        self.paths.push(TrafficPath::new(points, speed));
        id
    }
    /// Directed cycling paths: dedicated lanes where present, ordinary streets otherwise.
    pub fn for_bicycles(city: &City) -> (Self, Vec<bool>) {
        let mut graph = Self::default();
        let mut dedicated = Vec::new();
        for road in &city.roads {
            if road.definition.role != RoadRole::Ordinary {
                continue;
            }
            for forward in [true, false] {
                if !road.segment.permits_travel(forward) {
                    continue;
                }
                let candidates: Vec<_> = road
                    .definition
                    .elements
                    .iter()
                    .enumerate()
                    .filter(|(i, e)| {
                        matches!(e.kind, RoadElementKind::BikeLane | RoadElementKind::CarLane)
                            && road
                                .definition
                                .lane_direction(*i)
                                .is_some_and(|d| d.permits(forward))
                    })
                    .collect();
                let lane = candidates
                    .iter()
                    .filter(|(_, e)| e.kind == RoadElementKind::BikeLane)
                    .map(|(i, _)| *i)
                    .next()
                    .or_else(|| {
                        if forward {
                            candidates.last().map(|(i, _)| *i)
                        } else {
                            candidates.first().map(|(i, _)| *i)
                        }
                    });
                let Some(lane) = lane else { continue };
                let bike = road.definition.elements[lane].kind == RoadElementKind::BikeLane;
                let mut points: Vec<_> = (0..=64)
                    .filter_map(|i| {
                        let t = i as f32 / 64.;
                        let p = opencity::map::element_position(
                            &road.segment,
                            &road.definition,
                            lane,
                            t,
                        )?;
                        Some(Vec3::new(p.x, p.y, road.segment.height_at(t)) * METERS_PER_UNIT)
                    })
                    .collect();
                if points.len() < 2 {
                    continue;
                }
                if !forward {
                    points.reverse();
                }
                let id = graph.push(points, SpeedClass::Local);
                graph.paths[id].start_ref = if forward {
                    road.segment.start_ref.clone()
                } else {
                    road.segment.end_ref.clone()
                };
                graph.paths[id].end_ref = if forward {
                    road.segment.end_ref.clone()
                } else {
                    road.segment.start_ref.clone()
                };
                dedicated.push(bike);
            }
        }
        // Reserve a proper turning area before the shared junction center.
        // Lane endpoints at the center made tiny, unrideable corner connectors.
        for path in &mut graph.paths {
            let inset = 6_f32.min(path.length * 0.3);
            let from = if path.start_ref.is_some() { inset } else { 0. };
            let to = path.length - if path.end_ref.is_some() { inset } else { 0. };
            let mut points = vec![path.sample(from).0];
            let mut along = 0.;
            for pair in path.points.windows(2) {
                along += pair[0].distance(pair[1]);
                if along > from && along < to {
                    points.push(pair[1]);
                }
            }
            points.push(path.sample(to).0);
            path.points = points;
            path.length = to - from;
            path.bounds = path.points.iter().fold(
                [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)],
                |b, p| [b[0].min(*p), b[1].max(*p)],
            );
        }
        let count = graph.paths.len();
        let mut starts = HashMap::<String, Vec<usize>>::new();
        for (id, p) in graph.paths.iter().enumerate() {
            if let Some(r) = &p.start_ref {
                starts.entry(r.clone()).or_default().push(id);
            }
        }
        for i in 0..count {
            let Some(reference) = graph.paths[i].end_ref.as_ref() else {
                continue;
            };
            let candidates = starts.get(reference).cloned().unwrap_or_default();
            let (a, heading) = graph.paths[i].sample(graph.paths[i].length);
            for j in candidates {
                if i == j {
                    continue;
                }
                let (b, outgoing) = graph.paths[j].sample(0.);
                if heading.dot(outgoing) < -0.5 || (a.z - b.z).abs() > 1. {
                    continue;
                }
                let reach = a.distance(b).max(1.) * 0.5;
                let controls = [a, a + heading * reach, b - outgoing * reach, b];
                let points = (0..=16)
                    .map(|k| {
                        let t = k as f32 / 16.;
                        let u = 1. - t;
                        controls[0] * u * u * u
                            + controls[1] * 3. * u * u * t
                            + controls[2] * 3. * u * t * t
                            + controls[3] * t * t * t
                    })
                    .collect();
                let connector = graph.push(points, SpeedClass::Junction);
                graph.paths[connector].next.push(j);
                graph.paths[i].next.push(connector);
                dedicated.push(dedicated[j]);
            }
        }
        (graph, dedicated)
    }
    pub fn new(city: &City, program: &opencity::Program) -> Self {
        let mut graph = Self::default();
        let projection = opencity::map::project(program);
        let profiles = opencity::road_profiles::road_profiles(program, &projection);
        let attachments: Vec<_> = city
            .roads
            .iter()
            .flat_map(|r| [&r.segment.start_ref, &r.segment.end_ref])
            .flatten()
            .filter_map(|name| {
                opencity::map::lane_attachment(program, name)
                    .map(|attachment| (name.clone(), attachment))
            })
            .collect();
        // A lane attachment at a parent's endpoint shares that endpoint's topology.
        // Interior attachments remain lane-specific, and unrelated named crossings
        // retain distinct references.
        let endpoint_reference = |reference: Option<String>| {
            let Some((_, (parent, _, t))) = attachments
                .iter()
                .find(|(name, _)| Some(name) == reference.as_ref())
            else {
                return reference;
            };
            let Some(road) = city
                .roads
                .iter()
                .find(|r| r.segment.name.as_ref() == Some(parent))
            else {
                return reference;
            };
            if *t == 0. {
                road.segment.start_ref.clone().or(reference)
            } else if *t == 1. {
                road.segment.end_ref.clone().or(reference)
            } else {
                reference
            }
        };
        for road in &city.roads {
            let profile_index = projection
                .segments
                .iter()
                .position(|s| s.source == road.segment.source)
                .expect("runtime road belongs to the map projection");
            let profile = &profiles[profile_index];
            let lanes: Vec<_> = road
                .definition
                .elements
                .iter()
                .enumerate()
                .filter(|(_, e)| e.kind == RoadElementKind::CarLane)
                .map(|(i, _)| i)
                .collect();
            let speed = match road.definition.role {
                RoadRole::Highway => SpeedClass::Highway,
                RoadRole::Ramp => SpeedClass::Ramp,
                RoadRole::Merge => SpeedClass::Merge,
                RoadRole::Ordinary if lanes.len() > 2 => SpeedClass::Arterial,
                RoadRole::Ordinary => SpeedClass::Local,
            };
            for (order, &lane) in lanes.iter().enumerate() {
                let direction = match road.definition.lane_direction(lane).unwrap() {
                    RoadDirection::Both if lanes.len() > 1 => {
                        if order < lanes.len() / 2 {
                            RoadDirection::Reverse
                        } else {
                            RoadDirection::Forward
                        }
                    }
                    other => other,
                };
                let mut cuts = vec![0., 1.];
                // Split at authored ramp attachment points, never at geometric crossings.
                for (_, (parent, band, t)) in &attachments {
                    if road.segment.name.as_ref() == Some(parent) && *band == lane {
                        cuts.push(*t);
                    }
                }
                cuts.sort_by(f32::total_cmp);
                cuts.dedup();
                for forward in [true, false] {
                    if !direction.permits(forward) || !road.segment.permits_travel(forward) {
                        continue;
                    }
                    for cut in cuts.windows(2) {
                        let control_length: f32 = [
                            road.segment.start,
                            road.segment.controls[0],
                            road.segment.controls[1],
                            road.segment.end,
                        ]
                        .windows(2)
                        .map(|p| (p[1].x - p[0].x).hypot(p[1].y - p[0].y))
                        .sum();
                        let steps = ((control_length * METERS_PER_UNIT * (cut[1] - cut[0]) / 2.)
                            .ceil() as usize)
                            .clamp(2, 4096);
                        let mut points: Vec<_> = (0..=steps)
                            .filter_map(|i| {
                                let t = cut[0] + (cut[1] - cut[0]) * i as f32 / steps as f32;
                                let (widths, origin) = opencity::road_profiles::section_widths(
                                    &road.definition,
                                    profile,
                                    t,
                                );
                                let offset = origin
                                    + widths[..lane].iter().sum::<f32>()
                                    + widths.get(lane)? / 2.;
                                let p = road.segment.point_at(t);
                                let tangent = road.segment.tangent_at(t);
                                let heading = tangent.y.atan2(tangent.x);
                                Some(
                                    Vec3::new(
                                        p.x + heading.sin() * offset,
                                        p.y - heading.cos() * offset,
                                        road.segment.height_at(t),
                                    ) * METERS_PER_UNIT,
                                )
                            })
                            .collect();
                        if points.len() < 2 {
                            continue;
                        }
                        if !forward {
                            points.reverse();
                        }
                        let id = graph.push(points, speed);
                        let reference = |t: f32| {
                            attachments
                                .iter()
                                .find(|(_, (parent, band, at))| {
                                    road.segment.name.as_ref() == Some(parent)
                                        && *band == lane
                                        && (*at - t).abs() < 1e-6
                                })
                                .map(|(name, _)| name.clone())
                                .or_else(|| {
                                    if t == 0. {
                                        road.segment.start_ref.clone()
                                    } else if t == 1. {
                                        road.segment.end_ref.clone()
                                    } else {
                                        None
                                    }
                                })
                        };
                        graph.paths[id].start_ref =
                            endpoint_reference(reference(if forward { cut[0] } else { cut[1] }));
                        graph.paths[id].end_ref =
                            endpoint_reference(reference(if forward { cut[1] } else { cut[0] }));
                    }
                }
            }
        }
        let road_count = graph.paths.len();
        let mut reference_movements = HashMap::<String, Vec<usize>>::new();
        for (junction, component) in city.components.iter().enumerate() {
            let profile = city
                .junctions
                .iter()
                .find(|j| component.id == format!("profile:{}", j.point_ref));
            let (references, lanes) = if let Some(j) = profile {
                let references: Vec<_> = std::iter::once(j.point_ref.clone())
                    .chain(j.ports.iter().map(|p| p.point_ref.clone()))
                    .collect();
                let lanes = junction_lanes::port_offsets(
                    component,
                    &references,
                    &graph.paths[..road_count],
                );
                (references, lanes)
            } else {
                let references = component
                    .geometry
                    .ports
                    .iter()
                    .map(|p| format!("{}.{}.{}", component.name, p.name, p.role.keyword()))
                    .collect();
                (references, HashMap::new())
            };
            let first_movement = graph.paths.len();
            let incoming: Vec<_> = component
                .geometry
                .ports
                .iter()
                .filter(|p| p.role == opencity::network::LaneRole::In)
                .collect();
            // A one-way arm removes one incoming approach from a four-arm
            // crossing. Count distinct approach headings, not lane/port count.
            use opencity::network::Heading;
            let signalized = [Heading::North, Heading::East, Heading::South, Heading::West]
                .iter()
                .filter(|heading| incoming.iter().any(|port| port.heading == **heading))
                .count()
                >= 3;
            for movement in &component.geometry.movements {
                if movement.movement.class != "vehicle" || movement.points.len() < 2 {
                    continue;
                }
                let points: Vec<_> = movement
                    .points
                    .iter()
                    .map(|&p| {
                        let p = component.world_point(p);
                        Vec3::new(p.x, p.y, p.z) * METERS_PER_UNIT
                    })
                    .collect();
                let mut path = TrafficPath::new(points, SpeedClass::Junction);
                path.junction = Some(junction);
                let incoming_heading = path.sample(0.).1.truncate();
                let outgoing_heading = path.sample(path.length).1.truncate();
                path.turn = match incoming_heading.perp_dot(outgoing_heading) {
                    cross if cross > 0.2 => Turn::Left,
                    cross if cross < -0.2 => Turn::Right,
                    _ => Turn::Straight,
                };
                if signalized {
                    if let Some(port) = incoming
                        .iter()
                        .find(|p| format!("{}.in", p.name) == movement.movement.from)
                    {
                        let heading = port.heading.vector();
                        let phase = usize::from(heading[0].abs() > heading[1].abs());
                        let (position, forward) = path.sample(0.);
                        let signal = graph
                            .signals
                            .iter()
                            .position(|s| {
                                s.junction == junction && s.position.distance(position) < 0.1
                            })
                            .unwrap_or_else(|| {
                                graph.signals.push(SignalApproach {
                                    position,
                                    mast_base: signal_placement::sidewalk_base(
                                        city,
                                        position,
                                        forward,
                                        &graph.signals,
                                    ),
                                    forward,
                                    junction,
                                    phase,
                                });
                                graph.signals.len() - 1
                            });
                        path.signal = Some(signal);
                    }
                }
                junction_lanes::append_movement(&mut graph, path, &lanes, &movement.movement);
            }
            for reference in references {
                reference_movements
                    .entry(reference)
                    .or_default()
                    .extend(first_movement..graph.paths.len());
            }
        }
        // Runtime roads retain their authored centre endpoints; the rendered junction
        // cuts them back to its ports. Apply the same cut to lane paths before linking.
        for i in 0..graph.paths.len() {
            if graph.paths[i].speed == SpeedClass::Junction {
                continue;
            }
            let mut bounds = (0., graph.paths[i].length);
            for start in [true, false] {
                let reference = if start {
                    &graph.paths[i].start_ref
                } else {
                    &graph.paths[i].end_ref
                };
                let Some(reference) = reference else {
                    continue;
                };
                let candidate = reference_movements
                    .get(reference)
                    .into_iter()
                    .flatten()
                    .map(|&id| &graph.paths[id])
                    .filter_map(|movement| {
                        let point = if start {
                            *movement.points.last().unwrap()
                        } else {
                            movement.points[0]
                        };
                        let (along, error) = graph.paths[i].project(point);
                        let heading = graph.paths[i].sample(along).1;
                        let turn_heading =
                            movement.sample(if start { movement.length } else { 0. }).1;
                        if error > 4. || heading.dot(turn_heading) < 0.5 {
                            return None;
                        }
                        let distance = if start {
                            along
                        } else {
                            graph.paths[i].length - along
                        };
                        (distance < 80.).then_some((along, error))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((along, _)) = candidate {
                    if start {
                        bounds.0 = along;
                    } else {
                        bounds.1 = along;
                    }
                }
            }
            if bounds.1 - bounds.0 < 0.1 {
                continue;
            }
            let path = &graph.paths[i];
            let mut points = vec![path.sample(bounds.0).0];
            let mut distance = 0.;
            for pair in path.points.windows(2) {
                distance += pair[0].distance(pair[1]);
                if distance > bounds.0 && distance < bounds.1 {
                    points.push(pair[1]);
                }
            }
            points.push(path.sample(bounds.1).0);
            graph.paths[i].points = points;
            graph.paths[i].length = bounds.1 - bounds.0;
        }
        // Endpoint-only links preserve bridges and disconnected crossings. Named endpoints
        // with distinct names stay disconnected even if their coordinates coincide.
        let cell = |p: Vec3| {
            (
                (p.x / 2.).floor() as i32,
                (p.y / 2.).floor() as i32,
                (p.z / 2.).floor() as i32,
            )
        };
        let mut starts = HashMap::<_, Vec<usize>>::new();
        for (id, path) in graph.paths.iter().enumerate() {
            starts.entry(cell(path.points[0])).or_default().push(id);
        }
        for i in 0..graph.paths.len() {
            let end = *graph.paths[i].points.last().unwrap();
            let (_, heading) = graph.paths[i].sample(graph.paths[i].length);
            let (x, y, z) = cell(end);
            let mut nearby = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        if let Some(ids) = starts.get(&(x + dx, y + dy, z + dz)) {
                            nearby.extend(ids.iter().copied());
                        }
                    }
                }
            }
            let next = nearby
                .into_iter()
                .filter(|&j| {
                    let b = &graph.paths[j];
                    if i == j || end.distance(b.points[0]) > 0.25 {
                        return false;
                    }
                    if graph.paths[i].end_ref.is_some()
                        && b.start_ref.is_some()
                        && graph.paths[i].end_ref != b.start_ref
                    {
                        return false;
                    }
                    heading.dot(b.sample(0.).1) > -0.1
                })
                .collect();
            graph.paths[i].next = next;
        }
        graph
    }
}

#[cfg(test)]
mod cycling_map_tests {
    use super::*;
    #[test]
    fn saint_laurent_saint_denis_and_report_79_have_continuous_bike_lane_corridors() {
        let source = include_str!("../../data/maps/mtl/map.road");
        let program = opencity::parse(source).unwrap();
        let projection = opencity::map::project(&program);
        for row in [1, 2, 3] {
            for col in 0..4 {
                let name = format!("district_street_x_{col}_{row}");
                let road = projection
                    .segments
                    .iter()
                    .find(|s| s.name.as_deref() == Some(name.as_str()))
                    .unwrap();
                assert_eq!(
                    road.road_name, "cycling_street",
                    "missing cycling profile on {name}"
                );
            }
        }
    }
}
