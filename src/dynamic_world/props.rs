//! Independently collidable scenery. Vehicle handling remains game-owned; Rapier
//! supplies continuous collision queries and simulates detached debris.
use super::{Vehicle, METERS_PER_UNIT};
use glam::{Quat, Vec3};
use opencity::runtime::{City, Mesh, SurfaceKind};
use rapier3d::{
    na::{point, vector, Isometry3},
    parry::query::ShapeCastOptions,
    prelude::*,
};
use std::collections::{BTreeMap, HashMap};

const TILE: f32 = 200.;
const VIEW: f32 = 150.;
const DRAW_TILE: f32 = 50.;

/// Exact intact-prop selection for one 50 m render tile. The ordered IDs are
/// also its cache signature: movement or destruction changes the selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropBatch {
    pub key: (i32, i32),
    pub ids: Vec<usize>,
}

struct Prop {
    group: usize,
    mesh: Mesh,
    center: Vec3,
    half: Vec3,
    collider: Option<ColliderHandle>,
    debris: Option<RigidBodyHandle>,
    floor: Option<ColliderHandle>,
    age: f32,
}

pub struct Props {
    groups: Vec<Vec<usize>>,
    items: Vec<Prop>,
    active: Vec<usize>,
    tiles: HashMap<(i32, i32), Vec<usize>>,
    colliders: ColliderSet,
    bodies: RigidBodySet,
    query: QueryPipeline,
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
    #[cfg(test)]
    pub fn new(meshes: Vec<Mesh>) -> Self {
        Self::from_groups(meshes.into_iter().map(|mesh| vec![mesh]).collect())
    }
    /// Parts collide independently, preserving open space under structures.
    /// An impact detaches all parts of that assembly as separate rigid bodies.
    pub fn from_groups(groups: Vec<Vec<Mesh>>) -> Self {
        let mut result = Self {
            groups: vec![Vec::new(); groups.len()],
            items: Vec::new(),
            active: Vec::new(),
            tiles: HashMap::new(),
            colliders: ColliderSet::new(),
            bodies: RigidBodySet::new(),
            query: QueryPipeline::new(),
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
        for (group, mesh) in groups
            .into_iter()
            .enumerate()
            .flat_map(|(g, parts)| parts.into_iter().map(move |m| (g, m)))
        {
            if mesh.vertices.is_empty() {
                continue;
            }
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for v in &mesh.vertices {
                let p = Vec3::from_array(v.position) * METERS_PER_UNIT;
                lo = lo.min(p);
                hi = hi.max(p);
            }
            let center = (lo + hi) * 0.5;
            let half = ((hi - lo) * 0.5).max(Vec3::splat(0.03));
            let points: Vec<_> = mesh
                .vertices
                .iter()
                // Tree foliage is decorative; collision follows its trunk.
                .filter(|v| v.surface[0] != SurfaceKind::Grass as u32 as f32)
                .map(|v| {
                    let p = Vec3::from_array(v.position) * METERS_PER_UNIT - center;
                    point![p.x, p.y, p.z]
                })
                .collect();
            let shape = ColliderBuilder::convex_hull(&points)
                .unwrap_or_else(|| ColliderBuilder::cuboid(half.x, half.y, half.z));
            let id = result.items.len();
            result.groups[group].push(id);
            let handle = result.colliders.insert(
                shape
                    .translation(vector![center.x, center.y, center.z])
                    .user_data(id as u128)
                    .build(),
            );
            result
                .tiles
                .entry((
                    (center.x / TILE).floor() as i32,
                    (center.y / TILE).floor() as i32,
                ))
                .or_default()
                .push(id);
            result.items.push(Prop {
                group,
                mesh,
                center,
                half,
                collider: Some(handle),
                debris: None,
                floor: None,
                age: 0.,
            });
        }
        result.query.update(&result.colliders);
        result
    }
    pub fn vehicle_move(&mut self, vehicle: &mut Vehicle, before: Vec3, city: &City) {
        let delta = vehicle.position - before;
        if delta.length_squared() < 1e-10 {
            return;
        }
        let shape = Cuboid::new(vector![2.1, 0.9, 0.45]);
        let pose = Isometry3::new(
            vector![before.x, before.y, before.z],
            vector![0., 0., vehicle.yaw],
        );
        let motion = vector![delta.x, delta.y, delta.z];
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
            let inward = vehicle.velocity.dot(normal);
            if inward >= -3. {
                vehicle.position = before + delta * (contact.time_of_impact - 0.001).max(0.);
                if inward < 0. {
                    vehicle.velocity -= normal * inward;
                }
                vehicle.collisions += 1;
                break;
            }
            let id = self.colliders[handle].user_data as usize;
            for part in self.groups[self.items[id].group].clone() {
                if let Some(handle) = self.items[part].collider.take() {
                    self.colliders.remove(
                        handle,
                        &mut self.static_islands,
                        &mut self.bodies,
                        false,
                    );
                    self.break_prop(part, vehicle.velocity, city, part == id);
                }
            }
            // The query tree may retain removed handles; Rapier ignores those
            // absent from the collider set, and intact colliders never move.
            hit = true;
        }
        if hit {
            vehicle.velocity.x *= 0.88;
            vehicle.velocity.y *= 0.88;
            vehicle.collisions += 1;
        }
    }
    fn break_prop(&mut self, id: usize, velocity: Vec3, city: &City, direct_hit: bool) {
        let item = &mut self.items[id];
        let p = item.center;
        // Unsupported parts collapse with a small transferred impulse. Only
        // the part actually struck receives the full vehicle kick and spin.
        let transfer = if direct_hit { 1. } else { 0.2 };
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
                .build(),
        );
        self.debris_colliders.insert_with_parent(
            ColliderBuilder::cuboid(item.half.x, item.half.y, item.half.z)
                .density(20.)
                .build(),
            body,
            &mut self.debris_bodies,
        );
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
        self.active.retain(|&id| {
            let item = &mut self.items[id];
            if let Some(handle) = item.debris {
                item.age += dt;
                if item.age > 10. {
                    self.debris_bodies.remove(
                        handle,
                        &mut self.islands,
                        &mut self.debris_colliders,
                        &mut self.joints,
                        &mut self.multibody,
                        true,
                    );
                    item.debris = None;
                    if let Some(floor) = item.floor.take() {
                        self.debris_colliders.remove(
                            floor,
                            &mut self.islands,
                            &mut self.debris_bodies,
                            false,
                        );
                    }
                    return false;
                }
            }
            true
        });
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
                        if item.collider.is_some() {
                            output.vertices.extend_from_slice(&item.mesh.vertices);
                            continue;
                        }
                        output.vertices.extend(item.mesh.vertices.iter().map(|v| {
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
                        if item.collider.is_none()
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
            if item.collider.is_some() {
                output.vertices.extend_from_slice(&item.mesh.vertices);
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
            output.vertices.extend(item.mesh.vertices.iter().map(|v| {
                let mut v = *v;
                let local = Vec3::from_array(v.position) * METERS_PER_UNIT - item.center;
                v.position = ((position + rotation * local) / METERS_PER_UNIT).to_array();
                v.normal = (rotation * Vec3::from_array(v.normal)).to_array();
                v
            }));
        }
    }
}

#[cfg(test)]
mod tests {
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
            assert_eq!(with_props.yaw_rate, original.yaw_rate);
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
        let city = City::from_program(
            &opencity::parse("road street: sidewalk carlane sidewalk; end street(-25,3,-20,3);")
                .unwrap(),
            None,
        )
        .unwrap();
        let features = city.street_features(Region::WholeMap);
        let meshes = city.detail_objects(Region::WholeMap);
        assert_eq!(features.len(), meshes.len());
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
            let mut props = Props::new(vec![meshes[index].clone()]);
            props.vehicle_move(&mut vehicle, before, &city);
            assert!(props.items[0].debris.is_some(), "{kind:?}");
        }
    }
}
