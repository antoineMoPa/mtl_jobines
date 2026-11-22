//! Directed, length-weighted road graph. Crossings never imply connectivity.
use glam::Vec3;
use opencity::{network::LaneRole, runtime::City, RoadElementKind};
use std::{cmp::Ordering, collections::BinaryHeap};

#[derive(Clone)]
struct Edge {
    from: usize,
    to: usize,
    points: Vec<Vec3>,
    length: f32,
}
#[derive(Clone, Copy, Debug)]
struct Snap {
    edge: usize,
    segment: usize,
    point: Vec3,
    distance: f32,
}
#[derive(Default)]
pub struct Router {
    nodes: Vec<Vec3>,
    edges: Vec<Edge>,
    outgoing: Vec<Vec<usize>>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RouteStatus {
    #[default]
    Inactive,
    Active,
    Unreachable,
    Arrived,
}
pub struct Navigation {
    pub destination: Option<[f32; 2]>,
    pub points: Vec<Vec3>,
    pub status: RouteStatus,
    pub revision: u64,
    marker_only: bool,
    target: Option<Snap>,
    last_position: Option<Vec3>,
}
impl Default for Navigation {
    fn default() -> Self {
        Self {
            destination: None,
            points: Vec::new(),
            status: RouteStatus::Inactive,
            revision: 0,
            marker_only: false,
            target: None,
            last_position: None,
        }
    }
}
impl Router {
    pub(super) fn road_point(&self, near: Vec3) -> Option<Vec3> {
        let snap = self.snaps(near, true).first().copied()?;
        (snap.point.distance(near) < 0.14).then_some(snap.point)
    }

    pub(super) fn can_route(&self, from: Vec3, to: Vec3) -> bool {
        self.snaps(to, true)
            .first()
            .copied()
            .is_some_and(|target| self.route(from, target).is_some())
    }
    pub fn new(city: &City, program: &opencity::Program) -> Self {
        let mut graph = Self::default();
        let mut endpoints = Vec::new();
        let mut named_endpoints = std::collections::HashMap::<String, Vec<usize>>::new();
        let attachments: std::collections::HashMap<_, _> = city
            .roads
            .iter()
            .flat_map(|r| [&r.segment.start_ref, &r.segment.end_ref])
            .flatten()
            .filter_map(|name| {
                opencity::map::lane_attachment(program, name).map(|a| (name.as_str(), a))
            })
            .collect();
        let mut attachment_nodes = std::collections::HashMap::<&str, Vec<usize>>::new();
        for road in &city.roads {
            if !road
                .definition
                .elements
                .iter()
                .any(|e| e.kind == RoadElementKind::CarLane)
            {
                continue;
            }
            let s = &road.segment;
            let lane_points: Vec<_> = attachments
                .iter()
                .filter(|(_, (parent, _, _))| s.name.as_ref() == Some(parent))
                .collect();
            let mut cuts = vec![0., 1.];
            cuts.extend(lane_points.iter().map(|(_, (_, _, t))| *t));
            cuts.sort_by(f32::total_cmp);
            cuts.dedup();
            let point = |t| {
                let p = s.point_at(t);
                Vec3::new(p.x, p.y, s.height_at(t))
            };
            let mut endpoint = |point: Vec3, reference: Option<&str>| {
                let Some(reference) = reference else {
                    return graph.node(point);
                };
                if let Some(id) = named_endpoints
                    .get(reference)
                    .into_iter()
                    .flatten()
                    .copied()
                    .find(|&id| graph.nodes[id].distance_squared(point) < 1e-10)
                {
                    return id;
                }
                let id = graph.fresh_node(point);
                named_endpoints
                    .entry(reference.to_owned())
                    .or_default()
                    .push(id);
                id
            };
            let a = endpoint(point(0.), s.start_ref.as_deref());
            let b = endpoint(point(1.), s.end_ref.as_deref());
            endpoints.extend([(a, s.start_ref.as_deref()), (b, s.end_ref.as_deref())]);
            let controls = [s.start, s.controls[0], s.controls[1], s.end];
            let length: f32 = controls
                .windows(2)
                .map(|p| (p[1].x - p[0].x).hypot(p[1].y - p[0].y))
                .sum();
            for forward in [true, false] {
                let permits = s.permits_travel(forward)
                    && road.definition.elements.iter().enumerate().any(|(i, e)| {
                        e.kind == RoadElementKind::CarLane
                            && road
                                .definition
                                .lane_direction(i)
                                .is_some_and(|d| d.permits(forward))
                    });
                if !permits {
                    continue;
                }
                let mut nodes = vec![a];
                for &t in &cuts[1..cuts.len() - 1] {
                    // Keep the two carriageways separate at intermediate ramp attachments.
                    nodes.push(graph.fresh_node(point(t)));
                }
                nodes.push(b);
                for (&t, &node) in cuts.iter().zip(&nodes) {
                    for &(name, (_, lane, attach_t)) in &lane_points {
                        if (*attach_t - t).abs() < 1e-6
                            && road
                                .definition
                                .lane_direction(*lane)
                                .is_some_and(|d| d.permits(forward))
                        {
                            attachment_nodes.entry(name).or_default().push(node);
                        }
                    }
                }
                for (i, pair) in cuts.windows(2).enumerate() {
                    let steps =
                        ((length * (pair[1] - pair[0]) / 0.025).ceil() as usize).clamp(2, 4096);
                    let mut points: Vec<_> = (0..=steps)
                        .map(|j| point(pair[0] + (pair[1] - pair[0]) * j as f32 / steps as f32))
                        .collect();
                    let (from, to) = if forward {
                        (nodes[i], nodes[i + 1])
                    } else {
                        points.reverse();
                        (nodes[i + 1], nodes[i])
                    };
                    graph.edge(from, to, points);
                }
            }
        }
        // Authored lane_point references connect ramps to only their permitted carriageway.
        for &(endpoint, reference) in &endpoints {
            if let Some(nodes) = reference.and_then(|r| attachment_nodes.get(r)) {
                for &node in nodes {
                    graph.edge(
                        endpoint,
                        node,
                        vec![graph.nodes[endpoint], graph.nodes[node]],
                    );
                    graph.edge(
                        node,
                        endpoint,
                        vec![graph.nodes[node], graph.nodes[endpoint]],
                    );
                }
            }
        }
        // Component movements supply the permitted turns and curved junction geometry.
        for component in &city.components {
            let mut ports = std::collections::HashMap::new();
            let junction = city
                .junctions
                .iter()
                .find(|j| component.id == format!("profile:{}", j.point_ref));
            for port in &component.geometry.ports {
                let p = component.world_point(port.position);
                let point = Vec3::new(p.x, p.y, p.z);
                let node = graph.node(point);
                ports.insert(format!("{}.{}", port.name, port.role.keyword()), node);
                let reference = format!("{}.{}.{}", component.name, port.name, port.role.keyword());
                let endpoint = endpoints
                    .iter()
                    .filter(|&&(id, endpoint_ref)| {
                        let connected = match junction {
                            Some(j) => endpoint_ref.is_some_and(|r| {
                                r == j.point_ref || j.ports.iter().any(|p| p.point_ref == r)
                            }),
                            None => {
                                endpoint_ref == Some(reference.as_str())
                                    || graph.nodes[id].distance_squared(point) < 1e-10
                            }
                        };
                        connected && (graph.nodes[id].z - point.z).abs() < 0.02
                    })
                    .min_by(|&&(a, _), &&(b, _)| {
                        graph.nodes[a]
                            .distance_squared(point)
                            .total_cmp(&graph.nodes[b].distance_squared(point))
                    });
                if let Some(&(end, _)) = endpoint {
                    if graph.nodes[end].distance(point) < 0.16 {
                        let (from, to) = match port.role {
                            LaneRole::In => (end, node),
                            LaneRole::Out => (node, end),
                        };
                        graph.edge(from, to, vec![graph.nodes[from], graph.nodes[to]]);
                    }
                }
            }
            for movement in &component.geometry.movements {
                if movement.movement.class != "vehicle" {
                    continue;
                }
                if let (Some(&a), Some(&b)) = (
                    ports.get(movement.movement.from.as_str()),
                    ports.get(movement.movement.to.as_str()),
                ) {
                    let points = movement
                        .points
                        .iter()
                        .map(|&p| {
                            let p = component.world_point(p);
                            Vec3::new(p.x, p.y, p.z)
                        })
                        .collect();
                    graph.edge(a, b, points);
                }
            }
        }
        graph
    }
    fn node(&mut self, point: Vec3) -> usize {
        if let Some(id) = self
            .nodes
            .iter()
            .position(|p| p.distance_squared(point) < 1e-10)
        {
            return id;
        }
        self.fresh_node(point)
    }
    fn fresh_node(&mut self, point: Vec3) -> usize {
        let id = self.nodes.len();
        self.nodes.push(point);
        self.outgoing.push(Vec::new());
        id
    }
    fn edge(&mut self, from: usize, to: usize, points: Vec<Vec3>) {
        if points.len() < 2 {
            return;
        }
        let length = points.windows(2).map(|p| p[0].distance(p[1])).sum();
        self.outgoing[from].push(self.edges.len());
        self.edges.push(Edge {
            from,
            to,
            points,
            length,
        });
    }
    fn snaps(&self, point: Vec3, height: bool) -> Vec<Snap> {
        let mut best = f32::INFINITY;
        let mut snaps = Vec::new();
        for (id, edge) in self.edges.iter().enumerate() {
            let mut before = 0.;
            for (segment, pair) in edge.points.windows(2).enumerate() {
                let delta = pair[1] - pair[0];
                let t = if height {
                    (point - pair[0]).dot(delta) / delta.length_squared().max(1e-20)
                } else {
                    (point - pair[0]).truncate().dot(delta.truncate())
                        / delta.truncate().length_squared().max(1e-20)
                }
                .clamp(0., 1.);
                let p = pair[0] + delta * t;
                let distance = if height {
                    p.distance_squared(point)
                } else {
                    p.truncate().distance_squared(point.truncate())
                };
                if distance < best - 1e-8 {
                    best = distance;
                    snaps.clear();
                }
                if distance <= best + 1e-8 {
                    snaps.push(Snap {
                        edge: id,
                        segment,
                        point: p,
                        distance: before + delta.length() * t,
                    });
                }
                before += delta.length();
            }
        }
        snaps
    }
    fn route(&self, start: Vec3, target: Snap) -> Option<Vec<Vec3>> {
        let starts = self.snaps(start, true);
        let targets = self.snaps(target.point, true);
        let mut distances = vec![f32::INFINITY; self.nodes.len()];
        let mut previous: Vec<Option<usize>> = vec![None; self.nodes.len()];
        let mut roots = vec![None; self.nodes.len()];
        let mut heap = BinaryHeap::new();
        let mut best = f32::INFINITY;
        let mut result = None;
        // Virtual endpoints allow the shortest partial edge in either permitted direction.
        for &a in &starts {
            let edge = &self.edges[a.edge];
            for b in targets
                .iter()
                .filter(|b| b.edge == a.edge && b.distance >= a.distance)
            {
                let cost = b.distance - a.distance;
                if cost < best {
                    best = cost;
                    let mut p = vec![a.point];
                    if a.segment < b.segment {
                        p.extend_from_slice(&edge.points[a.segment + 1..=b.segment]);
                    }
                    p.push(b.point);
                    result = Some(p);
                }
            }
            let cost = edge.length - a.distance;
            if cost < distances[edge.to] {
                distances[edge.to] = cost;
                roots[edge.to] = Some(a);
                heap.push(State {
                    cost,
                    node: edge.to,
                });
            }
        }
        while let Some(State { cost, node }) = heap.pop() {
            if cost > distances[node] || cost > best {
                continue;
            }
            for &b in targets.iter().filter(|b| self.edges[b.edge].from == node) {
                if cost + b.distance < best {
                    best = cost + b.distance;
                    let mut trail = Vec::new();
                    let mut at = node;
                    while let Some(id) = previous[at] {
                        trail.push(id);
                        at = self.edges[id].from;
                    }
                    let a = roots[at].expect("reachable node has a source");
                    let mut p = vec![a.point];
                    p.extend_from_slice(&self.edges[a.edge].points[a.segment + 1..]);
                    for id in trail.into_iter().rev() {
                        p.extend_from_slice(&self.edges[id].points);
                    }
                    p.extend_from_slice(&self.edges[b.edge].points[..=b.segment]);
                    p.push(b.point);
                    result = Some(p);
                }
            }
            for &id in &self.outgoing[node] {
                let edge = &self.edges[id];
                let next = cost + edge.length;
                if next < distances[edge.to] {
                    distances[edge.to] = next;
                    previous[edge.to] = Some(id);
                    heap.push(State {
                        cost: next,
                        node: edge.to,
                    });
                }
            }
        }
        result
    }
}
#[derive(Clone, Copy, PartialEq)]
struct State {
    cost: f32,
    node: usize,
}
impl Eq for State {}
impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| self.node.cmp(&other.node))
    }
}
impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Navigation {
    pub fn select_marker(&mut self, destination: [f32; 2]) {
        self.clear();
        self.marker_only = true;
        self.destination = Some(destination);
        self.status = RouteStatus::Active;
    }

    pub fn select_road_point(&mut self, router: &Router, destination: Vec3, position: Vec3) {
        self.marker_only = false;
        self.target = router.snaps(destination, true).first().copied();
        self.destination = self.target.map(|target| target.point.truncate().to_array());
        self.last_position = None;
        self.update(router, position);
    }
    pub fn select(&mut self, router: &Router, destination: [f32; 2], position: Vec3) {
        self.marker_only = false;
        self.target = router
            .snaps(Vec3::new(destination[0], destination[1], 0.), false)
            .first()
            .copied();
        self.destination = Some(
            self.target
                .map_or(destination, |s| s.point.truncate().to_array()),
        );
        self.last_position = None;
        self.update(router, position);
    }
    pub fn clear(&mut self) {
        *self = Self {
            revision: self.revision + 1,
            ..Self::default()
        };
    }
    pub fn update(&mut self, router: &Router, position: Vec3) {
        if self.marker_only
            || self.destination.is_none()
            || self
                .last_position
                .is_some_and(|p| p.distance(position) < 0.02)
        {
            return;
        }
        self.last_position = Some(position);
        self.points.clear();
        self.status = RouteStatus::Unreachable;
        if let Some(target) = self.target {
            if position.distance(target.point) < 0.06 {
                self.status = RouteStatus::Arrived;
            } else if let Some(route) = router.route(position, target) {
                self.points = route;
                self.status = RouteStatus::Active;
            }
        }
        self.revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn router(source: &str) -> Router {
        let program = opencity::parse(source).unwrap();
        Router::new(&City::from_program(&program, None).unwrap(), &program)
    }
    fn path(graph: &Router, from: [f32; 3], to: [f32; 3]) -> Option<Vec<Vec3>> {
        let target = graph.snaps(to.into(), true).first().copied()?;
        graph.route(from.into(), target)
    }
    fn length(points: &[Vec3]) -> f32 {
        points.windows(2).map(|p| p[0].distance(p[1])).sum()
    }
    #[test]
    fn partial_edges_and_one_way_travel() {
        let graph = router("road street: carlane; direction forward; end street(0,0,10,0);");
        let p = path(&graph, [2., 0., 0.], [7., 0., 0.]).unwrap();
        assert!((length(&p) - 5.).abs() < 1e-5);
        assert!(path(&graph, [7., 0., 0.], [2., 0., 0.]).is_none());
        let graph = router("road street: carlane; direction reverse; end street(0,0,10,0);");
        assert!((length(&path(&graph, [7., 0., 0.], [2., 0., 0.]).unwrap()) - 5.).abs() < 1e-5);
        assert!(path(&graph, [2., 0., 0.], [7., 0., 0.]).is_none());
    }
    #[test]
    fn shortest_distance_wins_and_crossings_do_not_connect() {
        let graph = router(
            "road street: carlane; end street(0,0,2,0); street(2,0,4,0); \
                street(0,0,0,3); street(0,3,4,3); street(4,3,4,0); street(1,-1,1,\
                1);",
        );
        assert!((length(&path(&graph, [0.5, 0., 0.], [3.5, 0., 0.]).unwrap()) - 3.).abs() < 1e-4);
        assert!(path(&graph, [0.5, 0., 0.], [1., 0.8, 0.]).is_none());
    }
    #[test]
    fn stacked_decks_remain_disconnected() {
        let graph = router(
            "road street: carlane; end a=street(0,0,2,0); b=street(2,0,4,0); grade(b,0.5,0.5);",
        );
        assert!(path(&graph, [1., 0., 0.], [3., 0., 0.5]).is_none());
    }
    #[test]
    fn separate_named_endpoints_do_not_connect_by_position() {
        let graph = router(
            "road street: carlane; end a=map_point(0,0); b=map_point(2,0); \
                c=map_point(2,0); d=map_point(4,0); street(a,0.6,0,1.4,0,b); \
                street(c,2.6,0,3.4,0,d);",
        );
        assert!(path(&graph, [1., 0., 0.], [3., 0., 0.]).is_none());
        let joined = router(
            "road street: carlane; end a=map_point(0,0); b=map_point(2,0); \
                d=map_point(4,0); street(a,0.6,0,1.4,0,b); street(b,2.6,0,3.4,0,\
                d);",
        );
        assert!((length(&path(&joined, [1., 0., 0.], [3., 0., 0.]).unwrap()) - 2.).abs() < 1e-4);
    }
    #[test]
    fn one_way_cycle_takes_the_legal_detour() {
        let graph = router(
            "road street: carlane; direction forward; end street(0,0,4,0); \
                street(4,0,4,2); street(4,2,0,2); street(0,2,0,0);",
        );
        let points = path(&graph, [3., 0., 0.], [1., 0., 0.]).unwrap();
        assert!((length(&points) - 10.).abs() < 1e-4);
    }
    #[test]
    fn unreachable_destination_clears_the_previous_route() {
        let graph = router("road street: carlane; end street(0,0,4,0); street(0,3,4,3);");
        let mut nav = Navigation::default();
        nav.select(&graph, [3., 0.], Vec3::new(1., 0., 0.));
        assert_eq!(nav.status, RouteStatus::Active);
        nav.select(&graph, [3., 3.], Vec3::new(1., 0., 0.));
        assert_eq!(nav.status, RouteStatus::Unreachable);
        assert!(nav.points.is_empty());
        assert_eq!(nav.destination, Some([3., 3.]));
        let empty = Router::default();
        nav.select(&empty, [1., 2.], Vec3::ZERO);
        assert_eq!(nav.status, RouteStatus::Unreachable);
    }
    #[test]
    fn navigation_reroutes_arrives_and_clears() {
        let graph = router("road street: carlane; end street(0,0,10,0);");
        let mut nav = Navigation::default();
        nav.select(&graph, [8., 0.], Vec3::new(1., 0., 0.));
        assert_eq!(nav.status, RouteStatus::Active);
        let revision = nav.revision;
        nav.update(&graph, Vec3::new(1.001, 0., 0.));
        assert_eq!(nav.revision, revision);
        nav.update(&graph, Vec3::new(4., 0., 0.));
        assert_eq!(nav.points.first(), Some(&Vec3::new(4., 0., 0.)));
        nav.update(&graph, Vec3::new(8., 0., 0.));
        assert_eq!(nav.status, RouteStatus::Arrived);
        assert!(nav.points.is_empty());
        nav.update(&graph, Vec3::new(9., 0., 0.));
        assert_eq!(nav.status, RouteStatus::Active);
        nav.clear();
        assert_eq!(nav.status, RouteStatus::Inactive);
        assert!(nav.destination.is_none());
    }
}
