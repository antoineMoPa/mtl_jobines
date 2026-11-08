//! Distance-sampled vehicle poses become smooth, tangent-connected lane curves.
use super::road_ends::{End, LaneEnd};
use glam::{Vec2, Vec3};
use opencity::{runtime::City, RoadDirection, RoadElementKind};
use std::collections::BTreeSet;

#[cfg(not(target_arch = "wasm32"))]
pub(super) mod file;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub(super) struct Pose {
    pub position: Vec3,
    pub yaw: f32,
}
#[derive(Clone, Copy)]
struct Knot {
    offset: Vec3,
    angle: f32,
}
struct Lane {
    id: LaneEnd,
    parent: Option<String>,
    kind: RoadElementKind,
    direction: RoadDirection,
    anchor: Vec3,
    lateral: Vec2,
    heading: f32,
}
pub(super) struct Drawing {
    lanes: Vec<Lane>,
    origin: Pose,
    knots: Vec<Knot>,
    pub revision: u64,
}
fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}
fn angle_delta(a: f32, b: f32) -> f32 {
    (b - a).sin().atan2((b - a).cos())
}
impl Drawing {
    pub fn new(city: &City, selection: &BTreeSet<LaneEnd>, origin: Pose) -> Self {
        let mut lanes = Vec::new();
        for &id in selection {
            let road = &city.roads[id.road];
            let group: Vec<_> = selection
                .iter()
                .filter(|s| s.road == id.road && s.end == id.end)
                .filter_map(|s| {
                    opencity::map::element_position(
                        &road.segment,
                        &road.definition,
                        s.lane,
                        s.end.t(),
                    )
                })
                .map(|p| Vec2::new(p.x, p.y))
                .collect();
            let center = group.iter().copied().sum::<Vec2>() / group.len() as f32;
            let p = opencity::map::element_position(
                &road.segment,
                &road.definition,
                id.lane,
                id.end.t(),
            )
            .unwrap();
            let tangent = road.segment.tangent_at(id.end.t());
            let heading = (tangent.y * id.end.sign()).atan2(tangent.x * id.end.sign());
            let direction = road
                .definition
                .lane_direction(id.lane)
                .unwrap_or(RoadDirection::Both);
            let direction = match (id.end, direction) {
                (End::Start, RoadDirection::Forward) => RoadDirection::Reverse,
                (End::Start, RoadDirection::Reverse) => RoadDirection::Forward,
                _ => direction,
            };
            lanes.push(Lane {
                id,
                parent: road.segment.name.clone(),
                kind: road.definition.elements[id.lane].kind,
                direction,
                anchor: center.extend(road.segment.height_at(id.end.t())),
                lateral: Vec2::new(p.x, p.y) - center,
                heading,
            });
        }
        Self {
            lanes,
            origin,
            knots: vec![Knot {
                offset: Vec3::ZERO,
                angle: 0.,
            }],
            revision: 0,
        }
    }
    pub fn sample(&mut self, pose: Pose) -> bool {
        let offset = pose.position - self.origin.position;
        if !offset.is_finite() || !pose.yaw.is_finite() {
            return false;
        }
        let last = self.knots.last().unwrap();
        let distance = offset.truncate().distance(last.offset.truncate());
        if distance < 0.02 {
            return false;
        }
        let relative_yaw = angle_delta(self.origin.yaw, pose.yaw);
        if distance < 0.05 && angle_delta(last.angle, relative_yaw).abs() < 0.14 {
            return false;
        }
        // A reset, teleport, or implausible frame jump must never draw a connecting highway.
        if distance > 0.5 {
            return false;
        }
        let weight = distance / (distance + 0.025);
        let angle = last.angle + angle_delta(last.angle, relative_yaw) * weight;
        let height = last.offset.z + (offset.z - last.offset.z) * weight;
        self.knots.push(Knot {
            offset: Vec3::new(offset.x, offset.y, height),
            angle,
        });
        self.revision += 1;
        true
    }
    pub fn length_meters(&self) -> f32 {
        self.knots
            .windows(2)
            .map(|k| k[0].offset.distance(k[1].offset) * 100.)
            .sum()
    }
    fn point(&self, lane: &Lane, knot: Knot) -> Vec3 {
        let movement = rotate(knot.offset.truncate(), lane.heading - self.origin.yaw);
        let lateral = rotate(lane.lateral, knot.angle);
        lane.anchor + (movement + lateral).extend(knot.offset.z)
    }
    /// Append only ordinary .road definitions/calls; the map remains the authoring source.
    pub fn source(&self, prefix: &str) -> Result<String, String> {
        use std::fmt::Write;
        let mut out = format!("\n# Road extension recorded while driving: {prefix}\n");
        for (index, lane) in self.lanes.iter().enumerate() {
            let parent = lane
                .parent
                .as_deref()
                .ok_or("Selected road needs a placement name")?;
            let name = format!("{prefix}_lane_{index}");
            writeln!(
                out,
                "road {name}: {}; direction {}; end",
                lane.kind.keyword(),
                lane.direction.keyword()
            )
            .unwrap();
            writeln!(
                out,
                "{name}_p0 = lane_point({parent}, {}, {});",
                lane.id.lane,
                lane.id.end.t()
            )
            .unwrap();
            for (segment, pair) in self.knots.windows(2).enumerate() {
                let a = self.point(lane, pair[0]);
                let b = self.point(lane, pair[1]);
                let distance = a.truncate().distance(b.truncate());
                if distance < 0.0001 {
                    continue;
                }
                let heading = |k: Knot| rotate(Vec2::X, lane.heading + k.angle);
                let c1 = a.truncate() + heading(pair[0]) * distance / 3.;
                let c2 = b.truncate() - heading(pair[1]) * distance / 3.;
                let next = segment + 1;
                writeln!(out, "{name}_p{next} = map_point({:.9}, {:.9});", b.x, b.y).unwrap();
                writeln!(
                    out,
                    "{name}_s{segment} = {name}({name}_p{segment}, {:.9}, {:.9}, \
                     {:.9}, {:.9}, {name}_p{next});",
                    c1.x, c1.y, c2.x, c2.y
                )
                .unwrap();
                writeln!(out, "grade({name}_s{segment}, {:.9}, {:.9});", a.z, b.z).unwrap();
            }
        }
        Ok(out)
    }
}
