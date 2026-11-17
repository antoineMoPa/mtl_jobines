//! Bounded cyclist population sharing physical vehicles and riding the directed lane graph.
use super::{
    catalog, cycling_pose,
    pedestrian_catalog::{self, JOINT_LIMIT},
    Controls, SpawnPoint, Vehicle, METERS_PER_UNIT,
};
use crate::hermes::{
    traffic_paths::{SpeedClass, TrafficPaths},
    Hermes,
};
use glam::{Mat4, Vec3};
pub(super) const CAPACITY: usize = 12;
struct Cyclist {
    vehicle: usize,
    path: usize,
    next: Option<usize>,
    model: usize,
}
pub(super) struct Cyclists {
    graph: TrafficPaths,
    dedicated: Vec<bool>,
    riders: Vec<Cyclist>,
    poses: Vec<(usize, usize, [Mat4; JOINT_LIMIT])>,
    serial: usize,
}
impl Cyclists {
    pub fn new(hermes: &Hermes) -> Self {
        let (graph, dedicated) = TrafficPaths::for_bicycles(hermes.city());
        Self {
            graph,
            dedicated,
            riders: Vec::new(),
            poses: Vec::new(),
            serial: 0,
        }
    }
    pub fn release(&mut self, id: usize) {
        self.riders.retain(|r| r.vehicle != id);
    }
    fn choose(&self, path: usize, serial: usize) -> Option<usize> {
        let next = &self.graph.paths[path].next;
        let preferred: Vec<_> = next
            .iter()
            .copied()
            .filter(|&id| self.dedicated[id])
            .collect();
        let options = if preferred.is_empty() {
            next.as_slice()
        } else {
            preferred.as_slice()
        };
        (!options.is_empty()).then(|| options[serial % options.len()])
    }
    pub fn populate(&mut self, hermes: &Hermes, vehicles: &mut Vec<Vehicle>, center: Vec3) {
        if !self.dedicated.iter().any(|v| *v) {
            return;
        }
        let model = catalog::definitions()
            .iter()
            .position(|d| d.is_bicycle())
            .unwrap();
        let mut candidates: Vec<_> = self
            .graph
            .paths
            .iter()
            .enumerate()
            .filter(|(i, p)| self.dedicated[*i] && p.speed != SpeedClass::Junction && p.length > 8.)
            .collect();
        candidates.sort_by(|a, b| {
            a.1.sample(a.1.project(center).0)
                .0
                .distance_squared(center)
                .total_cmp(&b.1.sample(b.1.project(center).0).0.distance_squared(center))
        });
        for (path, p) in candidates {
            for fraction in [0.25, 0.65] {
                if self.riders.len() >= CAPACITY {
                    return;
                }
                let (point, heading) = p.sample(p.length * fraction);
                if vehicles.iter().any(|v| {
                    v.position.truncate().distance(point.truncate()) < 8.
                        && (v.position.z - point.z).abs() < 3.
                }) {
                    continue;
                }
                let Ok(mut bike) = Vehicle::spawn_at(
                    hermes.city(),
                    SpawnPoint {
                        point: (point.truncate() / METERS_PER_UNIT).to_array(),
                        heading: Some(heading.y.atan2(heading.x)),
                    },
                ) else {
                    continue;
                };
                bike.set_catalog_model(Some(model));
                let id = vehicles.len();
                vehicles.push(bike);
                self.riders.push(Cyclist {
                    vehicle: id,
                    path,
                    next: None,
                    model: self.serial % pedestrian_catalog::CROWD_MODEL_COUNT,
                });
                self.serial += 1;
            }
        }
    }
    pub fn controls(
        &mut self,
        traffic: &super::traffic::Traffic,
        vehicles: &[Vehicle],
        _center: Vec3,
        player: usize,
    ) -> Vec<(usize, Controls)> {
        let mut result = Vec::new();
        // Choose transitions before mutably visiting riders.
        for i in 0..self.riders.len() {
            if self.riders[i].next.is_none() {
                self.riders[i].next = self.choose(self.riders[i].path, self.serial + i);
            }
        }
        for r in &mut self.riders {
            if r.vehicle == player {
                continue;
            }
            let bike = &vehicles[r.vehicle];
            let path = &self.graph.paths[r.path];
            let (distance, _error) = path.project(bike.position - Vec3::Z * bike.ride_height());
            let end = path.sample(path.length).0;
            if path.length - distance < 1.5
                && bike.position.truncate().distance(end.truncate()) < 3.
            {
                if let Some(next) = r.next.take() {
                    r.path = next;
                }
            }
            let path = &self.graph.paths[r.path];
            let distance = path.project(bike.position - Vec3::Z * bike.ride_height()).0;
            let speed = bike.velocity.truncate().length();
            let look = 2.5 + speed * 0.6;
            let target = if distance + look > path.length {
                r.next
                    .map(|id| self.graph.paths[id].sample(distance + look - path.length).0)
                    .unwrap_or_else(|| path.sample(path.length).0)
            } else {
                path.sample(distance + look).0
            };
            let delta = target - bike.position;
            let angle = (delta.y.atan2(delta.x) - bike.yaw)
                .sin()
                .atan2((delta.y.atan2(delta.x) - bike.yaw).cos());
            let steering = (angle * 1.8).clamp(-0.8, 0.8);
            let forward = Vec3::new(bike.yaw.cos(), bike.yaw.sin(), 0.);
            let obstacle = vehicles.iter().enumerate().any(|(id, v)| {
                if id == r.vehicle || (v.position.z - bike.position.z).abs() > 2. {
                    return false;
                }
                let delta = v.position - bike.position;
                let ahead = delta.dot(forward);
                ahead > 0.
                    && ahead < 3. + speed * 0.8
                    && (delta - forward * ahead).truncate().length() < v.width() * 0.5 + 0.55
            });
            let red =
                path.length - distance < 4. + speed && traffic.bicycle_red_light(end, forward);
            let blocked = obstacle
                || red
                || (r.next.is_none() && path.next.is_empty() && path.length - distance < 2.);
            let desired = if path.speed == SpeedClass::Junction {
                3.
            } else {
                5.
            };
            result.push((
                r.vehicle,
                Controls {
                    throttle: if blocked || speed > desired { 0. } else { 0.8 },
                    brake: blocked || speed > desired + 0.5,
                    steering,
                    handbrake: false,
                    boost: false,
                },
            ));
        }
        result
    }
    pub fn update_poses(
        &mut self,
        vehicles: &[Vehicle],
        player: usize,
        driving: bool,
        season: crate::season::Season,
    ) {
        self.poses.clear();
        if driving && vehicles[player].is_bicycle() {
            let model = pedestrian_catalog::player_model(season);
            self.poses.push((
                player,
                model,
                cycling_pose::pose(&pedestrian_catalog::definitions()[model], &vehicles[player]),
            ));
        }
        for r in &self.riders {
            if r.vehicle != player && vehicles[r.vehicle].is_bicycle() {
                self.poses.push((
                    r.vehicle,
                    r.model,
                    cycling_pose::pose(
                        &pedestrian_catalog::definitions()[r.model],
                        &vehicles[r.vehicle],
                    ),
                ));
            }
        }
    }
    pub fn instances<'a>(
        &'a self,
        vehicles: &'a [Vehicle],
        center: Vec3,
    ) -> impl Iterator<Item = super::crowd::Instance<'a>> {
        self.poses
            .iter()
            .filter(move |(id, _, _)| {
                vehicles[*id].is_bicycle() && vehicles[*id].position.distance(center) < 180.
            })
            .map(move |(id, model, pose)| {
                let bike = &vehicles[*id];
                let axes =
                    Mat4::from_cols(glam::Vec4::Y, glam::Vec4::Z, glam::Vec4::X, glam::Vec4::W);
                super::crowd::Instance {
                    model: *model,
                    transform: Mat4::from_scale_rotation_translation(
                        Vec3::splat(1. / METERS_PER_UNIT),
                        bike.orientation(),
                        bike.position / METERS_PER_UNIT,
                    ) * Mat4::from_translation(-Vec3::Z * bike.ride_height())
                        * axes,
                    opacity: 1.,
                    idle_time: 0.,
                    walk_time: 0.,
                    movement: 0.,
                    pose: Some(pose),
                }
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MAP: &str="road cycle: sidewalk bikelane carlane middle carlane bikelane sidewalk; end road street: sidewalk carlane middle carlane sidewalk; end a=map_point(-1,0); b=map_point(0,0); c=map_point(1,0); first=cycle(a,-0.67,0,-0.33,0,b); second=street(b,0.33,0,0.67,0,c); land(-2,-1,2,-1,2,1,-2,1); player_start(-0.9,0);";
    #[test]
    fn bicycles_route_from_dedicated_lane_to_regular_street_without_teleporting() {
        let hermes = Hermes::test_map_from_source(MAP);
        let (graph, bike) = TrafficPaths::for_bicycles(hermes.city());
        let first = graph
            .paths
            .iter()
            .enumerate()
            .find(|(i, p)| bike[*i] && p.speed != SpeedClass::Junction && p.sample(0.).1.x > 0.9)
            .unwrap()
            .0;
        assert!(!graph.paths[first].next.is_empty());
        assert!(graph.paths[first]
            .next
            .iter()
            .any(|&connector| graph.paths[connector]
                .next
                .iter()
                .any(|&target| !bike[target])));
        let mut world = super::super::DynamicWorld::new(&hermes);
        assert!(!world.cyclists.riders.is_empty());
        let id = world
            .cyclists
            .riders
            .iter()
            .find(|r| r.path == first)
            .unwrap()
            .vehicle;
        let before = world.vehicles[id].position;
        for _ in 0..2400 {
            world.step(&hermes, Controls::default(), Default::default(), 1. / 60.);
        }
        let after = world.vehicles[id].position;
        assert!(
            after.x > 10.,
            "cyclist must reach regular street: {before:?} -> {after:?}"
        );
        assert!(world.cyclists.riders.len() <= CAPACITY);
        assert!(
            world
                .pedestrian_instances()
                .filter(|i| i.pose.is_some())
                .count()
                > 1
        );
    }
}
