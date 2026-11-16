use super::*;
use opencity::road_geometry::{CrossSection, RoadGeometry};

const WIDTH: f32 = 0.14;
const DEPTH: f32 = 0.18;
const SETBACK: f32 = 0.02;
const SPACING: f32 = 0.18;
#[derive(PartialEq, Eq)]
struct CandidateKey {
    city: usize,
    center: [u32; 3],
    span: u32,
    mode: CameraMode,
    revision: u64,
}
pub(super) struct CandidateCache {
    key: CandidateKey,
    features: Vec<LandscapeFeature>,
}
struct EdgePoint {
    position: Vec3,
    outside: Vec2,
}
struct Edge {
    points: Vec<EdgePoint>,
}
struct Surface {
    points: [[f32; 2]; 4],
    min: Vec2,
    max: Vec2,
    low: f32,
    high: f32,
}
#[derive(Default)]
pub(super) struct Roads {
    edges: Vec<Edge>,
    surfaces: Vec<Surface>,
}
impl Roads {
    pub fn new(program: &Program) -> Self {
        let (expanded, _) = opencity::junction_layouts::expand(program);
        let projection = opencity::map::project(&expanded);
        let geometry = RoadGeometry::compile(&expanded, &projection);
        let mut result = Self::default();
        for (index, segment) in projection.segments.iter().enumerate() {
            let rows = geometry.cross_sections(index);
            for pair in rows.windows(2) {
                let Some([a, b, c, d]) = corners(&pair[0], &pair[1]) else {
                    continue;
                };
                let p = [a, b, c, d];
                let points = p.map(|v| v.truncate().to_array());
                let min = p
                    .iter()
                    .map(|v| v.truncate())
                    .fold(Vec2::splat(f32::INFINITY), Vec2::min);
                let max = p
                    .iter()
                    .map(|v| v.truncate())
                    .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
                result.surfaces.push(Surface {
                    points,
                    min,
                    max,
                    low: p.iter().map(|v| v.z).fold(f32::INFINITY, f32::min),
                    high: p.iter().map(|v| v.z).fold(f32::NEG_INFINITY, f32::max),
                });
            }
            let travel = expanded.statements.iter().any(|s| {
                matches!(s,Statement::Road(r)
                if r.name==segment.road_name && r.elements.iter().any(|e|e.kind.is_travel_lane()))
            });
            if !travel {
                continue;
            }
            for side in [0, 1] {
                let mut points = Vec::new();
                for row in rows {
                    let (Some(left), Some(right)) = (row.boundaries.first(), row.boundaries.last())
                    else {
                        continue;
                    };
                    let left = Vec2::new(left.point.x, left.point.y);
                    let right = Vec2::new(right.point.x, right.point.y);
                    let (edge, across) = if side == 0 {
                        (left, left - right)
                    } else {
                        (right, right - left)
                    };
                    if across.length_squared() < 1e-12 {
                        continue;
                    }
                    let angle = across.y.atan2(across.x);
                    let height = if side == 0 {
                        row.boundaries[0].height
                    } else {
                        row.boundaries.last().unwrap().height
                    };
                    points.push(EdgePoint {
                        position: edge.extend(height),
                        outside: Vec2::new(angle.cos(), angle.sin()),
                    });
                }
                if points.len() > 1 {
                    result.edges.push(Edge { points });
                }
            }
        }
        result
    }
    pub fn overlaps(&self, feature: &LandscapeFeature) -> bool {
        let min = feature
            .points
            .iter()
            .map(|p| Vec2::from_array(*p))
            .fold(Vec2::splat(f32::INFINITY), Vec2::min);
        let max = feature
            .points
            .iter()
            .map(|p| Vec2::from_array(*p))
            .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max);
        self.surfaces.iter().any(|s| {
            s.max.x >= min.x
                && s.min.x <= max.x
                && s.max.y >= min.y
                && s.min.y <= max.y
                && s.high + 0.02 >= feature.elevation
                && s.low <= feature.elevation + feature.height.unwrap_or(0.)
                && geometry::overlap(&feature.points, &s.points)
        })
    }
    fn suggestions(&self, origin: Vec2, reach: f32) -> Vec<(Vec<[f32; 2]>, f32)> {
        let mut result = Vec::new();
        for edge in &self.edges {
            let total: f32 = edge
                .points
                .windows(2)
                .map(|p| p[0].position.truncate().distance(p[1].position.truncate()))
                .sum();
            let mut traveled = 0.;
            let mut station = WIDTH / 2. + SETBACK;
            for pair in edge.points.windows(2) {
                let a = &pair[0];
                let b = &pair[1];
                let length = a.position.truncate().distance(b.position.truncate());
                if length < 1e-8 {
                    continue;
                }
                while station <= traveled + length && station + WIDTH / 2. + SETBACK <= total {
                    let t = (station - traveled) / length;
                    let point = a.position.lerp(b.position, t);
                    let direction = a.outside.lerp(b.outside, t);
                    let angle = direction.y.atan2(direction.x);
                    let outside = Vec2::new(angle.cos(), angle.sin());
                    let along = Vec2::new(outside.y, -outside.x);
                    let front = point.truncate() + outside * SETBACK;
                    let c = front + outside * (DEPTH / 2.);
                    if c.distance_squared(origin) <= reach * reach {
                        result.push((
                            vec![
                                (front - along * WIDTH / 2.).to_array(),
                                (front + along * WIDTH / 2.).to_array(),
                                (front + along * WIDTH / 2. + outside * DEPTH).to_array(),
                                (front - along * WIDTH / 2. + outside * DEPTH).to_array(),
                            ],
                            point.z,
                        ));
                    }
                    station += SPACING;
                }
                traveled += length;
            }
        }
        result
    }
}
fn corners(a: &CrossSection, b: &CrossSection) -> Option<[Vec3; 4]> {
    let vertex = |v: &opencity::road_geometry::Vertex| Vec3::new(v.point.x, v.point.y, v.height);
    Some([
        vertex(a.boundaries.first()?),
        vertex(b.boundaries.first()?),
        vertex(b.boundaries.last()?),
        vertex(a.boundaries.last()?),
    ])
}
impl Document {
    pub(super) fn candidates(&mut self, city: &City) -> Vec<LandscapeFeature> {
        let key = CandidateKey {
            city: city as *const City as usize,
            center: self.center.to_array().map(f32::to_bits),
            span: self.span.to_bits(),
            mode: self.mode,
            revision: self.revision,
        };
        if let Some(cached) = &self.candidate_cache {
            if cached.key == key {
                return cached.features.clone();
            }
        }
        let origin = if self.mode == CameraMode::Build {
            self.center.truncate()
        } else {
            self.player.truncate()
        };
        let reach = if self.mode == CameraMode::Build {
            self.span * 0.75
        } else {
            0.5
        };
        let mut suggestions = self.roads.suggestions(origin, reach);
        suggestions.sort_by(|a, b| {
            let midpoint =
                |p: &Vec<[f32; 2]>| p.iter().map(|v| Vec2::from_array(*v)).sum::<Vec2>() / 4.;
            midpoint(&a.0)
                .distance_squared(origin)
                .total_cmp(&midpoint(&b.0).distance_squared(origin))
        });
        let mut result: Vec<LandscapeFeature> = Vec::new();
        for (points, road_height) in suggestions {
            let c = points.iter().map(|v| Vec2::from_array(*v)).sum::<Vec2>() / 4.;
            let ground = self.ground_height(city, c);
            if (ground - road_height).abs() > 0.05 {
                continue;
            }
            let feature = super::input::blank(points, ground);
            if self.validate(city, &feature, None).is_ok()
                && !result
                    .iter()
                    .any(|other| geometry::overlap(&feature.points, &other.points))
            {
                result.push(feature);
                if result.len() == 8 {
                    break;
                }
            }
        }
        self.candidate_cache = Some(CandidateCache {
            key,
            features: result.clone(),
        });
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagonal_frontages_follow_the_actual_outer_edges_and_setback() {
        let program = opencity::parse(
            "road avenue: sidewalk sidewalk carlane carlane carlane sidewalk; end
             avenue(0,0,1.2,1.6);",
        )
        .unwrap();
        let roads = Roads::new(&program);
        let direction = Vec2::new(0.6, 0.8);
        let lots = roads.suggestions(Vec2::new(0.6, 0.8), 2.);
        assert!(lots.len() > 10);
        for (points, elevation) in lots {
            let front = Vec2::from_array(points[1]) - Vec2::from_array(points[0]);
            assert!(front.perp_dot(direction).abs() < 1e-6);
            let midpoint = (Vec2::from_array(points[0]) + Vec2::from_array(points[1])) / 2.;
            let nearest_edge = roads
                .edges
                .iter()
                .map(|e| {
                    (midpoint - e.points[0].position.truncate())
                        .perp_dot(direction)
                        .abs()
                })
                .fold(f32::INFINITY, f32::min);
            assert!((nearest_edge - SETBACK).abs() < 1e-6);
            assert!(!roads.overlaps(&super::super::input::blank(points, elevation)));
            assert!(midpoint.dot(direction) >= WIDTH / 2. + SETBACK - 1e-5);
            assert!(midpoint.dot(direction) <= 2. - WIDTH / 2. - SETBACK + 1e-5);
        }
    }

    #[test]
    fn curved_frontages_turn_with_the_road_and_reject_inside_bend_collisions() {
        let program = opencity::parse(
            "road avenue: sidewalk carlane carlane sidewalk; end
             avenue(0,0,0.2,0.8,0.8,0.2,1,1);",
        )
        .unwrap();
        let roads = Roads::new(&program);
        let lots: Vec<_> = roads
            .suggestions(Vec2::splat(0.5), 2.)
            .into_iter()
            .filter(|(p, z)| !roads.overlaps(&super::super::input::blank(p.clone(), *z)))
            .collect();
        assert!(lots.len() > 5);
        let angles: Vec<_> = lots
            .iter()
            .map(|(p, _)| {
                let front = Vec2::from_array(p[1]) - Vec2::from_array(p[0]);
                front.y.atan2(front.x).rem_euclid(std::f32::consts::PI)
            })
            .collect();
        let min = angles.iter().copied().fold(f32::INFINITY, f32::min);
        let max = angles.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max - min > 0.3,
            "frontages did not follow the bend: {angles:?}"
        );
    }
}
