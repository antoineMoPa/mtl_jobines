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

impl World {
    pub fn new(json: &str) -> Result<Self, String> {
        let package: Package = serde_json::from_str(json).map_err(|e| e.to_string())?;
        for parts in package.prefabs.values() {
            if parts.is_empty()
                || parts.iter().any(|p| {
                    (0..3).any(|i| p.min[i] >= p.max[i] || !(0.0..=1.0).contains(&p.color[i]))
                })
            {
                return Err("invalid prefab dimensions or color".into());
            }
        }
        Ok(Self {
            package,
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
        let program = opencity::parse(&map.road).map_err(|e| e.to_string())?;
        let city = City::from_program_with_elevation_loader(&program, |spec| {
            opencity::heightmap::ElevationMap::from_png(spec.clone(), &map.elevation)
        })?;
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
        self.city = Some(Rc::new(city));
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
            1 => {
                self.pending_map = Some(key.to_owned());
                OK
            }
            2 => self.load_status,
            3 => {
                self.present = true;
                OK
            }
            4 => {
                self.message = key.to_owned();
                OK
            }
            _ if self.city.is_none() => NO_MAP,
            5 => {
                let Some(&id) = self.names.get(key) else {
                    return NOT_FOUND;
                };
                p.id = id;
                OK
            }
            6 => {
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
            7..=11 => {
                let Some(obj) = self.objects.get_mut(&p.id) else {
                    return STALE;
                };
                match p.op {
                    7 => p.xyz = obj.position.to_array(),
                    11 => self.focus = p.id,
                    _ if obj.fixed => return READ_ONLY,
                    8 => {
                        obj.position = xyz;
                        obj.velocity = Vec3::ZERO;
                    }
                    9 => obj.velocity = xyz,
                    10 => {
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
            12 => {
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
