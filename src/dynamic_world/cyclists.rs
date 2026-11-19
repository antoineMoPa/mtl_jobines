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
pub(super) const CAPACITY: usize = 24;
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
    free: Vec<usize>,
    spawns: Vec<(usize, SpawnPoint)>,
    refill: f32,
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
            free: Vec::new(),
            spawns: Vec::new(),
            refill: 0.,
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
        let mut candidates = Vec::new();
        for (path, p) in self.graph.paths.iter().enumerate() {
            if !self.dedicated[path] || p.speed == SpeedClass::Junction || p.length < 24. {
                continue;
            }
            let mut distance = 12.;
            while distance < p.length - 12. {
                let (point, heading) = p.sample(distance);
                if point.distance(center) < 400. {
                    candidates.push((path, point, heading));
                }
                distance += 32.;
            }
        }
        candidates.sort_by(|a, b| {
            a.1.distance_squared(center)
                .total_cmp(&b.1.distance_squared(center))
        });
        for (path, point, heading) in candidates {
            if self.riders.len() >= CAPACITY {
                break;
            }
            if vehicles.iter().any(|v| {
                v.position.truncate().distance(point.truncate()) < 10.
                    && (v.position.z - point.z).abs() < 3.
            }) {
                continue;
            }
            let start = SpawnPoint {
                point: (point.truncate() / METERS_PER_UNIT).to_array(),
                heading: Some(heading.y.atan2(heading.x)),
            };
            let Ok(mut bike) = Vehicle::spawn_at(hermes.city(), start) else {
                continue;
            };
            bike.set_catalog_model(Some(model));
            let id = if let Some(id) = self.free.pop() {
                vehicles[id] = bike;
                id
            } else {
                let id = vehicles.len();
                vehicles.push(bike);
                id
            };
            self.spawns.push((id, start));
            self.riders.push(Cyclist {
                vehicle: id,
                path,
                next: None,
                model: self.serial % pedestrian_catalog::CROWD_MODEL_COUNT,
            });
            self.serial += 1;
        }
    }
    pub fn inactive(&self, id: usize) -> bool {
        self.free.contains(&id)
    }
    pub fn take_spawns(&mut self) -> Vec<(usize, SpawnPoint)> {
        std::mem::take(&mut self.spawns)
    }
    pub fn refresh(&mut self, hermes: &Hermes, vehicles: &mut Vec<Vehicle>, center: Vec3, dt: f32) {
        self.refill -= dt;
        if self.refill > 0. {
            return;
        }
        self.refill = 2.;
        self.riders.retain(|r| {
            if vehicles[r.vehicle].position.distance(center) < 550. {
                true
            } else {
                self.free.push(r.vehicle);
                vehicles[r.vehicle].position.z = -1000.;
                vehicles[r.vehicle].velocity = Vec3::ZERO;
                false
            }
        });
        self.populate(hermes, vehicles, center);
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
            let red = path.speed != SpeedClass::Junction
                && path.length - distance < 4. + speed
                && traffic.bicycle_red_light(path.sample(path.length).0, forward);
            let blocked = obstacle
                || red
                || (r.next.is_none() && path.next.is_empty() && path.length - distance < 2.);
            let desired = if path.speed == SpeedClass::Junction {
                2.5
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
                    ..Controls::default()
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

#[cfg(test)]
mod intersection_tests {
    use super::*;
    #[test]
    fn cyclists_clear_a_signalized_four_way_junction_instead_of_stopping_inside_it() {
        let hermes=Hermes::test_map_from_source("road cycle: sidewalk bikelane carlane middle carlane bikelane sidewalk; end road street: sidewalk carlane middle carlane sidewalk; end a=map_point(-1,0); b=map_point(0,0); c=map_point(1,0); d=map_point(0,-1); e=map_point(0,1); west=cycle(a,-0.67,0,-0.33,0,b); east=street(b,0.33,0,0.67,0,c); south=street(d,0,-0.67,0,-0.33,b); north=street(b,0,0.33,0,0.67,e); intersection4(b,street); land(-2,-2,2,-2,2,2,-2,2);");
        let mut cyclists = Cyclists::new(&hermes);
        let mut vehicles = vec![Vehicle::spawn(hermes.city())];
        vehicles[0].position = Vec3::new(-150., -150., 1.);
        cyclists.populate(&hermes, &mut vehicles, Vec3::ZERO);
        assert!(cyclists.riders.len() >= 4);
        let mut traffic = super::super::traffic::Traffic::new(hermes.traffic_paths());
        for frame in 0..6000 {
            traffic.time = frame as f32 / 60.;
            for (id, input) in cyclists.controls(&traffic, &vehicles, Vec3::ZERO, 0) {
                vehicles[id].step(hermes.city(), input, 1. / 60.);
            }
            super::super::contacts::resolve_with_contacts(&mut vehicles);
        }
        for rider in &cyclists.riders {
            let bike = &vehicles[rider.vehicle];
            assert!(
                bike.position.truncate().length() > 20.,
                "cyclist {} stuck in intersection at {:?} on path {}",
                rider.vehicle,
                bike.position,
                rider.path
            );
        }
    }
    #[test]
    fn cyclists_spawn_along_lanes_and_relocate_to_the_players_new_neighborhood() {
        let hermes=Hermes::test_map_from_source("road r: sidewalk bikelane carlane middle carlane bikelane sidewalk; end first=r(-3,0,3,0); second=r(-3,10,3,10); land(-4,-2,4,-2,4,12,-4,12);");
        let mut cyclists = Cyclists::new(&hermes);
        let mut vehicles = vec![Vehicle::spawn(hermes.city())];
        vehicles[0].position = Vec3::new(-350., -100., 1.);
        cyclists.populate(&hermes, &mut vehicles, Vec3::ZERO);
        let slots = vehicles.len();
        assert!(cyclists.riders.len() > 12);
        for r in &cyclists.riders {
            let path = &cyclists.graph.paths[r.path];
            let along = path.project(vehicles[r.vehicle].position).0;
            assert!(along >= 11. && path.length - along >= 11.);
        }
        cyclists.refresh(&hermes, &mut vehicles, Vec3::new(0., 1000., 0.), 3.);
        assert!(cyclists.riders.len() > 12);
        assert_eq!(vehicles.len(), slots, "reuse retired cyclist slots");
        assert!(cyclists
            .riders
            .iter()
            .all(|r| vehicles[r.vehicle].position.y > 900.));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod report_78_tests {
    use super::*;
    #[test]
    fn report_78_cyclists_do_not_remain_stuck_at_junction_7() {
        let hermes = Hermes::load_with_game_data(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/maps/mtl/map.road"),
            None,
        )
        .unwrap();
        let junction = Vec3::new(163.88, 157.6, 10.2);
        let mut cycles = Cyclists::new(&hermes);
        let mut vehicles = vec![Vehicle::spawn(hermes.city())];
        vehicles[0].position = Vec3::new(-1000., -1000., 0.);
        cycles.populate(&hermes, &mut vehicles, junction);
        assert!(!cycles.riders.is_empty());
        let mut traffic = super::super::traffic::Traffic::new(hermes.traffic_paths());
        let mut stalled = vec![0.; vehicles.len()];
        for frame in 0..5400 {
            traffic.time = frame as f32 / 60.;
            for (id, input) in cycles.controls(&traffic, &vehicles, junction, 0) {
                vehicles[id].step(hermes.city(), input, 1. / 60.);
            }
            super::super::contacts::resolve_with_contacts(&mut vehicles);
            for r in &cycles.riders {
                let bike = &vehicles[r.vehicle];
                if bike.position.distance(junction) < 15. && bike.velocity.truncate().length() < 0.2
                {
                    stalled[r.vehicle] += 1. / 60.;
                } else {
                    stalled[r.vehicle] = 0.;
                }
                assert!(
                    stalled[r.vehicle] < 30.,
                    "report 78 rider {} stalled at {:?}, path {}, next {:?}",
                    r.vehicle,
                    bike.position,
                    r.path,
                    r.next
                );
            }
        }
    }
}
