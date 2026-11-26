//! A fixed-grip ski lift. Shared GLBs, terrain-following supports, and a short edited ride.
use super::{crowd::Instance, pedestrian_catalog, player, DynamicWorld, METERS_PER_UNIT};
use crate::hermes::{external_models, Hermes};
use glam::{Mat4, Quat, Vec3};
use opencity::runtime::{City, Mesh, SurfaceKind};
use std::{
    f32::consts::PI,
    sync::{Arc, OnceLock},
};

const TRACK_HALF: f32 = 2.4;
const SPEED: f32 = 2.5;
const RIDE_SECONDS: f32 = 8.;
const PYLON_CABLE: f32 = 12.95;
const STATION_CABLE: f32 = 3.1;
const LOAD_DISTANCE: f32 = 0.05;
const SIT_TIME: f32 = 0.65;

pub(super) struct Lift {
    base: Vec3,
    top: Vec3,
    forward: Vec3,
    right: Vec3,
    yaw: f32,
    supports: Vec<Vec3>,
    pylon_ground: Vec<Vec3>,
    path: Vec<Vec3>,
    distances: Vec<f32>,
    uphill_end: f32,
    cable: Mesh,
    clock: f32,
    ride: Option<Ride>,
    landing: Vec3,
    pose_model: usize,
    seated: [Mat4; pedestrian_catalog::JOINT_LIMIT],
    pose: [Mat4; pedestrian_catalog::JOINT_LIMIT],
}
struct Ride {
    elapsed: f32,
    chair: usize,
    start_distance: f32,
    seat_offset: f32,
    standing_worlds: [Mat4; pedestrian_catalog::JOINT_LIMIT],
}
fn ground(city: &City, point: Vec3) -> Option<Vec3> {
    let surface = city.surface_at(
        (point.truncate() / METERS_PER_UNIT).to_array(),
        f32::INFINITY,
    )?;
    (surface.kind != SurfaceKind::Water)
        .then_some(point.truncate().extend(surface.height * METERS_PER_UNIT))
}
fn model_assets() -> &'static [Arc<Mesh>; 4] {
    static ASSETS: OnceLock<[Arc<Mesh>; 4]> = OnceLock::new();
    ASSETS.get_or_init(|| {
        [
            crate::web_asset!(
                "../../data/equipment/chairlift/chair.glb",
                "data/equipment/chairlift/chair.glb"
            ),
            crate::web_asset!(
                "../../data/equipment/chairlift/pylon.glb",
                "data/equipment/chairlift/pylon.glb"
            ),
            crate::web_asset!(
                "../../data/equipment/chairlift/base-station.glb",
                "data/equipment/chairlift/base-station.glb"
            ),
            crate::web_asset!(
                "../../data/equipment/chairlift/top-station.glb",
                "data/equipment/chairlift/top-station.glb"
            ),
        ]
        .map(|bytes| external_models::decode_shared(bytes).expect("authored chairlift GLB"))
    })
}
fn transform(position: Vec3, yaw: f32, height_scale: f32) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::new(1., 1., height_scale) / METERS_PER_UNIT,
        Quat::from_rotation_z(yaw - PI / 2.),
        position / METERS_PER_UNIT,
    )
}
fn cable_segment(mesh: &mut Mesh, a: Vec3, b: Vec3) {
    let delta = b - a;
    let horizontal = delta.truncate().length();
    let side = if horizontal > 1e-6 {
        Vec3::new(-delta.y, delta.x, 0.) / horizontal
    } else {
        Vec3::X
    };
    let cross = delta.cross(side);
    let up = cross / cross.length().max(1e-6);
    for i in 0..6 {
        let offset = |j: usize| {
            let angle = j as f32 * 2. * PI / 6.;
            (side * angle.cos() + up * angle.sin()) * 0.035
        };
        let p = offset(i);
        let q = offset(i + 1);
        mesh.quad(
            [
                (a + p).to_array(),
                (b + p).to_array(),
                (b + q).to_array(),
                (a + q).to_array(),
            ],
            [0.035; 3],
            SurfaceKind::Prop,
        );
    }
}
impl Lift {
    pub(super) fn is_riding(&self) -> bool {
        self.ride.is_some()
    }
    pub fn new(hermes: &Hermes, [base_start, top_start]: [super::SpawnPoint; 2]) -> Option<Self> {
        let city = hermes.city();
        let base = ground(
            city,
            Vec3::new(base_start.point[0], base_start.point[1], 0.) * METERS_PER_UNIT,
        )?;
        let top = ground(
            city,
            Vec3::new(top_start.point[0], top_start.point[1], 0.) * METERS_PER_UNIT,
        )?;
        let delta = (top - base).truncate().extend(0.);
        let length = delta.length();
        if length < 40. || top.z <= base.z + 2. {
            return None;
        }
        let forward = delta / length;
        let right = Vec3::new(forward.y, -forward.x, 0.);
        let yaw = forward.y.atan2(forward.x);
        let mut supports = vec![base + Vec3::Z * STATION_CABLE];
        let count = (length / 65.).ceil() as usize;
        for i in 0..=count {
            let t = 0.035 + 0.93 * i as f32 / count as f32;
            supports.push(ground(city, base.lerp(top, t))? + Vec3::Z * PYLON_CABLE);
        }
        supports.push(top + Vec3::Z * STATION_CABLE);
        let pylon_ground = supports[1..supports.len() - 1]
            .iter()
            .map(|p| *p - Vec3::Z * PYLON_CABLE)
            .collect();
        // Raise intermediate towers when painted terrain rises above a cable span.
        for _ in 0..4 {
            for i in 0..supports.len() - 1 {
                let a = supports[i];
                let b = supports[i + 1];
                let sag = a.distance(b) * 0.012;
                let mut raise: f32 = 0.;
                for j in 1..20 {
                    let t = j as f32 / 20.;
                    let p = a.lerp(b, t) - Vec3::Z * 4. * sag * t * (1. - t);
                    let endpoint_distance = (p - base)
                        .truncate()
                        .length()
                        .min((p - top).truncate().length());
                    let clearance = 3.1 + (endpoint_distance / 15.).clamp(0., 1.) * 1.9;
                    raise = raise.max(ground(city, p)?.z + clearance - p.z);
                }
                if raise > 0. {
                    if i > 0 {
                        supports[i].z += raise;
                    }
                    if i + 1 < supports.len() - 1 {
                        supports[i + 1].z += raise;
                    }
                }
            }
        }
        let mut path = Vec::new();
        for side in [1., -1.] {
            let mut strand = Vec::new();
            for pair in supports.windows(2) {
                let a = pair[0];
                let b = pair[1];
                let sag = a.distance(b) * 0.012;
                for j in 0..12 {
                    let t = j as f32 / 12.;
                    strand.push(
                        a.lerp(b, t) + right * TRACK_HALF * side
                            - Vec3::Z * 4. * sag * t * (1. - t),
                    );
                }
            }
            strand.push(*supports.last()? + right * TRACK_HALF * side);
            if side > 0. {
                path.extend(strand);
                for i in 1..=16 {
                    let angle = i as f32 * PI / 16.;
                    path.push(
                        top + Vec3::Z * STATION_CABLE
                            + right * TRACK_HALF * angle.cos()
                            + forward * TRACK_HALF * angle.sin(),
                    );
                }
            } else {
                strand.reverse();
                path.extend(strand);
                for i in 1..=16 {
                    let angle = i as f32 * PI / 16.;
                    path.push(
                        base + Vec3::Z * STATION_CABLE
                            - right * TRACK_HALF * angle.cos()
                            - forward * TRACK_HALF * angle.sin(),
                    );
                }
            }
        }
        // Remove duplicate joins so arc-length lookup never divides by zero.
        path.dedup_by(|a, b| a.distance_squared(*b) < 1e-8);
        let mut distances = vec![0.];
        for p in path.windows(2) {
            distances.push(distances.last().copied().unwrap() + p[0].distance(p[1]));
        }
        let uphill_end = path
            .iter()
            .position(|p| p.distance(top + Vec3::Z * STATION_CABLE + right * TRACK_HALF) < 0.01)
            .map(|i| distances[i])?;
        let mut cable = Mesh::default();
        for p in path.windows(2) {
            cable_segment(&mut cable, p[0] - base, p[1] - base);
        }
        // The loading lane sits on the station's deck, under the uphill strand.
        let lane = base + right * TRACK_HALF + forward * 0.6 + Vec3::Z * 0.615;
        let corners = [
            lane - right * 1.05 - forward * 0.9,
            lane + right * 1.05 - forward * 0.9,
            lane + right * 1.05 + forward * 0.9,
            lane - right * 1.05 + forward * 0.9,
        ];
        cable.quad(
            corners.map(|p| (p - base).to_array()),
            [0.12, 0.42, 0.60],
            SurfaceKind::Prop,
        );
        cable.triangle(
            [
                (lane - right * 0.5 - forward * 0.2 - base).to_array(),
                (lane + right * 0.5 - forward * 0.2 - base).to_array(),
                (lane + forward * 0.55 - base).to_array(),
            ],
            [0.95, 0.95, 0.84],
            SurfaceKind::Prop,
            [0.; 3],
        );
        let landing = ground(city, top + forward * 9. + right * 6.)?;
        if player::Pedestrian::blocked(city, landing + Vec3::Z * player::HALF_HEIGHT) {
            return None;
        }
        Some(Self {
            base,
            top,
            forward,
            right,
            yaw,
            supports,
            pylon_ground,
            path,
            distances,
            uphill_end,
            cable,
            clock: 0.,
            ride: None,
            landing,
            pose_model: usize::MAX,
            seated: [Mat4::IDENTITY; pedestrian_catalog::JOINT_LIMIT],
            pose: [Mat4::IDENTITY; pedestrian_catalog::JOINT_LIMIT],
        })
    }
    fn at(&self, distance: f32) -> (Vec3, f32) {
        let total = *self.distances.last().unwrap();
        let d = distance.rem_euclid(total);
        let i = self
            .distances
            .partition_point(|&s| s <= d)
            .saturating_sub(1)
            .min(self.path.len() - 2);
        let a = self.path[i];
        let b = self.path[i + 1];
        let t = (d - self.distances[i]) / (self.distances[i + 1] - self.distances[i]);
        let delta = b - a;
        (a.lerp(b, t), delta.y.atan2(delta.x))
    }
    fn ride_distance(&self, time: f32) -> f32 {
        let short = 10_f32.min(self.uphill_end * 0.15);
        if time < 4. {
            self.ride
                .as_ref()
                .map_or(LOAD_DISTANCE, |r| r.start_distance)
                + short * (time / 4.).clamp(0., 1.)
        } else {
            self.uphill_end - short + short * ((time - 4.) / 4.).clamp(0., 1.)
        }
    }
    fn boarding(&self) -> Vec3 {
        self.base + self.right * TRACK_HALF + self.forward * 0.3 + Vec3::Z * 0.6
    }
    fn spacing(&self) -> (usize, f32) {
        let total = *self.distances.last().unwrap();
        let count = (total / 24.).ceil() as usize;
        (count, total / count as f32)
    }
    pub(super) fn platform(&self, p: Vec3) -> Option<super::pedestrian_gait::Ground> {
        for (center, sign, width, back) in [(self.base, 1., 5.2, 11.1), (self.top, -1., 4.7, 9.1)] {
            let delta = p - center;
            let x = delta.dot(self.right);
            let y = delta.dot(self.forward) * sign;
            if x.abs() > width || y < -back || y > 6.7 {
                continue;
            }
            let height = if y <= 3.7 { 0.6 } else { 0.6 * (6.7 - y) / 3. };
            if (p.z - center.z - height).abs() > 0.43 {
                continue;
            }
            let gradient = if y <= 3.7 {
                [0., 0.]
            } else {
                [-self.forward.x * sign * 0.2, -self.forward.y * sign * 0.2]
            };
            return Some(super::pedestrian_gait::Ground {
                point: p.truncate().extend(center.z + height),
                gradient,
            });
        }
        None
    }
    pub(super) fn blocked(&self, p: Vec3) -> bool {
        for (center, sign, width, depth) in [(self.base, 1., 4.5, 10.6), (self.top, -1., 4., 8.6)] {
            let d = p - center;
            let x = d.dot(self.right) * sign;
            let y = d.dot(self.forward) * sign;
            if x.abs() < width + 0.7
                && y > -depth - 0.5
                && y < 3.7
                && d.z - player::HALF_HEIGHT < 0.57
                && d.z + player::HALF_HEIGHT > -4.
            {
                return true;
            }
            if d.z > -0.4 && d.z < 6. {
                if ((x.abs() - width).abs() < 0.5 && y < -0.6 && y > -depth)
                    || ((y + depth).abs() < 0.5 && x.abs() < width + 0.4)
                    || (x.abs() < 0.5 && y.abs() < 0.5)
                {
                    return true;
                }
            }
        }
        self.supports[1..self.supports.len() - 1].iter().any(|s| {
            let d = p - *s;
            d.truncate().length() < 0.95 && d.z < 0.5 && d.z > -30.
        })
    }
}
// Pose in motion-space metres: pelvis on the seat, knees forward, feet hanging.
fn seated_pose(model: usize) -> [Mat4; pedestrian_catalog::JOINT_LIMIT] {
    let def = &pedestrian_catalog::definitions()[model];
    let locals = def.rest.clone();
    let rest = def.rest_worlds();
    let hip = rest[def.node("Hips")].w_axis.truncate();
    let root = Mat4::from_translation(Vec3::new(0., 0.12, 0.) - hip);
    let mut overrides = vec![None; locals.len()];
    for (side, label) in [(1., "L"), (-1., "R")] {
        let foot = def.node(&format!("Foot_{label}"));
        let (_, rotation, _) = rest[foot].to_scale_rotation_translation();
        super::cycling_pose::limb(
            def,
            &locals,
            root,
            &mut overrides,
            [
                &format!("Thigh_{label}"),
                &format!("Shin_{label}"),
                &format!("Foot_{label}"),
            ],
            Vec3::new(side * 0.14, -0.30, 0.42),
            Vec3::Z,
            rotation,
        );
        let hand = def.node(&format!("Hand_{label}"));
        let (_, rotation, _) = rest[hand].to_scale_rotation_translation();
        super::cycling_pose::limb(
            def,
            &locals,
            root,
            &mut overrides,
            [
                &format!("UpperArm_{label}"),
                &format!("Forearm_{label}"),
                &format!("Hand_{label}"),
            ],
            Vec3::new(side * 0.24, 0.16, 0.40),
            Vec3::new(side, -0.5, 0.),
            rotation,
        );
    }
    let worlds = def.worlds(&locals, root, &overrides);
    let mut skin = [Mat4::IDENTITY; pedestrian_catalog::JOINT_LIMIT];
    for (i, &node) in def.joints.iter().enumerate() {
        skin[i] = worlds[node] * def.inverse_bind[i];
    }
    skin
}
impl DynamicWorld {
    pub fn riding_chairlift(&self) -> bool {
        self.chairlifts.iter().any(|l| l.ride.is_some())
    }
    fn can_load_chairlift(&self) -> bool {
        self.pedestrian
            .as_ref()
            .is_some_and(|p| !p.swimming && p.fallen.is_none())
            && !self.riding_chairlift()
            && self.entry.is_none()
            && !self.metro.aboard
            && self.health > 0
            && self.wanted_stars() == 0
            && self.cat_grip().is_none()
    }
    fn start_chairlift(&mut self, line: usize, chair: usize, distance: f32) {
        let model = pedestrian_catalog::player_model(self.season);
        let def = &pedestrian_catalog::definitions()[model];
        let person = self.pedestrian.as_ref().unwrap();
        let mut standing = def.joint_worlds(person.instance(self.season).pose.unwrap());
        // At pickup, feet are on the 0.6 m deck and the seat is at 1.1 m.
        for joint in &mut standing {
            *joint = Mat4::from_translation(Vec3::new(0., -0.5, 0.)) * *joint;
        }
        let lift = &mut self.chairlifts[line];
        let across = (person.position - lift.base).dot(lift.right) - TRACK_HALF;
        let slot = ((across + 1.175) / 0.5875).floor().clamp(0., 3.);
        lift.pose_model = model;
        lift.seated = seated_pose(model);
        lift.ride = Some(Ride {
            elapsed: 0.,
            chair,
            start_distance: distance,
            seat_offset: -1.175 + (slot + 0.5) * 0.5875,
            standing_worlds: standing,
        });
        lift.pose = def.skin_from_worlds(&standing);
        self.place_chairlift_rider();
    }
    fn place_chairlift_rider(&mut self) {
        let Some(lift) = self.chairlifts.iter().find(|l| l.ride.is_some()) else {
            return;
        };
        let time = lift.ride.as_ref().unwrap().elapsed;
        let (grip, yaw) = lift.at(lift.ride_distance(time));
        if let Some(p) = self.pedestrian.as_mut() {
            let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.);
            p.position = grip + right * lift.ride.as_ref().unwrap().seat_offset
                - Vec3::Z * (2. - player::HALF_HEIGHT);
            p.yaw = yaw;
        }
    }
    pub(super) fn step_chairlift(&mut self, hermes: &Hermes, dt: f32) {
        let can_load = self.can_load_chairlift();
        let feet = self
            .pedestrian
            .as_ref()
            .map(|p| p.position - Vec3::Z * player::HALF_HEIGHT);
        let model = pedestrian_catalog::player_model(self.season);
        let mut pickup = None;
        let mut landing = None;
        for (line, lift) in self.chairlifts.iter_mut().enumerate() {
            let old = lift.clock * SPEED;
            let total = *lift.distances.last().unwrap();
            let (count, spacing) = lift.spacing();
            lift.clock = (lift.clock + dt).rem_euclid(total / SPEED);
            if lift.ride.is_none() {
                let in_lane = feet.is_some_and(|p| {
                    let d = p - lift.boarding();
                    d.dot(lift.right).abs() < 1.0
                        && d.dot(lift.forward).abs() < 0.65
                        && d.z.abs() < 0.25
                });
                if can_load && in_lane && pickup.is_none() {
                    if let Some(chair) = (0..count).find(|&i| {
                        (LOAD_DISTANCE - old - i as f32 * spacing).rem_euclid(total) <= SPEED * dt
                    }) {
                        pickup = Some((
                            line,
                            chair,
                            (lift.clock * SPEED + chair as f32 * spacing).rem_euclid(total),
                        ));
                    }
                }
                continue;
            }
            if self.pedestrian.is_none() {
                lift.ride = None;
                continue;
            }
            let ride = lift.ride.as_mut().unwrap();
            ride.elapsed = (ride.elapsed + dt).min(RIDE_SECONDS);
            if lift.pose_model != model {
                lift.seated = seated_pose(model);
                lift.pose_model = model;
            }
            let amount = (ride.elapsed / SIT_TIME).clamp(0., 1.);
            let amount = amount * amount * (3. - 2. * amount);
            lift.pose = if amount < 1. {
                let def = &pedestrian_catalog::definitions()[model];
                def.blend_worlds(
                    &ride.standing_worlds,
                    &def.joint_worlds(&lift.seated),
                    amount,
                )
            } else {
                lift.seated
            };
            let elapsed = ride.elapsed;
            let chair = ride.chair;
            let distance = lift.ride_distance(elapsed);
            lift.clock = (distance - chair as f32 * spacing).rem_euclid(total) / SPEED;
            if elapsed >= RIDE_SECONDS {
                landing = Some((lift.landing, lift.yaw + PI));
                lift.ride = None;
            }
        }
        if let Some((feet, yaw)) = landing {
            self.pedestrian = Some(player::Pedestrian::new(
                hermes.city(),
                feet + Vec3::Z * player::HALF_HEIGHT,
                yaw,
            ));
        } else if let Some((line, chair, distance)) = pickup {
            self.start_chairlift(line, chair, distance);
        } else {
            self.place_chairlift_rider();
        }
    }
    pub(super) fn reset_chairlift(&mut self) {
        for lift in &mut self.chairlifts {
            lift.ride = None;
        }
    }
    pub fn chairlift_fade(&self) -> f32 {
        self.chairlifts
            .iter()
            .find_map(|l| l.ride.as_ref())
            .map_or(0., |r| (1. - (r.elapsed - 4.).abs() / 0.4).clamp(0., 1.))
    }
    pub fn chairlift_camera(&self) -> Option<(Vec3, Vec3)> {
        if !self.riding_chairlift() {
            return None;
        }
        let p = self.pedestrian.as_ref()?;
        let forward = Vec3::new(p.yaw.cos(), p.yaw.sin(), 0.);
        let eye = (p.position + Vec3::Z * 0.15 + forward * 0.25) / METERS_PER_UNIT;
        Some((eye, eye + forward * 0.1 - Vec3::Z * 0.012))
    }
    pub(super) fn chairlift_rider_instance(&self) -> Option<Instance<'_>> {
        let l = self.chairlifts.iter().find(|l| l.ride.is_some())?;
        let p = self.pedestrian.as_ref()?;
        Some(Instance {
            model: pedestrian_catalog::player_model(self.season),
            transform: pedestrian_catalog::instance_transform(
                p.position - Vec3::Z * player::HALF_HEIGHT,
                p.yaw,
            ),
            zombie: false,
            opacity: 1.,
            idle_time: 0.,
            walk_time: 0.,
            movement: 0.,
            pose: Some(&l.pose),
        })
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn prepare_chairlift_boarding(&mut self, hermes: &Hermes) {
        let lift = self.chairlifts.first().expect("authored chairlift");
        let feet = lift.boarding();
        self.pedestrian = Some(player::Pedestrian::new(
            hermes.city(),
            feet + Vec3::Z * player::HALF_HEIGHT,
            lift.yaw,
        ));
    }
    pub fn chairlift_model_meshes(&self) -> Vec<&Mesh> {
        if self.chairlifts.is_empty() {
            return Vec::new();
        }
        model_assets()
            .iter()
            .map(|m| m.as_ref())
            .chain(self.chairlifts.iter().map(|l| &l.cable))
            .collect()
    }
    pub fn chairlift_instances(&self) -> Vec<(usize, Mat4)> {
        let center = self.player_position();
        let mut out = Vec::new();
        for (line, lift) in self.chairlifts.iter().enumerate() {
            if center.distance(lift.base).min(center.distance(lift.top)) > 1800. {
                continue;
            }
            out.push((4 + line, transform(lift.base, PI / 2., 1.)));
            for (model, p, yaw) in [(2, lift.base, lift.yaw), (3, lift.top, lift.yaw + PI)] {
                if p.distance(center) < 900. {
                    out.push((model, transform(p, yaw, 1.)));
                }
            }
            for (s, &ground) in lift.supports[1..lift.supports.len() - 1]
                .iter()
                .zip(&lift.pylon_ground)
            {
                if ground.distance(center) < 900. {
                    out.push((
                        1,
                        transform(ground, lift.yaw, (s.z - ground.z) / PYLON_CABLE),
                    ));
                }
            }
            let (count, spacing) = lift.spacing();
            for chair in 0..count {
                let (p, yaw) = lift.at(lift.clock * SPEED + chair as f32 * spacing);
                if p.distance(center) < 450. {
                    out.push((0, transform(p, yaw, 1.)));
                }
            }
        }
        out
    }
}
#[cfg(test)]
mod tests;
