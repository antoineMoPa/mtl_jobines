//! Profile junctions describe each direction with one port spanning all of its lanes.
use super::*;
use opencity::network::{LaneRole, Movement, NetworkInstance};

pub(super) struct PortLanes {
    offsets: Vec<Vec3>,
    has_through_movement: bool,
}

impl PortLanes {
    fn for_turn(&self, turn: Turn) -> &[Vec3] {
        // Where drivers can continue straight, assign turns to the lane on that side.
        // At a turn-only arm every lane needs a way through the junction.
        match turn {
            Turn::Left if self.has_through_movement => &self.offsets[..1],
            Turn::Right if self.has_through_movement => &self.offsets[self.offsets.len() - 1..],
            _ => &self.offsets,
        }
    }
}

pub(super) fn port_offsets(
    component: &NetworkInstance,
    references: &[String],
    roads: &[TrafficPath],
) -> HashMap<String, PortLanes> {
    let mut offsets = HashMap::new();
    let lane_width = opencity::map::element_world_width(RoadElementKind::CarLane);
    for port in &component.geometry.ports {
        let Some(width) = port.width.filter(|width| *width > lane_width + 1e-5) else {
            continue;
        };
        let center = component.world_point(port.position);
        let center = Vec3::new(center.x, center.y, center.z) * METERS_PER_UNIT;
        let [x, y] = component.world_direction(port.heading);
        let right = Vec3::new(y, -x, 0.);
        let radius = width * METERS_PER_UNIT * 0.5;
        let mut lanes = Vec::new();
        for road in roads {
            let reference = match port.role {
                LaneRole::In => &road.end_ref,
                LaneRole::Out => &road.start_ref,
            };
            if !reference.as_ref().is_some_and(|r| references.contains(r))
                || !road.near(center, radius)
            {
                continue;
            }
            let (along, error) = road.project(center);
            let (point, heading) = road.sample(along);
            let delta = point - center;
            if error < radius && delta.z.abs() < 0.25 && heading.dot(Vec3::new(x, y, 0.)) > 0.5 {
                lanes.push(delta);
            }
        }
        if !lanes.is_empty() {
            lanes.sort_by(|a, b| a.dot(right).total_cmp(&b.dot(right)));
            let name = format!("{}.{}", port.name, port.role.keyword());
            let has_through_movement = component.geometry.ports.iter().any(|other| {
                if other.role == port.role || other.heading != port.heading {
                    return false;
                }
                let other_name = format!("{}.{}", other.name, other.role.keyword());
                let (from, to) = match port.role {
                    LaneRole::In => (&name, &other_name),
                    LaneRole::Out => (&other_name, &name),
                };
                component.geometry.movements.iter().any(|m| {
                    m.movement.class == "vehicle"
                        && m.movement.from == *from
                        && m.movement.to == *to
                })
            });
            offsets.insert(
                name,
                PortLanes {
                    offsets: lanes,
                    has_through_movement,
                },
            );
        }
    }
    offsets
}

pub(super) fn append_movement(
    graph: &mut TrafficPaths,
    path: TrafficPath,
    offsets: &HashMap<String, PortLanes>,
    movement: &Movement,
) {
    let from = offsets.get(&movement.from);
    let to = offsets.get(&movement.to);
    if from.is_none() && to.is_none() {
        graph.paths.push(path);
        return;
    }
    let from = from
        .map(|lanes| lanes.for_turn(path.turn))
        .unwrap_or(&[Vec3::ZERO]);
    let to = to
        .map(|lanes| lanes.for_turn(path.turn))
        .unwrap_or(&[Vec3::ZERO]);
    let count = from.len().max(to.len());
    for i in 0..count {
        let entry = from[i * from.len() / count];
        let exit = to[i * to.len() / count];
        let points = path
            .points
            .iter()
            .enumerate()
            .map(|(index, &point)| {
                let t = index as f32 / (path.points.len() - 1) as f32;
                point + entry.lerp(exit, t * t * (3. - 2. * t))
            })
            .collect();
        let id = graph.push(points, SpeedClass::Junction);
        graph.paths[id].junction = path.junction;
        graph.paths[id].turn = path.turn;
        graph.paths[id].signal = path.signal;
    }
}
