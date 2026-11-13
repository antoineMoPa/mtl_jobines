//! Two-way foot routes sampled from authored sidewalk bands and their real support.
use crate::dynamic_world::METERS_PER_UNIT;
use glam::Vec3;
use opencity::{
    runtime::{City, SurfaceKind},
    RoadElementKind,
};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};

#[derive(Clone)]
pub(crate) struct WalkingPath {
    pub points: Vec<Vec3>,
    pub length: f32,
    pub reverse: usize,
    pub next: Vec<usize>,
    pub connector: bool,
    bounds: [Vec3; 2],
}
impl WalkingPath {
    pub fn near(&self, center: Vec3, radius: f32) -> bool {
        center.distance_squared(center.clamp(self.bounds[0], self.bounds[1])) < radius * radius
    }
    pub fn sample(&self, distance: f32) -> (Vec3, Vec3) {
        let mut remaining = distance.max(0.);
        for pair in self.points.windows(2) {
            let delta = pair[1] - pair[0];
            let length = delta.length();
            if length <= 1e-5 {
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
}
/// A clearance query in game metres. The ceiling and step limit keep stacked
/// roads separate, and collision shells keep routes out of facades and walls.
pub(crate) fn support(city: &City, point: Vec3) -> Option<Vec3> {
    let xy = (point.truncate() / METERS_PER_UNIT).to_array();
    let surface = city.surface_at(xy, (point.z + 0.42) / METERS_PER_UNIT)?;
    let z = surface.height * METERS_PER_UNIT;
    if surface.kind != SurfaceKind::Sidewalk || (z - point.z).abs() > 0.42 {
        return None;
    }
    let bottom = (z + 0.12) / METERS_PER_UNIT;
    let top = (z + 1.75) / METERS_PER_UNIT;
    if city.building_collision(xy, 0.3 / METERS_PER_UNIT, bottom, top)
        || city
            .road_barrier_contact(xy, 0.3 / METERS_PER_UNIT, bottom, top)
            .is_some()
    {
        return None;
    }
    Some(point.truncate().extend(z))
}
fn supported_line(city: &City, a: Vec3, b: Vec3) -> Option<Vec<Vec3>> {
    let steps = (a.distance(b) / 0.4).ceil().max(1.) as usize;
    (0..=steps)
        .map(|i| support(city, a.lerp(b, i as f32 / steps as f32)))
        .collect()
}
/// Short corner searches follow sidewalk paving; they never infer a crosswalk
/// from the proximity of two road mouths. The bounded search runs only at load.
fn corner(city: &City, a: Vec3, b: Vec3) -> Option<Vec<Vec3>> {
    if let Some(points) = supported_line(city, a, b) {
        return Some(points);
    }
    const CELL: f32 = 0.75;
    const WIDTH: usize = 49;
    const MID: i32 = (WIDTH / 2) as i32;
    let point = |i: usize| {
        let x = (i % WIDTH) as i32 - MID;
        let y = (i / WIDTH) as i32 - MID;
        let xy = a + Vec3::new(x as f32 * CELL, y as f32 * CELL, 0.);
        let along = ((xy - a).dot(b - a) / a.distance_squared(b).max(0.01)).clamp(0., 1.);
        Vec3::new(xy.x, xy.y, a.z + (b.z - a.z) * along)
    };
    let start = WIDTH * (WIDTH / 2) + WIDTH / 2;
    let mut costs = vec![u32::MAX; WIDTH * WIDTH];
    let mut previous = vec![usize::MAX; WIDTH * WIDTH];
    let mut open = BinaryHeap::new();
    costs[start] = 0;
    open.push(Reverse((0u32, start)));
    let mut visited = 0;
    while let Some(Reverse((_, i))) = open.pop() {
        visited += 1;
        if visited > 768 {
            break;
        }
        let p = support(city, point(i))?;
        if p.distance(b) < 1.1 && supported_line(city, p, b).is_some() {
            let mut result = vec![b, p];
            let mut at = i;
            while at != start {
                at = previous[at];
                result.push(support(city, point(at))?);
            }
            result.reverse();
            // Remove grid stair-steps wherever the same paving supports a chord.
            let mut smooth = vec![a];
            let mut from = 0;
            while from + 1 < result.len() {
                let to = (from + 1..result.len())
                    .rev()
                    .find(|&j| supported_line(city, result[from], result[j]).is_some())
                    .unwrap();
                smooth.push(result[to]);
                from = to;
            }
            return Some(smooth);
        }
        for (dx, dy) in [
            (-1, -1),
            (-1, 0),
            (-1, 1),
            (0, -1),
            (0, 1),
            (1, -1),
            (1, 0),
            (1, 1),
        ] {
            let x = (i % WIDTH) as i32 + dx;
            let y = (i / WIDTH) as i32 + dy;
            if x < 0 || y < 0 || x >= WIDTH as i32 || y >= WIDTH as i32 {
                continue;
            }
            let j = y as usize * WIDTH + x as usize;
            let cost = costs[i] + if dx == 0 || dy == 0 { 1000 } else { 1414 };
            if cost >= costs[j] {
                continue;
            }
            let Some(q) = support(city, point(j)) else {
                continue;
            };
            if supported_line(city, p, q).is_none() {
                continue;
            }
            costs[j] = cost;
            previous[j] = i;
            let heuristic = (q.distance(b) / CELL * 1000.) as u32;
            open.push(Reverse((cost + heuristic, j)));
        }
    }
    None
}

#[derive(Default)]
pub(crate) struct WalkingPaths {
    pub paths: Vec<WalkingPath>,
}
impl WalkingPaths {
    fn pair(&mut self, points: Vec<Vec3>, connector: bool) -> Option<usize> {
        let length: f32 = points.windows(2).map(|p| p[0].distance(p[1])).sum();
        if points.len() < 2 || length < 0.05 {
            return None;
        }
        let bounds = points.iter().fold(
            [Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)],
            |bounds, &p| [bounds[0].min(p), bounds[1].max(p)],
        );
        let id = self.paths.len();
        let mut reversed = points.clone();
        reversed.reverse();
        self.paths.push(WalkingPath {
            points,
            length,
            reverse: id + 1,
            next: Vec::new(),
            connector,
            bounds,
        });
        self.paths.push(WalkingPath {
            points: reversed,
            length,
            reverse: id,
            next: Vec::new(),
            connector,
            bounds,
        });
        Some(id)
    }
    pub fn new(city: &City) -> Self {
        let mut graph = Self::default();
        for road in &city.roads {
            let control_length: f32 = [
                road.segment.start,
                road.segment.controls[0],
                road.segment.controls[1],
                road.segment.end,
            ]
            .windows(2)
            .map(|p| (p[1].x - p[0].x).hypot(p[1].y - p[0].y))
            .sum();
            let steps = (control_length * METERS_PER_UNIT / 1.)
                .ceil()
                .clamp(2., 8192.) as usize;
            for (band, _) in road
                .definition
                .elements
                .iter()
                .enumerate()
                .filter(|(_, e)| e.kind == RoadElementKind::Sidewalk)
            {
                let mut run = Vec::new();
                for i in 0..=steps {
                    let t = i as f32 / steps as f32;
                    let p =
                        opencity::map::element_position(&road.segment, &road.definition, band, t)
                            .and_then(|p| {
                                support(
                                    city,
                                    Vec3::new(p.x, p.y, road.segment.height_at(t))
                                        * METERS_PER_UNIT,
                                )
                            });
                    if let Some(p) = p {
                        if run.last().is_some_and(|last: &Vec3| {
                            last.distance(p) > 2. || supported_line(city, *last, p).is_none()
                        }) {
                            graph.pair(std::mem::take(&mut run), false);
                        }
                        run.push(p);
                    } else {
                        graph.pair(std::mem::take(&mut run), false);
                    }
                }
                graph.pair(run, false);
            }
        }
        let count = graph.paths.len();
        // Every directed base path has one end; its paired path starts there.
        let cell = |p: Vec3| ((p.x / 12.).floor() as i32, (p.y / 12.).floor() as i32);
        let mut ends = HashMap::<_, Vec<usize>>::new();
        for (i, path) in graph.paths.iter().enumerate() {
            ends.entry(cell(*path.points.last().unwrap()))
                .or_default()
                .push(i);
        }
        for i in 0..count {
            let a = *graph.paths[i].points.last().unwrap();
            let (x, y) = cell(a);
            let mut candidates = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(ids) = ends.get(&(x + dx, y + dy)) {
                        candidates.extend(ids.iter().copied());
                    }
                }
            }
            candidates.sort_unstable();
            for j in candidates {
                if j <= i || j == graph.paths[i].reverse {
                    continue;
                }
                let b = *graph.paths[j].points.last().unwrap();
                if a.distance(b) > 12. || (a.z - b.z).abs() > 1.2 {
                    continue;
                }
                let ri = graph.paths[i].reverse;
                let rj = graph.paths[j].reverse;
                if a.distance(b) < 0.08 {
                    graph.paths[i].next.push(rj);
                    graph.paths[j].next.push(ri);
                } else if let Some(points) = corner(city, a, b) {
                    if let Some(link) = graph.pair(points, true) {
                        graph.paths[i].next.push(link);
                        graph.paths[link].next.push(rj);
                        graph.paths[j].next.push(link + 1);
                        graph.paths[link + 1].next.push(ri);
                    }
                }
            }
        }
        graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn city(source: &str) -> City {
        City::from_program(&opencity::parse(source).unwrap(), None).unwrap()
    }
    #[test]
    fn sidewalks_are_bidirectional_and_roads_without_sidewalks_have_no_foot_routes() {
        let c = city("road street: sidewalk carlane carlane sidewalk; end street(-1,0,1,0);");
        let graph = WalkingPaths::new(&c);
        assert_eq!(graph.paths.len(), 4);
        for (id, path) in graph.paths.iter().enumerate() {
            assert!(path.length > 190.);
            assert_eq!(graph.paths[path.reverse].reverse, id);
            assert!(path.points.iter().all(|&p| support(&c, p).is_some()));
        }
        assert!(
            WalkingPaths::new(&city("road street: carlane carlane; end street(-1,0,1,0);"))
                .paths
                .is_empty()
        );
    }
    #[test]
    fn pedestrian_links_stay_on_paving_and_do_not_join_stacked_decks() {
        let c = city(
            "road street: sidewalk carlane carlane sidewalk; end
            a=street(-1,0,0,0); b=street(0,0,1,0); upper=street(-1,0,1,0); grade(upper,0.1,0.1);",
        );
        let graph = WalkingPaths::new(&c);
        assert!(graph.paths.iter().any(|p| !p.next.is_empty()));
        for path in &graph.paths {
            for &next in &path.next {
                assert!(
                    path.points
                        .last()
                        .unwrap()
                        .distance(graph.paths[next].points[0])
                        < 0.1
                );
            }
            for pair in path.points.windows(2) {
                assert!(supported_line(&c, pair[0], pair[1]).is_some());
                assert!((pair[0].z - pair[1].z).abs() < 0.45);
            }
        }
        let low = graph
            .paths
            .iter()
            .find(|p| p.points[0].z < 1.)
            .unwrap()
            .points[0];
        let high = low + Vec3::Z * 10.;
        assert!(corner(&c, low, high).is_none());
    }
}
