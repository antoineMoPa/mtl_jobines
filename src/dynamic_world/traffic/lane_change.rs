//! Lane changes on adjoining, same-direction car lanes of the same road.
use super::*;

#[derive(Default)]
pub(super) enum Maneuver {
    #[default]
    Following,
    Backing {
        until: f32,
    },
    Merging,
}

impl Driver {
    pub(super) fn alternate_lane(
        &self,
        graph: &TrafficPaths,
        ground: Vec3,
        vehicles: &[Vehicle],
    ) -> Option<(usize, f32)> {
        let car = &vehicles[self.vehicle];
        graph.paths[self.path].adjacent.iter().find_map(|&id| {
            let lane = &graph.paths[id];
            if lane.next.is_empty() {
                return None;
            }
            let (along, error) = lane.project(ground);
            let (point, forward) = lane.sample(along);
            if error > 6. || (point.z - ground.z).abs() > 1. || lane.length - along < 5. {
                return None;
            }
            let occupied = vehicles.iter().enumerate().any(|(id, other)| {
                let delta = other.position - point;
                let gap = (other.length() + car.length()) * 0.5 + 3.;
                id != self.vehicle
                    && delta.z.abs() < 2.5
                    && forward.truncate().perp_dot(delta.truncate()).abs()
                        < (other.width() + car.width()) * 0.5 + 0.3
                    && (-gap - other.velocity.length()..gap + car.velocity.length())
                        .contains(&delta.dot(forward))
            });
            (!occupied).then_some((id, along))
        })
    }

    pub(super) fn start_merge(&mut self, lane: usize, distance: f32) {
        self.path = lane;
        self.distance = distance;
        self.next = None;
        self.stopped = false;
        self.stranded = 0.;
        self.motion = lod::Motion::Physical;
        self.maneuver = Maneuver::Merging;
    }
}
