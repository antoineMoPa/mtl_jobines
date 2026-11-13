//! Embedded, shared web/native vehicle assets. GLB: +X nose, +Y up, +Z right, metres.
//! Exact wheel_* names select axle-local spin/steering; other nodes belong to the sprung body.
use glam::{Mat4, Quat, Vec3};
#[cfg(test)]
use opencity::runtime::Mesh;
use opencity::runtime::{SurfaceKind, Vertex};
use serde::Deserialize;
use std::sync::OnceLock;
pub(crate) const SNOWPLOW_ID: &str = "vehicle-j03";

#[cfg(test)]
#[path = "catalog_collecteur_tests.rs"]
mod collecteur_tests;
#[cfg(test)]
#[path = "catalog_snowplow_tests.rs"]
mod snowplow_tests;
#[cfg(test)]
#[path = "catalog_vehicle_qa.rs"]
mod vehicle_qa;

#[derive(Deserialize, Debug)]
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
}
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct Dimensions {
    pub length: f32,
    pub width: f32,
    pub height: f32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub name: String,
    pub research_id: String,
    pub category: String,
    pub road_vehicle: bool,
    pub model: String,
    pub dimensions: Dimensions,
    pub physics: Physics,
    #[serde(default)]
    pub(crate) sound: crate::audio::VehicleSound,
    pub qa: serde_json::Value,
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum PartRole {
    Body,
    RearWheel,
    FrontWheel,
}
fn part_role(name: &str) -> Result<PartRole, String> {
    match name {
        "wheel_front_left" | "wheel_front_right" => Ok(PartRole::FrontWheel),
        "wheel_rear_left" | "wheel_rear_right" | "wheel_middle_left" | "wheel_middle_right" => {
            Ok(PartRole::RearWheel)
        }
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
pub struct Definition {
    pub config: Config,
    pub(crate) texture_png: Option<Vec<u8>>,
    parts: Vec<Part>,
    rear_lenses: Vec<Vertex>,
    headlights: Vec<Vec3>,
    rear_emitters: Vec<Vec3>,
}
/// Explicit lens roles keep housings and roof beacons out of the light sources.
fn headlight_lens(name: &str) -> bool {
    matches!(
        name,
        "frontlamp0"
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
impl Definition {
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
    fn load(config: &[u8], bytes: &[u8], texture_layer: u32) -> Result<Self, String> {
        let config: Config = serde_json::from_slice(config).map_err(|e| e.to_string())?;
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
                p.track,
                config.dimensions.length,
                config.dimensions.width,
                config.dimensions.height,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.)
            || p.max_steer_angle >= std::f32::consts::FRAC_PI_2
        {
            return Err(format!("Invalid SI vehicle configuration: {}", config.name));
        }
        let glb = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
        let binary = glb.blob.as_deref().ok_or("GLB has no embedded buffer")?;
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
        let mut parts = Vec::new();
        let mut rear_lenses = Vec::new();
        fn walk(
            node: gltf::Node<'_>,
            parent: Mat4,
            binary: &[u8],
            ride: f32,
            paint: Option<[f32; 4]>,
            texture_layer: u32,
            parts: &mut Vec<Part>,
            rear_lenses: &mut Vec<Vertex>,
        ) -> Result<(), String> {
            let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
            if let Some(mesh) = node.mesh() {
                let role = part_role(node.name().unwrap_or("body"))?;
                let pivot =
                    game_vector(transform.transform_point3(Vec3::ZERO).to_array()) - Vec3::Z * ride;
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
                                    if role == PartRole::Body && paint == Some(color) {
                                        1.
                                    } else {
                                        0.
                                    },
                                    vehicle_lamp_marker(node.name().unwrap_or("")),
                                ]
                            },
                        });
                    }
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
                    parts.push(Part {
                        role,
                        pivot,
                        vertices,
                        shadow_vertices: match shadow_role(node.name().unwrap_or("")) {
                            ShadowRole::Interior => Some(Vec::new()),
                            ShadowRole::Exterior => None,
                        },
                    });
                }
            }
            for child in node.children() {
                walk(
                    child,
                    transform,
                    binary,
                    ride,
                    paint,
                    texture_layer,
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
                config.physics.height_offset,
                paint,
                texture_layer,
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
        let mut articulated = Vec::new();
        for part in parts {
            if part.role == PartRole::Body {
                let caster = part.shadow_vertices.as_deref().unwrap_or(&part.vertices);
                shadow_body.extend(
                    caster
                        .chunks_exact(3)
                        .filter(|t| {
                            t.iter()
                                .all(|v| v.surface[1] >= 0.999 && !headlight_vertex(v))
                        })
                        .flatten()
                        .copied(),
                );
                body.extend(part.vertices);
            } else {
                articulated.push(part);
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
        if rear_lenses.is_empty() {
            return Err(format!("Missing authored rear lenses: {}", config.name));
        }
        let mut headlights = Vec::new();
        for left in [false, true] {
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for v in articulated[0]
                .vertices
                .iter()
                .filter(|v| headlight_vertex(v) && (v.position[1] > 0.) == left)
            {
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
            config,
            texture_png,
            parts: articulated,
            rear_lenses,
            headlights,
            rear_emitters,
        })
    }
    pub fn part_vertices(&self) -> impl Iterator<Item = &[Vertex]> {
        self.parts.iter().map(|p| p.vertices.as_slice())
    }
    pub(crate) fn part_shadow_vertices(&self) -> impl Iterator<Item = &[Vertex]> {
        self.parts
            .iter()
            .map(|part| part.shadow_vertices.as_deref().unwrap_or(&part.vertices))
    }
    pub fn part_transforms(&self, vehicle: &super::Vehicle) -> Vec<Mat4> {
        let body = Mat4::from_scale_rotation_translation(
            Vec3::splat(1. / super::METERS_PER_UNIT),
            vehicle.orientation(),
            vehicle.position / super::METERS_PER_UNIT,
        );
        let lean = Quat::from_rotation_y(-vehicle.body_lean.x)
            * Quat::from_rotation_x(vehicle.body_lean.y);
        self.parts
            .iter()
            .map(|part| {
                let rotation = match part.role {
                    PartRole::Body => lean,
                    PartRole::RearWheel => Quat::from_rotation_y(vehicle.wheel_angle),
                    PartRole::FrontWheel => {
                        Quat::from_rotation_z(vehicle.steering)
                            * Quat::from_rotation_y(vehicle.wheel_angle)
                    }
                };
                body * Mat4::from_rotation_translation(
                    rotation,
                    if part.role == PartRole::Body {
                        Vec3::ZERO
                    } else {
                        part.pivot
                    },
                )
            })
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
        let lean = Quat::from_rotation_y(-vehicle.body_lean.x)
            * Quat::from_rotation_x(vehicle.body_lean.y);
        let orientation = vehicle.orientation();
        for part in &self.parts {
            let q = match part.role {
                PartRole::Body => lean,
                PartRole::RearWheel => Quat::from_rotation_y(vehicle.wheel_angle),
                PartRole::FrontWheel => {
                    Quat::from_rotation_z(vehicle.steering)
                        * Quat::from_rotation_y(vehicle.wheel_angle)
                }
            };
            for vertex in &part.vertices {
                let mut v = *vertex;
                let local = q * Vec3::from_array(v.position)
                    + if part.role == PartRole::Body {
                        Vec3::ZERO
                    } else {
                        part.pivot
                    };
                v.position =
                    ((orientation * local + vehicle.position) / super::METERS_PER_UNIT).to_array();
                v.normal = (orientation * q * Vec3::from_array(v.normal)).to_array();
                output.vertices.push(v);
            }
        }
    }
}
macro_rules! asset {
    ($path:literal) => {
        (
            include_bytes!(concat!("../../data/vehicle/", $path, "/vehicle.json")).as_slice(),
            include_bytes!(concat!("../../data/vehicle/", $path, "/model.glb")).as_slice(),
        )
    };
}
pub fn definitions() -> &'static [Definition] {
    static CATALOG: OnceLock<Vec<Definition>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut texture_layer = 0;
        ASSETS
            .iter()
            .map(|(json, glb)| {
                let definition = Definition::load(json, glb, texture_layer)
                    .expect("valid bundled Claydash vehicle");
                texture_layer += u32::from(definition.texture_png.is_some());
                definition
            })
            .collect()
    })
}
const ASSETS: &[(&[u8], &[u8])] = &[
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
];

#[cfg(test)]
mod tests {
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
                if index > 0 {
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
    fn authored_lenses_are_moved_once_without_adding_or_dropping_source_triangles() {
        for (definition, (_, bytes)) in definitions().iter().zip(ASSETS) {
            let glb = gltf::Gltf::from_slice(bytes).unwrap();
            let source_vertices: usize = glb
                .meshes()
                .flat_map(|m| m.primitives())
                .map(|p| p.indices().unwrap().count())
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
            assert_eq!(
                definition
                    .parts
                    .iter()
                    .map(|part| part.vertices.len())
                    .sum::<usize>()
                    + definition.rear_lenses.len(),
                source_vertices,
                "{}",
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
                    Some("runtime-packager" | "claydash-game-package" | "rust-source-cleanup")
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
            for wheel in definition.parts.iter().filter(|p| p.role != PartRole::Body) {
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
        assert_eq!(definition.config.qa["wheelStyle"], "plain-steel-cylinder");
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
            // A 48-sided closed cylinder has 48 * 4 triangles, without spokes/hubs.
            assert_eq!(
                steel
                    .reader(|_| Some(blob))
                    .read_indices()
                    .unwrap()
                    .into_u32()
                    .count(),
                48 * 4 * 3
            );
        }
        for part in definition.parts.iter().filter(|p| p.role != PartRole::Body) {
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
                    .filter(|v| v.surface[2] == 1.)
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
                        .skip(1)
                        .flat_map(|p| &p.vertices)
                        .all(|v| v.surface[2] == 0.),
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
    fn every_export_loads_with_four_or_six_axle_local_wheels_and_si_data() {
        assert_eq!(definitions().len(), 11);
        assert_eq!(
            definitions()
                .iter()
                .filter(|d| d.config.road_vehicle)
                .count(),
            9
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
            let wheels = def
                .parts
                .iter()
                .filter(|p| p.role != PartRole::Body)
                .collect::<Vec<_>>();
            assert!([4, 6].contains(&wheels.len()), "{}", def.config.name);
            assert_eq!(
                wheels
                    .iter()
                    .filter(|p| p.role == PartRole::FrontWheel)
                    .count(),
                2
            );
            for part in wheels {
                assert!(part.pivot.x.abs() > 0.5);
                // Wheel vertices lie on a circular rim, not a square profile.
                let radius = def.config.physics.wheel_radius;
                for v in &part.vertices {
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
                    (min + max).abs() < 1e-4,
                    "wheel must rotate around its axle"
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
