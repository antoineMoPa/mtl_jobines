//! A winter equipment pickup. Geometry stays shared; only the six placements move.
use super::{pedestrian_catalog, player, DynamicWorld, SpawnPoint, METERS_PER_UNIT};
mod movement;
mod pose;
use crate::{hermes::Hermes, season::Season};
use glam::{Mat4, Quat, Vec3};
use opencity::runtime::{Mesh, SurfaceKind};
use std::sync::OnceLock;

#[derive(Clone, Copy)]
struct Placement {
    position: Vec3,
    yaw: f32,
}
struct Slot {
    home: Placement,
    ground: Option<Placement>,
}
pub(super) struct SkiSet {
    slots: Vec<Slot>,
    active_slot: usize,
    pub(super) worn: bool,
    motion: Option<movement::Motion>,
    pose: Option<pose::Pose>,
}
impl SkiSet {
    pub fn new(hermes: &Hermes) -> Self {
        let slots = hermes
            .ski_sets
            .iter()
            .filter_map(|start: &SpawnPoint| {
                let support = hermes.city().surface_at(start.point, f32::INFINITY)?;
                (support.kind != SurfaceKind::Water).then(|| {
                    let home = Placement {
                        position: Vec3::new(start.point[0], start.point[1], support.height)
                            * METERS_PER_UNIT,
                        yaw: start.heading.unwrap_or(0.),
                    };
                    Slot {
                        home,
                        ground: Some(home),
                    }
                })
            })
            .collect();
        Self {
            slots,
            active_slot: 0,
            worn: false,
            motion: None,
            pose: None,
        }
    }
    fn nearest(&self, feet: Vec3) -> Option<usize> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let distance = feet.distance(slot.ground?.position);
                (distance <= 2.).then_some((index, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
    }
    pub(super) fn step(
        &mut self,
        person: &mut player::Pedestrian,
        city: &opencity::runtime::City,
        controls: super::WalkControls,
        dt: f32,
        blocked: impl Fn(Vec3) -> bool,
        extra_ground: impl Fn(Vec3) -> Option<super::pedestrian_gait::Ground>,
    ) {
        if !dt.is_finite() || dt <= 0. {
            return;
        }
        let support =
            |p| extra_ground(p).or_else(|| super::pedestrian_gait::footing(city, p, false));
        let feet = person.position - Vec3::Z * player::HALF_HEIGHT;
        if self
            .motion
            .as_ref()
            .is_none_or(|m| m.position.distance(feet) > 0.5)
        {
            let Some(ground) = support(feet) else {
                self.motion = None;
                self.pose = None;
                return;
            };
            self.motion = Some(movement::Motion::new(ground.point, person.yaw, ground));
        }
        let motion = self.motion.as_mut().unwrap();
        motion.input = controls.into();
        let steps = (dt / (1. / 120.)).ceil().max(1.) as usize;
        for _ in 0..steps {
            motion.step(dt / steps as f32, &support, &blocked);
            // Contact reach is part of propulsion, so solve at the fixed simulation rate.
            self.pose = Some(pose::Pose::build(motion, &support));
        }
        person.ski_position(motion.position, motion.yaw);
    }
    pub fn reset(&mut self) {
        for slot in &mut self.slots {
            slot.ground = Some(slot.home);
        }
        self.active_slot = 0;
        self.worn = false;
        self.motion = None;
        self.pose = None;
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn prepare_ski_set_pickup(&mut self, hermes: &Hermes) {
        let home = self.skis.slots.first().expect("authored ski pickup").home;
        self.pedestrian = Some(player::Pedestrian::new(
            hermes.city(),
            home.position + Vec3::Z * player::HALF_HEIGHT,
            home.yaw,
        ));
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn prepare_skiing_run(&mut self, hermes: &Hermes, point: glam::Vec2, yaw: f32, speed: f32) {
        let surface = hermes
            .city()
            .surface_at((point / METERS_PER_UNIT).to_array(), f32::INFINITY)
            .expect("ski preview support");
        let ground = super::pedestrian_gait::Ground {
            point: point.extend(surface.height * METERS_PER_UNIT),
            gradient: surface.gradient,
        };
        self.pedestrian = Some(player::Pedestrian::new(
            hermes.city(),
            ground.point + Vec3::Z * player::HALF_HEIGHT,
            yaw,
        ));
        let mut motion = movement::Motion::new(ground.point, yaw, ground);
        motion.velocity = movement::basis(yaw, ground).1 * speed;
        let support = |p| super::pedestrian_gait::footing(hermes.city(), p, false);
        self.skis.pose = Some(pose::Pose::build(&mut motion, &support));
        self.skis.motion = Some(motion);
        self.skis.worn = true;
        if self.skis.slots.is_empty() {
            let home = Placement {
                position: ground.point,
                yaw,
            };
            self.skis.slots.push(Slot { home, ground: None });
        }
        self.skis.active_slot = 0;
        self.skis.slots[0].ground = None;
    }
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
        let feet = person.position - Vec3::Z * player::HALF_HEIGHT;
        self.skis.nearest(feet).map(|_| "Chausser les skis")
    }
    pub fn toggle_skis(&mut self) -> bool {
        if self.skis_hint().is_none() {
            return false;
        }
        if self.skis.worn {
            self.put_down_skis();
        } else {
            self.skis.motion = None;
            self.skis.pose = None;
            let feet = self.pedestrian.as_ref().unwrap().position - Vec3::Z * player::HALF_HEIGHT;
            let index = self.skis.nearest(feet).expect("validated ski pickup");
            self.skis.active_slot = index;
            self.skis.worn = true;
            self.skis.slots[index].ground = None;
        }
        true
    }
    pub(super) fn put_down_skis(&mut self) {
        if !self.skis.worn {
            return;
        }
        if let Some(person) = &self.pedestrian {
            self.skis.slots[self.skis.active_slot].ground = Some(Placement {
                position: person.position - Vec3::Z * player::HALF_HEIGHT,
                yaw: person.yaw,
            });
        } else {
            let slot = &mut self.skis.slots[self.skis.active_slot];
            slot.ground = Some(slot.home);
        }
        self.skis.worn = false;
        self.skis.motion = None;
        self.skis.pose = None;
    }
    pub(super) fn skiing_instance(&self) -> Option<super::crowd::Instance<'_>> {
        if !self.skis.worn || self.entry.is_some() || self.riding_chairlift() {
            return None;
        }
        let person = self
            .pedestrian
            .as_ref()
            .filter(|p| !p.swimming && p.fallen.is_none())?;
        let pose = self.skis.pose.as_ref()?;
        let mut instance = person.instance(self.season);
        instance.transform = pose.transform;
        instance.pose = Some(&pose.palette);
        Some(instance)
    }
    pub(super) fn ski_snow_contacts(
        &self,
        eye: Vec3,
    ) -> impl Iterator<Item = super::snow::SnowWheel> + '_ {
        let skiing = self.skiing_instance().is_some();
        self.skis
            .motion
            .iter()
            .zip(self.skis.pose.iter())
            .filter(move |(m, _)| {
                skiing && m.speed() > 0.4 && m.position.distance_squared(eye) < 100_f32.powi(2)
            })
            .flat_map(|(m, p)| {
                (0..2).map(move |i| {
                    let side = if i == 0 { 1. } else { -1. };
                    let point = p.gear[i].transform_point3(Vec3::new(0., side * 0.13, 0.));
                    let (_, _, r) = movement::basis(m.yaw, m.ground);
                    super::snow::SnowWheel {
                        key: (usize::MAX - 1, i),
                        point,
                        outward: r * side,
                        width: 0.07,
                        sprays: m.hockey_side != 0. || m.speed() > 4.,
                    }
                })
            })
    }
    pub fn skiing_camera(&self) -> Option<(Vec3, Vec3)> {
        self.skiing_instance()?;
        let m = self.skis.motion.as_ref()?;
        let delta = m.hockey_travel_yaw - m.yaw;
        let yaw = m.yaw + delta.sin().atan2(delta.cos()) * m.hockey_blend;
        let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
        let speed = m.speed();
        let eye = m.position - forward * (6.3 + (speed * 0.10).min(3.))
            + Vec3::Z * (2.25 + speed * 0.018);
        let target = m.position + Vec3::Z + forward * (speed * 0.06).min(2.);
        Some((eye / METERS_PER_UNIT, target / METERS_PER_UNIT))
    }
    pub(super) fn append_skis(&self, output: &mut Mesh, player_delta: Mat4) {
        if self.season != Season::Winter {
            return;
        }
        for ground in self
            .skis
            .slots
            .iter()
            .filter_map(|slot| slot.ground)
            .filter(|g| g.position.distance(self.player_position()) < 60.)
        {
            let transform = placement(ground.position, ground.yaw);
            for mesh in meshes() {
                append(mesh, output, transform);
            }
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
            if !self.riding_chairlift() {
                if let Some(pose) = self.skis.pose.as_ref().filter(|_| person.fallen.is_none()) {
                    let scale = Mat4::from_scale(Vec3::splat(1. / METERS_PER_UNIT));
                    for (mesh, transform) in meshes().iter().zip(pose.gear) {
                        append(mesh, output, player_delta * scale * transform);
                    }
                    return;
                }
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
                    transform *= Mat4::from_rotation_y(if self.riding_chairlift() {
                        0.12
                    } else {
                        (height / 1.13).acos()
                    });
                }
                let transform = player_delta * transform * Mat4::from_translation(-datum);
                append(mesh, output, transform);
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod movement_tests;
