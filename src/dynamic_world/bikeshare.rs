//! Map-authored sidewalk racks. Occupied docks refer to the same physical bikes used on the road.
use super::{DynamicWorld, Vehicle, METERS_PER_UNIT};
use glam::{Quat, Vec2, Vec3};
use opencity::{
    runtime::{City, Mesh},
    Argument, Program, RoadElementKind, Statement,
};

pub(crate) struct Site {
    pub center: Vec3,
    pub forward: Vec2,
}

impl Site {
    pub(crate) fn from_program(program: &Program, city: &City) -> Result<Vec<Self>, String> {
        let mut sites = Vec::new();
        for statement in &program.statements {
            let Statement::Call(call) = statement else {
                continue;
            };
            if call.name != "bikeshare_station" {
                continue;
            }
            let [Argument::Identifier(name)] = call.args.as_slice() else {
                return Err("bikeshare_station expects a road name".into());
            };
            let road = city
                .roads
                .iter()
                .find(|r| r.segment.name.as_ref() == Some(name))
                .ok_or_else(|| format!("Unknown bikeshare road {name}"))?;
            let sidewalk = road
                .definition
                .elements
                .iter()
                .rposition(|e| e.kind == RoadElementKind::Sidewalk)
                .ok_or_else(|| format!("Bikeshare road {name} needs a sidewalk"))?;
            let point =
                opencity::map::element_position(&road.segment, &road.definition, sidewalk, 0.5)
                    .ok_or_else(|| format!("Missing sidewalk position on {name}"))?;
            let tangent = road.segment.tangent_at(0.5);
            let heading = tangent.y.atan2(tangent.x);
            let center = Vec3::new(point.x, point.y, road.segment.height_at(0.5)) * METERS_PER_UNIT;
            sites.push(Self {
                center,
                forward: Vec2::from_angle(heading),
            });
        }
        Ok(sites)
    }
}

pub(super) struct Dock {
    pub ground: Vec3,
    pub bike: Option<usize>,
}

impl Dock {
    fn available(&self, vehicles: &[Vehicle], rider: usize) -> bool {
        self.bike.is_none()
            && !vehicles
                .iter()
                .enumerate()
                .any(|(id, v)| id != rider && v.entry_distance(self.ground + Vec3::Z * 0.5) < 0.60)
    }
}

pub(super) struct Station {
    pub docks: [Dock; 3],
    pub path: usize,
    pub lane: Vec3,
    pub yaw: f32,
    pub mesh: Mesh,
    // Pedestrian model and time left before checkout.
    pub waiters: std::collections::VecDeque<(usize, f32)>,
}

impl Station {
    pub fn free_dock(&self) -> Option<usize> {
        self.docks.iter().position(|d| d.bike.is_none())
    }

    pub fn open_dock(&self, vehicles: &[Vehicle], rider: usize) -> Option<usize> {
        self.docks
            .iter()
            .position(|dock| dock.available(vehicles, rider))
    }

    pub fn center(&self) -> Vec3 {
        self.docks[1].ground
    }

    pub fn align(&self, slot: usize, bike: &mut Vehicle) {
        bike.position = self.docks[slot].ground + Vec3::Z * bike.ride_height();
        bike.yaw = self.yaw;
        bike.pitch = 0.;
        bike.roll = 0.;
        bike.velocity = Vec3::ZERO;
        bike.angular_momentum = Vec3::ZERO;
        bike.body_lean = Vec2::ZERO;
        bike.body_lean_rate = Vec2::ZERO;
        bike.steering = 0.;
        bike.sound_running = false;
        bike.grounded = true;
    }

    pub fn parked_pose(&self, slot: usize, bike: &Vehicle) -> Vehicle {
        let mut parked = bike.clone();
        self.align(slot, &mut parked);
        parked
    }

    pub fn place_dock_models(&mut self, dock_model: &Mesh, kiosk_model: &Mesh) {
        let rotation = Quat::from_rotation_z(self.yaw);
        for (slot, dock) in self.docks.iter().enumerate() {
            let model = if slot == 0 { kiosk_model } else { dock_model };
            self.mesh
                .vertices
                .extend(model.vertices.iter().copied().map(|mut vertex| {
                    vertex.position = ((dock.ground
                        + rotation * Vec3::from_array(vertex.position))
                        / METERS_PER_UNIT)
                        .to_array();
                    vertex.normal = (rotation * Vec3::from_array(vertex.normal)).to_array();
                    vertex
                }));
        }
    }
}

impl DynamicWorld {
    pub(super) fn bike_return_dock(&self) -> Option<(usize, usize)> {
        let bike = self.player();
        if !self.is_driving() || !bike.is_bicycle() || bike.velocity.length() > 3. {
            return None;
        }
        self.cyclists
            .stations
            .iter()
            .enumerate()
            .flat_map(|(index, s)| {
                s.docks.iter().enumerate().map(move |(slot, d)| {
                    (
                        (index, slot),
                        (d.ground + Vec3::Z * bike.ride_height()).distance(bike.position),
                    )
                })
            })
            .filter(|&((station, slot), dist)| {
                dist <= 3.
                    && self.cyclists.stations[station].docks[slot]
                        .available(&self.vehicles, self.player_vehicle.0)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    pub fn bikeshare_hint(&self) -> Option<&'static str> {
        if self.health == 0 || self.metro.aboard {
            return None;
        }
        if self.bike_return_dock().is_some() {
            return Some("Déposer le vélo");
        }
        if let Some(p) = &self.pedestrian {
            let id = self.nearest_enterable_vehicle(p.position)?;
            if self.cyclists.is_docked(id) {
                return Some("Emprunter le vélo");
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dynamic_world::Pedestrian, hermes::Hermes};
    use opencity::runtime::SurfaceKind;

    fn stations_map() -> Hermes {
        Hermes::test_map_from_source(
            "road street: sidewalk bikelane carlane middle carlane bikelane sidewalk; end
            a=map_point(-1,0); b=map_point(0,0); c=map_point(1,0);
            west=street(a,-0.67,0,-0.33,0,b); east=street(b,0.33,0,0.67,0,c);
            bikeshare_station(west); bikeshare_station(east);
            land(-2,-1,2,-1,2,1,-2,1); player_start(0,0.4,0);",
        )
    }

    #[test]
    fn player_can_borrow_and_return_the_same_bike_at_another_station() {
        let map = stations_map();
        let mut world = DynamicWorld::new(&map);
        let id = world.cyclists.stations[0].docks[0].bike.unwrap();
        let bike = &world.vehicles[id];
        world.pedestrian = Pedestrian::beside(map.city(), bike);
        assert_eq!(world.bikeshare_hint(), Some("Emprunter le vélo"));
        assert!(world.toggle_vehicle(&map));
        assert_eq!(world.player_vehicle.0, id);
        assert!(!world.cyclists.is_docked(id));
        let ground = world.cyclists.stations[1].docks[1].ground;
        world.vehicles[id].position = ground + Vec3::Z * world.player().ride_height();
        assert_eq!(world.bikeshare_hint(), Some("Déposer le vélo"));
        assert!(world.toggle_vehicle(&map));
        assert!(!world.is_driving());
        assert_eq!(world.cyclists.stations[1].docks[1].bike, Some(id));
        assert!(world.cyclists.stations[1]
            .docks
            .iter()
            .all(|d| d.bike.is_some()));
        world.reset_player(&map);
        assert!(!world.cyclists.is_docked(id));
    }

    #[test]
    fn npc_rides_a_checked_out_bike_to_the_next_station() {
        let map = stations_map();
        let mut world = DynamicWorld::new(&map);
        let bike = world.cyclists.stations[0].docks[0].bike.unwrap();
        for _ in 0..60 * 90 {
            world.step(&map, Default::default(), Default::default(), 1. / 60.);
            if world.cyclists.stations[1]
                .docks
                .iter()
                .any(|d| d.bike == Some(bike))
            {
                assert!(!world.cyclists.stations[1].waiters.is_empty());
                return;
            }
        }
        panic!(
            "Bike at {:?}, destination {:?}",
            world.vehicles[bike].position, world.cyclists.stations[1].lane
        );
    }

    #[test]
    fn montreal_stations_have_bikes_and_a_free_return_dock() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let map = Hermes::load_with_game_data(&path, None).unwrap();
        let world = DynamicWorld::new(&map);
        assert_eq!(world.cyclists.stations.len(), 8);
        for station in &world.cyclists.stations {
            assert_eq!(station.docks.iter().filter(|d| d.bike.is_some()).count(), 2);
            for dock in &station.docks {
                let point = (dock.ground.truncate() / METERS_PER_UNIT).to_array();
                assert_eq!(
                    map.city().surface_at(point, f32::INFINITY).unwrap().kind,
                    SurfaceKind::Sidewalk
                );
            }
        }
    }
}
