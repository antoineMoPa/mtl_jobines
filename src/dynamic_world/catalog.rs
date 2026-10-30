//! Embedded, shared web/native vehicle assets. GLB: +X nose, +Y up, +Z right, metres.
//! Exact wheel_* names select axle-local spin/steering; other nodes belong to the sprung body.
use glam::{Mat4, Quat, Vec3};
#[cfg(test)]
use opencity::runtime::Mesh;
use opencity::runtime::{SurfaceKind, Vertex};
use serde::Deserialize;
use std::sync::OnceLock;

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
struct Part {
    role: PartRole,
    pivot: Vec3,
    vertices: Vec<Vertex>,
}
pub struct Definition {
    pub config: Config,
    parts: Vec<Part>,
}
fn game_vector(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], -p[2], p[1])
}
impl Definition {
    fn load(config: &[u8], bytes: &[u8]) -> Result<Self, String> {
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
        let mut parts = Vec::new();
        fn walk(
            node: gltf::Node<'_>,
            parent: Mat4,
            binary: &[u8],
            ride: f32,
            parts: &mut Vec<Part>,
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
                    let color = primitive
                        .material()
                        .pbr_metallic_roughness()
                        .base_color_factor();
                    for index in indices {
                        let i = index as usize;
                        let position = positions.get(i).ok_or("Index outside positions")?;
                        let normal = normals.get(i).ok_or("Index outside normals")?;
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
                            color: [color[0], color[1], color[2]],
                            surface: [SurfaceKind::Prop as u32 as f32, 0., 0., 0.],
                        });
                    }
                }
                parts.push(Part {
                    role,
                    pivot,
                    vertices,
                });
            }
            for child in node.children() {
                walk(child, transform, binary, ride, parts)?;
            }
            Ok(())
        }
        for node in glb.default_scene().ok_or("Missing default scene")?.nodes() {
            walk(
                node,
                Mat4::IDENTITY,
                binary,
                config.physics.height_offset,
                &mut parts,
            )?;
        }
        if parts.is_empty() {
            return Err("Empty vehicle GLB".into());
        }
        // Every fixed panel shares the body's transform; upload it as one rigid part.
        let mut body = Vec::new();
        let mut articulated = Vec::new();
        for part in parts {
            if part.role == PartRole::Body {
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
                vertices: body,
            },
        );
        Ok(Self {
            config,
            parts: articulated,
        })
    }
    pub fn part_vertices(&self) -> impl Iterator<Item = &[Vertex]> {
        self.parts.iter().map(|p| p.vertices.as_slice())
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
        ASSETS
            .iter()
            .map(|(json, glb)| Definition::load(json, glb).expect("valid bundled Claydash vehicle"))
            .collect()
    })
}
const ASSETS: &[(&[u8], &[u8])] = &[
    asset!("compact/laurent-laurentien"),
    asset!("suv/boreal-sentier"),
    asset!("suv/laurent-traverse"),
    asset!("hatchback/montcalm-trois"),
    asset!("hatchback/laurent-moineau"),
    asset!("hatchback/boreal-echo"),
    asset!("minivan/castor-familial"),
    asset!("suv/boreal-colline"),
    asset!("taxi/bonjour-electrique"),
    asset!("bus/saint-laurent-urbain"),
    asset!("municipal/castor-blizzard"),
    asset!("municipal/castor-niveleur"),
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
    fn every_export_loads_with_four_or_six_axle_local_wheels_and_si_data() {
        assert_eq!(definitions().len(), 17);
        assert_eq!(
            definitions()
                .iter()
                .filter(|d| d.config.road_vehicle)
                .count(),
            15
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
