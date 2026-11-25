//! A winter equipment pickup. Geometry stays shared; only the six placements move.
use super::{pedestrian_catalog, player, DynamicWorld, SpawnPoint, METERS_PER_UNIT};
use crate::{hermes::Hermes, season::Season};
use glam::{Mat4, Quat, Vec3};
use opencity::runtime::{Mesh, SurfaceKind};
use std::sync::OnceLock;

#[derive(Clone, Copy)]
struct Placement {
    position: Vec3,
    yaw: f32,
}
pub(super) struct SkiSet {
    home: Option<Placement>,
    ground: Option<Placement>,
    worn: bool,
}
impl SkiSet {
    pub fn new(hermes: &Hermes) -> Self {
        let home = hermes.ski_set.and_then(|start: SpawnPoint| {
            let support = hermes.city().surface_at(start.point, f32::INFINITY)?;
            (support.kind != SurfaceKind::Water).then_some(Placement {
                position: Vec3::new(start.point[0], start.point[1], support.height)
                    * METERS_PER_UNIT,
                yaw: start.heading.unwrap_or(0.),
            })
        });
        Self {
            home,
            ground: home,
            worn: false,
        }
    }
    pub fn reset(&mut self) {
        self.ground = self.home;
        self.worn = false;
    }
}

#[derive(Clone, Copy)]
enum Piece {
    Ski,
    Boot,
    Pole,
}
impl Piece {
    fn from_node(name: &str) -> Self {
        if name.starts_with("ski_") {
            Self::Ski
        } else if name.starts_with("boot_") {
            Self::Boot
        } else if name.starts_with("pole_") {
            Self::Pole
        } else {
            panic!("unknown ski-set part: {name}")
        }
    }
    fn offset(self) -> usize {
        match self {
            Self::Ski => 0,
            Self::Boot => 2,
            Self::Pole => 4,
        }
    }
}
fn meshes() -> &'static [Mesh; 6] {
    static ASSET: OnceLock<[Mesh; 6]> = OnceLock::new();
    ASSET.get_or_init(|| {
        let bytes = crate::web_asset!(
            "../../data/equipment/ski-set/model.glb",
            "data/equipment/ski-set/model.glb"
        );
        let glb = gltf::Gltf::from_slice(bytes).expect("authored ski set");
        let binary = glb.blob.as_deref().expect("embedded equipment geometry");
        let mut parts: [Mesh; 6] = std::array::from_fn(|_| Mesh::default());
        for node in glb.nodes() {
            if node.mesh().is_none() {
                continue;
            }
            let piece = Piece::from_node(node.name().expect("named ski-set part"));
            let mut vertices = Vec::new();
            crate::hermes::external_models::append_rigid_mesh(
                &node,
                Mat4::IDENTITY,
                binary,
                &mut vertices,
            )
            .expect("ski-set mesh");
            let left = vertices.iter().map(|v| v.position[1]).sum::<f32>() > 0.;
            parts[piece.offset() + usize::from(!left)]
                .vertices
                .extend(vertices);
        }
        assert!(parts.iter().all(|p| !p.vertices.is_empty()));
        parts
    })
}
fn append(source: &Mesh, output: &mut Mesh, transform: Mat4) {
    output.vertices.extend(source.vertices.iter().map(|v| {
        let mut vertex = *v;
        vertex.position = transform.transform_point3(v.position.into()).to_array();
        vertex.normal = (transform.transform_vector3(v.normal.into()) * METERS_PER_UNIT).to_array();
        vertex
    }));
}
fn placement(position: Vec3, yaw: f32) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::splat(1. / METERS_PER_UNIT),
        Quat::from_rotation_z(yaw),
        position / METERS_PER_UNIT,
    )
}
impl DynamicWorld {
    pub fn is_wearing_skis(&self) -> bool {
        self.skis.worn
    }
    pub fn skis_hint(&self) -> Option<&'static str> {
        let person = self.pedestrian.as_ref()?;
        if self.season != Season::Winter
            || self.health == 0
            || self.entry.is_some()
            || self.metro.aboard
            || self.riding_chairlift()
            || person.swimming
            || person.fallen.is_some()
            || self.arrest_progress().is_some()
        {
            return None;
        }
        if self.skis.worn {
            return Some("Retirer les skis");
        }
        let ground = self.skis.ground?;
        let feet = person.position - Vec3::Z * player::HALF_HEIGHT;
        (feet.distance(ground.position) <= 2.).then_some("Chausser les skis")
    }
    pub fn toggle_skis(&mut self) -> bool {
        if self.skis_hint().is_none() {
            return false;
        }
        if self.skis.worn {
            self.put_down_skis();
        } else {
            self.skis.worn = true;
            self.skis.ground = None;
        }
        true
    }
    pub(super) fn put_down_skis(&mut self) {
        if !self.skis.worn {
            return;
        }
        if let Some(person) = &self.pedestrian {
            self.skis.ground = Some(Placement {
                position: person.position - Vec3::Z * player::HALF_HEIGHT,
                yaw: person.yaw,
            });
        } else {
            self.skis.ground = self.skis.home;
        }
        self.skis.worn = false;
    }
    pub(super) fn append_skis(&self, output: &mut Mesh, player_delta: Mat4) {
        if self.season != Season::Winter {
            return;
        }
        if self.skis.worn {
            let Some(person) = self
                .pedestrian
                .as_ref()
                .filter(|p| !p.swimming && p.fallen.is_none())
            else {
                return;
            };
            if self.entry.is_some() {
                return;
            }
            let instance = self
                .chairlift_rider_instance()
                .unwrap_or_else(|| person.instance(self.season));
            let definition = &pedestrian_catalog::definitions()[instance.model];
            let joints = definition.joint_worlds(instance.pose.expect("posed player"));
            for (index, mesh) in meshes().iter().enumerate() {
                let left = index % 2 == 0;
                let pole = index >= 4;
                let joint = definition.joint(match (pole, left) {
                    (false, true) => "Foot_L",
                    (false, false) => "Foot_R",
                    (true, true) => "Hand_L",
                    (true, false) => "Hand_R",
                });
                let point = instance
                    .transform
                    .transform_point3(joints[joint].w_axis.truncate())
                    * METERS_PER_UNIT;
                let side = if left { 1. } else { -1. };
                let (point, datum) = if pole {
                    (point, Vec3::new(0., side * 0.30, 1.13))
                } else {
                    (point - Vec3::Z * 0.09, Vec3::new(0., side * 0.13, 0.))
                };
                let mut transform = placement(point, person.yaw);
                if pole {
                    // Lean the shaft from the hand to the snow instead of burying its tip.
                    let height =
                        (point.z - person.position.z + player::HALF_HEIGHT).clamp(0.15, 1.13);
                    transform *= Mat4::from_rotation_y((height / 1.13).acos());
                }
                let transform = player_delta * transform * Mat4::from_translation(-datum);
                append(mesh, output, transform);
            }
        } else if let Some(ground) = self
            .skis
            .ground
            .filter(|g| g.position.distance(self.player_position()) < 60.)
        {
            let transform = placement(ground.position, ground.yaw);
            for mesh in meshes() {
                append(mesh, output, transform);
            }
        }
    }
}

#[cfg(test)]
mod tests;
