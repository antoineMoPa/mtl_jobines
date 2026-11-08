//! Lane-end handles belong to the current immutable map snapshot.
use crate::dynamic_world::METERS_PER_UNIT;
use glam::{Mat4, Vec3};
use opencity::{
    runtime::{City, Mesh, SurfaceKind},
    RoadElementKind,
};
use std::collections::BTreeSet;

const HALF: f32 = 1. / METERS_PER_UNIT;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum End {
    Start,
    Finish,
}
impl End {
    pub fn t(self) -> f32 {
        match self {
            Self::Start => 0.,
            Self::Finish => 1.,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn sign(self) -> f32 {
        match self {
            Self::Start => -1.,
            Self::Finish => 1.,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct LaneEnd {
    pub road: usize,
    pub end: End,
    pub lane: usize,
}
#[derive(Clone)]
pub(super) struct Handle {
    pub id: LaneEnd,
    /// Bottom centre of the two-metre cube, in map units.
    pub position: Vec3,
}
pub(super) struct RoadEnds {
    pub handles: Vec<Handle>,
    pub selected: BTreeSet<LaneEnd>,
    pub revision: u64,
}
impl RoadEnds {
    pub fn new(city: &City) -> Self {
        let mut handles = Vec::new();
        for (road, r) in city.roads.iter().enumerate() {
            for end in [End::Start, End::Finish] {
                for (lane, element) in r.definition.elements.iter().enumerate() {
                    if !matches!(
                        element.kind,
                        RoadElementKind::CarLane
                            | RoadElementKind::BusLane
                            | RoadElementKind::BikeLane
                            | RoadElementKind::ParkingLane
                    ) {
                        continue;
                    }
                    if let Some(p) =
                        opencity::map::element_position(&r.segment, &r.definition, lane, end.t())
                    {
                        handles.push(Handle {
                            id: LaneEnd { road, end, lane },
                            position: Vec3::new(p.x, p.y, r.segment.height_at(end.t()) + 0.0004),
                        });
                    }
                }
            }
        }
        Self {
            handles,
            selected: BTreeSet::new(),
            revision: 0,
        }
    }
    pub fn toggle(&mut self, id: LaneEnd) {
        if !self.selected.remove(&id) {
            self.selected.insert(id);
        }
        self.revision += 1;
    }
    pub fn clear(&mut self) {
        self.selected.clear();
        self.revision += 1;
    }
    pub fn mesh(&self, reveal: bool) -> Mesh {
        let mut mesh = Mesh::default();
        for h in &self.handles {
            let selected = self.selected.contains(&h.id);
            if !reveal && !selected {
                continue;
            }
            let (min, max) = h.bounds();
            mesh.cuboid(
                min.to_array(),
                max.to_array(),
                if selected {
                    [1., 0.58, 0.03]
                } else {
                    [0.02, 0.65, 1.]
                },
                SurfaceKind::Light,
            );
        }
        mesh
    }
    pub fn pick(&self, view: Mat4, size: [f32; 2], cursor: [f32; 2]) -> Option<LaneEnd> {
        if size.iter().any(|v| *v <= 0.) {
            return None;
        }
        let ndc = glam::Vec2::new(cursor[0] / size[0] * 2. - 1., 1. - cursor[1] / size[1] * 2.);
        let inverse = view.inverse();
        let origin = inverse.project_point3(ndc.extend(0.));
        let delta = inverse.project_point3(ndc.extend(1.)) - origin;
        self.handles
            .iter()
            .filter_map(|h| {
                let (min, max) = h.bounds();
                ray_box(origin, delta, min, max).map(|t| (h.id, t))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
}
impl Handle {
    fn bounds(&self) -> (Vec3, Vec3) {
        (
            self.position - Vec3::new(HALF, HALF, 0.),
            self.position + Vec3::new(HALF, HALF, 2. * HALF),
        )
    }
}
fn ray_box(origin: Vec3, delta: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let (mut lo, mut hi) = (0_f32, 1_f32);
    for axis in 0..3 {
        if delta[axis].abs() < 1e-9 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
        } else {
            let a = (min[axis] - origin[axis]) / delta[axis];
            let b = (max[axis] - origin[axis]) / delta[axis];
            lo = lo.max(a.min(b));
            hi = hi.min(a.max(b));
            if lo > hi {
                return None;
            }
        }
    }
    Some(lo)
}

#[cfg(test)]
mod tests;
