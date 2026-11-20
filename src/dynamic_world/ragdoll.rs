//! Recoverable pedestrian reactions. Physics uses metres and the game's +Z up.
use super::{
    pedestrian_catalog::{self, Definition, JOINT_LIMIT},
    Vehicle, METERS_PER_UNIT,
};
use glam::{Mat4, Quat, Vec3};
use opencity::runtime::{City, SurfaceKind};
use rapier3d::{
    na::{point, vector, Isometry3, Quaternion, Translation3, UnitQuaternion},
    prelude::*,
};

struct Limb {
    joint: usize,
    body: RigidBodyHandle,
    bone_from_body: Mat4,
}
pub(super) struct Ragdoll {
    pub age: f32,
    pub palette: [Mat4; JOINT_LIMIT],
    pub anchor: Vec3,
    yaw: f32,
    start: [Mat4; JOINT_LIMIT],
    initial_root: Mat4,
    initial_hips: Mat4,
    limbs: Vec<Limb>,
    cars: Vec<(usize, RigidBodyHandle)>,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    joints: ImpulseJointSet,
    multibody: MultibodyJointSet,
    pipeline: PhysicsPipeline,
    islands: IslandManager,
    broad: BroadPhaseMultiSap,
    narrow: NarrowPhase,
    ccd: CCDSolver,
    floor_ceiling: f32,
}
fn isometry(matrix: Mat4) -> Isometry3<f32> {
    let (_, q, p) = matrix.to_scale_rotation_translation();
    Isometry3::from_parts(
        Translation3::new(p.x, p.y, p.z),
        UnitQuaternion::new_unchecked(Quaternion::new(q.w, q.x, q.y, q.z)),
    )
}
fn matrix(iso: &Isometry3<f32>) -> Mat4 {
    let q = iso.rotation.quaternion();
    let p = iso.translation.vector;
    Mat4::from_rotation_translation(
        Quat::from_xyzw(q.i, q.j, q.k, q.w),
        Vec3::new(p.x, p.y, p.z),
    )
}
fn actor_matrix(anchor: Vec3, yaw: f32) -> Mat4 {
    let mut matrix = pedestrian_catalog::instance_transform(anchor, yaw);
    matrix.x_axis *= METERS_PER_UNIT;
    matrix.y_axis *= METERS_PER_UNIT;
    matrix.z_axis *= METERS_PER_UNIT;
    matrix.w_axis = anchor.extend(1.);
    matrix
}
fn car_matrix(vehicle: &Vehicle) -> Mat4 {
    let height = vehicle
        .definition()
        .map_or(1.5, |d| d.config.dimensions.height);
    Mat4::from_rotation_translation(
        Quat::from_rotation_z(vehicle.yaw),
        vehicle.position + Vec3::Z * (height * 0.5 - vehicle.ride_height()),
    )
}
impl Ragdoll {
    pub fn new(
        def: &Definition,
        palette: [Mat4; JOINT_LIMIT],
        anchor: Vec3,
        yaw: f32,
        velocity: Vec3,
        city: &City,
        vehicles: &[Vehicle],
    ) -> Self {
        let actor = actor_matrix(anchor, yaw);
        let mut start = def.joint_worlds(&palette);
        for world in start.iter_mut().take(def.joint_count()) {
            *world = actor * *world;
        }
        let mut doll = Self {
            age: 0.,
            palette,
            anchor,
            yaw,
            initial_root: start[def.joint("Root")],
            initial_hips: start[def.joint("Hips")],
            start,
            limbs: Vec::new(),
            cars: Vec::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            joints: ImpulseJointSet::new(),
            multibody: MultibodyJointSet::new(),
            pipeline: PhysicsPipeline::new(),
            islands: IslandManager::new(),
            broad: BroadPhaseMultiSap::new(),
            narrow: NarrowPhase::new(),
            ccd: CCDSolver::new(),
            floor_ceiling: anchor.z + 0.8,
        };
        let definitions = [
            ("Hips", "Spine", None, 0.14, 12.),
            ("Spine", "Chest", Some("Hips"), 0.13, 10.),
            ("Chest", "Neck", Some("Spine"), 0.16, 14.),
            ("Head", "Head", Some("Chest"), 0.105, 5.),
            ("UpperArm_L", "Forearm_L", Some("Chest"), 0.065, 2.3),
            ("Forearm_L", "Hand_L", Some("UpperArm_L"), 0.055, 1.4),
            ("Hand_L", "Hand_L", Some("Forearm_L"), 0.045, 0.5),
            ("UpperArm_R", "Forearm_R", Some("Chest"), 0.065, 2.3),
            ("Forearm_R", "Hand_R", Some("UpperArm_R"), 0.055, 1.4),
            ("Hand_R", "Hand_R", Some("Forearm_R"), 0.045, 0.5),
            ("Thigh_L", "Shin_L", Some("Hips"), 0.085, 7.),
            ("Shin_L", "Foot_L", Some("Thigh_L"), 0.065, 3.5),
            ("Foot_L", "Toe_L", Some("Shin_L"), 0.065, 1.),
            ("Thigh_R", "Shin_R", Some("Hips"), 0.085, 7.),
            ("Shin_R", "Foot_R", Some("Thigh_R"), 0.065, 3.5),
            ("Foot_R", "Toe_R", Some("Shin_R"), 0.065, 1.),
        ];
        for (name, end, parent, radius, mass) in definitions {
            let joint = def.joint(name);
            let a = doll.start[joint].w_axis.truncate();
            let mut b = doll.start[def.joint(end)].w_axis.truncate();
            if end == name {
                b += if name == "Head" {
                    actor.transform_vector3(Vec3::Y) * 0.13
                } else {
                    (a - doll.start[def.joint(if name == "Hand_L" {
                        "Forearm_L"
                    } else {
                        "Forearm_R"
                    })]
                    .w_axis
                    .truncate())
                        * 0.28
                };
            }
            let delta = b - a;
            let length = delta.length().max(0.01);
            let rotation = Quat::from_rotation_arc(Vec3::Y, delta / length);
            let frame = Mat4::from_rotation_translation(rotation, (a + b) * 0.5);
            let mut body = RigidBodyBuilder::dynamic()
                .position(isometry(frame))
                .linear_damping(0.8)
                .angular_damping(2.8)
                .ccd_enabled(true)
                .additional_solver_iterations(6)
                .build();
            let speed = velocity.length();
            let kick = if speed > 0.01 {
                velocity / speed * (speed * 0.38).clamp(1.2, 7.)
            } else {
                Vec3::ZERO
            };
            let upper = 0.7 + (a.z - anchor.z).max(0.) * 0.25;
            body.set_linvel(vector![kick.x * upper, kick.y * upper, kick.z + 0.6], true);
            body.set_angvel(vector![-kick.y * 0.18, kick.x * 0.18, 0.], true);
            let handle = doll.bodies.insert(body);
            doll.colliders.insert_with_parent(
                ColliderBuilder::capsule_y((length * 0.5 - radius).max(0.01), radius)
                    .mass(mass)
                    .friction(0.8)
                    .restitution(0.02)
                    .collision_groups(InteractionGroups::new(Group::GROUP_1, Group::GROUP_2)),
                handle,
                &mut doll.bodies,
            );
            if let Some(parent) = parent {
                let parent_joint = def.joint(parent);
                let parent_body = doll
                    .limbs
                    .iter()
                    .find(|l| l.joint == parent_joint)
                    .unwrap()
                    .body;
                let anchor_point = if name == "Head" {
                    doll.start[def.joint("Neck")].w_axis.truncate()
                } else {
                    a
                };
                let mut anchor_frame = actor;
                anchor_frame.w_axis = anchor_point.extend(1.);
                let anchor_iso = isometry(anchor_frame);
                let knee = name.starts_with("Shin");
                let mask = if knee {
                    JointAxesMask::LOCKED_REVOLUTE_AXES
                } else {
                    JointAxesMask::LOCKED_SPHERICAL_AXES
                };
                let mut constraint = GenericJointBuilder::new(mask)
                    .local_frame1(doll.bodies[parent_body].position().inverse() * anchor_iso)
                    .local_frame2(doll.bodies[handle].position().inverse() * anchor_iso)
                    .limits(
                        JointAxis::AngX,
                        if knee { [-0.15, 2.1] } else { [-1.1, 1.1] },
                    )
                    .contacts_enabled(false);
                if !knee {
                    constraint = constraint
                        .limits(JointAxis::AngY, [-0.65, 0.65])
                        .limits(JointAxis::AngZ, [-1.1, 1.1]);
                }
                doll.joints.insert(parent_body, handle, constraint, true);
            }
            doll.limbs.push(Limb {
                joint,
                body: handle,
                bone_from_body: frame.inverse() * doll.start[joint],
            });
        }
        // A bounded, deck-aware ground patch includes curbs and road slopes.
        let mut points = Vec::new();
        let mut triangles = Vec::new();
        const N: usize = 25;
        for y in 0..N {
            for x in 0..N {
                let p = anchor + Vec3::new(x as f32 - 12., y as f32 - 12., 0.);
                let z = city
                    .surface_at(
                        (p.truncate() / METERS_PER_UNIT).to_array(),
                        doll.floor_ceiling / METERS_PER_UNIT,
                    )
                    .filter(|s| s.kind != SurfaceKind::Water)
                    .map_or(anchor.z, |s| s.height * METERS_PER_UNIT);
                points.push(point![p.x, p.y, z]);
            }
        }
        for y in 0..N - 1 {
            for x in 0..N - 1 {
                let a = (y * N + x) as u32;
                let n = N as u32;
                triangles.extend([[a, a + 1, a + n], [a + 1, a + n + 1, a + n]]);
            }
        }
        doll.colliders.insert(
            ColliderBuilder::trimesh(points, triangles)
                .friction(0.9)
                .collision_groups(InteractionGroups::new(Group::GROUP_2, Group::GROUP_1)),
        );
        for (id, vehicle) in vehicles.iter().enumerate() {
            if vehicle.position.distance(anchor) > 22. {
                continue;
            }
            let height = vehicle
                .definition()
                .map_or(1.5, |d| d.config.dimensions.height);
            let handle = doll.bodies.insert(
                RigidBodyBuilder::kinematic_position_based()
                    .position(isometry(car_matrix(vehicle))),
            );
            doll.colliders.insert_with_parent(
                ColliderBuilder::cuboid(
                    vehicle.length() * 0.5,
                    vehicle.width() * 0.5,
                    height * 0.5,
                )
                .friction(0.65)
                .collision_groups(InteractionGroups::new(Group::GROUP_2, Group::GROUP_1)),
                handle,
                &mut doll.bodies,
            );
            doll.cars.push((id, handle));
        }
        doll
    }
    pub fn step(&mut self, def: &Definition, city: &City, vehicles: &[Vehicle], dt: f32) {
        self.age += dt;
        let steps = (dt * 120.).ceil().max(1.) as usize;
        for _ in 0..steps {
            for &(id, handle) in &self.cars {
                if let Some(vehicle) = vehicles.get(id) {
                    self.bodies[handle].set_next_kinematic_position(isometry(car_matrix(vehicle)));
                }
            }
            self.pipeline.step(
                &vector![0., 0., -9.81],
                &IntegrationParameters {
                    dt: dt / steps as f32,
                    ..Default::default()
                },
                &mut self.islands,
                &mut self.broad,
                &mut self.narrow,
                &mut self.bodies,
                &mut self.colliders,
                &mut self.joints,
                &mut self.multibody,
                &mut self.ccd,
                None,
                &(),
                &(),
            );
        }
        let mut worlds = self.start;
        for limb in &self.limbs {
            worlds[limb.joint] = matrix(self.bodies[limb.body].position()) * limb.bone_from_body;
        }
        let root = def.joint("Root");
        let hips = def.joint("Hips");
        worlds[root] = worlds[hips] * self.initial_hips.inverse() * self.initial_root;
        fn resolve(
            doll: &Ragdoll,
            def: &Definition,
            i: usize,
            worlds: &mut [Mat4; JOINT_LIMIT],
            done: &mut [bool; JOINT_LIMIT],
        ) {
            if done[i] {
                return;
            }
            if let Some(p) = def.joint_parent(i) {
                resolve(doll, def, p, worlds, done);
                worlds[i] = worlds[p] * doll.start[p].inverse() * doll.start[i];
            }
            done[i] = true;
        }
        let mut done = [false; JOINT_LIMIT];
        done[root] = true;
        for limb in &self.limbs {
            done[limb.joint] = true;
        }
        for i in 0..def.joint_count() {
            resolve(self, def, i, &mut worlds, &mut done);
        }
        let hip = worlds[hips].w_axis.truncate();
        let z = city
            .surface_at(
                (hip.truncate() / METERS_PER_UNIT).to_array(),
                self.floor_ceiling / METERS_PER_UNIT,
            )
            .map_or(self.anchor.z, |s| s.height * METERS_PER_UNIT);
        if hip.is_finite() && hip.distance(self.anchor) < 30. {
            self.anchor = hip.truncate().extend(z);
            let inverse = actor_matrix(self.anchor, self.yaw).inverse();
            for world in worlds.iter_mut().take(def.joint_count()) {
                *world = inverse * *world;
            }
            self.palette = def.skin_from_worlds(&worlds);
        }
    }
    pub fn ready_to_rise(&self) -> bool {
        self.age >= 4.
            || (self.age >= 2.5
                && self
                    .limbs
                    .iter()
                    .all(|l| self.bodies[l.body].linvel().norm_squared() < 0.12))
    }
}

/// Swept player capsule. Only accepted travel toward a pedestrian can start a fall.
pub(super) fn walking_impact(position: Vec3, before: Vec3, after: Vec3, dt: f32) -> Option<Vec3> {
    if !dt.is_finite() || dt <= 0. || !before.is_finite() || !after.is_finite() {
        return None;
    }
    let travel = (after - before).truncate();
    let distance_squared = travel.length_squared();
    if distance_squared < (0.5 * dt).powi(2) || (position - before).truncate().dot(travel) <= 0. {
        return None;
    }
    let along = ((position - before).truncate().dot(travel) / distance_squared).clamp(0., 1.);
    let contact = before.lerp(after, along);
    let radius = super::player::RADIUS + 0.30;
    if (position - contact).truncate().length_squared() >= radius * radius
        || contact.z - super::player::HALF_HEIGHT >= position.z + 1.75
        || contact.z + super::player::HALF_HEIGHT <= position.z
    {
        return None;
    }
    Some((travel / dt).extend(0.))
}

/// Swept oriented vehicle footprint, including vertical clearance. Standing
/// or very slow cars do not repeatedly knock down a recovering pedestrian.
pub(super) fn impact(
    position: Vec3,
    previous: &[Vec3],
    vehicles: &[Vehicle],
    dt: f32,
) -> Option<(usize, Vec3)> {
    for (id, vehicle) in vehicles.iter().enumerate() {
        let speed = vehicle.velocity.length();
        if speed < 1.2 || !speed.is_finite() {
            continue;
        }
        let before = previous
            .get(id)
            .copied()
            .unwrap_or(vehicle.position - vehicle.velocity * dt);
        if before.distance(vehicle.position) > 25. {
            continue;
        }
        let bottom = vehicle.position.z - vehicle.ride_height();
        let height = vehicle
            .definition()
            .map_or(1.5, |d| d.config.dimensions.height);
        if position.z + 1.75 < bottom || position.z > bottom + height {
            continue;
        }
        let forward = Vec3::new(vehicle.yaw.cos(), vehicle.yaw.sin(), 0.);
        let left = Vec3::new(-forward.y, forward.x, 0.);
        let a = before - position;
        let delta = vehicle.position - before;
        let mut enter = 0f32;
        let mut exit = 1f32;
        for (axis, half) in [
            (forward, vehicle.length() * 0.5 + 0.32),
            (left, vehicle.width() * 0.5 + 0.32),
        ] {
            let start = a.dot(axis);
            let travel = delta.dot(axis);
            if travel.abs() < 1e-6 {
                if start.abs() > half {
                    enter = 2.;
                    break;
                }
            } else {
                let a = (-half - start) / travel;
                let b = (half - start) / travel;
                enter = enter.max(a.min(b));
                exit = exit.min(a.max(b));
            }
        }
        if enter <= exit && enter <= 1. && exit >= 0. {
            return Some((id, vehicle.velocity));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hermes::Hermes;
    #[test]
    fn player_sweep_catches_walk_and_sprint_but_respects_height_and_near_misses() {
        for (speed, dt) in [(1., 1. / 60.), (5.2, 1. / 30.), (15.6, 0.1)] {
            let before = Vec3::new(-0.705, 0., super::super::player::HALF_HEIGHT);
            let after = before + Vec3::X * speed * dt;
            assert!(walking_impact(Vec3::ZERO, before, after, dt)
                .unwrap()
                .abs_diff_eq(Vec3::X * speed, 1e-4));
            assert!(walking_impact(Vec3::Y, before, after, dt).is_none());
            assert!(walking_impact(Vec3::Z * 4., before, after, dt).is_none());
            assert!(
                walking_impact(Vec3::ZERO, before + Vec3::Z * 3., after + Vec3::Z * 3., dt)
                    .is_none()
            );
        }
        let before = Vec3::new(-0.4, 0., 0.9);
        assert!(walking_impact(Vec3::ZERO, before, before, 1. / 60.).is_none());
        assert!(walking_impact(Vec3::ZERO, before, before - Vec3::X, 1.).is_none());
        assert!(walking_impact(Vec3::ZERO, before, before + Vec3::X * 0.001, 1. / 60.).is_none());
        assert!(walking_impact(Vec3::ZERO, before, before + Vec3::X, 0.).is_none());
        // Both endpoints miss: a fast crossing must still contact the body between them.
        assert!(walking_impact(
            Vec3::ZERO,
            Vec3::new(-2., 0., 0.9),
            Vec3::new(2., 0., 0.9),
            0.1
        )
        .is_some());
    }
    #[test]
    fn ragdoll_rigs_keep_joint_anchors_together_and_recover_to_idle_for_all_six_models() {
        let hermes = Hermes::test_map();
        for def in pedestrian_catalog::definitions() {
            let palette = def.pose(0.3, 0.24, 1.);
            let mut doll = Ragdoll::new(
                def,
                palette,
                Vec3::ZERO,
                0.3,
                Vec3::new(6., 1., 0.),
                hermes.city(),
                &[],
            );
            assert_eq!(doll.limbs.len(), 16);
            assert_eq!(doll.joints.len(), 15);
            for _ in 0..480 {
                doll.step(def, hermes.city(), &[], 1. / 120.);
            }
            assert!(doll.ready_to_rise());
            assert!(doll.palette.iter().all(|m| m.is_finite()));
            let mut worst = 0f32;
            for (_, joint) in doll.joints.iter() {
                let a = doll.bodies[joint.body1].position() * joint.data.local_frame1;
                let b = doll.bodies[joint.body2].position() * joint.data.local_frame2;
                worst = worst.max((a.translation.vector - b.translation.vector).norm());
            }
            assert!(worst < 0.08, "ragdoll joints separated: {worst}");
            let recovered = def.get_up_pose(&doll.palette, 1.);
            let idle = def.pose(0.3, 0., 0.);
            assert!(recovered
                .iter()
                .zip(idle)
                .all(|(a, b)| a.abs_diff_eq(b, 0.0002)));
            assert!(doll.anchor.z.abs() < 0.5);
        }
    }
    #[test]
    fn vehicle_sweep_catches_fast_passes_but_ignores_stationary_cars_and_other_decks() {
        let hermes = Hermes::test_map();
        let mut car = Vehicle::spawn(hermes.city());
        car.yaw = 0.;
        car.position = Vec3::new(5., 0., car.ride_height());
        car.velocity = Vec3::X * 30.;
        let before = vec![Vec3::new(-5., 0., car.ride_height())];
        assert!(impact(Vec3::ZERO, &before, std::slice::from_ref(&car), 1. / 60.).is_some());
        assert!(impact(Vec3::Z * 10., &before, std::slice::from_ref(&car), 1. / 60.).is_none());
        car.velocity = Vec3::ZERO;
        car.position.x = 0.;
        assert!(impact(Vec3::ZERO, &before, &[car], 1. / 60.).is_none());
    }
}
