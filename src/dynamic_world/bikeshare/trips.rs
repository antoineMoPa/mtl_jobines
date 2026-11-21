use super::*;
use crate::dynamic_world::{
    bikeshare::{Dock, Station},
    crowd::Instance,
    landmark_respawn::vehicles_overlap,
};
use crate::hermes::external_models::decode_street_prop;
use glam::Vec2;
use opencity::runtime::Mesh;

impl Cyclists {
    pub(in crate::dynamic_world) fn install_stations(
        &mut self,
        hermes: &Hermes,
        vehicles: &mut Vec<Vehicle>,
    ) -> Result<(), String> {
        if hermes.bikeshare_sites.is_empty() {
            return Ok(());
        }
        let dock_model =
            decode_street_prop(include_bytes!("../../../data/props/bikeshare-dock.glb"))?;
        let kiosk_model = decode_street_prop(include_bytes!(
            "../../../data/props/bikeshare-dock-kiosk.glb"
        ))?;
        let model = catalog::definitions()
            .iter()
            .position(|d| d.is_bicycle())
            .expect("bundled shared bicycle");
        for (i, site) in hermes.bikeshare_sites.iter().enumerate() {
            let (path, along, _) = self
                .graph
                .paths
                .iter()
                .enumerate()
                .filter(|(_, p)| p.speed != SpeedClass::Junction && p.near(site.center, 16.))
                .map(|(id, p)| {
                    let (along, error) = p.project(site.center);
                    (id, along, error)
                })
                .min_by(|a, b| a.2.total_cmp(&b.2))
                .ok_or("Bikeshare station needs a nearby cycling route")?;
            let lane = self.graph.paths[path].sample(along).0;
            let yaw = site.forward.y.atan2(site.forward.x) + std::f32::consts::FRAC_PI_2;
            let mut docks = Vec::new();
            for slot in 0..3 {
                let point = site.center.truncate() + site.forward * (slot as f32 - 1.) * 1.65;
                let start = SpawnPoint {
                    point: (point / METERS_PER_UNIT).to_array(),
                    heading: Some(yaw),
                };
                let mut bike = Vehicle::spawn_at(hermes.city(), start)?;
                bike.set_catalog_model(Some(model));
                let ground = bike.position - Vec3::Z * bike.ride_height();
                let occupied = slot != 1 && !vehicles.iter().any(|v| vehicles_overlap(&bike, v));
                docks.push(Dock {
                    ground,
                    bike: occupied.then_some(vehicles.len()),
                });
                if occupied {
                    vehicles.push(bike);
                }
            }
            let mut station = Station {
                docks: docks.try_into().ok().expect("three docks"),
                path,
                lane,
                yaw,
                mesh: Mesh::default(),
                waiters: (i < CAPACITY)
                    .then_some((
                        i % pedestrian_catalog::CROWD_MODEL_COUNT,
                        6. + (i % 4) as f32,
                    ))
                    .into_iter()
                    .collect(),
            };
            station.place_dock_models(&dock_model, &kiosk_model);
            self.stations.push(station);
        }
        Ok(())
    }

    pub(in crate::dynamic_world) fn is_docked(&self, id: usize) -> bool {
        self.stations
            .iter()
            .any(|s| s.docks.iter().any(|d| d.bike == Some(id)))
    }
    pub(in crate::dynamic_world) fn undock(&mut self, id: usize) {
        for dock in self.stations.iter_mut().flat_map(|s| &mut s.docks) {
            if dock.bike == Some(id) {
                dock.bike = None;
                return;
            }
        }
    }

    pub(in crate::dynamic_world) fn anchor_docked_bikes(&self, vehicles: &mut [Vehicle]) {
        for station in &self.stations {
            for (slot, dock) in station.docks.iter().enumerate() {
                if let Some(id) = dock.bike {
                    station.align(slot, &mut vehicles[id]);
                }
            }
        }
    }

    pub(in crate::dynamic_world) fn visit_stations(
        &mut self,
        vehicles: &mut [Vehicle],
        center: Vec3,
        player: usize,
        dt: f32,
    ) {
        for (station_id, station) in self.stations.iter_mut().enumerate() {
            if station.center().distance_squared(center) > 180_f32.powi(2) {
                continue;
            }
            if let Some((_, delay)) = station.waiters.front_mut() {
                *delay = (*delay - dt).max(0.);
            }
            if let Some(&(model, 0.)) = station.waiters.front() {
                // The last player vehicle stays available for reset and vehicle selection on foot.
                if let Some(dock) = station
                    .docks
                    .iter_mut()
                    .find(|d| d.bike.is_some_and(|id| id != player))
                {
                    let id = dock.bike.unwrap();
                    let path = &self.graph.paths[station.path];
                    let (exit, heading) = path.sample(path.project(dock.ground).0 + 1.5);
                    let seat = exit + Vec3::Z * vehicles[id].ride_height();
                    // Mount facing downstream when there is a gap to merge into the cycling lane.
                    if !vehicles
                        .iter()
                        .enumerate()
                        .any(|(other, v)| other != id && v.entry_distance(seat) < 1.8)
                    {
                        dock.bike = None;
                        vehicles[id].position = seat;
                        vehicles[id].yaw = heading.y.atan2(heading.x);
                        self.riders.push(Cyclist {
                            vehicle: id,
                            model,
                            path: station.path,
                            next: None,
                            checkout_station: Some(station_id),
                        });
                        station.waiters.pop_front();
                    }
                }
            }
            let Some((index, slot)) = self.riders.iter().enumerate().find_map(|(index, r)| {
                let position = vehicles[r.vehicle].position;
                if r.path != station.path
                    || r.checkout_station == Some(station_id)
                    || position.distance_squared(station.lane) > 100.
                {
                    return None;
                }
                let slot = station.open_dock(vehicles, r.vehicle)?;
                (position.distance(station.docks[slot].ground) < 1.7).then_some((index, slot))
            }) else {
                continue;
            };
            let rider = self.riders.swap_remove(index);
            station.align(slot, &mut vehicles[rider.vehicle]);
            station.docks[slot].bike = Some(rider.vehicle);
            station.waiters.push_back((rider.model, 8.));
        }
    }

    pub(in crate::dynamic_world) fn waiting_instances(
        &self,
        center: Vec3,
    ) -> impl Iterator<Item = Instance<'_>> {
        self.stations
            .iter()
            .filter(move |s| s.center().distance_squared(center) < 180_f32.powi(2))
            .flat_map(|s| {
                s.waiters
                    .iter()
                    .enumerate()
                    .map(move |(i, &(model, delay))| {
                        let forward = Vec2::from_angle(s.yaw);
                        let position = s.center()
                            + (-forward * 1.35 + forward.perp() * (i as f32 * 0.8)).extend(0.);
                        Instance {
                            zombie: false,
                            model,
                            transform: pedestrian_catalog::instance_transform(position, s.yaw),
                            opacity: 1.,
                            idle_time: delay,
                            walk_time: 0.,
                            movement: 0.,
                            pose: None,
                        }
                    })
            })
    }

    pub(in crate::dynamic_world) fn append_stations(&self, mesh: &mut Mesh, center: Vec3) {
        for station in &self.stations {
            if station.center().distance_squared(center) < 180_f32.powi(2) {
                mesh.vertices.extend_from_slice(&station.mesh.vertices);
            }
        }
    }
}
