//! Independently collidable scenery. Vehicle handling remains game-owned; Rapier
//! supplies continuous collision queries and simulates detached debris.
use super::{Vehicle, METERS_PER_UNIT};
use crate::props::Part;
use glam::{Mat4, Quat, Vec3};
use opencity::runtime::{City, Mesh, SurfaceKind};
use rapier3d::{
    na::{point, vector, Isometry3},
    parry::query::ShapeCastOptions,
    prelude::*,
};
use std::collections::{BTreeMap, HashMap, HashSet};

mod signs;
use signs::SignBody;

const TILE: f32 = 200.;
const VIEW: f32 = 150.;
const TREE_VIEW: f32 = 400.;
const DRAW_TILE: f32 = 50.;
const RESIDENT_RANGE: i32 = 2;
const RESET_DISTANCE: f32 = 500.;
const MAX_DEBRIS: usize = 128;

/// Exact intact-prop selection for one 50 m render tile. The ordered IDs are
/// also its cache signature: movement or destruction changes the selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropBatch {
    pub key: (i32, i32),
    pub ids: Vec<usize>,
}

fn is_tree_bark(surface: f32) -> bool {
    surface == SurfaceKind::TreeBark as u32 as f32
        || surface == SurfaceKind::TreeBarkLinden as u32 as f32
        || surface == SurfaceKind::TreeBarkLocust as u32 as f32
}

struct Prop {
    tree: bool,
    lamp: Option<Vec3>,
    group: usize,
    mesh: Part,
    center: Vec3,
    half: Vec3,
    sign: Option<SignBody>,
    collider: Option<ColliderHandle>,
    debris: Option<RigidBodyHandle>,
    floor: Option<ColliderHandle>,
    age: f32,
    broken: bool,
}

pub struct Props {
    sound_materials: Vec<crate::audio::Material>,
    groups: Vec<Vec<usize>>,
    items: Vec<Prop>,
    active: Vec<usize>,
    tiles: HashMap<(i32, i32), Vec<usize>>,
    resident: Option<(i32, i32)>,
    damaged: HashSet<usize>,
    colliders: ColliderSet,
    bodies: RigidBodySet,
    query: QueryPipeline,
    debris_query: QueryPipeline,
    debris_colliders: ColliderSet,
    debris_bodies: RigidBodySet,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    static_islands: IslandManager,
    broad: BroadPhaseMultiSap,
    narrow: NarrowPhase,
    joints: ImpulseJointSet,
    multibody: MultibodyJointSet,
    ccd: CCDSolver,
}
impl Props {
    fn cone_shape() -> ColliderBuilder {
        ColliderBuilder::compound(vec![
            (
                Isometry3::translation(0., 0., -0.945),
                SharedShape::cuboid(0.5, 0.5, 0.055),
            ),
            (
                Isometry3::translation(0., 0., 0.01),
                SharedShape::capsule_z(0.60, 0.26),
            ),
        ])
    }
    pub(super) fn damaged_groups(&self) -> impl Iterator<Item = usize> + '_ {
        self.damaged.iter().copied()
    }
    pub(super) fn group_intact(&self, group: usize) -> bool {
        self.groups[group].iter().any(|&id| !self.items[id].broken)
    }
    #[cfg(test)]
    pub fn new(meshes: Vec<Mesh>) -> Self {
        Self::from_groups(meshes.into_iter().map(|mesh| vec![mesh]).collect())
    }
    /// Parts collide independently, preserving open space under structures.
    /// An impact detaches all parts of that assembly as separate rigid bodies.
    pub fn from_groups(groups: Vec<Vec<Mesh>>) -> Self {
        Self::build(
            groups
                .into_iter()
                .map(|g| g.into_iter().map(Part::baked).collect())
                .collect(),
            None,
        )
    }
    /// Keep immutable map meshes, but only build collision hulls near the player.
    pub fn from_groups_near(groups: Vec<Vec<Mesh>>, center: Vec3) -> Self {
        Self::build(
            groups
                .into_iter()
                .map(|g| g.into_iter().map(Part::baked).collect())
                .collect(),
            Some(Self::tile(center)),
        )
    }
    pub fn from_parts_near(groups: Vec<Vec<Part>>, center: Vec3) -> Self {
        Self::build(groups, Some(Self::tile(center)))
    }
    fn tile(center: Vec3) -> (i32, i32) {
        (
            (center.x / TILE).floor() as i32,
            (center.y / TILE).floor() as i32,
        )
    }
    fn resident_tile(tile: (i32, i32), focus: (i32, i32)) -> bool {
        (tile.0 - focus.0).abs() <= RESIDENT_RANGE && (tile.1 - focus.1).abs() <= RESIDENT_RANGE
    }
    fn build(groups: Vec<Vec<Part>>, resident: Option<(i32, i32)>) -> Self {
        let sound_materials = groups
            .iter()
            .map(|parts| {
                if let Some((kind, _)) = parts.first().and_then(|p| p.template) {
                    return match kind {
                        kind if kind.is_tree() => crate::audio::Material::Timber,
                        crate::props::Kind::Lamp => crate::audio::Material::Lamp,
                        _ => crate::audio::Material::Hard,
                    };
                }
                let has_surface = |kind: SurfaceKind| {
                    parts.iter().any(|mesh| {
                        mesh.mesh
                            .vertices
                            .iter()
                            .any(|v| v.surface[0] == kind as u32 as f32)
                    })
                };
                if has_surface(SurfaceKind::Fence) {
                    crate::audio::Material::Fence
                } else if has_surface(SurfaceKind::Light) {
                    crate::audio::Material::Lamp
                } else if has_surface(SurfaceKind::Grass)
                    || has_surface(SurfaceKind::TreeBark)
                    || has_surface(SurfaceKind::TreeBarkLinden)
                    || has_surface(SurfaceKind::TreeBarkLocust)
                {
                    crate::audio::Material::Timber
                } else {
                    crate::audio::Material::Hard
                }
            })
            .collect();
        let mut result = Self {
            sound_materials,
            groups: vec![Vec::new(); groups.len()],
            items: Vec::new(),
            active: Vec::new(),
            tiles: HashMap::new(),
            resident,
            damaged: HashSet::new(),
            colliders: ColliderSet::new(),
            bodies: RigidBodySet::new(),
            query: QueryPipeline::new(),
            debris_query: QueryPipeline::new(),
            debris_colliders: ColliderSet::new(),
            debris_bodies: RigidBodySet::new(),
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            static_islands: IslandManager::new(),
            broad: BroadPhaseMultiSap::new(),
            narrow: NarrowPhase::new(),
            joints: ImpulseJointSet::new(),
            multibody: MultibodyJointSet::new(),
            ccd: CCDSolver::new(),
        };
        let mut model_bounds = HashMap::new();
        let mut sign_shapes = HashMap::new();
        for (group, mesh) in groups
            .into_iter()
            .enumerate()
            .flat_map(|(g, parts)| parts.into_iter().map(move |m| (g, m)))
        {
            if mesh.mesh.vertices.is_empty() {
                continue;
            }
            let bounds = || {
                let mut lo = Vec3::splat(f32::INFINITY);
                let mut hi = Vec3::splat(f32::NEG_INFINITY);
                let mut light_lo = lo;
                let mut light_hi = hi;
                for vertex in &mesh.mesh.vertices {
                    let p = Vec3::from_array(vertex.position);
                    lo = lo.min(p);
                    hi = hi.max(p);
                    if vertex.surface[0] == SurfaceKind::Light as u32 as f32
                        && vertex.surface[3] == 0.
                    {
                        light_lo = light_lo.min(p);
                        light_hi = light_hi.max(p);
                    }
                }
                (
                    lo,
                    hi,
                    light_lo.is_finite().then_some((light_lo + light_hi) * 0.5),
                )
            };
            let (local_lo, local_hi, lamp) = if mesh.template.is_some() {
                let key = std::sync::Arc::as_ptr(&mesh.mesh) as usize;
                *model_bounds.entry(key).or_insert_with(bounds)
            } else {
                bounds()
            };
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for x in [local_lo.x, local_hi.x] {
                for y in [local_lo.y, local_hi.y] {
                    for z in [local_lo.z, local_hi.z] {
                        let p =
                            mesh.transform.transform_point3(Vec3::new(x, y, z)) * METERS_PER_UNIT;
                        lo = lo.min(p);
                        hi = hi.max(p);
                    }
                }
            }
            let center = (lo + hi) * 0.5;
            let half = ((hi - lo) * 0.5).max(Vec3::splat(0.03));
            let (sign, center, half) = signs::prepare(&mesh, &mut sign_shapes)
                .map_or((None, center, half), |(sign, center, half)| {
                    (Some(sign), center, half)
                });
            let id = result.items.len();
            result.groups[group].push(id);
            result
                .tiles
                .entry((
                    (center.x / TILE).floor() as i32,
                    (center.y / TILE).floor() as i32,
                ))
                .or_default()
                .push(id);
            let tree = match mesh.template {
                Some((kind, _)) => kind.is_tree(),
                None => mesh
                    .mesh
                    .vertices
                    .iter()
                    .any(|v| is_tree_bark(v.surface[0])),
            };
            result.items.push(Prop {
                tree,
                lamp: lamp.map(|p| mesh.transform.transform_point3(p)),
                group,
                mesh,
                center,
                half,
                sign,
                collider: None,
                debris: None,
                floor: None,
                age: 0.,
                broken: false,
            });
            if resident.is_none_or(|focus| Self::resident_tile(Self::tile(center), focus)) {
                result.insert_collider(id);
            }
        }
        result.query.update(&result.colliders);
        result
    }
    fn insert_collider(&mut self, id: usize) {
        let item = &mut self.items[id];
        let center = item.center;
        let half = item.half;
        let cone = item
            .mesh
            .template
            .is_some_and(|(k, _)| k == crate::props::Kind::Cone);
        let shape = if let Some(sign) = &item.sign {
            ColliderBuilder::new(sign.shape.clone()).rotation(sign.rotation.scaled_axis())
        } else if cone {
            Self::cone_shape()
        } else {
            let points: Vec<_> = item
                .mesh
                .mesh
                .vertices
                .iter()
                // Tree foliage is decorative; collision follows its trunk.
                .filter(|v| {
                    if item.tree {
                        is_tree_bark(v.surface[0]) && v.position[2] - v.surface[3] < 0.022
                    } else {
                        v.surface[0] != SurfaceKind::Grass as u32 as f32
                            && !(15.0..=23.0).contains(&v.surface[0])
                    }
                })
                .map(|v| {
                    let p = item
                        .mesh
                        .transform
                        .transform_point3(Vec3::from_array(v.position))
                        * METERS_PER_UNIT
                        - center;
                    point![p.x, p.y, p.z]
                })
                .collect();
            (points.len() >= 4)
                .then(|| ColliderBuilder::convex_hull(&points))
                .flatten()
                .unwrap_or_else(|| ColliderBuilder::cuboid(half.x, half.y, half.z))
        };
        item.collider = Some(
            self.colliders.insert(
                shape
                    .translation(vector![center.x, center.y, center.z])
                    .user_data(id as u128)
                    .build(),
            ),
        );
    }
    /// Reset only when the complete assembly AND its displaced pieces are far
    /// away. Expiring debris nearby must never pop a collider back into traffic.
    pub fn update_focus(&mut self, center: Vec3) {
        let reset: Vec<_> = self
            .damaged
            .iter()
            .copied()
            .filter(|&group| {
                self.groups[group].iter().all(|&id| {
                    let item = &self.items[id];
                    let far = |p: Vec3| {
                        p.truncate().distance(center.truncate())
                            > RESET_DISTANCE + item.half.length()
                    };
                    far(item.center)
                        && item.debris.is_none_or(|handle| {
                            let p = self.debris_bodies[handle].translation();
                            far(Vec3::new(p.x, p.y, p.z))
                        })
                })
            })
            .collect();
        let mut changed = false;
        for group in reset {
            for id in self.groups[group].clone() {
                self.remove_debris(id);
                self.items[id].broken = false;
                if self.items[id].collider.is_none()
                    && self
                        .resident
                        .map(|_| Self::tile(center))
                        .is_none_or(|focus| {
                            Self::resident_tile(Self::tile(self.items[id].center), focus)
                        })
                {
                    self.insert_collider(id);
                    changed = true;
                }
            }
            self.damaged.remove(&group);
        }
        self.active.retain(|&id| self.items[id].debris.is_some());
        if let Some(previous) = self.resident {
            let focus = Self::tile(center);
            if previous != focus {
                // Only visit the previous and incoming neighbourhoods, never
                // scan the city's complete prop catalogue on a physics tick.
                let mut ids = Vec::new();
                for at in [previous, focus] {
                    for x in at.0 - RESIDENT_RANGE..=at.0 + RESIDENT_RANGE {
                        for y in at.1 - RESIDENT_RANGE..=at.1 + RESIDENT_RANGE {
                            if let Some(tile) = self.tiles.get(&(x, y)) {
                                ids.extend_from_slice(tile);
                            }
                        }
                    }
                }
                ids.sort_unstable();
                ids.dedup();
                for id in ids {
                    let item = &mut self.items[id];
                    if Self::resident_tile(Self::tile(item.center), focus) {
                        if !item.broken && item.collider.is_none() {
                            self.insert_collider(id);
                            changed = true;
                        }
                    } else if let Some(handle) = item.collider.take() {
                        self.colliders.remove(
                            handle,
                            &mut self.static_islands,
                            &mut self.bodies,
                            false,
                        );
                        changed = true;
                    }
                }
                self.resident = Some(focus);
            }
        }
        if changed {
            self.query.update(&self.colliders);
        }
    }
    pub fn vehicle_move(&mut self, vehicle: &mut Vehicle, before: Vec3, city: &City) {
        let delta = vehicle.position - before;
        if delta.length_squared() < 1e-10 {
            return;
        }
        let shape = Cuboid::new(vector![
            vehicle.length() * 0.5,
            vehicle.width() * 0.5,
            (vehicle.height() * 0.5).min(1.5)
        ]);
        let q = vehicle.orientation();
        let center = before + q * Vec3::Z * (vehicle.height() * 0.5 - vehicle.ride_height());
        let pose = Isometry3::from_parts(
            Translation::new(center.x, center.y, center.z),
            Rotation::new_unchecked(rapier3d::na::Quaternion::new(q.w, q.x, q.y, q.z)),
        );
        let motion = vector![delta.x, delta.y, delta.z];
        // Fallen cones remain physical targets: a later pass can push them
        // again, without giving the local debris floor a vehicle collider.
        let mut touched = HashSet::new();
        loop {
            let predicate = |_: ColliderHandle, c: &Collider| {
                c.user_data > 0 && c.parent().is_some_and(|p| !touched.contains(&p))
            };
            let Some((handle, contact)) = self.debris_query.cast_shape(
                &self.debris_bodies,
                &self.debris_colliders,
                &pose,
                &motion,
                &shape,
                ShapeCastOptions {
                    stop_at_penetration: false,
                    ..ShapeCastOptions::with_max_time_of_impact(1.)
                },
                QueryFilter::only_dynamic().predicate(&predicate),
            ) else {
                break;
            };
            let parent = self.debris_colliders[handle].parent().unwrap();
            touched.insert(parent);
            let body = &mut self.debris_bodies[parent];
            let point = contact.witness1;
            let n = contact.normal1;
            let normal = Vec3::new(n.x, n.y, n.z);
            let arm =
                Vec3::new(point.x, point.y, point.z) - (before + delta * contact.time_of_impact);
            let speed = body.velocity_at_point(&point);
            let inward =
                (vehicle.velocity_at(arm) - Vec3::new(speed.x, speed.y, speed.z)).dot(normal);
            if inward < -0.05 {
                let magnitude = -inward
                    / (vehicle.impulse_response(arm, normal) / vehicle.mass() + 1. / body.mass());
                let impulse = normal * magnitude;
                body.apply_impulse_at_point(
                    vector![-impulse.x, -impulse.y, -impulse.z],
                    point,
                    true,
                );
                vehicle.apply_impulse(arm, impulse / vehicle.mass());
                vehicle.collisions += 1;
                vehicle.sound_impact(-inward * 0.35, crate::audio::Material::Hard, false);
            }
        }
        let mut hit = false;
        // Remove every object crossed this frame, including closely spaced fences.
        while let Some((handle, contact)) = self.query.cast_shape(
            &self.bodies,
            &self.colliders,
            &pose,
            &motion,
            &shape,
            ShapeCastOptions {
                stop_at_penetration: false,
                ..ShapeCastOptions::with_max_time_of_impact(1.)
            },
            QueryFilter::default(),
        ) {
            let n = contact.normal1;
            let normal = Vec3::new(n.x, n.y, n.z);
            let point = contact.witness1;
            let arm =
                Vec3::new(point.x, point.y, point.z) - (before + delta * contact.time_of_impact);
            let inward = vehicle.velocity_at(arm).dot(normal);
            let id = self.colliders[handle].user_data as usize;
            let material = self.sound_materials[self.items[id].group];
            let cone = self.items[id]
                .mesh
                .template
                .is_some_and(|(k, _)| k == crate::props::Kind::Cone);
            if self.items[id].tree || inward >= -(if cone { 0.05 } else { 3. }) {
                vehicle.position = before + delta * (contact.time_of_impact - 0.001).max(0.);
                if inward < 0. {
                    let impulse = -inward / vehicle.impulse_response(arm, normal);
                    vehicle.apply_impulse(arm, normal * impulse);
                }
                vehicle.collisions += 1;
                vehicle.sound_impact(-inward, material, false);
                break;
            }
            // One assembly break event, before its individual parts detach.
            vehicle.sound_impact(-inward, material, true);
            let velocity = vehicle.velocity_at(arm);
            self.break_assembly(id, velocity, city);
            // Breakage transfers a modest contact impulse back to the chassis.
            let impulse = -inward * (if cone { 0.0015 } else { 0.12 })
                / vehicle.impulse_response(arm, normal);
            vehicle.apply_impulse(arm, normal * impulse);
            // The query tree may retain removed handles; Rapier ignores those
            // absent from the collider set, and intact colliders never move.
            hit = true;
        }
        if hit {
            vehicle.collisions += 1;
        }
    }
    pub(super) fn ray_hit(
        &self,
        origin: Vec3,
        direction: Vec3,
        distance: f32,
    ) -> Option<(usize, f32, Vec3)> {
        let ray = Ray::new(
            point![origin.x, origin.y, origin.z],
            vector![direction.x, direction.y, direction.z],
        );
        let standing = self
            .query
            .cast_ray_and_get_normal(
                &self.bodies,
                &self.colliders,
                &ray,
                distance,
                true,
                QueryFilter::default(),
            )
            .map(|(h, v)| (self.colliders[h].user_data as usize, v));
        let limit = standing
            .as_ref()
            .map_or(distance, |(_, v)| v.time_of_impact);
        let tagged = |_: ColliderHandle, c: &Collider| c.user_data > 0;
        let moving = self
            .debris_query
            .cast_ray_and_get_normal(
                &self.debris_bodies,
                &self.debris_colliders,
                &ray,
                limit,
                true,
                QueryFilter::only_dynamic().predicate(&tagged),
            )
            .map(|(h, v)| (self.debris_colliders[h].user_data as usize - 1, v));
        moving.or(standing).map(|(id, v)| {
            (
                id,
                v.time_of_impact,
                Vec3::new(v.normal.x, v.normal.y, v.normal.z),
            )
        })
    }
    pub(super) fn shot_surface(&self, id: usize) -> super::ShotSurface {
        use super::ShotSurface;
        let item = &self.items[id];
        if item
            .mesh
            .template
            .is_some_and(|(k, _)| k == crate::props::Kind::Cone)
        {
            return ShotSurface::Rubber;
        }
        match self.sound_materials[item.group] {
            crate::audio::Material::Timber => ShotSurface::Wood,
            crate::audio::Material::Fence
            | crate::audio::Material::Lamp
            | crate::audio::Material::Body => ShotSurface::Metal,
            _ => ShotSurface::Concrete,
        }
    }
    pub(super) fn strike_prop(
        &mut self,
        id: usize,
        point: Vec3,
        direction: Vec3,
        force: f32,
        city: &City,
    ) {
        let rubber = self.shot_surface(id) == super::ShotSurface::Rubber;
        let intact = !self.items[id].broken;
        if intact {
            self.break_assembly(
                id,
                if rubber {
                    Vec3::ZERO
                } else {
                    direction * force * 3.
                },
                city,
            );
        }
        if let Some(handle) = self.items[id].debris {
            let body = &mut self.debris_bodies[handle];
            if intact && rubber {
                body.set_linvel(vector![0., 0., 0.], true);
                body.set_angvel(vector![0., 0., 0.], true);
            }
            let impulse = direction * force * if rubber { 5.8 } else { 2.4 };
            body.apply_impulse_at_point(
                vector![impulse.x, impulse.y, impulse.z],
                point![point.x, point.y, point.z],
                true,
            );
        }
    }
    pub(super) fn break_assembly(&mut self, id: usize, velocity: Vec3, city: &City) {
        for part in self.groups[self.items[id].group].clone() {
            if !self.items[part].broken {
                if let Some(handle) = self.items[part].collider.take() {
                    self.colliders.remove(
                        handle,
                        &mut self.static_islands,
                        &mut self.bodies,
                        false,
                    );
                }
                self.break_prop(part, velocity, city, part == id);
            }
        }
    }
    fn break_prop(&mut self, id: usize, velocity: Vec3, city: &City, direct_hit: bool) {
        if self.active.len() >= MAX_DEBRIS {
            let oldest = self
                .active
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| self.items[**a].age.total_cmp(&self.items[**b].age))
                .map(|(index, _)| index)
                .unwrap();
            let retired = self.active.swap_remove(oldest);
            self.remove_debris(retired);
        }
        let item = &mut self.items[id];
        item.broken = true;
        item.age = 0.;
        self.damaged.insert(item.group);
        let p = item.center;
        // Unsupported parts collapse with a small transferred impulse. Only
        // the part actually struck receives the full vehicle kick and spin.
        let transfer = if direct_hit { 1. } else { 0.2 };
        let cone = item
            .mesh
            .template
            .is_some_and(|(k, _)| k == crate::props::Kind::Cone);
        let body = self.debris_bodies.insert(
            RigidBodyBuilder::dynamic()
                .translation(vector![p.x, p.y, p.z])
                .linvel(vector![
                    velocity.x * 0.6 * transfer,
                    velocity.y * 0.6 * transfer,
                    if direct_hit { 1.5 } else { 0. }
                ])
                .angvel(vector![
                    velocity.y * 0.2 * transfer,
                    -velocity.x * 0.2 * transfer,
                    0.3 * transfer
                ])
                .ccd_enabled(true)
                .angular_damping(if cone { 0.35 } else { 0. })
                .build(),
        );
        if let Some(sign) = &item.sign {
            self.debris_colliders.insert_with_parent(
                ColliderBuilder::new(sign.shape.clone())
                    .rotation(sign.rotation.scaled_axis())
                    .mass(sign.mass)
                    .friction(0.65)
                    .restitution(0.08)
                    .user_data(id as u128 + 1)
                    .build(),
                body,
                &mut self.debris_bodies,
            );
        } else if cone {
            for collider in [
                ColliderBuilder::cuboid(0.5, 0.5, 0.055)
                    .translation(vector![0., 0., -0.945])
                    .mass(3.2),
                ColliderBuilder::capsule_z(0.60, 0.26)
                    .translation(vector![0., 0., 0.01])
                    .mass(0.8),
            ] {
                self.debris_colliders.insert_with_parent(
                    collider
                        .friction(0.85)
                        .restitution(0.18)
                        .user_data(id as u128 + 1)
                        .build(),
                    body,
                    &mut self.debris_bodies,
                );
            }
        } else {
            self.debris_colliders.insert_with_parent(
                ColliderBuilder::cuboid(item.half.x, item.half.y, item.half.z)
                    .density(20.)
                    .user_data(id as u128 + 1)
                    .build(),
                body,
                &mut self.debris_bodies,
            );
        }
        let floor = city
            .surface_at(
                [p.x / METERS_PER_UNIT, p.y / METERS_PER_UNIT],
                (p.z + item.half.z) / METERS_PER_UNIT,
            )
            .map(|s| s.height * METERS_PER_UNIT)
            .unwrap_or(p.z - item.half.z);
        // A small local floor supports debris without introducing a world-sized plane.
        item.floor = Some(
            self.debris_colliders.insert(
                ColliderBuilder::cuboid(8., 8., 0.1)
                    .translation(vector![p.x, p.y, floor - 0.1])
                    .build(),
            ),
        );
        item.debris = Some(body);
        self.active.push(id);
    }
    fn remove_debris(&mut self, id: usize) {
        let item = &mut self.items[id];
        if let Some(handle) = item.debris.take() {
            self.debris_bodies.remove(
                handle,
                &mut self.islands,
                &mut self.debris_colliders,
                &mut self.joints,
                &mut self.multibody,
                true,
            );
        }
        if let Some(floor) = item.floor.take() {
            self.debris_colliders
                .remove(floor, &mut self.islands, &mut self.debris_bodies, false);
        }
        item.age = 0.;
    }
    pub fn blocked(&self, position: Vec3) -> bool {
        let mut blocked = false;
        let pose = Isometry3::translation(position.x, position.y, position.z);
        self.query.intersections_with_shape(
            &self.bodies,
            &self.colliders,
            &pose,
            &Cuboid::new(vector![0.4, 0.4, 0.5]),
            QueryFilter::default(),
            |_| {
                blocked = true;
                false
            },
        );
        blocked
    }
    pub fn step(&mut self, dt: f32) {
        if self.active.is_empty() {
            return;
        }
        let params = IntegrationParameters {
            dt,
            ..Default::default()
        };
        self.pipeline.step(
            &vector![0., 0., -9.81],
            &params,
            &mut self.islands,
            &mut self.broad,
            &mut self.narrow,
            &mut self.debris_bodies,
            &mut self.debris_colliders,
            &mut self.joints,
            &mut self.multibody,
            &mut self.ccd,
            None,
            &(),
            &(),
        );
        if self.active.iter().any(|&id| {
            self.items[id].sign.is_some()
                || self.items[id]
                    .mesh
                    .template
                    .is_some_and(|(kind, _)| kind == crate::props::Kind::Cone)
        }) {
            self.debris_query.update(&self.debris_colliders);
        }
        let mut index = 0;
        while index < self.active.len() {
            let id = self.active[index];
            self.items[id].age += dt;
            if self.items[id].age > 10. {
                self.remove_debris(id);
                self.active.swap_remove(index);
            } else {
                index += 1;
            }
        }
    }
    #[cfg(test)]
    pub fn append_visible(&self, output: &mut Mesh, center: Vec3) {
        let range = (VIEW / TILE).ceil() as i32 + 1;
        let tile = (
            (center.x / TILE).floor() as i32,
            (center.y / TILE).floor() as i32,
        );
        for x in tile.0 - range..=tile.0 + range {
            for y in tile.1 - range..=tile.1 + range {
                if let Some(ids) = self.tiles.get(&(x, y)) {
                    for &id in ids {
                        let item = &self.items[id];
                        let (position, rotation) = if let Some(handle) = item.debris {
                            let body = &self.debris_bodies[handle];
                            let p = body.translation();
                            let q = body.rotation().quaternion();
                            (
                                Vec3::new(p.x, p.y, p.z),
                                Quat::from_xyzw(q.i, q.j, q.k, q.w),
                            )
                        } else if item.collider.is_some() {
                            (item.center, Quat::IDENTITY)
                        } else {
                            continue;
                        };
                        if (position.truncate() - center.truncate()).length()
                            > VIEW + item.half.length()
                        {
                            continue;
                        }
                        if item.mesh.template.is_some() {
                            continue;
                        }
                        if item.collider.is_some() {
                            output.vertices.extend_from_slice(&item.mesh.mesh.vertices);
                            continue;
                        }
                        output
                            .vertices
                            .extend(item.mesh.mesh.vertices.iter().map(|v| {
                                let mut v = *v;
                                let local =
                                    Vec3::from_array(v.position) * METERS_PER_UNIT - item.center;
                                v.position =
                                    ((position + rotation * local) / METERS_PER_UNIT).to_array();
                                v.normal = (rotation * Vec3::from_array(v.normal)).to_array();
                                v
                            }));
                    }
                }
            }
        }
    }
    pub(super) fn append_local_lights(&self, lights: &mut Vec<super::LocalLight>, center: Vec3) {
        // Emitters are cached once with collision geometry; broken assemblies
        // lose power immediately, including overhead parts and expired debris.
        let tile = Self::tile(center);
        let range = (VIEW / TILE).ceil() as i32 + 1;
        for x in tile.0 - range..=tile.0 + range {
            for y in tile.1 - range..=tile.1 + range {
                if let Some(ids) = self.tiles.get(&(x, y)) {
                    for &id in ids {
                        let item = &self.items[id];
                        if item.collider.is_some()
                            && item.center.truncate().distance(center.truncate()) <= VIEW
                        {
                            if let Some(position) = item.lamp {
                                lights.push(super::LocalLight::streetlamp(position));
                            }
                        }
                    }
                }
            }
        }
    }
    pub fn visible_batches(&self, center: Vec3) -> Vec<PropBatch> {
        let range = (VIEW / TILE).ceil() as i32 + 1;
        let tile = (
            (center.x / TILE).floor() as i32,
            (center.y / TILE).floor() as i32,
        );
        let mut batches = BTreeMap::<(i32, i32), Vec<usize>>::new();
        // Preserve the original coarse lookup and exact individual distance
        // cutoff; including a whole nearby render tile would extend visibility.
        for x in tile.0 - range..=tile.0 + range {
            for y in tile.1 - range..=tile.1 + range {
                if let Some(ids) = self.tiles.get(&(x, y)) {
                    for &id in ids {
                        let item = &self.items[id];
                        if item.mesh.template.is_some()
                            || item.collider.is_none()
                            || (item.center.truncate() - center.truncate()).length()
                                > VIEW + item.half.length()
                        {
                            continue;
                        }
                        let key = (
                            (item.center.x / DRAW_TILE).floor() as i32,
                            (item.center.y / DRAW_TILE).floor() as i32,
                        );
                        batches.entry(key).or_default().push(id);
                    }
                }
            }
        }
        batches
            .into_iter()
            .map(|(key, ids)| PropBatch { key, ids })
            .collect()
    }
    pub fn write_batch(&self, batch: &PropBatch, output: &mut Mesh) {
        output.vertices.clear();
        for &id in &batch.ids {
            let item = &self.items[id];
            // A caller holding an older descriptor must never restore a prop
            // already removed by physics.
            if item.mesh.template.is_none() && item.collider.is_some() {
                output.vertices.extend_from_slice(&item.mesh.mesh.vertices);
            }
        }
    }
    pub fn append_moving(&self, output: &mut Mesh, center: Vec3) {
        let range = (VIEW / TILE).ceil() as i32 + 1;
        let tile = (
            (center.x / TILE).floor() as i32,
            (center.y / TILE).floor() as i32,
        );
        // Debris is sparse. Iterate only active bodies, retaining the old
        // source-tile ordering and lookup bounds even after debris has moved.
        let mut ids = self.active.clone();
        ids.sort_unstable_by_key(|&id| {
            let item = &self.items[id];
            (
                (item.center.x / TILE).floor() as i32,
                (item.center.y / TILE).floor() as i32,
                id,
            )
        });
        for id in ids {
            let item = &self.items[id];
            if item.mesh.template.is_some() {
                continue;
            }
            let source = (
                (item.center.x / TILE).floor() as i32,
                (item.center.y / TILE).floor() as i32,
            );
            if (source.0 - tile.0).abs() > range || (source.1 - tile.1).abs() > range {
                continue;
            }
            let Some(handle) = item.debris else {
                continue;
            };
            let body = &self.debris_bodies[handle];
            let p = body.translation();
            let position = Vec3::new(p.x, p.y, p.z);
            if (position.truncate() - center.truncate()).length() > VIEW + item.half.length() {
                continue;
            }
            let q = body.rotation().quaternion();
            let rotation = Quat::from_xyzw(q.i, q.j, q.k, q.w);
            output
                .vertices
                .extend(item.mesh.mesh.vertices.iter().map(|v| {
                    let mut v = *v;
                    let local = Vec3::from_array(v.position) * METERS_PER_UNIT - item.center;
                    v.position = ((position + rotation * local) / METERS_PER_UNIT).to_array();
                    v.normal = (rotation * Vec3::from_array(v.normal)).to_array();
                    v
                }));
        }
    }
    pub fn authored_instances(&self, center: Vec3) -> Vec<crate::props::Instance> {
        let tile = Self::tile(center);
        let mut ids = Vec::new();
        for x in tile.0 - 2..=tile.0 + 2 {
            for y in tile.1 - 2..=tile.1 + 2 {
                if let Some(items) = self.tiles.get(&(x, y)) {
                    ids.extend_from_slice(items);
                }
            }
        }
        ids.extend_from_slice(&self.active);
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| {
                let item = &self.items[id];
                let (kind, part) = item.mesh.template?;
                let (position, transform) = if let Some(handle) = item.debris {
                    let body = &self.debris_bodies[handle];
                    let p = body.translation();
                    let q = body.rotation().quaternion();
                    let position = Vec3::new(p.x, p.y, p.z);
                    (
                        position,
                        Mat4::from_rotation_translation(
                            Quat::from_xyzw(q.i, q.j, q.k, q.w),
                            position / METERS_PER_UNIT,
                        ) * Mat4::from_translation(-item.center / METERS_PER_UNIT)
                            * item.mesh.transform,
                    )
                } else if item.collider.is_some() {
                    (item.center, item.mesh.transform)
                } else {
                    return None;
                };
                let draw_view = if kind.is_tree() { TREE_VIEW } else { VIEW };
                (position.truncate().distance(center.truncate()) <= draw_view + item.half.length())
                    .then(|| crate::props::Instance {
                        kind,
                        index: part,
                        mesh: item.mesh.mesh.clone(),
                        transform: transform * Mat4::from_scale(Vec3::splat(1. / METERS_PER_UNIT)),
                        automatic: item.mesh.automatic,
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod signs_tests;
#[cfg(test)]
mod tests {
    #[test]
    fn tree_impostors_remain_visible_beyond_other_props() {
        use super::*;
        let placement = crate::props::Placement {
            kind: crate::props::Kind::RandomTree,
            position: Vec3::ZERO,
            heading: 0.,
            sign: None,
        };
        let mut props = Props::from_parts_near(vec![placement.parts()], Vec3::ZERO);
        assert_eq!(props.authored_instances(Vec3::ZERO).len(), 1);
        props.update_focus(Vec3::X * 350.);
        assert_eq!(props.authored_instances(Vec3::X * 350.).len(), 1);
        props.update_focus(Vec3::X * 450.);
        assert!(props.authored_instances(Vec3::X * 450.).is_empty());
    }

    #[test]
    fn baked_tree_foliage_keeps_collision_near_the_trunk() {
        use super::*;
        let placement = crate::props::Placement {
            kind: crate::props::Kind::Tree,
            position: Vec3::ZERO,
            heading: 0.,
            sign: None,
        };
        let props = Props::from_parts_near(vec![placement.parts()], Vec3::ZERO);
        assert!(props.blocked(Vec3::new(0., 0., 0.5)));
        assert!(!props.blocked(Vec3::new(2., 0., 4.)));
    }
    #[test]
    fn vehicle_impact_stops_at_the_tree_and_keeps_its_trunk_collidable() {
        use super::*;
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3);").unwrap(),
            None,
        )
        .unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        let start = vehicle.position;
        let trunk = start + Vec3::X * 8.;
        let placement = crate::props::Placement {
            kind: crate::props::Kind::RandomTree,
            position: trunk / METERS_PER_UNIT,
            heading: 0.,
            sign: None,
        };
        let mut props = Props::from_parts_near(vec![placement.parts()], start);
        vehicle.position.x += 16.;
        vehicle.velocity = Vec3::X * 25.;
        props.vehicle_move(&mut vehicle, start, &city);

        assert!(vehicle.position.x > start.x && vehicle.position.x < trunk.x);
        assert_eq!(vehicle.collisions, 1);
        assert!(!vehicle.sound_impacts[0].broken);
        assert!(props.blocked(trunk + Vec3::Z * 0.5));
        assert!(props.active.is_empty());
    }

    #[test]
    fn authored_streetlamp_emitter_loses_power_when_its_base_breaks() {
        use super::*;
        let hermes = crate::hermes::Hermes::test_map_from_source(
            "road r: sidewalk carlane sidewalk; end r(-1,0,1,0);",
        );
        let features = hermes
            .city()
            .street_features(opencity::runtime::Region::WholeMap);
        let index = features
            .iter()
            .position(|f| f.kind == opencity::runtime::StreetFeatureKind::Lamp)
            .unwrap();
        let mut props = Props::from_groups(vec![hermes.prop_groups()[index].clone()]);
        let center = props.items[0].center;
        let mut lights = Vec::new();
        props.append_local_lights(&mut lights, center);
        assert_eq!(lights.len(), 1);
        assert!(lights[0].position().z > features[index].position[2] + 0.05);
        let mut vehicle = Vehicle::spawn(hermes.city());
        vehicle.yaw = 0.;
        vehicle.velocity = Vec3::X * 12.;
        vehicle.position = center + Vec3::X * 4.;
        props.vehicle_move(&mut vehicle, center - Vec3::X * 4., hermes.city());
        assert!(props.items.iter().all(|p| p.collider.is_none()));
        lights.clear();
        props.append_local_lights(&mut lights, center);
        assert!(lights.is_empty(), "broken overhead lamp must lose power");
    }
    use super::*;
    use crate::dynamic_world::Controls;
    use opencity::runtime::SurfaceKind;

    fn vertex_bits(mesh: &Mesh) -> Vec<Vec<u32>> {
        let mut result: Vec<_> = mesh
            .vertices
            .iter()
            .map(|v| {
                v.position
                    .into_iter()
                    .chain(v.normal)
                    .chain(v.color)
                    .chain(v.surface)
                    .map(f32::to_bits)
                    .collect::<Vec<_>>()
            })
            .collect();
        result.sort_unstable();
        result
    }

    #[test]
    fn cached_batches_match_visibility_and_remove_broken_props_immediately() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3);").unwrap(),
            None,
        )
        .unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        let before = vehicle.position;
        let meshes = [4., 80., 300.].map(|offset| {
            let center = (before + Vec3::X * offset) / METERS_PER_UNIT;
            let mut mesh = Mesh::default();
            mesh.cuboid(
                (center - Vec3::splat(0.005)).to_array(),
                (center + Vec3::splat(0.005)).to_array(),
                [0.5; 3],
                SurfaceKind::Prop,
            );
            mesh
        });
        let mut props = Props::new(meshes.into());
        let original_batches = props.visible_batches(before);
        assert_eq!(original_batches.iter().flat_map(|b| &b.ids).count(), 2);
        // Compare every vertex field, independently of draw-batch ordering.
        let compare = |props: &Props, center| {
            let mut original = Mesh::default();
            props.append_visible(&mut original, center);
            let mut cached = Mesh::default();
            for batch in props.visible_batches(center) {
                let mut tile = Mesh::default();
                props.write_batch(&batch, &mut tile);
                cached.vertices.extend(tile.vertices);
            }
            props.append_moving(&mut cached, center);
            assert_eq!(vertex_bits(&original), vertex_bits(&cached));
        };
        for offset in [-401., -200., -0.1, 0., 150., 200., 401.] {
            compare(&props, before + Vec3::X * offset);
        }
        let item = &props.items[0];
        let cutoff = VIEW + item.half.length();
        let selected = |center| {
            props
                .visible_batches(center)
                .iter()
                .any(|b| b.ids.contains(&0))
        };
        assert!(selected(item.center + Vec3::X * (cutoff - 0.1)));
        assert!(!selected(item.center + Vec3::X * (cutoff + 0.1)));
        vehicle.position.x += 5.;
        vehicle.velocity = Vec3::X * 12.;
        props.vehicle_move(&mut vehicle, before, &city);
        assert!(props.items[0].debris.is_some());
        assert!(props
            .visible_batches(before)
            .iter()
            .all(|b| !b.ids.contains(&0)));
        let stale = original_batches
            .iter()
            .find(|b| b.ids.contains(&0))
            .unwrap();
        let mut stale_mesh = Mesh::default();
        props.write_batch(stale, &mut stale_mesh);
        assert!(stale_mesh.vertices.is_empty());
        compare(&props, before);
        for _ in 0..1201 {
            props.step(1. / 120.);
        }
        assert!(props.items[0].debris.is_none());
        assert!(props
            .visible_batches(before)
            .iter()
            .all(|b| !b.ids.contains(&0)));
        compare(&props, before);
    }

    #[test]
    fn fence_plane_blocks_and_breaks_while_retaining_its_pattern_coordinates() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3);").unwrap(),
            None,
        )
        .unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        let before = vehicle.position;
        let center = (before + Vec3::new(4., 0., 0.)) / METERS_PER_UNIT;
        let mut mesh = Mesh::default();
        mesh.quad(
            [
                [center.x, center.y - 0.01, center.z - 0.005],
                [center.x, center.y + 0.01, center.z - 0.005],
                [center.x, center.y + 0.01, center.z + 0.005],
                [center.x, center.y - 0.01, center.z + 0.005],
            ],
            [0.1; 3],
            SurfaceKind::Fence,
        );
        let uv = [[0., 0.], [2., 0.], [2., 1.], [0., 0.], [2., 1.], [0., 1.]];
        for (v, uv) in mesh.vertices.iter_mut().zip(uv) {
            v.surface[1] = uv[0];
            v.surface[2] = uv[1];
        }
        let pattern: Vec<_> = mesh.vertices.iter().map(|v| v.surface).collect();
        let mut props = Props::new(vec![mesh]);
        assert!(props.blocked(center * METERS_PER_UNIT));
        vehicle.position.x += 5.;
        vehicle.velocity = Vec3::X * 12.;
        props.vehicle_move(&mut vehicle, before, &city);
        assert!(props.items[0].debris.is_some());
        assert!(!props.blocked(center * METERS_PER_UNIT));
        props.step(1. / 120.);
        let mut visible = Mesh::default();
        props.append_visible(&mut visible, before);
        assert_eq!(
            visible
                .vertices
                .iter()
                .map(|v| v.surface)
                .collect::<Vec<_>>(),
            pattern
        );
    }

    #[test]
    fn destruction_audio_follows_assembly_material_and_never_replays_removed_parts() {
        for (surface, expected) in [
            (SurfaceKind::Fence, crate::audio::Material::Fence),
            (SurfaceKind::Light, crate::audio::Material::Lamp),
            (SurfaceKind::Grass, crate::audio::Material::Timber),
        ] {
            let city = City::from_program(
                &opencity::parse("road street: carlane; end street(-25,3,-20,3);").unwrap(),
                None,
            )
            .unwrap();
            let mut vehicle = Vehicle::spawn(&city);
            let start = vehicle.position;
            let center = (start + Vec3::X * 4.) / METERS_PER_UNIT;
            let mut trunk = Mesh::default();
            trunk.cuboid(
                (center - Vec3::splat(0.005)).to_array(),
                (center + Vec3::splat(0.005)).to_array(),
                [0.5; 3],
                SurfaceKind::Prop,
            );
            let mut part = Mesh::default();
            part.cuboid(
                (center - Vec3::splat(0.004)).to_array(),
                (center + Vec3::splat(0.004)).to_array(),
                [0.5; 3],
                surface,
            );
            let mut props = Props::from_groups(vec![vec![trunk, part]]);
            vehicle.position.x += 5.;
            vehicle.velocity = Vec3::X * 12.;
            props.vehicle_move(&mut vehicle, start, &city);
            assert_eq!(
                vehicle.sound_impact_count, 1,
                "one assembly, not one event per detached part"
            );
            assert!(vehicle.sound_impacts[0].broken);
            assert_eq!(vehicle.sound_impacts[0].material, expected);
            assert!(vehicle.sound_impacts[0].speed >= 3.);
            assert!(props.items.iter().all(|item| item.debris.is_some()));
            vehicle.sound_impact_count = 0;
            props.vehicle_move(&mut vehicle, start, &city);
            assert_eq!(
                vehicle.sound_impact_count, 0,
                "removed assembly must not retrigger"
            );
        }
    }
    #[test]
    fn resting_prop_contacts_do_not_emit_crashes() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3);").unwrap(),
            None,
        )
        .unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        let start = vehicle.position;
        let center = (start + Vec3::X * 4.) / METERS_PER_UNIT;
        let mut mesh = Mesh::default();
        mesh.cuboid(
            (center - Vec3::splat(0.005)).to_array(),
            (center + Vec3::splat(0.005)).to_array(),
            [0.5; 3],
            SurfaceKind::Prop,
        );
        let mut props = Props::new(vec![mesh]);
        for _ in 0..100 {
            vehicle.position = start + Vec3::X * 5.;
            vehicle.velocity = Vec3::X * 0.2;
            props.vehicle_move(&mut vehicle, start, &city);
        }
        assert!(vehicle.collisions > 0);
        assert_eq!(vehicle.sound_impact_count, 0);
        assert!(props.items[0].collider.is_some());
    }
    #[test]
    fn impact_breaks_prop_and_rapier_moves_then_removes_debris() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3);").unwrap(),
            None,
        )
        .unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        let start = vehicle.position;
        let mut mesh = Mesh::default();
        let center = (start + Vec3::new(4., 0., 0.)) / METERS_PER_UNIT;
        mesh.cuboid(
            (center - Vec3::splat(0.005)).to_array(),
            (center + Vec3::splat(0.005)).to_array(),
            [0.5; 3],
            SurfaceKind::Prop,
        );
        let mut props = Props::new(vec![mesh]);
        assert!(props.blocked(center * METERS_PER_UNIT));
        let mut visible = Mesh::default();
        props.append_visible(&mut visible, start);
        assert!(!visible.vertices.is_empty());
        vehicle.position.x += 5.;
        vehicle.velocity = Vec3::X * 12.;
        props.vehicle_move(&mut vehicle, start, &city);
        assert!(!props.blocked(center * METERS_PER_UNIT));
        assert_eq!(vehicle.collisions, 1);
        assert_eq!(vehicle.sound_impact_count, 1);
        assert!(vehicle.sound_impacts[0].broken);
        assert_eq!(
            vehicle.sound_impacts[0].material,
            crate::audio::Material::Hard
        );
        let handle = props.items[0].debris.expect("impact creates rigid body");
        let before = *props.debris_bodies[handle].translation();
        for _ in 0..120 {
            props.step(1. / 120.);
        }
        assert!((props.debris_bodies[handle].translation() - before).norm() > 0.1);
        for _ in 0..1200 {
            props.step(1. / 120.);
        }
        let mut gone = Mesh::default();
        props.append_visible(&mut gone, start);
        assert!(gone.vertices.is_empty());
        assert!(props.debris_colliders.is_empty());
        vehicle.step(&city, Controls::default(), 1. / 120.);
    }

    #[test]
    fn real_commercial_canopy_has_open_space_and_collapses_when_one_post_is_hit() {
        let city = City::from_program(
            &opencity::parse(
                "road street: sidewalk carlane sidewalk; end street(-1,0,1,0);
            ground=land(-2,-2,2,-2,2,2,-2,2);elevation(ground,0.02);
            generated_shop_0000000000000003=building(0.08,{use:commercial,front_edge:0},
                -0.04,0.06,0.04,0.06,0.04,0.16,-0.04,0.16);",
            )
            .unwrap(),
            None,
        )
        .unwrap();
        let groups = crate::hermes::commerce_mesh::prop_groups(&city);
        let assembly = groups
            .into_iter()
            .find(|p| p.len() == 3)
            .expect("canopy roof and two posts");
        let mut neighbor = Mesh::default();
        neighbor.cuboid(
            [0.2, 0., 0.],
            [0.21, 0.01, 0.01],
            [0.5; 3],
            SurfaceKind::Prop,
        );
        let mut props = Props::from_groups(vec![assembly, vec![neighbor]]);
        assert!(
            !props.blocked(Vec3::new(0., 4., 0.7)),
            "open under the canopy"
        );
        assert!(
            props.blocked(Vec3::new(-3.65, 3.95, 0.7)),
            "post has collision"
        );
        let mut vehicle = Vehicle::spawn(&city);
        let before = Vec3::new(-7., 3.95, 0.7);
        vehicle.position = before + Vec3::X * 4.;
        vehicle.velocity = Vec3::X * 12.;
        vehicle.yaw = 0.;
        props.vehicle_move(&mut vehicle, before, &city);
        assert_eq!(vehicle.collisions, 1);
        assert!(props.items[..3]
            .iter()
            .all(|p| p.collider.is_none() && p.debris.is_some()));
        assert!(
            props.items[3].collider.is_some(),
            "unrelated props stay intact"
        );
        let roof = props.items[0].debris.unwrap();
        let height = props.debris_bodies[roof].translation().z;
        for _ in 0..60 {
            props.step(1. / 120.);
        }
        assert!(
            props.debris_bodies[roof].translation().z < height,
            "roof falls with its supports"
        );
        assert!(
            !props.blocked(Vec3::new(3.75, 3.95, 0.7)),
            "other post detached too"
        );
    }

    #[test]
    fn actual_shop_sandwich_board_is_collidable_then_detaches_on_impact() {
        let (city, board) = (0..64)
            .find_map(|id| {
                let source = format!(
                    "road street: carlane; end street(-1,0,1,0);
                p=concrete(-1,0.05,1,0.05,1,0.12,-1,0.12);elevation(p,0.02);
                generated_shop_{id:016x}=building(0.08,{{use:commercial,front_edge:0}},
                    -0.04,0.12,0.04,0.12,0.04,0.22,-0.04,0.22);"
                );
                let city = City::from_program(&opencity::parse(&source).unwrap(), None).unwrap();
                let board = crate::hermes::commerce_mesh::prop_groups(&city)
                    .into_iter()
                    .find(|g| g.len() == 1)?;
                Some((city, board))
            })
            .expect("a sparse board on eligible pavement");
        let mut props = Props::from_groups(vec![board]);
        let center = props.items[0].center;
        assert!(props.blocked(center));
        let mut car = Vehicle::spawn(&city);
        let before = Vec3::new(center.x - 4., center.y, 0.7);
        car.position = before + Vec3::X * 5.;
        car.velocity = Vec3::X * 12.;
        car.yaw = 0.;
        props.vehicle_move(&mut car, before, &city);
        assert_eq!(car.collisions, 1);
        assert!(props.items[0].collider.is_none());
        assert!(props.items[0].debris.is_some());
        assert!(
            props.visible_batches(center).is_empty(),
            "intact rendering removed immediately"
        );
    }

    #[test]
    fn gentle_contact_stops_car_without_destroying_the_object() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3);").unwrap(),
            None,
        )
        .unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        let before = vehicle.position;
        let center = (before + Vec3::new(4., 0., 0.)) / METERS_PER_UNIT;
        let mut mesh = Mesh::default();
        mesh.cuboid(
            (center - Vec3::splat(0.005)).to_array(),
            (center + Vec3::splat(0.005)).to_array(),
            [0.5; 3],
            SurfaceKind::Prop,
        );
        let mut props = Props::new(vec![mesh]);
        vehicle.position.x += 5.;
        vehicle.velocity = Vec3::X * 2.;
        props.vehicle_move(&mut vehicle, before, &city);
        assert!(props.blocked(center * METERS_PER_UNIT));
        assert!(vehicle.position.x < before.x + 4.);
        assert!(vehicle.velocity.x.abs() < 0.01);
        assert!(props.items[0].debris.is_none());

        // A car already overlapping an object must still be able to reverse away.
        vehicle.position = before + Vec3::new(2., 0., 0.);
        let overlapping = vehicle.position;
        vehicle.position.x -= 0.5;
        vehicle.velocity = -Vec3::X;
        props.vehicle_move(&mut vehicle, overlapping, &city);
        assert!((vehicle.position.x - (overlapping.x - 0.5)).abs() < 0.001);
        assert!(props.items[0].collider.is_some());
    }

    #[test]
    fn empty_prop_world_preserves_vehicle_motion_exactly() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3);").unwrap(),
            None,
        )
        .unwrap();
        let mut original = Vehicle::spawn(&city);
        let mut with_props = Vehicle::spawn(&city);
        let mut props = Props::new(Vec::new());
        for i in 0..600 {
            let controls = Controls {
                throttle: 1.,
                steering: if i > 240 { 0.5 } else { 0. },
                ..Default::default()
            };
            let before = with_props.position;
            original.step(&city, controls, 1. / 120.);
            with_props.step(&city, controls, 1. / 120.);
            props.vehicle_move(&mut with_props, before, &city);
            props.step(1. / 120.);
            assert_eq!(with_props.position, original.position);
            assert_eq!(with_props.velocity, original.velocity);
            assert_eq!(with_props.yaw, original.yaw);
            assert_eq!(with_props.angular_momentum, original.angular_momentum);
        }
    }

    #[test]
    fn sweep_breaks_multiple_objects_in_the_same_frame() {
        let city = City::from_program(
            &opencity::parse("road street: carlane; end street(-25,3,-20,3);").unwrap(),
            None,
        )
        .unwrap();
        let mut vehicle = Vehicle::spawn(&city);
        let before = vehicle.position;
        let meshes = [4., 6.].map(|offset| {
            let center = (before + Vec3::new(offset, 0., 0.)) / METERS_PER_UNIT;
            let mut mesh = Mesh::default();
            mesh.cuboid(
                (center - Vec3::splat(0.005)).to_array(),
                (center + Vec3::splat(0.005)).to_array(),
                [0.5; 3],
                SurfaceKind::Prop,
            );
            mesh
        });
        let mut props = Props::new(meshes.into());
        vehicle.position.x += 8.;
        vehicle.velocity = Vec3::X * 12.;
        props.vehicle_move(&mut vehicle, before, &city);
        assert!(props
            .items
            .iter()
            .all(|p| p.collider.is_none() && p.debris.is_some()));
    }

    #[test]
    fn generated_lamps_and_benches_can_be_hit() {
        use opencity::runtime::{Region, StreetFeatureKind};
        let hermes = crate::hermes::Hermes::test_map_from_source(
            "road street: sidewalk carlane sidewalk; end street(-25,3,-20,3);",
        );
        let city = hermes.city();
        let features = city.street_features(Region::WholeMap);
        let meshes = hermes.prop_groups();
        assert!(features.len() <= meshes.len());
        for kind in [StreetFeatureKind::Lamp, StreetFeatureKind::Bench] {
            let index = features.iter().position(|f| f.kind == kind).unwrap();
            let feature = features[index];
            let forward = Vec3::new(feature.heading.cos(), feature.heading.sin(), 0.);
            let center = Vec3::from_array(feature.position) * METERS_PER_UNIT + Vec3::Z * 0.65;
            let before = center - forward * 5.;
            let mut vehicle = Vehicle::spawn(&city);
            vehicle.position = center + forward * 5.;
            vehicle.velocity = forward * 12.;
            vehicle.yaw = feature.heading;
            let mut props = Props::from_groups(vec![meshes[index].clone()]);
            if kind == StreetFeatureKind::Lamp {
                let base = Vec3::from_array(feature.position) * METERS_PER_UNIT;
                assert!(props.blocked(base + Vec3::Z * 0.65));
                assert!(!props.blocked(base + forward + Vec3::Z * 0.9));
            }
            props.vehicle_move(&mut vehicle, before, &city);
            assert!(props.items.iter().all(|p| p.debris.is_some()), "{kind:?}");
        }
    }

    #[test]
    fn generated_bin_is_collidable_and_breaks_into_visible_debris() {
        let hermes = crate::hermes::Hermes::test_map_from_source(
            "road r: sidewalk carlane sidewalk; end r(-1,0,1,0);",
        );
        let city = hermes.city();
        let first_bin = city
            .street_features(opencity::runtime::Region::WholeMap)
            .len();
        let groups = hermes.prop_groups();
        let bin = &groups[first_bin];
        let (lo, hi) = crate::hermes::external_models::bounds(&bin[0]);
        let center = (lo + hi) * 50.;
        let mut props = Props::from_groups(vec![bin.clone()]);
        assert!(props.blocked(center));
        let mut vehicle = Vehicle::spawn(city);
        vehicle.yaw = 0.;
        vehicle.velocity = Vec3::X * 12.;
        vehicle.position = center + Vec3::X * 4.;
        props.vehicle_move(&mut vehicle, center - Vec3::X * 4., city);
        assert!(props
            .items
            .iter()
            .all(|p| p.collider.is_none() && p.debris.is_some()));
        let mut debris = Mesh::default();
        props.append_moving(&mut debris, center);
        assert!(!debris.vertices.is_empty());
    }
}
