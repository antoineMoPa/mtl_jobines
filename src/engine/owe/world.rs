//! Game-owned content, expressed in metres. OpenCity mesh coordinates use map units.
use glam::Vec3;
use opencity::runtime::{City, Mesh, Region, SurfaceKind};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

#[derive(Deserialize)]
pub struct Package {
    pub title: String,
    maps: HashMap<String, Map>,
    prefabs: HashMap<String, Vec<Part>>,
}
#[derive(Deserialize)]
struct Map {
    road: String,
    #[serde(default)]
    elevation: Vec<u8>,
    objects: BTreeMap<String, Placement>,
}
#[derive(Deserialize)]
struct Placement {
    prefab: String,
    position: [f32; 3],
    #[serde(default)]
    fixed: bool,
}
#[derive(Deserialize)]
struct Part {
    min: [f32; 3],
    max: [f32; 3],
    color: [f32; 3],
}
struct Object {
    prefab: String,
    position: Vec3,
    velocity: Vec3,
    fixed: bool,
}
pub struct World {
    pub package: Package,
    prepared: HashMap<String, Result<Rc<City>, String>>,
    pub city: Option<Rc<City>>,
    objects: BTreeMap<u32, Object>,
    names: HashMap<String, u32>,
    next_id: u32,
    pub focus: u32,
    pub message: String,
    pub time: f32,
    pub present: bool,
    pub steps_left: u32,
    pub pending_map: Option<String>,
    pub load_status: i32,
}

pub const OK: i32 = 0;
const NO_MAP: i32 = 1;
const NOT_FOUND: i32 = 2;
const UNKNOWN_PREFAB: i32 = 3;
const STALE: i32 = 4;
const READ_ONLY: i32 = 5;
pub const INVALID: i32 = 6;
pub const CLOSED: i32 = 7;
pub const DT: f32 = 1. / 60.;
pub const SCALE: f32 = crate::dynamic_world::METERS_PER_UNIT;

// Layout shared with sdk/owe/include/owe/abi.h. Contains no pointers.
#[repr(C)]
#[derive(bytemuck::Pod, bytemuck::Zeroable, Clone, Copy)]
pub struct Packet {
    pub op: u32,
    pub id: u32,
    pub xyz: [f32; 3],
    pub text: [u8; 160],
}

// Wire values are shared with owe_operation_v1 in the C header.
mod op {
    pub const LOAD: u32 = 1;
    pub const LOAD_STATUS: u32 = 2;
    pub const PRESENT: u32 = 3;
    pub const MESSAGE: u32 = 4;
    pub const FIND: u32 = 5;
    pub const SPAWN: u32 = 6;
    pub const POSITION: u32 = 7;
    pub const SET_POSITION: u32 = 8;
    pub const VELOCITY: u32 = 9;
    pub const DESTROY: u32 = 10;
    pub const FOLLOW: u32 = 11;
    pub const STEP: u32 = 12;
}

impl World {
    pub fn new(json: &str) -> Result<Self, String> {
        let mut package: Package = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for parts in package.prefabs.values() {
            if parts.is_empty()
                || parts.iter().any(|p| {
                    (0..3).any(|i| p.min[i] >= p.max[i] || !(0.0..=1.0).contains(&p.color[i]))
                })
            {
                return Err("invalid prefab dimensions or color".into());
            }
        }
        // Compile before starting the event loop. Map switches only install a prepared snapshot.
        let mut prepared = HashMap::new();
        for (name, map) in &mut package.maps {
            let source = std::mem::take(&mut map.road);
            let elevation = std::mem::take(&mut map.elevation);
            let result = opencity::parse(&source)
                .map_err(|e| e.to_string())
                .and_then(|program| {
                    City::from_program_with_elevation_loader(&program, |spec| {
                        opencity::heightmap::ElevationMap::from_png(spec.clone(), &elevation)
                    })
                })
                .map(Rc::new);
            prepared.insert(name.clone(), result);
        }
        Ok(Self {
            package,
            prepared,
            city: None,
            objects: BTreeMap::new(),
            names: HashMap::new(),
            next_id: 1,
            focus: 0,
            message: String::new(),
            time: 0.,
            present: false,
            steps_left: 0,
            pending_map: None,
            load_status: NO_MAP,
        })
    }

    pub fn load(&mut self, name: &str) -> Result<(), String> {
        let map = self
            .package
            .maps
            .get(name)
            .ok_or_else(|| format!("unknown map: {name}"))?;
        let city = self.prepared[name].clone()?;
        let mut objects = BTreeMap::new();
        let mut names = HashMap::new();
        let mut next = self.next_id;
        for (name, place) in &map.objects {
            if !self.package.prefabs.contains_key(&place.prefab) {
                return Err(format!("{name}: unknown prefab {}", place.prefab));
            }
            names.insert(name.clone(), next);
            objects.insert(
                next,
                Object {
                    prefab: place.prefab.clone(),
                    position: place.position.into(),
                    velocity: Vec3::ZERO,
                    fixed: place.fixed,
                },
            );
            next = next.checked_add(1).ok_or("entity IDs exhausted")?;
        }
        self.next_id = next;
        self.city = Some(city);
        self.objects = objects;
        self.names = names;
        self.focus = 0;
        Ok(())
    }

    pub fn call(&mut self, p: &mut Packet) -> i32 {
        let end = p.text.iter().position(|&c| c == 0).unwrap_or(p.text.len());
        let Ok(key) = std::str::from_utf8(&p.text[..end]) else {
            return INVALID;
        };
        let xyz = Vec3::from_array(p.xyz);
        if !xyz.is_finite() {
            return INVALID;
        }
        match p.op {
            op::LOAD => {
                self.pending_map = Some(key.to_owned());
                OK
            }
            op::LOAD_STATUS => self.load_status,
            op::PRESENT => {
                self.present = true;
                OK
            }
            op::MESSAGE => {
                self.message = key.to_owned();
                OK
            }
            _ if self.city.is_none() => NO_MAP,
            op::FIND => {
                let Some(&id) = self.names.get(key) else {
                    return NOT_FOUND;
                };
                p.id = id;
                OK
            }
            op::SPAWN => {
                if !self.package.prefabs.contains_key(key) {
                    return UNKNOWN_PREFAB;
                }
                let id = self.next_id;
                let Some(next) = id.checked_add(1) else {
                    return INVALID;
                };
                self.next_id = next;
                self.objects.insert(
                    id,
                    Object {
                        prefab: key.to_owned(),
                        position: xyz,
                        velocity: Vec3::ZERO,
                        fixed: false,
                    },
                );
                p.id = id;
                OK
            }
            op::POSITION..=op::FOLLOW => {
                let Some(obj) = self.objects.get_mut(&p.id) else {
                    return STALE;
                };
                match p.op {
                    op::POSITION => p.xyz = obj.position.to_array(),
                    op::FOLLOW => self.focus = p.id,
                    _ if obj.fixed => return READ_ONLY,
                    op::SET_POSITION => {
                        obj.position = xyz;
                        obj.velocity = Vec3::ZERO;
                    }
                    op::VELOCITY => obj.velocity = xyz,
                    op::DESTROY => {
                        self.objects.remove(&p.id);
                        self.names.retain(|_, id| *id != p.id);
                        if self.focus == p.id {
                            self.focus = 0
                        }
                    }
                    _ => unreachable!(),
                }
                OK
            }
            op::STEP => {
                if self.steps_left == 0 || p.xyz[0] != DT {
                    return INVALID;
                }
                self.steps_left -= 1;
                for obj in self.objects.values_mut() {
                    obj.position += obj.velocity * DT;
                }
                self.time += DT;
                OK
            }
            _ => INVALID,
        }
    }

    pub fn target(&self) -> Vec3 {
        self.objects
            .get(&self.focus)
            .map_or(Vec3::ZERO, |o| o.position / SCALE)
    }

    pub fn terrain(&self) -> Result<Mesh, String> {
        let p = self.target();
        self.city.as_ref().ok_or("no map")?.mesh(
            Region::Radius {
                center: [p.x, p.y],
                radius: 3.,
            },
            0.06,
        )
    }

    pub fn actors(&self) -> Mesh {
        let mut mesh = Mesh::default();
        for obj in self.objects.values() {
            let base = obj.position;
            for part in &self.package.prefabs[&obj.prefab] {
                mesh.cuboid(
                    ((base + Vec3::from(part.min)) / SCALE).into(),
                    ((base + Vec3::from(part.max)) / SCALE).into(),
                    part.color,
                    SurfaceKind::Prop,
                );
            }
        }
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;
    fn command(w: &mut World, op: u32, id: u32, pos: [f32; 3], key: &str) -> (i32, Packet) {
        let mut p = Packet {
            op,
            id,
            xyz: pos,
            ..Packet::zeroed()
        };
        p.text[..key.len()].copy_from_slice(key.as_bytes());
        let result = w.call(&mut p);
        (result, p)
    }
    #[test]
    fn pack_controls_content_and_handles_across_map_changes() {
        let text = include_str!("../../../examples/signal-run/game.json");
        let mut pack: serde_json::Value = serde_json::from_str(text).unwrap();
        pack["maps"]["junction"]["road"] =
            include_str!("../../../opencity/examples/default_intersection.road").into();
        pack["maps"]["bad"] = pack["maps"]["junction"].clone();
        pack["maps"]["bad"]["objects"]["lin"]["prefab"] = "missing".into();
        let mut world = World::new(&pack.to_string()).unwrap();
        let zero = [0.; 3];
        assert_eq!(
            command(&mut world, op::FIND, 0, zero, "line_cart").0,
            NO_MAP
        );
        world.load("junction").unwrap();
        let (status, cart) = command(&mut world, op::FIND, 0, zero, "line_cart");
        assert_eq!(status, OK);
        assert_eq!(
            command(&mut world, op::SPAWN, 0, zero, "unknown").0,
            UNKNOWN_PREFAB
        );
        let (_, lamp) = command(&mut world, op::SPAWN, 0, zero, "signal_lamp");
        assert_eq!(
            command(&mut world, op::VELOCITY, lamp.id, [6., 0., 0.], "").0,
            OK
        );
        world.steps_left = 1;
        assert_eq!(command(&mut world, op::STEP, 0, [DT, 0., 0.], "").0, OK);
        assert_eq!(
            command(&mut world, op::STEP, 0, [DT, 0., 0.], "").0,
            INVALID
        );
        let (_, p) = command(&mut world, op::POSITION, lamp.id, zero, "");
        assert!((p.xyz[0] - 6. * DT).abs() < 1e-6);
        assert_eq!(command(&mut world, op::DESTROY, lamp.id, zero, "").0, OK);
        assert_eq!(
            command(&mut world, op::POSITION, lamp.id, zero, "").0,
            STALE
        );
        let (_, tower) = command(&mut world, op::FIND, 0, zero, "east_relay");
        assert_eq!(
            command(&mut world, op::SET_POSITION, tower.id, zero, "").0,
            READ_ONLY
        );
        assert!(world.load("bad").is_err());
        assert_eq!(command(&mut world, op::POSITION, cart.id, zero, "").0, OK);
        world.load("junction").unwrap();
        assert_eq!(
            command(&mut world, op::POSITION, cart.id, zero, "").0,
            STALE
        );
    }
}
