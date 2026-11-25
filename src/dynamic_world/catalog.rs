//! Embedded, shared web/native vehicle assets. GLB: +X nose, +Y up, +Z right, metres.
//! Exact wheel_* names select axle-local spin/steering; other nodes belong to the sprung body.
mod crash_mesh;
mod door_mesh;
use super::vehicle::damage::Piece;
mod shot_mesh;
use super::doors::{self, Door, Motion};
use super::vehicle::tire::{Wheel, FLAT_HEIGHT};
use glam::{Mat4, Quat, Vec3};
#[cfg(test)]
use opencity::runtime::Mesh;
use opencity::runtime::{SurfaceKind, Vertex};
use serde::Deserialize;
use std::sync::OnceLock;
pub const BICYCLE_ID: &str = "vehicle-bike-share";

#[derive(Default, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VehicleKind {
    #[default]
    Motorized,
    Bicycle,
    Aircraft,
    Boat,
}

pub const SNOWMOBILE_ID: &str = "vehicle-winter-snowmobile";

pub const SNOWPLOW_ID: &str = "vehicle-j03";

// Preserve the integer paint-variant mask; fractional bits carry the authored
// GLB finish without increasing vertex stride or changing lamp/snow metadata.
fn vehicle_finish(roughness: f32, metallic: f32, paint: bool, coat: bool) -> f32 {
    let rough = (roughness.clamp(0., 1.) * 127.).round() as u32;
    let metal = (metallic.clamp(0., 1.) * 127.).round() as u32;
    f32::from(paint) + (32768 | rough | (metal << 7) | (u32::from(coat) << 14)) as f32 / 65536.
}

#[cfg(test)]
#[path = "catalog_collecteur_tests.rs"]
mod collecteur_tests;
#[cfg(test)]
#[path = "snowmobile_tests.rs"]
mod snowmobile_tests;
#[cfg(test)]
#[path = "catalog_snowplow_tests.rs"]
mod snowplow_tests;
#[cfg(test)]
#[path = "catalog_vehicle_qa.rs"]
mod vehicle_qa;

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Physics {
    pub mass: f32,
    pub max_speed: f32,
    pub acceleration: f32,
    pub friction: f32,
    pub max_steer_angle: f32,
    pub height_offset: f32,
    pub wheel_radius: f32,
    pub wheelbase: f32,
    pub track: f32,
    #[serde(default)]
    pub traction: Option<Traction>,
}
/// Powered progress on seasonal ground; ordinary tires retain their existing response.
#[derive(Clone, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Traction {
    pub snow: f32,
    pub grass: f32,
    pub road: f32,
}
impl Traction {
    pub fn multiplier(&self, surface: SurfaceKind, season: crate::season::Season) -> f32 {
        match (surface, season) {
            (SurfaceKind::Grass | SurfaceKind::Sand, crate::season::Season::Winter) => self.snow,
            (SurfaceKind::Grass | SurfaceKind::Sand, _) => self.grass,
            _ => self.road,
        }
    }
}
#[derive(Clone, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Dimensions {
    pub length: f32,
    pub width: f32,
    pub height: f32,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub name: String,
    pub research_id: String,
    pub category: String,
    pub road_vehicle: bool,
    #[serde(default)]
    pub vehicle_kind: VehicleKind,
    pub model: String,
    pub dimensions: Dimensions,
    pub physics: Physics,
    #[serde(default)]
    pub sound: crate::audio::VehicleSound,
    pub qa: serde_json::Value,
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum PartRole {
    Body,
    Crash(Piece),
    Door(Door, f32),
    Outboard,
    BaggageCargo,
    BaggageTrailer,
    BaggageWheel,
    RearWheel,
    FrontWheel,
    Steering,
    Crank,
    Pedal,
    AircraftGear(Gear),
    FlightControl(Surface, Vec3),
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Surface {
    LeftAileron,
    RightAileron,
    Elevator,
    Rudder,
}

impl Surface {
    fn from_node(name: &str) -> Option<Self> {
        match name {
            "Left Aileron" => Some(Self::LeftAileron),
            "Right Aileron" => Some(Self::RightAileron),
            "Left Elevator" | "Right Elevator" => Some(Self::Elevator),
            "Rudder" => Some(Self::Rudder),
            _ => None,
        }
    }
    fn rotation(self, axis: Vec3, vehicle: &super::Vehicle) -> Quat {
        let deflection = vehicle.aircraft_surfaces;
        let angle = match self {
            Self::LeftAileron => -deflection.x * 0.42,
            Self::RightAileron => deflection.x * 0.42,
            Self::Elevator => -deflection.y * 0.44,
            Self::Rudder => -deflection.z * 0.50,
        };
        Quat::from_axis_angle(axis, angle)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Gear {
    Nose,
    Left,
    Right,
}

impl Gear {
    fn rotation(self, vehicle: &super::Vehicle) -> Quat {
        let fold = Quat::from_rotation_y(vehicle.aircraft_gear * std::f32::consts::FRAC_PI_2);
        match self {
            Self::Nose => fold * Quat::from_rotation_z(vehicle.steering),
            Self::Left | Self::Right => fold,
        }
    }

    fn hinge(self, length: f32, track: f32) -> Vec3 {
        let radius = track / 1.78;
        let (x, y) = match self {
            Self::Nose => (super::aircraft::NOSE_GEAR_X * length, 0.),
            Self::Left => (super::aircraft::MAIN_GEAR_X * length, track * 0.5),
            Self::Right => (super::aircraft::MAIN_GEAR_X * length, -track * 0.5),
        };
        Vec3::new(x, y, -0.76 * radius)
    }
}

fn part_role(name: &str) -> Result<PartRole, String> {
    match name {
        "Gear strut nose" | "Tire nose" | "Hub nose -1" | "Hub nose 1" => {
            Ok(PartRole::AircraftGear(Gear::Nose))
        }
        "Gear strut main left" | "Tire main left" | "Hub main left -1" | "Hub main left 1" => {
            Ok(PartRole::AircraftGear(Gear::Left))
        }
        "Gear strut main right" | "Tire main right" | "Hub main right -1" | "Hub main right 1" => {
            Ok(PartRole::AircraftGear(Gear::Right))
        }
        "baggage_cargo" => Ok(PartRole::BaggageCargo),
        "baggage_trailer" => Ok(PartRole::BaggageTrailer),
        "baggage_wheel_front_left"
        | "baggage_wheel_front_right"
        | "baggage_wheel_rear_left"
        | "baggage_wheel_rear_right" => Ok(PartRole::BaggageWheel),
        "wheel_front" | "wheel_front_left" | "wheel_front_right" => Ok(PartRole::FrontWheel),
        "wheel_rear" | "wheel_rear_left" | "wheel_rear_right" | "wheel_middle_left"
        | "wheel_middle_right" => Ok(PartRole::RearWheel),
        "crank_arm_-1" | "crank_arm_1" => Ok(PartRole::Crank),
        "pedal_-1" | "pedal_1" => Ok(PartRole::Pedal),
        "Outboard leg"
        | "Outboard powerhead"
        | "Cowling cap"
        | "Cowling flash -1"
        | "Cowling flash 1"
        | "Anti-ventilation plate"
        | "Gearcase"
        | "Skeg"
        | "Propeller blade 0"
        | "Propeller blade 1"
        | "Propeller blade 2" => Ok(PartRole::Outboard),
        "bicycle_steering" => Ok(PartRole::Steering),
        name if name.starts_with("wheel_") => Err(format!("Unknown wheel role: {name}")),
        _ => Ok(PartRole::Body),
    }
}
/// Exact authored lens names; body panels and emergency beacons are excluded.
fn rear_lens(name: &str) -> bool {
    matches!(
        name,
        "rear_lamp0"
            | "rear_lamp1"
            | "rear_lamp_-1"
            | "rear_lamp_1"
            | "taillight_-1"
            | "taillight_1"
            | "taill"
            | "tailr"
            | "tailL"
            | "tailR"
            | "tail_lamp0"
            | "tail_lamp1"
            | "taillampm"
            | "taillampp"
            | "tail1"
            | "tail2"
            | "rearLamp0"
            | "rearLamp2"
            | "rear_lamp"
    )
}
struct Part {
    role: PartRole,
    pivot: Vec3,
    vertices: Vec<Vertex>,
    shadow_vertices: Option<Vec<Vertex>>,
}
enum ShadowRole {
    Exterior,
    Interior,
}
fn shadow_role(name: &str) -> ShadowRole {
    match name {
        name if name.starts_with("seat")
            || name.starts_with("Seat")
            || name.starts_with("interior_")
            || name.starts_with("dashboard")
            || name.starts_with("steering_wheel") =>
        {
            ShadowRole::Interior
        }
        _ => ShadowRole::Exterior,
    }
}
fn excludes_snow(name: &str, role: PartRole) -> bool {
    role != PartRole::Body
        || matches!(shadow_role(name), ShadowRole::Interior)
        || name.starts_with("steering_")
        || matches!(name, "plow_control_console" | "steering_column")
}
struct AircraftHull {
    pivot: Vec3,
    role: PartRole,
    shape: rapier3d::prelude::SharedShape,
}

fn folded_gear_pivot(pivot: Vec3, fraction: f32) -> Vec3 {
    pivot - Vec3::Z * (pivot.z * fraction.powi(2))
}

pub struct Definition {
    pub config: Config,
    pub texture_png: Option<Vec<u8>>,
    geometry: OnceLock<ModelGeometry>,
    source: Option<(&'static [u8], u32)>,
    plate_layer: Option<u32>,
}

/// Geometry is decoded on the first physics or rendering request, never for ID lookup.
pub struct ModelGeometry {
    pub(super) road_wheels: Option<[usize; 4]>,
    cargo_ground_origin: OnceLock<Vec3>,
    shot_meshes: OnceLock<Vec<rapier3d::prelude::TriMesh>>,
    aircraft_hulls: Vec<AircraftHull>,
    pub water_hull: Option<super::water_hull::WaterHull>,
    parts: Vec<Part>,
    rear_lenses: Vec<Vertex>,
    headlights: Vec<Vec3>,
    rear_emitters: Vec<Vec3>,
}
impl std::ops::Deref for Definition {
    type Target = ModelGeometry;
    fn deref(&self) -> &ModelGeometry {
        self.geometry.get_or_init(|| {
            let (bytes, layer) = self.source.expect("lazy model source");
            let mut decoded = Self::load_config(self.config.clone(), bytes, layer)
                .expect("validated bundled vehicle geometry");
            if let Some(layer) = self.plate_layer {
                decoded.add_rear_plate(layer);
            }
            decoded.geometry.into_inner().expect("decoded geometry")
        })
    }
}
impl std::ops::DerefMut for Definition {
    fn deref_mut(&mut self) -> &mut ModelGeometry {
        let _ = std::ops::Deref::deref(self);
        self.geometry.get_mut().unwrap()
    }
}
/// Explicit lens roles keep housings and roof beacons out of the light sources.
fn headlight_lens(name: &str) -> bool {
    matches!(
        name,
        "headlamp_lens"
            | "frontlamp0"
            | "frontlamp2"
            | "headL"
            | "headR"
            | "head_fl"
            | "head_fr"
            | "headlamp0"
            | "headlamp1"
            | "headlamp_-1"
            | "headlamp_1"
            | "headlight_-1"
            | "headlight_1"
            | "headlight_lenses"
            | "lamp0"
            | "lamp1"
            | "lampL"
            | "lampR"
            | "lampm"
            | "lampp"
            | "lamp_inner0"
            | "lamp_inner1"
            | "lamp_lower0"
            | "lamp_lower1"
            | "lamp_upper0"
            | "lamp_upper1"
            | "lampupper0"
            | "lampupper2"
            | "lampvertical0"
            | "lampvertical2"
            | "left_front_lamp"
            | "right_front_lamp"
            | "running_lamp_-1"
            | "running_lamp_1"
    )
}
fn vehicle_lamp_marker(name: &str) -> f32 {
    match name {
        "roof_beacon_red" => 65.,
        "roof_beacon_blue" => 66.,
        "Port position light" | "Starboard position light" | "Tail position light" => 68.,
        "Dorsal beacon" => 69.,
        name if headlight_lens(name) => 64.,
        _ => 0.,
    }
}
fn headlight_vertex(vertex: &Vertex) -> bool {
    if vertex.surface[0] < 0. {
        (-vertex.surface[0]).fract() == 0.25
    } else {
        vertex.surface[3] == 64.
    }
}
fn game_vector(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], -p[2], p[1])
}
impl Part {
    fn opaque_shadow_vertices(&self) -> impl Iterator<Item = Vertex> + '_ {
        self.shadow_vertices
            .as_deref()
            .unwrap_or(&self.vertices)
            .chunks_exact(3)
            .filter(|t| {
                t.iter()
                    .all(|v| v.surface[1] >= 0.999 && !headlight_vertex(v))
            })
            .flatten()
            .copied()
    }
    fn animated_pivot(&self, vehicle: &super::Vehicle) -> Vec3 {
        if self.role == PartRole::FrontWheel && vehicle.is_bicycle() {
            let stem = Vec3::new(0.35, 0., 0.90) - Vec3::Z * vehicle.ride_height();
            stem + Quat::from_rotation_z(vehicle.steering) * (self.pivot - stem)
        } else if matches!(self.role, PartRole::AircraftGear(_)) {
            folded_gear_pivot(self.pivot, vehicle.aircraft_gear)
        } else if self.role == PartRole::Pedal {
            let crank = super::cycling_pose::CRANK - Vec3::Z * vehicle.ride_height();
            crank + Quat::from_rotation_y(vehicle.pedal_angle) * (self.pivot - crank)
        } else {
            self.pivot
        }
    }
}
impl Definition {
    pub(super) fn aircraft_hulls<'a>(
        &'a self,
        vehicle: &'a super::Vehicle,
    ) -> impl Iterator<Item = (Vec3, Quat, &'a dyn rapier3d::prelude::Shape)> {
        self.aircraft_hulls.iter().map(move |hull| {
            let (pivot, rotation) = match hull.role {
                PartRole::AircraftGear(gear) => (
                    folded_gear_pivot(hull.pivot, vehicle.aircraft_gear),
                    gear.rotation(vehicle),
                ),
                PartRole::FlightControl(control, axis) => {
                    (hull.pivot, control.rotation(axis, vehicle))
                }
                _ => (
                    Vec3::ZERO,
                    Quat::from_rotation_y(-vehicle.body_lean.x)
                        * Quat::from_rotation_x(vehicle.body_lean.y),
                ),
            };
            (pivot, rotation, hull.shape.as_ref())
        })
    }

    pub fn is_bicycle(&self) -> bool {
        self.config.vehicle_kind == VehicleKind::Bicycle
    }
    /// Rear face in GLB coordinates (+X nose, +Y up). These positions follow
    /// the authored tailgate, hatch, or equipment panel of each model.
    pub fn plate_mount(&self) -> (f32, f32) {
        match self.config.research_id.as_str() {
            "vehicle-v2-01" => (-2.767, 1.04),
            "vehicle-12" => (-2.340, 1.06),
            "vehicle-v2-09" => (-2.318, 0.79),
            "vehicle-j01" | "vehicle-j06" => (-2.318, 0.72),
            "vehicle-j02" => (-6.105, 1.12),
            "vehicle-j03" => (-4.510, 1.65),
            "vehicle-j05" => (-4.130, 0.72),
            "vehicle-j07" => (-3.930, 1.05),
            "vehicle-r01" => (-7.940, 0.75),
            "vehicle-r02" => (-8.432, 0.75),
            id => unreachable!("missing rear plate mount: {id}"),
        }
    }
    fn add_rear_plate(&mut self, texture_layer: u32) {
        let (x, height) = self.plate_mount();
        let z = height - self.config.physics.height_offset;
        let (half_width, half_height) = (0.15, 0.075);
        let corners = [
            ([x, -half_width, z + half_height], [1., 0.]),
            ([x, half_width, z + half_height], [0., 0.]),
            ([x, half_width, z - half_height], [0., 1.]),
            ([x, -half_width, z - half_height], [1., 1.]),
        ];
        let body = &mut self.parts[0];
        if body.shadow_vertices.is_none() {
            body.shadow_vertices = Some(body.vertices.clone());
        }
        for index in [0, 1, 2, 0, 2, 3] {
            let (position, uv) = corners[index];
            body.vertices.push(Vertex {
                position,
                normal: [-1., 0., 0.],
                color: [1.; 3],
                // The half-layer suffix excludes snow while preserving the PNG layer.
                surface: [-(texture_layer as f32 + 1.) - 0.5, 1., uv[0], uv[1]],
            });
        }
    }
    pub(super) fn paint_variant(&self, slot: usize) -> u8 {
        match (self.config.research_id.as_str(), slot % 4) {
            ("vehicle-v2-01" | "vehicle-v2-09" | "vehicle-12", 1) => 1,
            ("vehicle-v2-01" | "vehicle-v2-09" | "vehicle-12", 2) => 2,
            ("vehicle-v2-01" | "vehicle-v2-09" | "vehicle-12", 3) => 3,
            _ => 0,
        }
    }
    pub(super) fn rear_emitters(&self) -> &[Vec3] {
        &self.rear_emitters
    }
    pub(super) fn headlights(&self) -> &[Vec3] {
        &self.headlights
    }
    pub(super) fn rear_lenses(&self) -> &[Vertex] {
        &self.rear_lenses
    }
    fn lazy(config: &[u8], bytes: &'static [u8], layer: u32) -> Result<Self, String> {
        Self::lazy_config(
            serde_json::from_slice(config).map_err(|e| e.to_string())?,
            bytes,
            layer,
        )
    }
    pub(super) fn lazy_config(
        config: Config,
        bytes: &'static [u8],
        layer: u32,
    ) -> Result<Self, String> {
        let raw = gltf::binary::Glb::from_slice(bytes).map_err(|e| e.to_string())?;
        let document = gltf::Document::from_json(
            serde_json::from_slice(&raw.json).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let binary = raw.bin.as_deref().ok_or("GLB has no buffer")?;
        let texture_png = document
            .textures()
            .next()
            .map(|texture| match texture.source().source() {
                gltf::image::Source::View {
                    view,
                    mime_type: "image/png",
                } => Ok(binary[view.offset()..view.offset() + view.length()].to_vec()),
                _ => Err("Vehicle albedo requires an embedded PNG".to_string()),
            })
            .transpose()?;
        Ok(Self {
            config,
            texture_png,
            geometry: OnceLock::new(),
            source: Some((bytes, layer)),
            plate_layer: None,
        })
    }
    #[cfg(test)]
    fn load(config: &[u8], bytes: &[u8], texture_layer: u32) -> Result<Self, String> {
        let config: Config = serde_json::from_slice(config).map_err(|e| e.to_string())?;
        Self::load_config(config, bytes, texture_layer)
    }
    pub(super) fn load_config(
        config: Config,
        bytes: &[u8],
        texture_layer: u32,
    ) -> Result<Self, String> {
        config.sound.validate()?;
        let p = &config.physics;
        if !config.research_id.starts_with("vehicle-")
            || config.category.is_empty()
            || config.qa["claydashExport"] != "completed"
            || config.schema_version != 1
            || config.model != "model.glb"
            || [
                p.mass,
                p.max_speed,
                p.acceleration,
                p.friction,
                p.max_steer_angle,
                p.height_offset,
                p.wheel_radius,
                p.wheelbase,
                config.dimensions.length,
                config.dimensions.width,
                config.dimensions.height,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
            || p.traction.as_ref().is_some_and(|t| {
                [t.snow, t.grass, t.road]
                    .iter()
                    .any(|v| !v.is_finite() || *v <= 0. || *v > 1.)
            })
            || !p.track.is_finite()
            || match config.vehicle_kind {
                VehicleKind::Bicycle => p.track != 0.,
                VehicleKind::Motorized | VehicleKind::Aircraft | VehicleKind::Boat => p.track <= 0.,
            }
            || p.max_steer_angle >= std::f32::consts::FRAC_PI_2
        {
            return Err(format!("Invalid SI vehicle configuration: {}", config.name));
        }
        let raw = gltf::binary::Glb::from_slice(bytes).map_err(|e| e.to_string())?;
        let glb = gltf::Document::from_json(
            serde_json::from_slice(&raw.json).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let binary = raw.bin.as_deref().ok_or("GLB has no embedded buffer")?;
        let texture_png = glb
            .textures()
            .next()
            .map(|texture| match texture.source().source() {
                gltf::image::Source::View {
                    view,
                    mime_type: "image/png",
                } => Ok(binary[view.offset()..view.offset() + view.length()].to_vec()),
                _ => Err("Vehicle albedo requires an embedded PNG".to_string()),
            })
            .transpose()?;
        let paint = match config.research_id.as_str() {
            "vehicle-v2-01" | "vehicle-v2-09" | "vehicle-12" => glb
                .nodes()
                .find(|node| node.name() == Some("body_shell"))
                .and_then(|node| node.mesh())
                .and_then(|mesh| mesh.primitives().next())
                .map(|primitive| {
                    primitive
                        .material()
                        .pbr_metallic_roughness()
                        .base_color_factor()
                }),
            _ => None,
        };
        let mut shell_points = Vec::new();
        let mut aircraft_hulls = Vec::new();
        let mut parts = Vec::new();
        let mut rear_lenses = Vec::new();
        fn walk(
            node: gltf::Node<'_>,
            parent: Mat4,
            binary: &[u8],
            config: &Config,
            paint: Option<[f32; 4]>,
            texture_layer: u32,
            shell_points: &mut Vec<rapier3d::prelude::Point<f32>>,
            aircraft_hulls: &mut Vec<AircraftHull>,
            parts: &mut Vec<Part>,
            rear_lenses: &mut Vec<Vertex>,
        ) -> Result<(), String> {
            let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
            if let Some(mesh) = node.mesh() {
                let name = node.name().unwrap_or("body");
                let role = if let Some(control) = Surface::from_node(name) {
                    let axis = if control == Surface::Rudder {
                        Vec3::Y
                    } else {
                        Vec3::Z
                    };
                    PartRole::FlightControl(
                        control,
                        game_vector(transform.transform_vector3(axis).to_array()),
                    )
                } else {
                    part_role(name)?
                };
                let ride = config.physics.height_offset;
                let pivot = match role {
                    // Swivel at the aft face of the fixed transom bracket.
                    PartRole::Outboard => Vec3::new(-2.43, 0., 0.27 - ride),
                    PartRole::AircraftGear(gear) => {
                        gear.hinge(config.dimensions.length, config.physics.track)
                    }
                    _ => {
                        game_vector(transform.transform_point3(Vec3::ZERO).to_array())
                            - Vec3::Z * ride
                    }
                };
                let mut vertices = Vec::new();
                for primitive in mesh.primitives() {
                    if primitive.mode() != gltf::mesh::Mode::Triangles {
                        return Err("Vehicle requires triangles".into());
                    }
                    let reader = primitive.reader(|buffer| match buffer.source() {
                        gltf::buffer::Source::Bin => Some(binary),
                        _ => None,
                    });
                    let positions = reader
                        .read_positions()
                        .ok_or("Missing positions")?
                        .collect::<Vec<_>>();
                    let normals = reader
                        .read_normals()
                        .ok_or("Missing normals")?
                        .collect::<Vec<_>>();
                    let indices = reader.read_indices().ok_or("Missing indices")?.into_u32();
                    let colors = reader
                        .read_colors(0)
                        .map(|colors| colors.into_rgba_f32().collect::<Vec<_>>());
                    let texture = primitive
                        .material()
                        .pbr_metallic_roughness()
                        .base_color_texture();
                    let uvs = match texture {
                        Some(info) => {
                            if info.tex_coord() != 0 || info.texture().index() != 0 {
                                return Err(
                                    "Vehicle albedo requires texture 0 and TEXCOORD_0".into()
                                );
                            }
                            Some(
                                reader
                                    .read_tex_coords(0)
                                    .ok_or("Missing texture UVs")?
                                    .into_f32()
                                    .collect::<Vec<_>>(),
                            )
                        }
                        None => None,
                    };
                    let color = primitive
                        .material()
                        .pbr_metallic_roughness()
                        .base_color_factor();
                    let finish = primitive.material().pbr_metallic_roughness();
                    let coat =
                        matches!(node.name(), Some("body_shell" | "body" | "hood" | "trunk"))
                            && finish.metallic_factor() < 0.8;
                    let coverage = match primitive.material().alpha_mode() {
                        gltf::material::AlphaMode::Opaque => 1.,
                        gltf::material::AlphaMode::Blend => color[3].clamp(0., 1.),
                        gltf::material::AlphaMode::Mask => {
                            if color[3] >= primitive.material().alpha_cutoff().unwrap_or(0.5) {
                                1.
                            } else {
                                0.
                            }
                        }
                    };
                    for index in indices {
                        let i = index as usize;
                        let position = positions.get(i).ok_or("Index outside positions")?;
                        let normal = normals.get(i).ok_or("Index outside normals")?;
                        let tint = match &colors {
                            Some(colors) => *colors.get(i).ok_or("Missing vertex color")?,
                            None => [1.; 4],
                        };
                        let world = game_vector(
                            transform
                                .transform_point3(Vec3::from_array(*position))
                                .to_array(),
                        ) - Vec3::Z * ride;
                        vertices.push(Vertex {
                            position: (world
                                - if role == PartRole::Body {
                                    Vec3::ZERO
                                } else {
                                    pivot
                                })
                            .to_array(),
                            normal: game_vector(
                                transform
                                    .transform_vector3(Vec3::from_array(*normal))
                                    .to_array(),
                            )
                            .to_array(),
                            color: [color[0] * tint[0], color[1] * tint[1], color[2] * tint[2]],
                            // Vehicle shaders use y as MSAA coverage for GLB glass.
                            surface: if let Some(uvs) = &uvs {
                                let uv = uvs.get(i).ok_or("Index outside texture UVs")?;
                                if uv.iter().any(|v| !v.is_finite()) {
                                    return Err("Invalid vehicle texture UVs".into());
                                }
                                // Negative material IDs select an albedo layer; zw carry UVs.
                                [
                                    -(texture_layer as f32 + 1.)
                                        - if headlight_lens(node.name().unwrap_or("")) {
                                            0.25
                                        } else {
                                            0.
                                        },
                                    coverage * tint[3],
                                    uv[0],
                                    uv[1],
                                ]
                            } else {
                                [
                                    SurfaceKind::Prop as u32 as f32,
                                    coverage * tint[3],
                                    vehicle_finish(
                                        finish.roughness_factor(),
                                        finish.metallic_factor(),
                                        role == PartRole::Body && paint == Some(color),
                                        coat,
                                    ),
                                    vehicle_lamp_marker(node.name().unwrap_or("")),
                                ]
                            },
                        });
                    }
                }
                // Keep each structural mesh separate before the render batching below.
                // Glazing, paint, and door trim follow the fuselage surface.
                let name = node.name().unwrap_or("");
                if config.vehicle_kind == VehicleKind::Boat && super::water_hull::hull_panel(name) {
                    shell_points.extend(
                        vertices
                            .iter()
                            .map(|v| rapier3d::prelude::Point::from(v.position)),
                    );
                }
                if config.vehicle_kind == VehicleKind::Aircraft
                    && ![
                        "Cabin window",
                        "Windscreen",
                        "Livery",
                        "Boarding door",
                        "Hub ",
                        "Fan ",
                    ]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                    && !matches!(vehicle_lamp_marker(name), 68. | 69.)
                {
                    use rapier3d::prelude::{Point, SharedShape};
                    let mut points: Vec<_> =
                        vertices.iter().map(|v| Point::from(v.position)).collect();
                    points.sort_unstable_by(|a, b| {
                        a.x.total_cmp(&b.x)
                            .then(a.y.total_cmp(&b.y))
                            .then(a.z.total_cmp(&b.z))
                    });
                    points.dedup();
                    let shape = SharedShape::convex_hull(&points)
                        .ok_or_else(|| format!("Invalid aircraft contact hull for {name}"))?;
                    aircraft_hulls.push(AircraftHull { pivot, role, shape });
                }
                if excludes_snow(node.name().unwrap_or(""), role) {
                    for vertex in &mut vertices {
                        if vertex.surface[0] < 0. && !headlight_vertex(vertex) {
                            // Half-layer marker preserves both texture selection and UVs.
                            vertex.surface[0] = -((-vertex.surface[0]).floor() + 0.5);
                        } else if vertex.surface[0] >= 0. && vertex.surface[3] == 0. {
                            vertex.surface[3] = -3.;
                        }
                    }
                }
                if rear_lens(node.name().unwrap_or("")) {
                    let (lo, hi) = vertices.iter().fold(
                        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                        |(lo, hi), v| {
                            let p = Vec3::from_array(v.position);
                            (lo.min(p), hi.max(p))
                        },
                    );
                    // Lens coordinates select an inset white segment without
                    // adding vertices or changing the authored lens silhouette.
                    let width = (hi.y - lo.y).max(0.001);
                    let height = (hi.z - lo.z).max(0.001);
                    let strip = lo.y < -0.2 && hi.y > 0.2;
                    for mut vertex in vertices {
                        vertex.surface = [
                            SurfaceKind::Light as u32 as f32,
                            (vertex.position[1] - lo.y) / width,
                            (vertex.position[2] - lo.z) / height,
                            if strip { 10. } else { 2. },
                        ];
                        rear_lenses.push(vertex);
                    }
                } else {
                    let part = Part {
                        role,
                        pivot,
                        vertices,
                        shadow_vertices: match shadow_role(node.name().unwrap_or("")) {
                            ShadowRole::Interior => Some(Vec::new()),
                            ShadowRole::Exterior => None,
                        },
                    };
                    if let Some(part) = crash_mesh::assign(&config.research_id, name, part, parts) {
                        door_mesh::separate(config, name, part, parts);
                    }
                }
            }
            for child in node.children() {
                walk(
                    child,
                    transform,
                    binary,
                    config,
                    paint,
                    texture_layer,
                    shell_points,
                    aircraft_hulls,
                    parts,
                    rear_lenses,
                )?;
            }
            Ok(())
        }
        for node in glb.default_scene().ok_or("Missing default scene")?.nodes() {
            walk(
                node,
                Mat4::IDENTITY,
                binary,
                &config,
                paint,
                texture_layer,
                &mut shell_points,
                &mut aircraft_hulls,
                &mut parts,
                &mut rear_lenses,
            )?;
        }
        if parts.is_empty() {
            return Err("Empty vehicle GLB".into());
        }
        // Every fixed panel shares the body's transform; upload it as one rigid part.
        let mut body = Vec::new();
        let mut shadow_body = Vec::new();
        let mut articulated: Vec<Part> = Vec::new();
        for part in parts {
            if part.role == PartRole::Body {
                shadow_body.extend(part.opaque_shadow_vertices());
                body.extend(part.vertices);
            } else if matches!(part.role, PartRole::Door(..) | PartRole::Crash(_)) {
                if let Some(group) = articulated.iter_mut().find(|g| g.role == part.role) {
                    group
                        .shadow_vertices
                        .as_mut()
                        .unwrap()
                        .extend(part.opaque_shadow_vertices());
                    group.vertices.extend(part.vertices);
                } else {
                    articulated.push(Part {
                        pivot: Vec3::ZERO,
                        shadow_vertices: Some(part.opaque_shadow_vertices().collect()),
                        ..part
                    });
                }
            } else if part.role == PartRole::Outboard {
                // The cowling, leg and prop share a single swivel and draw call.
                if let Some(motor) = articulated
                    .iter_mut()
                    .find(|p| p.role == PartRole::Outboard)
                {
                    motor.vertices.extend(part.vertices);
                } else {
                    articulated.push(part);
                }
            } else {
                articulated.push(part);
            }
        }
        for part in &mut articulated {
            if matches!(part.role, PartRole::Crash(_)) {
                let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
                for v in &part.vertices {
                    let p = Vec3::from(v.position);
                    lo = lo.min(p);
                    hi = hi.max(p);
                }
                part.pivot = (lo + hi) * 0.5;
            }
        }
        articulated.insert(
            0,
            Part {
                role: PartRole::Body,
                pivot: Vec3::ZERO,
                shadow_vertices: if shadow_body.len() < body.len() {
                    Some(shadow_body)
                } else {
                    None
                },
                vertices: body,
            },
        );
        if rear_lenses.is_empty() && config.vehicle_kind == VehicleKind::Motorized {
            return Err(format!("Missing authored rear lenses: {}", config.name));
        }
        let mut headlights = Vec::new();
        for left in [false, true] {
            if config.vehicle_kind == VehicleKind::Bicycle && left {
                continue;
            }
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for v in articulated[0].vertices.iter().filter(|v| {
                headlight_vertex(v)
                    && (config.vehicle_kind == VehicleKind::Bicycle || (v.position[1] > 0.) == left)
            }) {
                let p = Vec3::from_array(v.position);
                lo = lo.min(p);
                hi = hi.max(p);
            }
            if lo.is_finite() {
                headlights.push((lo + hi) * 0.5);
            }
        }
        let mut rear_emitters = Vec::new();
        for left in [false, true] {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for v in rear_lenses.iter().filter(|v| (v.position[1] > 0.) == left) {
                let p = Vec3::from_array(v.position);
                lo = lo.min(p);
                hi = hi.max(p);
            }
            if lo.is_finite() {
                rear_emitters.push((lo + hi) * 0.5);
            }
        }
        Ok(Self {
            config: config.clone(),
            texture_png,
            source: None,
            plate_layer: None,
            geometry: OnceLock::from(ModelGeometry {
                road_wheels: Self::road_wheel_parts(&config, &articulated),
                cargo_ground_origin: OnceLock::new(),
                shot_meshes: OnceLock::new(),
                water_hull: if config.vehicle_kind == VehicleKind::Boat {
                    Some(super::water_hull::WaterHull::new(shell_points)?)
                } else {
                    None
                },
                aircraft_hulls,
                parts: articulated,
                rear_lenses,
                headlights,
                rear_emitters,
            }),
        })
    }
    pub(crate) fn crash_pieces(&self) -> impl Iterator<Item = (usize, Piece, Vec3)> + '_ {
        self.parts
            .iter()
            .enumerate()
            .filter_map(|(i, p)| match p.role {
                PartRole::Crash(piece) => Some((i, piece, p.pivot)),
                _ => None,
            })
    }
    pub(crate) fn receives_dents(&self, index: usize) -> bool {
        matches!(
            self.parts[index].role,
            PartRole::Body | PartRole::Door(..) | PartRole::Crash(Piece::Panel(_))
        )
    }
    pub fn part_vertices(&self) -> impl Iterator<Item = &[Vertex]> {
        self.parts.iter().map(|p| p.vertices.as_slice())
    }
    pub fn part_shadow_vertices(&self) -> impl Iterator<Item = &[Vertex]> {
        self.parts
            .iter()
            .map(|part| part.shadow_vertices.as_deref().unwrap_or(&part.vertices))
    }
    fn door_transform(&self, vehicle: &super::Vehicle, door: Door, leaf: f32) -> Mat4 {
        let manual = vehicle.doors.fraction(door);
        if manual == 0. && vehicle.metro_doors == 0. {
            return Mat4::IDENTITY;
        }
        let opening = doors::opening(&self.config.research_id, door).expect("mapped door");
        let fraction = if opening.motion == Motion::Metro && door.side() > 0. {
            manual.max(vehicle.metro_doors)
        } else {
            manual
        };
        opening.transform(door, fraction, leaf)
    }
    fn part_transform(&self, index: usize, vehicle: &super::Vehicle, lean: Quat) -> Mat4 {
        let part = &self.parts[index];
        let rotation = match part.role {
            PartRole::Crash(piece) => {
                let mut pose = Mat4::from_quat(lean);
                if let (Piece::Panel(zone), Some(damage)) = (piece, &vehicle.damage) {
                    let sag = damage.strain(zone).min(1.);
                    pose *= Mat4::from_translation(part.pivot - Vec3::Z * (0.035 * sag))
                        * Mat4::from_rotation_y(0.09 * sag)
                        * Mat4::from_translation(-part.pivot);
                }
                return pose;
            }
            PartRole::Body => return Mat4::from_quat(lean),
            PartRole::FlightControl(control, axis) => control.rotation(axis, vehicle),
            PartRole::AircraftGear(gear) => gear.rotation(vehicle),
            PartRole::Door(door, leaf) => {
                return Mat4::from_quat(lean) * self.door_transform(vehicle, door, leaf)
            }
            PartRole::Crank => Quat::from_rotation_y(vehicle.pedal_angle),
            PartRole::Pedal => Quat::IDENTITY,
            PartRole::Steering => Quat::from_rotation_z(vehicle.steering),
            PartRole::Outboard => Quat::from_rotation_z(-vehicle.steering),
            PartRole::BaggageCargo | PartRole::BaggageTrailer | PartRole::BaggageWheel => {
                Quat::IDENTITY
            }
            PartRole::RearWheel => Quat::from_rotation_y(vehicle.wheel_angle),
            PartRole::FrontWheel => {
                Quat::from_rotation_z(vehicle.steering) * Quat::from_rotation_y(vehicle.wheel_angle)
            }
        };
        let pivot = part.animated_pivot(vehicle);
        if vehicle.flat_tires.contains(&true)
            && self
                .wheel_at_part(index)
                .is_some_and(|w| vehicle.is_flat(w))
        {
            // Compress vertically after spin, so the flat stays against the road.
            return Mat4::from_translation(pivot)
                * Mat4::from_scale(Vec3::new(1., 1., FLAT_HEIGHT))
                * Mat4::from_quat(rotation);
        }
        Mat4::from_rotation_translation(rotation, pivot)
    }

    pub fn cart_part(&self, index: usize) -> bool {
        matches!(
            self.parts[index].role,
            PartRole::BaggageCargo | PartRole::BaggageTrailer | PartRole::BaggageWheel
        )
    }

    pub(super) fn cart_parts(
        &self,
        spin: f32,
        cargo: bool,
    ) -> impl Iterator<Item = (usize, Mat4)> + '_ {
        let eye = super::baggage::hitch(self.config.physics.height_offset);
        self.parts.iter().enumerate().filter_map(move |(i, p)| {
            let rotation = match p.role {
                PartRole::BaggageCargo if cargo => Quat::IDENTITY,
                PartRole::BaggageTrailer => Quat::IDENTITY,
                PartRole::BaggageWheel => Quat::from_rotation_y(spin),
                _ => return None,
            };
            Some((i, Mat4::from_rotation_translation(rotation, p.pivot - eye)))
        })
    }

    pub(super) fn ground_cargo_parts(&self) -> impl Iterator<Item = (usize, Mat4)> + '_ {
        let origin = *self.cargo_ground_origin.get_or_init(|| {
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for part in self
                .parts
                .iter()
                .filter(|p| p.role == PartRole::BaggageCargo)
            {
                for vertex in &part.vertices {
                    let point = Vec3::from_array(vertex.position) + part.pivot;
                    min = min.min(point);
                    max = max.max(point);
                }
            }
            if min.is_finite() {
                Vec3::new((min.x + max.x) * 0.5, (min.y + max.y) * 0.5, min.z)
            } else {
                Vec3::ZERO
            }
        });
        self.parts.iter().enumerate().filter_map(move |(i, p)| {
            (p.role == PartRole::BaggageCargo)
                .then(|| (i, Mat4::from_translation(p.pivot - origin)))
        })
    }

    pub fn part_transforms(&self, vehicle: &super::Vehicle) -> Vec<Mat4> {
        let body = Mat4::from_scale_rotation_translation(
            Vec3::splat(1. / super::METERS_PER_UNIT),
            vehicle.orientation(),
            vehicle.position / super::METERS_PER_UNIT,
        );
        let lean = vehicle.cabin_lean();
        (0..self.parts.len())
            .map(|i| body * self.part_transform(i, vehicle, lean))
            .collect()
    }
    #[cfg(test)]
    pub fn mesh(&self, vehicle: &super::Vehicle) -> Mesh {
        let mut output = Mesh::default();
        self.append_mesh(vehicle, &mut output);
        output
    }
    #[cfg(test)]
    pub fn append_mesh(&self, vehicle: &super::Vehicle, output: &mut Mesh) {
        output
            .vertices
            .reserve(self.parts.iter().map(|p| p.vertices.len()).sum());
        let lean = vehicle.cabin_lean();
        let orientation = vehicle.orientation();
        for (index, part) in self.parts.iter().enumerate() {
            let q = match part.role {
                PartRole::Body | PartRole::Crash(_) => lean,
                PartRole::FlightControl(control, axis) => control.rotation(axis, vehicle),
                PartRole::AircraftGear(gear) => gear.rotation(vehicle),
                PartRole::Door(..) => lean,
                PartRole::Crank => Quat::from_rotation_y(vehicle.pedal_angle),
                PartRole::Pedal => Quat::IDENTITY,
                PartRole::Steering => Quat::from_rotation_z(vehicle.steering),
                PartRole::Outboard => Quat::from_rotation_z(-vehicle.steering),
                PartRole::BaggageCargo | PartRole::BaggageTrailer | PartRole::BaggageWheel => {
                    Quat::IDENTITY
                }
                PartRole::RearWheel => Quat::from_rotation_y(vehicle.wheel_angle),
                PartRole::FrontWheel => {
                    Quat::from_rotation_z(vehicle.steering)
                        * Quat::from_rotation_y(vehicle.wheel_angle)
                }
            };
            let flat = self
                .wheel_at_part(index)
                .is_some_and(|w| vehicle.is_flat(w));
            for vertex in &part.vertices {
                let mut v = *vertex;
                let mut point = Vec3::from_array(v.position);
                if let PartRole::Door(door, leaf) = part.role {
                    let transform = self.door_transform(vehicle, door, leaf);
                    point = transform.transform_point3(point);
                    v.normal = transform.transform_vector3(v.normal.into()).to_array();
                }
                let mut local = q * point;
                let mut normal = q * Vec3::from(v.normal);
                if flat {
                    local.z *= FLAT_HEIGHT;
                    normal.z /= FLAT_HEIGHT;
                }
                if !matches!(part.role, PartRole::Body | PartRole::Crash(_)) {
                    local += part.animated_pivot(vehicle);
                }
                v.position =
                    ((orientation * local + vehicle.position) / super::METERS_PER_UNIT).to_array();
                v.normal = (orientation * normal).to_array();
                output.vertices.push(v);
            }
        }
    }
}
macro_rules! asset {
    ($path:literal) => {
        (
            include_bytes!(concat!("../../data/vehicle/", $path, "/vehicle.json")).as_slice(),
            crate::web_asset!(
                concat!("../../data/vehicle/", $path, "/model.glb"),
                concat!("data/vehicle/", $path, "/model.glb")
            ),
        )
    };
}
fn assets() -> [(&'static [u8], &'static [u8]); 12] {
    [
        asset!("pickup/castor-rive"),
        asset!("suv/boreal-colline"),
        asset!("compact/laurent-clair"),
        asset!("taxi/bonjour-electrique"),
        asset!("bus/saint-laurent-urbain"),
        asset!("municipal/castor-blizzard"),
        asset!("municipal/castor-collecteur"),
        asset!("emergency/laurent-vigile"),
        asset!("emergency/saint-laurent-secours"),
        asset!("rail/azur-classique"),
        asset!("rail/prairie-express"),
        asset!("bicycle/velo-partage"),
    ]
}
pub fn definitions() -> &'static [Definition] {
    static CATALOG: OnceLock<Vec<Definition>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut texture_layer = 0;
        let mut definitions: Vec<_> = assets()
            .iter()
            .map(|(json, glb)| {
                let definition = Definition::lazy(json, glb, texture_layer)
                    .expect("valid bundled Claydash vehicle");
                texture_layer += u32::from(definition.texture_png.is_some());
                definition
            })
            .collect();
        let plated = definitions.len();
        definitions
            .extend(super::aircraft::Model::ALL.map(|model| model.definition(texture_layer)));
        for (cfg, glb) in [
            asset!("boat/voile"),
            asset!("boat/cabine"),
            asset!("boat/runabout"),
        ] {
            definitions
                .push(Definition::lazy(cfg, glb, texture_layer).expect("bundled Claydash boat"));
        }
        let (cfg, glb) = asset!("airport/baggage-tractor");
        let tug =
            Definition::lazy(cfg, glb, texture_layer).expect("bundled airport baggage tractor");
        texture_layer += u32::from(tug.texture_png.is_some());
        definitions.push(tug);
        let (cfg, glb) = asset!("winter/snowmobile");
        let snowmobile = Definition::lazy(cfg, glb, texture_layer).expect("bundled snowmobile");
        definitions.push(snowmobile);
        for definition in &mut definitions[..plated] {
            if !definition.is_bicycle() {
                definition.plate_layer = Some(texture_layer);
            }
        }
        definitions
    })
}
#[cfg(test)]
mod tests {
    #[test]
    fn metadata_and_texture_access_defer_geometry_and_preserve_plates() {
        let (json, bytes) = super::assets()[0];
        let mut lazy = super::Definition::lazy(json, bytes, 2).unwrap();
        assert!(!lazy.config.research_id.is_empty());
        let texture = lazy.texture_png.clone();
        lazy.plate_layer = Some(7);
        assert!(lazy.geometry.get().is_none());
        let mut eager = super::Definition::load(json, bytes, 2).unwrap();
        eager.add_rear_plate(7);
        let signature = |definition: &super::Definition| {
            definition
                .part_vertices()
                .map(|vertices| {
                    vertices
                        .iter()
                        .flat_map(|v| {
                            v.position
                                .into_iter()
                                .chain(v.normal)
                                .chain(v.color)
                                .chain(v.surface)
                                .map(f32::to_bits)
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(signature(&lazy), signature(&eager));
        assert_eq!(texture, eager.texture_png);
        assert!(lazy.geometry.get().is_some());
    }
    #[test]
    fn authored_finish_round_trips_without_changing_paint_mask() {
        for paint in [false, true] {
            for coat in [false, true] {
                for (rough, metal) in [(0.07, 0.), (0.4, 0.65), (0.95, 1.)] {
                    let value = super::vehicle_finish(rough, metal, paint, coat);
                    assert_eq!(value.floor(), f32::from(paint));
                    let bits = (value.fract() * 65536.).round() as u32;
                    assert_ne!(bits & 32768, 0);
                    assert!(((bits & 127) as f32 / 127. - rough).abs() < 0.004);
                    assert!((((bits >> 7) & 127) as f32 / 127. - metal).abs() < 0.004);
                    assert_eq!((bits >> 14) & 1, u32::from(coat));
                }
            }
        }
    }
    use super::*;
    #[test]
    fn shadow_sources_omit_cabin_details_without_changing_wheel_geometry() {
        let mut saved = 0;
        for definition in definitions() {
            assert_eq!(
                definition.part_vertices().count(),
                definition.part_shadow_vertices().count()
            );
            for (index, (camera, shadow)) in definition
                .part_vertices()
                .zip(definition.part_shadow_vertices())
                .enumerate()
            {
                assert!(shadow.len() <= camera.len());
                saved += camera.len() - shadow.len();
                if matches!(
                    definition.parts[index].role,
                    PartRole::FrontWheel | PartRole::RearWheel
                ) {
                    assert_eq!(
                        camera.as_ptr(),
                        shadow.as_ptr(),
                        "wheel source was replaced"
                    );
                }
            }
        }
        assert!(
            saved > 0,
            "no cabin details were omitted from shadow sources"
        );
    }
    #[test]
    fn door_cuts_and_separate_lenses_preserve_the_authored_surface_area() {
        for (definition, (_, bytes)) in definitions().iter().zip(assets()) {
            let glb = gltf::Gltf::from_slice(bytes).unwrap();
            fn area(a: Vec3, b: Vec3, c: Vec3) -> f64 {
                0.5 * (b - a).cross(c - a).length() as f64
            }
            let source_area: f64 = glb
                .nodes()
                .filter_map(|node| node.mesh().map(|mesh| (node, mesh)))
                .map(|(node, mesh)| {
                    let transform = Mat4::from_cols_array_2d(&node.transform().matrix());
                    mesh.primitives()
                        .map(|primitive| {
                            let reader = primitive.reader(|_| glb.blob.as_deref());
                            let points: Vec<_> = reader
                                .read_positions()
                                .unwrap()
                                .map(|p| transform.transform_point3(p.into()))
                                .collect();
                            let indices: Vec<_> =
                                reader.read_indices().unwrap().into_u32().collect();
                            indices
                                .chunks_exact(3)
                                .map(|i| {
                                    area(
                                        points[i[0] as usize],
                                        points[i[1] as usize],
                                        points[i[2] as usize],
                                    )
                                })
                                .sum::<f64>()
                        })
                        .sum::<f64>()
                })
                .sum();
            let lamp_vertices: usize = glb
                .nodes()
                .filter(|n| rear_lens(n.name().unwrap_or("")))
                .flat_map(|n| n.mesh().unwrap().primitives())
                .map(|p| p.indices().unwrap().count())
                .sum();
            assert_eq!(
                definition.rear_lenses.len(),
                lamp_vertices,
                "{}",
                definition.config.name
            );
            let surface_area: f64 = definition
                .part_vertices()
                .chain([definition.rear_lenses.as_slice()])
                .flat_map(|vertices| vertices.chunks_exact(3))
                .map(|t| {
                    area(
                        t[0].position.into(),
                        t[1].position.into(),
                        t[2].position.into(),
                    )
                })
                .sum();
            let plate_area = if definition.is_bicycle() {
                0.
            } else {
                0.30 * 0.15
            };
            assert!(
                (surface_area - source_area - plate_area).abs() < 0.0001,
                "{} lost or duplicated geometry: {surface_area} vs {source_area}",
                definition.config.name
            );
            assert!(
                definition
                    .rear_lenses
                    .iter()
                    .all(|v| v.color[0] > v.color[1] * 1.5 && v.color[0] > v.color[2] * 1.5),
                "only authored red rear lenses: {}",
                definition.config.name
            );
        }
        for name in [
            "tailgate",
            "tailgate_inset",
            "rear_bumper",
            "lamp0",
            "frontlamp0",
        ] {
            assert!(!rear_lens(name), "non-lens part must stay intact: {name}");
        }
    }
    #[test]
    fn bundled_catalog_matches_the_manifest_and_consumer_glass_survives_loading() {
        let manifest: Vec<serde_json::Value> =
            serde_json::from_slice(include_bytes!("../../data/vehicle/catalog.json")).unwrap();
        assert_eq!(manifest.len(), definitions().len());
        for (entry, definition) in manifest.iter().zip(definitions()) {
            assert_eq!(entry["name"], definition.config.name);
            assert_eq!(entry["researchId"], definition.config.research_id);
            assert_eq!(entry["roadVehicle"], definition.config.road_vehicle);
        }
        let studies = definitions()
            .iter()
            .filter(|d| {
                matches!(
                    d.config.qa["packager"].as_str(),
                    Some(
                        "runtime-packager"
                            | "claydash-game-package"
                            | "rust-source-cleanup"
                            | "rear-half-mirror"
                    )
                )
            })
            .collect::<Vec<_>>();
        let study_ids: std::collections::BTreeSet<_> = studies
            .iter()
            .map(|d| d.config.research_id.as_str())
            .collect();
        assert_eq!(
            study_ids,
            std::collections::BTreeSet::from([
                "vehicle-12",
                "vehicle-j02",
                "vehicle-j03",
                "vehicle-j05",
                "vehicle-v2-01",
                "vehicle-v2-09",
            ])
        );
        for definition in studies {
            let glass = if definition.config.research_id == "vehicle-v2-09" {
                assert_eq!(definition.config.name, "Laurent Clair");
                assert_eq!(definition.config.qa["interiorPrimitives"], 8);
                let source = gltf::Gltf::from_slice(include_bytes!(
                    "../../data/vehicle/compact/laurent-clair/model.glb"
                ))
                .unwrap();
                assert_eq!(source.nodes().count(), 28);
                assert_eq!(source.images().count(), 0);
                assert_eq!(
                    source
                        .nodes()
                        .filter(|n| n.name().is_some_and(|name| name.starts_with("interior_")))
                        .count(),
                    8
                );
                0.65
            } else {
                0.7
            };
            let vertices = definition.parts.iter().flat_map(|p| &p.vertices);
            assert!(vertices
                .clone()
                .any(|v| (v.surface[1] - glass).abs() < 0.0001));
            assert!(vertices.clone().any(|v| v.surface[1] == 1.));
            assert!(vertices.clone().all(|v| (0. ..=1.).contains(&v.surface[1])));
            for wheel in definition
                .parts
                .iter()
                .filter(|p| matches!(p.role, PartRole::FrontWheel | PartRole::RearWheel))
            {
                // Tires and metal rim/hub details must move together at one axle.
                let first = wheel.vertices[0].color;
                assert!(wheel.vertices.iter().any(|v| v.color != first));
            }
        }
    }
    #[test]
    fn detailed_bus_has_passenger_cabin_glazing_and_runtime_lights() {
        let source = gltf::Gltf::from_slice(include_bytes!(
            "../../data/vehicle/bus/saint-laurent-urbain/model.glb"
        ))
        .unwrap();
        let definition = definitions()
            .iter()
            .find(|d| d.config.research_id == "vehicle-j02")
            .unwrap();
        assert_eq!(definition.config.category, "bus");
        assert_eq!(definition.config.qa["wheelStyle"], "recessed-vented-steel");
        assert_eq!(definition.config.qa["wheelVentCount"], 10);
        assert!(definition.config.road_vehicle);
        assert_eq!(source.images().count(), 0);
        let nodes: std::collections::HashMap<_, _> =
            source.nodes().map(|n| (n.name().unwrap(), n)).collect();
        assert_eq!(nodes.len(), source.nodes().count());
        for name in [
            "body_shell",
            "engine_hatch",
            "front_bumper",
            "rear_bumper",
            "roof_panel",
            "interior_floor",
            "interior_dashboard",
            "interior_rear_platform",
            "rear_engine_grille",
            "head_fl",
            "head_fr",
            "taillight_-1",
            "taillight_1",
            "glass_door_front_right_forward",
            "glass_door_rear_right_forward",
        ] {
            assert!(nodes.contains_key(name), "missing bus part: {name}");
        }
        let glass: Vec<_> = nodes
            .iter()
            .filter(|(name, _)| {
                name.starts_with("glass_") || matches!(**name, "windshield" | "rear_glass")
            })
            .collect();
        assert_eq!(glass.len(), 23);
        for (_, node) in glass {
            for primitive in node.mesh().unwrap().primitives() {
                assert_eq!(
                    primitive.material().alpha_mode(),
                    gltf::material::AlphaMode::Blend
                );
                assert_eq!(
                    primitive
                        .material()
                        .pbr_metallic_roughness()
                        .base_color_factor()[3],
                    0.7
                );
            }
        }
        let interior_primitives: usize = nodes
            .iter()
            .filter(|(name, _)| name.starts_with("interior_"))
            .map(|(_, node)| node.mesh().unwrap().primitives().count())
            .sum();
        assert!((10..=15).contains(&interior_primitives));
        assert_eq!(
            definition.config.qa["interiorPrimitives"],
            interior_primitives
        );
        assert_eq!(
            nodes
                .keys()
                .filter(|n| n.starts_with("interior_bench_") && n.ends_with("_cushion"))
                .count(),
            4
        );
        // Curved source glazing must retain its multiple depths after separation.
        let blob = source.blob.as_deref().unwrap();
        for name in ["windshield", "glass_right_0", "glass_left_0"] {
            let mut lo = glam::Vec3::splat(f32::INFINITY);
            let mut hi = glam::Vec3::splat(f32::NEG_INFINITY);
            for primitive in nodes[name].mesh().unwrap().primitives() {
                for p in primitive.reader(|_| Some(blob)).read_positions().unwrap() {
                    lo = lo.min(glam::Vec3::from(p));
                    hi = hi.max(glam::Vec3::from(p));
                }
            }
            assert!(
                (hi - lo).min_element() > 0.001,
                "flattened source glazing: {name}"
            );
        }
        assert_eq!(definition.headlights.len(), 2);
        assert_eq!(definition.rear_emitters.len(), 2);
        for name in [
            "wheel_front_left",
            "wheel_front_right",
            "wheel_rear_left",
            "wheel_rear_right",
        ] {
            let wheel = nodes[name].mesh().unwrap();
            assert!((2..=3).contains(&wheel.primitives().count()));
            for primitive in wheel.primitives() {
                let finish = primitive.material().pbr_metallic_roughness();
                assert!(
                    finish.metallic_factor() > 0.5
                        || finish.base_color_factor()[..3].iter().all(|v| *v < 0.12),
                    "wheel must contain only rubber shades and plain steel"
                );
            }
            let steel = wheel
                .primitives()
                .find(|p| p.material().pbr_metallic_roughness().metallic_factor() > 0.5)
                .unwrap();
            vehicle_qa::vented_bus_rim(steel, blob, name.ends_with("left"));
        }
        for part in definition
            .parts
            .iter()
            .filter(|p| matches!(p.role, PartRole::FrontWheel | PartRole::RearWheel))
        {
            assert!(part
                .vertices
                .iter()
                .any(|v| v.color != part.vertices[0].color));
        }
        let front = definition
            .parts
            .iter()
            .find(|p| p.role == PartRole::FrontWheel)
            .unwrap();
        let rear = definition
            .parts
            .iter()
            .find(|p| p.role == PartRole::RearWheel)
            .unwrap();
        assert!((front.pivot.x - rear.pivot.x - definition.config.physics.wheelbase).abs() < 1e-4);
        assert!((front.pivot.y.abs() * 2. - definition.config.physics.track).abs() < 1e-4);
        assert!(definitions()
            .iter()
            .all(|d| d.config.research_id != "vehicle-j04"));
    }
    #[test]
    fn detailed_suv_keeps_cut_glass_cabin_and_authored_lights() {
        let bytes = include_bytes!("../../data/vehicle/suv/boreal-colline/model.glb");
        let source = gltf::Gltf::from_slice(bytes).unwrap();
        let definition = definitions()
            .iter()
            .find(|d| d.config.research_id == "vehicle-12")
            .unwrap();
        assert_eq!(definition.config.name, "Boreal Colline");
        assert_eq!(source.images().count(), 0);
        let names: std::collections::HashSet<_> = source.nodes().filter_map(|n| n.name()).collect();
        assert_eq!(names.len(), source.nodes().count());
        for name in [
            "windshield",
            "rear_glass",
            "glass_front_left",
            "glass_front_right",
            "glass_rear_left",
            "glass_rear_right",
            "glass_quarter_left",
            "glass_quarter_right",
            "hood",
            "door_front_left",
            "door_front_right",
            "door_rear_left",
            "door_rear_right",
            "tailgate",
            "interior_dashboard",
            "interior_console",
            "head_fl",
            "head_fr",
            "taillight_-1",
            "taillight_1",
        ] {
            assert!(names.contains(name), "missing detailed SUV part: {name}");
        }
        assert_eq!(names.iter().filter(|n| n.ends_with("_headrest")).count(), 6);
        let glass: Vec<_> = source
            .nodes()
            .filter(|n| {
                n.name().is_some_and(|name| {
                    matches!(name, "windshield" | "rear_glass") || name.starts_with("glass_")
                })
            })
            .collect();
        assert_eq!(glass.len(), 8);
        for node in glass {
            for primitive in node.mesh().unwrap().primitives() {
                assert_eq!(
                    primitive.material().alpha_mode(),
                    gltf::material::AlphaMode::Blend
                );
                assert_eq!(
                    primitive
                        .material()
                        .pbr_metallic_roughness()
                        .base_color_factor()[3],
                    0.7
                );
            }
        }
        assert!(definition.headlights.len() >= 2);
        assert_eq!(definition.rear_emitters.len(), 2);
        let front = definition
            .parts
            .iter()
            .find(|p| p.role == PartRole::FrontWheel)
            .unwrap();
        let rear = definition
            .parts
            .iter()
            .find(|p| p.role == PartRole::RearWheel)
            .unwrap();
        assert!((front.pivot.x - rear.pivot.x - definition.config.physics.wheelbase).abs() < 1e-4);
        assert!((front.pivot.y.abs() * 2. - definition.config.physics.track).abs() < 1e-4);
    }
    #[test]
    fn detailed_pickup_keeps_open_bed_cabin_glass_and_runtime_lenses() {
        let source = gltf::Gltf::from_slice(include_bytes!(
            "../../data/vehicle/pickup/castor-rive/model.glb"
        ))
        .unwrap();
        let definition = definitions()
            .iter()
            .find(|d| d.config.research_id == "vehicle-v2-01")
            .unwrap();
        assert_eq!(definition.config.name, "Castor Rive");
        assert_eq!(definition.config.category, "pickup");
        assert_eq!(source.images().count(), 0);
        let nodes: std::collections::HashMap<_, _> = source
            .nodes()
            .map(|node| (node.name().unwrap(), node))
            .collect();
        assert_eq!(nodes.len(), source.nodes().count());
        for name in [
            "bed_floor",
            "bed_front_liner",
            "tailgate",
            "cab_rear_panel",
            "door_front_left",
            "door_front_right",
            "door_rear_left",
            "door_rear_right",
            "interior_dashboard",
            "interior_console",
            "steering_wheel_ring",
            "head_fl",
            "head_fr",
            "taillight_-1",
            "taillight_1",
        ] {
            assert!(nodes.contains_key(name), "missing pickup part: {name}");
        }
        assert_eq!(nodes.keys().filter(|n| n.ends_with("_headrest")).count(), 4);
        for name in [
            "windshield",
            "rear_glass",
            "glass_front_left",
            "glass_front_right",
            "glass_rear_left",
            "glass_rear_right",
        ] {
            for primitive in nodes[name].mesh().unwrap().primitives() {
                assert_eq!(
                    primitive.material().alpha_mode(),
                    gltf::material::AlphaMode::Blend
                );
                assert_eq!(
                    primitive
                        .material()
                        .pbr_metallic_roughness()
                        .base_color_factor()[3],
                    0.7
                );
            }
        }
        let binary = source.blob.as_deref().unwrap();
        let positions = |name: &str| {
            nodes[name]
                .mesh()
                .unwrap()
                .primitives()
                .flat_map(|p| {
                    p.reader(|_| Some(binary))
                        .read_positions()
                        .unwrap()
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        // The cargo floor is below the bed rail and behind the closed cabin.
        assert!(positions("bed_floor")
            .iter()
            .all(|p| p[0] < -0.89 && p[1] < 1.0));
        assert!(positions("interior_dashboard").iter().all(|p| p[0] > 0.9));
        // Door trim must stay inside the source body rather than poke through its skin.
        for name in ["interior_armrest_-0.94", "interior_armrest_0.94"] {
            assert!(positions(name).iter().all(|p| p[2].abs() < 0.87));
        }
        assert_eq!(definition.headlights.len(), 2);
        assert_eq!(definition.rear_emitters.len(), 2);
        let expected_lens_vertices: usize = ["taillight_-1", "taillight_1"]
            .iter()
            .map(|name| positions(name).len())
            .sum();
        assert_eq!(definition.rear_lenses.len(), expected_lens_vertices);
        let tailgate_edge = positions("tailgate")
            .iter()
            .map(|p| p[2].abs())
            .fold(0., f32::max);
        for name in ["taillight_-1", "taillight_1"] {
            assert!(
                positions(name)
                    .iter()
                    .all(|p| p[2].abs() < tailgate_edge && (p[0] + 2.75).abs() < 0.025),
                "pickup lenses must sit on the tailgate, not outside its corners"
            );
        }
    }
    #[test]
    fn service_variants_reuse_compact_cabin_wheels_and_keep_distinct_roof_parts() {
        let base = gltf::Gltf::from_slice(include_bytes!(
            "../../data/vehicle/compact/laurent-clair/model.glb"
        ))
        .unwrap();
        for (path, bytes, id, marker) in [
            (
                "taxi",
                include_bytes!("../../data/vehicle/taxi/bonjour-electrique/model.glb").as_slice(),
                "vehicle-j01",
                "taxi_roof_sign",
            ),
            (
                "police",
                include_bytes!("../../data/vehicle/emergency/laurent-vigile/model.glb").as_slice(),
                "vehicle-j06",
                "roof_beacon_red",
            ),
        ] {
            let source = gltf::Gltf::from_slice(bytes).unwrap();
            let names: std::collections::HashSet<_> =
                source.nodes().filter_map(|n| n.name()).collect();
            assert_eq!(source.images().count(), 0);
            assert_eq!(source.nodes().count(), names.len());
            assert!(names.contains(marker));
            for node in base.nodes() {
                assert!(
                    names.contains(node.name().unwrap()),
                    "{path}: missing compact component"
                );
                let name = node.name().unwrap();
                if name.starts_with("wheel_")
                    || name.starts_with("interior_")
                    || matches!(name, "windshield" | "side_windows" | "rear_window")
                {
                    let copied = source.nodes().find(|n| n.name() == Some(name)).unwrap();
                    let indices = |node: gltf::Node<'_>| {
                        node.mesh()
                            .unwrap()
                            .primitives()
                            .map(|p| p.indices().unwrap().count())
                            .sum::<usize>()
                    };
                    assert_eq!(
                        indices(node),
                        indices(copied),
                        "{path}: changed shared component {name}"
                    );
                }
            }
            let def = definitions()
                .iter()
                .find(|d| d.config.research_id == id)
                .unwrap();
            assert!((def.config.physics.wheel_radius - 0.323).abs() < 1e-5);
            assert!(def
                .parts
                .iter()
                .flat_map(|p| &p.vertices)
                .any(|v| (v.surface[1] - 0.65).abs() < 1e-5));
            if path == "police" {
                let body = def.parts.iter().flat_map(|p| &p.vertices);
                assert!(body.clone().any(|v| v.surface[3] == 65.));
                assert!(body.clone().any(|v| v.surface[3] == 66.));
                assert!(names.contains("police_blue_side_stripe"));
                assert_eq!(
                    def.headlights.len(),
                    definitions()
                        .iter()
                        .find(|d| d.config.research_id == "vehicle-v2-09")
                        .unwrap()
                        .headlights
                        .len()
                );
            }
        }
        assert_eq!(vehicle_lamp_marker("roof_lightbar_mount"), 0.);
    }
    #[test]
    fn everyday_catalog_keeps_only_detailed_bodies_with_shared_paint_masks() {
        let mut ids = Vec::new();
        for definition in definitions() {
            if matches!(
                definition.config.category.as_str(),
                "compact" | "suv" | "pickup"
            ) {
                ids.push(definition.config.research_id.as_str());
                assert_eq!(
                    (0..4)
                        .map(|slot| definition.paint_variant(slot))
                        .collect::<Vec<_>>(),
                    [0, 1, 2, 3]
                );
                let painted: Vec<_> = definition.parts[0]
                    .vertices
                    .iter()
                    .filter(|v| v.surface[0] >= 0. && v.surface[2].floor() == 1.)
                    .collect();
                assert!(
                    !painted.is_empty(),
                    "missing paint mask: {}",
                    definition.config.name
                );
                assert!(painted
                    .iter()
                    .all(|v| v.surface[1] == 1. && v.surface[3] == 0.));
                assert!(
                    definition
                        .parts
                        .iter()
                        .filter(|p| matches!(p.role, PartRole::FrontWheel | PartRole::RearWheel))
                        .flat_map(|p| &p.vertices)
                        .all(|v| v.surface[2].floor() == 0.),
                    "wheels must retain authored rubber and rim colors"
                );
            } else {
                assert!(
                    (0..4).all(|slot| definition.paint_variant(slot) == 0),
                    "service livery must remain fixed"
                );
            }
        }
        ids.sort();
        assert_eq!(ids, ["vehicle-12", "vehicle-v2-01", "vehicle-v2-09"]);
    }
    #[test]
    fn runabout_outboard_turns_about_its_fixed_transom_bracket() {
        let hermes = crate::hermes::Hermes::test_map();
        let mut boat = super::super::Vehicle::spawn(hermes.city());
        let model = definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-boat-runabout")
            .unwrap();
        boat.set_catalog_model(Some(model));
        let def = &definitions()[model];
        assert_eq!(def.parts.len(), 2);
        assert_eq!(def.parts[1].role, PartRole::Outboard);
        let straight = def.part_transforms(&boat);
        for angle in [-0.5, 0.5] {
            boat.steering = angle;
            let turned = def.part_transforms(&boat);
            assert_eq!(straight[0], turned[0]);
            let relative = straight[1].inverse() * turned[1];
            assert!(relative.transform_point3(Vec3::ZERO).length() < 1e-4);
            assert!(
                relative.transform_vector3(Vec3::X).distance(Vec3::new(
                    angle.cos(),
                    -angle.sin(),
                    0.
                )) < 1e-4
            );
        }
    }

    #[test]
    fn aircraft_nose_gear_steers_the_taxi_path() {
        use super::super::{aircraft::Model, Controls, SpawnPoint, Vehicle};

        let hermes = crate::hermes::Hermes::test_map_from_source(
            "field=concrete(-4,-4,4,-4,4,4,-4,4); elevation(field,0.122001); player_start(0,0,0);",
        );
        let city = hermes.city();
        for model in Model::ALL {
            for direction in [-1., 1.] {
                let mut plane = Vehicle::spawn_at(
                    city,
                    SpawnPoint {
                        point: [0., 0.],
                        heading: Some(0.),
                    },
                )
                .unwrap();
                plane.set_catalog_model(Some(model.catalog_index()));
                let def = plane.definition().unwrap();
                let neutral = def.part_transforms(&plane);
                let steer = Controls {
                    steering: direction,
                    ..Controls::default()
                };
                plane.step(city, steer, 0.5);
                assert_eq!(plane.yaw, 0.);
                assert!(plane.steering * direction > 0.5);
                let parked_angle = plane.steering;
                let turned = def.part_transforms(&plane);
                for (i, part) in def.parts.iter().enumerate() {
                    if let PartRole::AircraftGear(gear) = part.role {
                        let angle = if gear == Gear::Nose { parked_angle } else { 0. };
                        let rotation = neutral[i].inverse() * turned[i];
                        let expected = Quat::from_rotation_z(angle) * Vec3::X;
                        assert!(rotation.transform_vector3(Vec3::X).distance(expected) < 1e-4);
                    }
                }
                let start = plane.position;
                plane.velocity = Vec3::X * 4.;
                plane.step(city, steer, 0.25);
                assert!(plane.yaw * direction > 0.);
                assert!((plane.position.y - start.y) * direction > 0.);
                let speed = plane
                    .velocity
                    .dot(Vec3::new(plane.yaw.cos(), plane.yaw.sin(), 0.));
                let expected_rate = speed * plane.steering.tan() / plane.wheelbase();
                assert!((plane.aircraft_rates.z / expected_rate - 1.).abs() < 0.02);
                plane.step(city, Controls::default(), 1.);
                assert!(plane.steering.abs() < 0.03);
            }
        }
    }

    #[test]
    fn aircraft_render_parts_hinge_with_the_commanded_surfaces() {
        let hermes = crate::hermes::Hermes::test_map();
        for model in super::super::aircraft::Model::ALL {
            let mut plane = super::super::Vehicle::spawn(hermes.city());
            plane.set_catalog_model(Some(model.catalog_index()));
            plane.position = Vec3::ZERO;
            plane.yaw = 0.;
            let def = plane.definition().unwrap();
            let neutral = def.part_transforms(&plane);
            plane.aircraft_surfaces = Vec3::ONE;
            let actuated = def.part_transforms(&plane);
            let mut moving = 0;
            for (i, part) in def.parts.iter().enumerate() {
                let PartRole::FlightControl(surface, axis) = part.role else {
                    assert_eq!(neutral[i], actuated[i]);
                    continue;
                };
                moving += 1;
                for hinge in [Vec3::ZERO, axis] {
                    assert!(
                        neutral[i]
                            .transform_point3(hinge)
                            .distance(actuated[i].transform_point3(hinge))
                            < 1e-5
                    );
                }
                // The panel centroid lies aft of the hinge.
                let center = part
                    .vertices
                    .iter()
                    .map(|v| Vec3::from_array(v.position))
                    .sum::<Vec3>()
                    / part.vertices.len() as f32;
                let motion =
                    actuated[i].transform_point3(center) - neutral[i].transform_point3(center);
                assert!(
                    match surface {
                        Surface::LeftAileron | Surface::Elevator => motion.z > 0.,
                        Surface::RightAileron => motion.z < 0.,
                        Surface::Rudder => motion.y > 0.,
                    },
                    "{model:?} {surface:?}: {motion:?}"
                );
            }
            assert_eq!(moving, 5);
        }
    }

    #[test]
    fn every_export_loads_with_authored_running_gear_and_si_data() {
        assert_eq!(definitions().len(), 19);
        assert_eq!(
            definitions()
                .iter()
                .filter(|d| d.config.road_vehicle)
                .count(),
            10
        );
        let mut page_ids = std::collections::HashSet::new();
        for def in definitions() {
            assert!(
                page_ids.insert(&def.config.research_id),
                "duplicate research page ID"
            );
            assert!(def.config.research_id.starts_with("vehicle-"));
            assert!(!def.config.category.is_empty());
            assert_eq!(def.config.qa["claydashExport"], "completed");
            if def.config.vehicle_kind == VehicleKind::Aircraft {
                for gear in [Gear::Nose, Gear::Left, Gear::Right] {
                    let parts = def
                        .parts
                        .iter()
                        .filter(|p| p.role == PartRole::AircraftGear(gear));
                    assert_eq!(parts.count(), 4, "strut, tire and two hubs");
                }
                continue;
            }
            if def.config.vehicle_kind == VehicleKind::Boat
                || def.config.research_id == SNOWMOBILE_ID
            {
                assert!(!def.config.road_vehicle);
                continue;
            }
            let wheels = def
                .parts
                .iter()
                .filter(|p| matches!(p.role, PartRole::FrontWheel | PartRole::RearWheel))
                .collect::<Vec<_>>();
            assert!(
                if def.is_bicycle() {
                    wheels.len() == 2
                } else {
                    [4, 6].contains(&wheels.len())
                },
                "{}",
                def.config.name
            );
            assert_eq!(
                wheels
                    .iter()
                    .filter(|p| p.role == PartRole::FrontWheel)
                    .count(),
                if def.is_bicycle() { 1 } else { 2 }
            );
            for part in wheels {
                assert!(part.pivot.x.abs() > 0.5);
                // Wheel vertices lie on a circular rim, not a square profile.
                let radius = def.config.physics.wheel_radius;
                for v in part.vertices.iter().filter(|v| v.surface[0] >= 0.) {
                    let radial = (v.position[0].powi(2) + v.position[2].powi(2)).sqrt();
                    assert!(
                        radial <= radius + 0.0001,
                        "wheel exceeds cylindrical radius"
                    );
                }

                let min = part
                    .vertices
                    .iter()
                    .map(|v| v.position[0])
                    .fold(f32::INFINITY, f32::min);
                let max = part
                    .vertices
                    .iter()
                    .map(|v| v.position[0])
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!(
                    (min + max).abs() < if def.is_bicycle() { 0.0011 } else { 1e-4 },
                    "wheel must rotate around its axle (native bike tessellation within 1.1 mm)"
                );
            }
        }
    }
    #[test]
    fn gpu_part_transforms_match_the_cpu_reference_for_every_model() {
        let hermes = crate::hermes::Hermes::test_map();
        let mut vehicle = super::super::Vehicle::spawn(hermes.city());
        vehicle.position += Vec3::new(1.2, -3.4, 0.5);
        vehicle.yaw = 0.7;
        vehicle.pitch = 0.13;
        vehicle.roll = -0.17;
        vehicle.body_lean = glam::Vec2::new(0.08, -0.04);
        vehicle.steering = -0.3;
        vehicle.wheel_angle = 1.4;
        for definition in definitions() {
            let reference = definition.mesh(&vehicle);
            let mut offset = 0;
            for (part, model) in definition
                .parts
                .iter()
                .zip(definition.part_transforms(&vehicle))
            {
                for vertex in &part.vertices {
                    let actual = &reference.vertices[offset];
                    let position = model.transform_point3(Vec3::from_array(vertex.position));
                    let normal = model.transform_vector3(Vec3::from_array(vertex.normal)) * 100.;
                    assert!(position.distance(Vec3::from_array(actual.position)) < 0.00001);
                    assert!(normal.distance(Vec3::from_array(actual.normal)) < 0.00001);
                    offset += 1;
                }
            }
            assert_eq!(offset, reference.vertices.len());
            assert_eq!(
                definition
                    .parts
                    .iter()
                    .filter(|p| p.role == PartRole::Body)
                    .count(),
                1
            );
        }
    }
    #[test]
    fn wheel_names_are_an_explicit_contract() {
        assert_eq!(part_role("wheel_front_left").unwrap(), PartRole::FrontWheel);
        assert!(part_role("wheel_frnot_left").is_err());
    }
}

#[cfg(test)]
mod cycling_tests {
    use super::*;
    #[test]
    fn bicycle_rendered_pedal_centers_match_rider_contact_targets() {
        let world = super::super::DynamicWorld::new(&crate::hermes::Hermes::test_map());
        let def = definitions().iter().find(|d| d.is_bicycle()).unwrap();
        for frame in 0..24 {
            let mut bike = super::super::Vehicle::spawn(crate::hermes::Hermes::test_map().city());
            bike.set_catalog_model(world.player().definition);
            bike.pedal_angle = frame as f32 * std::f32::consts::TAU / 24.;
            for part in def.parts.iter().filter(|p| p.role == PartRole::Pedal) {
                let side = if part.pivot.y > 0. { 0 } else { 1 };
                let rendered = part.animated_pivot(&bike) + Vec3::Z * bike.ride_height();
                let target = super::super::cycling_pose::pedal_position(side, bike.pedal_angle);
                assert!(
                    rendered.distance(target) < 0.0001,
                    "{rendered:?} vs {target:?}"
                );
            }
        }
    }
}

#[cfg(test)]
mod baggage_cargo_tests {
    use super::*;
    #[test]
    fn baggage_ground_props_contain_only_cargo_and_rest_on_the_floor() {
        let definition = definitions()
            .iter()
            .find(|d| d.config.research_id == super::super::baggage::MODEL_ID)
            .unwrap();
        let mut lowest = f32::INFINITY;
        let mut count = 0;
        for (i, transform) in definition.ground_cargo_parts() {
            assert_eq!(definition.parts[i].role, PartRole::BaggageCargo);
            for vertex in &definition.parts[i].vertices {
                let p = transform.transform_point3(Vec3::from_array(vertex.position));
                lowest = lowest.min(p.z);
                assert!(p.z >= -0.0001);
                assert!(p.x.abs() < 1.4 && p.y.abs() < 1.1);
            }
            count += 1;
        }
        assert!(count > 0);
        assert!(lowest.abs() < 0.0001);
    }
}
