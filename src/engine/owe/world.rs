//! Game-owned content, expressed in metres. OpenCity mesh coordinates use map units.

use crate::dynamic_world::ShotSurface as Material;
use crate::engine::impact::{Contact, Effects, PanelLayer as Part};
use glam::Vec3;
use opencity::runtime::{City, Mesh, Region, SurfaceKind};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

#[derive(Default, Deserialize)]
pub struct Package {
    pub title: String,
    #[serde(default)]
    pub skydome: super::skydome::Assets,
    #[serde(default)]
    view_models: HashMap<String, ViewAsset>,
    maps: HashMap<String, Map>,
    prefabs: HashMap<String, Vec<Part>>,
    #[serde(default)]
    materials: HashMap<String, Material>,
}
#[derive(Deserialize)]
struct ViewAsset {
    glb: Vec<u8>,
    length: f32,
}

#[derive(Default, Deserialize)]
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
struct Object {
    prefab: String,
    position: Vec3,
    velocity: Vec3,
    yaw: f32,
    fixed: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MessageLocation {
    Status,
    Corner,
    Center,
}

pub struct World {
    pub package: Package,
    prepared: HashMap<String, Result<Rc<City>, String>>,
    pub city: Option<Rc<City>>,
    objects: BTreeMap<u32, Object>,
    names: HashMap<String, u32>,
    next_id: u32,
    pub(super) effects: Effects,
    sound_serial: u64,
    sounds: [crate::audio::Impact; crate::audio::IMPACT_VOICES],
    road_loop: Option<RoadLoop>,
    pub focus: u32,
    pub first_person: Option<(glam::Quat, f32)>,
    pub look_delta: [f32; 2],
    pub view_models: HashMap<String, Mesh>,
    pub view_model: Option<(String, Vec3)>,
    pub message: String,
    pub message_location: MessageLocation,
    pub time: f32,
    pub present: bool,
    pub retro_lens: f32,
    pub skydome: super::skydome::Skydome,
    pub steps_left: u32,
    pub pending_map: Option<String>,
    pub load_status: i32,
}

pub const OK: i32 = 0;
const NO_MAP: i32 = 1;
pub(super) const NOT_FOUND: i32 = 2;
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
    pub const SET_FACING: u32 = 13;
    pub const RETRO_LENS: u32 = 14;
    pub const FIRST_PERSON: u32 = 15;
    pub const VIEW_MODEL: u32 = 16;
    pub const LOOK_DELTA: u32 = 17;
    pub const PICK_RETICLE: u32 = 18;
    pub const ROAD_LOOP: u32 = 19;
    pub const STRIKE_RETICLE: u32 = 20;
    pub const SHATTER_PANEL: u32 = 21;
    pub const MUZZLE_FLASH: u32 = 22;
    pub const SKYDOME_TEXTURE: u32 = 32;
    pub const SKYDOME_SHADER: u32 = 33;
}

impl Package {
    pub(super) fn define(
        &mut self,
        kind: u32,
        name: &str,
        owner: &str,
        prefab: &str,
        v: &[f32],
        bytes: &[u8],
    ) -> Result<(), String> {
        if name.is_empty() || v.len() != 9 || !v.iter().all(|n| n.is_finite()) {
            return Err("invalid content declaration".into());
        }
        let text = || {
            std::str::from_utf8(bytes)
                .map(str::to_owned)
                .map_err(|e| e.to_string())
        };
        let triple = |i: usize| [v[i], v[i + 1], v[i + 2]];
        match kind {
            1 => self.title = name.to_owned(),
            2 => {
                let map = self.maps.entry(name.into()).or_default();
                if !map.road.is_empty() {
                    map.road.push('\n');
                }
                map.road.push_str(&text()?);
            }
            3 => self.maps.entry(name.into()).or_default().elevation = bytes.into(),
            4 => self.prefabs.entry(name.into()).or_default().push(Part {
                min: triple(0),
                max: triple(3),
                color: triple(6),
            }),
            5 => {
                let placements = &mut self.maps.entry(owner.into()).or_default().objects;
                if placements.contains_key(name) {
                    return Err(format!("duplicate object {name}"));
                }
                placements.insert(
                    name.into(),
                    Placement {
                        prefab: prefab.into(),
                        position: triple(0),
                        fixed: v[3] != 0.,
                    },
                );
            }
            6 => {
                self.view_models.insert(
                    name.into(),
                    ViewAsset {
                        glb: bytes.into(),
                        length: v[0],
                    },
                );
            }
            7 => {
                self.skydome.textures.insert(name.into(), bytes.into());
            }
            8 => {
                self.skydome.shaders.insert(name.into(), text()?);
            }
            9 => {
                let material = match v[0] {
                    0. => Material::Wood,
                    1. => Material::Concrete,
                    _ => return Err("unknown impact material".into()),
                };
                self.materials.insert(name.into(), material);
            }
            _ => return Err(format!("unknown content declaration {kind}")),
        }
        Ok(())
    }
}
impl World {
    pub fn new(json: &str) -> Result<Self, String> {
        Self::from_content(serde_json::from_str(json).map_err(|e| e.to_string())?)
    }
    pub fn from_content(mut package: Package) -> Result<Self, String> {
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
        let mut view_models = HashMap::new();
        for (name, asset) in std::mem::take(&mut package.view_models) {
            if !asset.length.is_finite() || asset.length <= 0. {
                return Err(format!("invalid length for {name}"));
            }
            let mut mesh = crate::hermes::external_models::decode_equipment(&asset.glb)?;
            let (lo, hi) = crate::hermes::external_models::bounds(&mesh);
            for v in &mut mesh.vertices {
                v.position = ((Vec3::from(v.position) - (lo + hi) * 0.5)
                    * (asset.length / (hi.x - lo.x) / SCALE))
                    .into();
            }
            view_models.insert(name, mesh);
        }
        Ok(Self {
            view_models,
            first_person: None,
            look_delta: [0.; 2],
            view_model: None,
            package,
            prepared,
            city: None,
            objects: BTreeMap::new(),
            names: HashMap::new(),
            next_id: 1,
            effects: Effects::default(),
            sound_serial: 0,
            sounds: Default::default(),
            road_loop: None,
            focus: 0,
            message: String::new(),
            message_location: MessageLocation::Status,
            time: 0.,
            present: false,
            retro_lens: 0.,
            skydome: Default::default(),
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
                    yaw: 0.,
                    fixed: place.fixed,
                },
            );
            next = next.checked_add(1).ok_or("entity IDs exhausted")?;
        }
        self.next_id = next;
        self.effects = Effects::default();
        self.sounds = Default::default();
        self.road_loop = None;
        self.city = Some(city);
        self.objects = objects;
        self.names = names;
        self.focus = 0;
        self.first_person = None;
        self.view_model = None;
        self.look_delta = [0.; 2];
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
            op::SKYDOME_TEXTURE => self.skydome.textures.select(key),
            op::SKYDOME_SHADER => self.skydome.shaders.select(key),
            op::RETRO_LENS => {
                if !(0.0..=1.0).contains(&xyz.x) {
                    return INVALID;
                }
                self.retro_lens = xyz.x;
                OK
            }
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
                self.message_location = match p.id {
                    0 => MessageLocation::Status,
                    1 => MessageLocation::Corner,
                    2 => MessageLocation::Center,
                    _ => return INVALID,
                };
                self.message = key.to_owned();
                OK
            }
            _ if self.city.is_none() => NO_MAP,
            op::ROAD_LOOP => {
                if self.road_loop.as_ref().is_none_or(|r| r.name != key) {
                    let Some(route) = RoadLoop::read(self.city.as_ref().unwrap(), key) else {
                        return NOT_FOUND;
                    };
                    self.road_loop = Some(route);
                }
                let route = self.road_loop.as_ref().unwrap();
                let p2 = route.point(xyz.x);
                p.xyz = [p2.x, p2.y, route.length];
                OK
            }
            op::FIRST_PERSON => {
                if !self.objects.contains_key(&p.id) {
                    return STALE;
                }
                if xyz.y.abs() >= std::f32::consts::FRAC_PI_2 {
                    return INVALID;
                }
                self.focus = p.id;
                self.first_person = Some((
                    glam::Quat::from_rotation_z(xyz.x) * glam::Quat::from_rotation_y(-xyz.y),
                    xyz.z,
                ));
                OK
            }
            op::LOOK_DELTA => {
                p.xyz = [self.look_delta[0], self.look_delta[1], 0.];
                self.look_delta = [0.; 2];
                OK
            }
            op::VIEW_MODEL => {
                if key.is_empty() {
                    self.effects.muzzle.clear();
                    self.view_model = None;
                } else if self.view_models.contains_key(key) {
                    if let Some((_, pos)) = self.view_model.as_mut().filter(|(n, _)| n == key) {
                        *pos = xyz;
                    } else {
                        self.effects.muzzle.clear();
                        self.view_model = Some((key.to_owned(), xyz));
                    }
                } else {
                    return NOT_FOUND;
                }
                OK
            }
            op::MUZZLE_FLASH => {
                use crate::engine::muzzle::FlashStyle;
                if self.first_person.is_none() || self.view_model.is_none() {
                    return INVALID;
                }
                let style = match p.id {
                    0 => FlashStyle::Rifle,
                    1 => FlashStyle::Handgun,
                    _ => return INVALID,
                };
                self.effects.muzzle.ignite(style);
                OK
            }
            op::PICK_RETICLE | op::STRIKE_RETICLE => {
                let Some((rotation, height)) = self.first_person else {
                    return INVALID;
                };
                let strike = p.op == op::STRIKE_RETICLE;
                if xyz.x <= 0. || (strike && (xyz.x > 250. || !(0.25..=2.).contains(&xyz.y))) {
                    return INVALID;
                }
                let eye = self.objects[&self.focus].position + Vec3::Z * height;
                let dir = rotation * Vec3::X;
                let mut nearest = xyz.x;
                let mut hit = None;
                for (&id, obj) in &self.objects {
                    if id == self.focus {
                        continue;
                    }
                    let inv = glam::Quat::from_rotation_z(-obj.yaw);
                    let origin = inv * (eye - obj.position);
                    for part in &self.package.prefabs[&obj.prefab] {
                        if let Some((t, face)) = box_hit(origin, inv * dir, part, nearest) {
                            nearest = t;
                            hit = Some(Contact {
                                mark_owner: Some(id),
                                point: eye + dir * t,
                                normal: inv.conjugate() * face,
                                local_min: part.min.into(),
                                local_max: part.max.into(),
                                color: part.color.into(),
                                material: self
                                    .package
                                    .materials
                                    .get(&obj.prefab)
                                    .copied()
                                    .unwrap_or(Material::Concrete),
                            });
                        }
                    }
                }
                if strike {
                    if let Some(ground) = self.ground_contact(eye, dir, nearest) {
                        hit = Some(ground);
                    }
                }
                let Some(hit) = hit else {
                    return NOT_FOUND;
                };
                p.id = hit.mark_owner.unwrap_or_default();
                p.xyz = hit.point.into();
                if strike {
                    let floor = self.floor(hit.point);
                    self.impact_sound(hit.point, hit.material.sound(), xyz.y * 7., false);
                    let obj = self.objects.get(&p.id);
                    self.effects
                        .strike(hit, obj.map(|o| (o.position, o.yaw)), xyz.y, floor);
                }
                OK
            }
            op::SHATTER_PANEL => {
                let Some(obj) = self.objects.get(&p.id) else {
                    return STALE;
                };
                if obj.fixed {
                    return READ_ONLY;
                }
                let floor = self.floor(xyz);
                self.effects.shatter(
                    obj.position,
                    obj.yaw,
                    &self.package.prefabs[&obj.prefab],
                    xyz,
                    floor,
                );
                self.impact_sound(xyz, crate::audio::Material::Timber, 9., true);
                self.destroy(p.id);
                OK
            }
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
                        yaw: 0.,
                        fixed: false,
                    },
                );
                p.id = id;
                OK
            }
            op::POSITION..=op::FOLLOW | op::SET_FACING => {
                let Some(obj) = self.objects.get_mut(&p.id) else {
                    return STALE;
                };
                match p.op {
                    op::POSITION => p.xyz = obj.position.to_array(),
                    op::FOLLOW => {
                        self.focus = p.id;
                        self.first_person = None;
                        self.effects.muzzle.clear();
                        self.view_model = None;
                    }
                    _ if obj.fixed => return READ_ONLY,
                    op::SET_FACING => {
                        if xyz.x == 0. && xyz.y == 0. {
                            return INVALID;
                        }
                        obj.yaw = xyz.y.atan2(xyz.x);
                    }
                    op::SET_POSITION => {
                        obj.position = xyz;
                        obj.velocity = Vec3::ZERO;
                    }
                    op::VELOCITY => obj.velocity = xyz,
                    op::DESTROY => self.destroy(p.id),
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
                self.effects.step(DT);
                for sound in &mut self.sounds {
                    sound.age += DT;
                }
                self.time += DT;
                OK
            }
            _ => INVALID,
        }
    }

    fn impact_sound(
        &mut self,
        position: Vec3,
        material: crate::audio::Material,
        speed: f32,
        broken: bool,
    ) {
        self.sound_serial = self.sound_serial.wrapping_add(1).max(1);
        let slot = (self.sound_serial as usize - 1) % self.sounds.len();
        self.sounds[slot] = crate::audio::Impact {
            serial: self.sound_serial,
            position,
            material,
            speed,
            broken,
            age: 0.,
        };
    }
    pub fn has_impact_materials(&self) -> bool {
        !self.package.materials.is_empty()
    }
    pub fn impact_audio(&self) -> Option<crate::audio::Frame> {
        let (rot, h) = self.first_person?;
        if !self.has_impact_materials() {
            return None;
        }
        Some(crate::audio::Frame {
            eye: self.target() * SCALE + Vec3::Z * h,
            forward: rot * Vec3::X,
            impacts: self.sounds,
            settings: crate::audio::Settings {
                ambience: 0.,
                ..Default::default()
            },
            ..Default::default()
        })
    }
    fn destroy(&mut self, id: u32) {
        self.objects.remove(&id);
        self.names.retain(|_, entry| *entry != id);
        self.effects.remove_marks(id);
        if self.focus == id {
            self.focus = 0;
            self.first_person = None;
            self.effects.muzzle.clear();
            self.view_model = None;
        }
    }
    fn floor(&self, point: Vec3) -> f32 {
        self.city
            .as_ref()
            .and_then(|c| c.surface_at([point.x / SCALE, point.y / SCALE], point.z / SCALE + 0.001))
            .map_or(0., |s| s.height * SCALE)
    }
    fn ground_contact(&self, eye: Vec3, dir: Vec3, limit: f32) -> Option<Contact> {
        let hit = crate::dynamic_world::ground_shot_contact(self.city.as_ref()?, eye, dir, limit)?;
        Some(Contact {
            mark_owner: hit.stationary.then_some(0),
            point: hit.point,
            normal: hit.normal,
            local_min: Vec3::splat(f32::NEG_INFINITY),
            local_max: Vec3::splat(f32::INFINITY),
            color: if hit.surface == Material::Earth {
                Vec3::new(0.28, 0.22, 0.13)
            } else {
                Vec3::splat(0.45)
            },
            material: hit.surface,
        })
    }
    pub fn impact_sprites(&self) -> Vec<crate::engine::render::ImpactVertex> {
        let Some((rot, h)) = self.first_person else {
            return Vec::new();
        };
        self.effects.sprites(
            |id| self.objects.get(&id).map(|o| (o.position, o.yaw)),
            rot,
            self.target() * SCALE + Vec3::Z * h,
        )
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
        for (&id, obj) in &self.objects {
            if self.first_person.is_some() && id == self.focus {
                continue;
            }
            let first = mesh.vertices.len();
            let base = obj.position / SCALE;
            for part in &self.package.prefabs[&obj.prefab] {
                mesh.cuboid(
                    (Vec3::from(part.min) / SCALE).into(),
                    (Vec3::from(part.max) / SCALE).into(),
                    part.color,
                    SurfaceKind::Prop,
                );
            }
            let rotation = glam::Quat::from_rotation_z(obj.yaw);
            for v in &mut mesh.vertices[first..] {
                v.position = (base + rotation * Vec3::from(v.position)).into();
                v.normal = (rotation * Vec3::from(v.normal)).into();
            }
        }
        self.effects.solids(&mut mesh);
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
    fn strikes_respect_cover_and_reset_clears_surface_effects() {
        let mut pkg = Package::default();
        pkg.define(
            2,
            "bay",
            "",
            "",
            &[0.; 9],
            include_bytes!("../../../examples/breakwater/range.road"),
        )
        .unwrap();
        pkg.define(
            4,
            "board",
            "",
            "",
            &[-0.1, -0.5, 0., 0.1, 0.5, 2., 0.6, 0.3, 0.1],
            &[],
        )
        .unwrap();
        pkg.define(9, "board", "", "", &[0.; 9], &[]).unwrap();
        for (name, x) in [("actor", 0.), ("near", 3.), ("far", 6.)] {
            let mut v = [0.; 9];
            v[0] = x;
            pkg.define(5, name, "bay", "board", &v, &[]).unwrap();
        }
        let mut w = World::from_content(pkg).unwrap();
        w.load("bay").unwrap();
        let (_, actor) = command(&mut w, op::FIND, 0, [0.; 3], "actor");
        command(&mut w, op::FIRST_PERSON, actor.id, [0., 0., 1.], "");
        let (_, near) = command(&mut w, op::FIND, 0, [0.; 3], "near");
        let (status, hit) = command(&mut w, op::STRIKE_RETICLE, 0, [20., 1., 0.], "");
        assert_eq!(status, OK);
        assert_eq!(hit.id, near.id);
        assert!((hit.xyz[0] - 2.9).abs() < 0.001);
        assert!(w.effects.has_mark(Material::Wood));
        assert_eq!(
            command(&mut w, op::SHATTER_PANEL, hit.id, hit.xyz, "").0,
            OK
        );
        assert!(!w.effects.has_mark(Material::Wood));
        let (_, far) = command(&mut w, op::FIND, 0, [0.; 3], "far");
        assert_eq!(
            command(&mut w, op::PICK_RETICLE, 0, [20., 0., 0.], "").1.id,
            far.id
        );
        w.load("bay").unwrap();
        assert!(w.impact_sprites().is_empty());
    }

    #[test]
    fn pack_controls_content_and_handles_across_map_changes() {
        let mut content = Package::default();
        let road = include_bytes!("../../../examples/switchback-circuit/circuit.road");
        for map in ["circuit", "bad"] {
            content.define(2, map, "", "", &[0.; 9], road).unwrap();
        }
        content
            .define(
                4,
                "comet",
                "",
                "",
                &[-1., -1., 0., 1., 1., 1., 1., 0., 0.],
                &[],
            )
            .unwrap();
        content
            .define(5, "player", "circuit", "comet", &[0.; 9], &[])
            .unwrap();
        let mut fixed = [0.; 9];
        fixed[3] = 1.;
        content
            .define(5, "start_line", "circuit", "comet", &fixed, &[])
            .unwrap();
        content
            .define(5, "rival_a", "bad", "missing", &fixed, &[])
            .unwrap();
        let mut world = World::from_content(content).unwrap();
        let zero = [0.; 3];
        assert_eq!(command(&mut world, op::FIND, 0, zero, "player").0, NO_MAP);
        world.load("circuit").unwrap();
        let (status, route) = command(&mut world, op::ROAD_LOOP, 0, zero, "raceway");
        assert_eq!(status, OK);
        assert!((route.xyz[2] - 458.9332).abs() < 0.02);
        assert!((route.xyz[1] + 46.).abs() < 0.01);
        let (_, wrapped) = command(
            &mut world,
            op::ROAD_LOOP,
            0,
            [route.xyz[2], 0., 0.],
            "raceway",
        );
        assert_eq!(route.xyz, wrapped.xyz);
        let (status, cart) = command(&mut world, op::FIND, 0, zero, "player");
        assert_eq!(status, OK);
        assert_eq!(
            command(&mut world, op::SPAWN, 0, zero, "unknown").0,
            UNKNOWN_PREFAB
        );
        let (_, lamp) = command(&mut world, op::SPAWN, 0, zero, "comet");
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
        let (_, tower) = command(&mut world, op::FIND, 0, zero, "start_line");
        assert_eq!(
            command(&mut world, op::SET_POSITION, tower.id, zero, "").0,
            READ_ONLY
        );
        assert!(world.load("bad").is_err());
        assert_eq!(command(&mut world, op::POSITION, cart.id, zero, "").0, OK);
        world.load("circuit").unwrap();
        assert_eq!(
            command(&mut world, op::POSITION, cart.id, zero, "").0,
            STALE
        );
    }
}

fn box_hit(origin: Vec3, dir: Vec3, part: &Part, limit: f32) -> Option<(f32, Vec3)> {
    let mut near = 0f32;
    let mut far = limit;
    let mut normal = -dir;
    for i in 0..3 {
        if dir[i].abs() < 1e-7 {
            if origin[i] < part.min[i] || origin[i] > part.max[i] {
                return None;
            }
        } else {
            let a = (part.min[i] - origin[i]) / dir[i];
            let b = (part.max[i] - origin[i]) / dir[i];
            if a.min(b) >= near {
                near = a.min(b);
                normal = Vec3::ZERO;
                normal[i] = -dir[i].signum();
            }
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    Some((near, normal))
}

struct RoadLoop {
    name: String,
    samples: Vec<(f32, glam::Vec2)>,
    length: f32,
}
impl RoadLoop {
    fn read(city: &City, name: &str) -> Option<Self> {
        let mut samples = Vec::new();
        let mut length = 0.;
        for road in city.roads.iter().filter(|r| r.definition.name == name) {
            for i in 0..=128 {
                let p = road.segment.point_at(i as f32 / 128.);
                let point = glam::Vec2::new(p.x, p.y) * SCALE;
                if let Some(&(_, previous)) = samples.last() {
                    let d = point.distance(previous);
                    if i == 0 && d > 0.01 {
                        return None;
                    }
                    if d < 0.00001 {
                        continue;
                    }
                    length += d;
                }
                samples.push((length, point));
            }
        }
        if length < 1. || samples.first()?.1.distance(samples.last()?.1) > 0.01 {
            return None;
        }
        Some(Self {
            name: name.into(),
            samples,
            length,
        })
    }
    fn point(&self, distance: f32) -> glam::Vec2 {
        let d = distance.rem_euclid(self.length);
        let i = self
            .samples
            .partition_point(|&(s, _)| s < d)
            .clamp(1, self.samples.len() - 1);
        let (s, a) = self.samples[i - 1];
        let (e, b) = self.samples[i];
        a.lerp(b, (d - s) / (e - s))
    }
}
