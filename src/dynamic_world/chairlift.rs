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
    pose: [Mat4; pedestrian_catalog::JOINT_LIMIT],
}
struct Ride {
    elapsed: f32,
    start_clock: f32,
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
    pub fn new(hermes: &Hermes) -> Option<Self> {
        let [base_start, top_start] = hermes.chairlift?;
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
            path,
            distances,
            uphill_end,
            cable,
            clock: 0.,
            ride: None,
            landing,
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
            short * (time / 4.).clamp(0., 1.)
        } else {
            self.uphill_end - short + short * ((time - 4.) / 4.).clamp(0., 1.)
        }
    }
    fn boarding(&self) -> Vec3 {
        self.base + self.right * 5. + self.forward * 4.
    }
    fn blocked(&self, p: Vec3) -> bool {
        for (center, sign, width, depth) in [(self.base, 1., 4.5, 10.6), (self.top, -1., 4., 8.6)] {
            let d = p - center;
            let x = d.dot(self.right) * sign;
            let y = d.dot(self.forward) * sign;
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
            Vec3::new(side * 0.14, -0.66, 0.43),
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
        self.chairlift.as_ref().is_some_and(|l| l.ride.is_some())
    }
    pub fn chairlift_hint(&self) -> Option<&'static str> {
        let lift = self.chairlift.as_ref()?;
        let p = self.pedestrian.as_ref()?;
        if lift.ride.is_some()
            || self.entry.is_some()
            || self.metro.aboard
            || self.health == 0
            || p.swimming
            || p.fallen.is_some()
            || self.wanted_stars() > 0
            || self.cat_grip().is_some()
        {
            return None;
        }
        let feet = p.position - Vec3::Z * player::HALF_HEIGHT;
        (feet.distance(lift.boarding()) < 6.).then_some("Prendre le télésiège · sommet (8 s)")
    }
    pub fn board_chairlift(&mut self) -> bool {
        if self.chairlift_hint().is_none() {
            return false;
        }
        let lift = self.chairlift.as_mut().unwrap();
        lift.pose = seated_pose(pedestrian_catalog::player_model(self.season));
        lift.ride = Some(Ride {
            elapsed: 0.,
            start_clock: lift.clock,
        });
        self.place_chairlift_rider();
        true
    }
    fn place_chairlift_rider(&mut self) {
        let Some(lift) = self.chairlift.as_ref().filter(|l| l.ride.is_some()) else {
            return;
        };
        let time = lift.ride.as_ref().unwrap().elapsed;
        let (grip, yaw) = lift.at(lift.ride_distance(time));
        if let Some(p) = self.pedestrian.as_mut() {
            p.position = grip - Vec3::Z * (2. - player::HALF_HEIGHT);
            p.yaw = yaw;
        }
    }
    pub(super) fn step_chairlift(&mut self, hermes: &Hermes, dt: f32) {
        let Some(lift) = self.chairlift.as_mut() else {
            return;
        };
        lift.clock = (lift.clock + dt).rem_euclid(10000.);
        let Some(ride) = lift.ride.as_mut() else {
            return;
        };
        ride.elapsed = (ride.elapsed + dt).min(RIDE_SECONDS);
        if self.pedestrian.is_none() {
            lift.ride = None;
            return;
        }
        if ride.elapsed >= RIDE_SECONDS {
            let landing = lift.landing;
            let yaw = lift.yaw + PI;
            lift.ride = None;
            self.pedestrian = Some(player::Pedestrian::new(
                hermes.city(),
                landing + Vec3::Z * player::HALF_HEIGHT,
                yaw,
            ));
        } else {
            self.place_chairlift_rider();
        }
    }
    pub(super) fn reset_chairlift(&mut self) {
        if let Some(lift) = self.chairlift.as_mut() {
            lift.ride = None;
        }
    }
    pub fn chairlift_fade(&self) -> f32 {
        self.chairlift
            .as_ref()
            .and_then(|l| l.ride.as_ref())
            .map_or(0., |r| (1. - (r.elapsed - 4.).abs() / 0.4).clamp(0., 1.))
    }
    pub fn chairlift_camera(&self) -> Option<(Vec3, Vec3)> {
        if !self.riding_chairlift() {
            return None;
        }
        let p = self.pedestrian.as_ref()?;
        let forward = Vec3::new(p.yaw.cos(), p.yaw.sin(), 0.);
        let eye = (p.position + Vec3::Z * 0.65 + forward * 0.25) / METERS_PER_UNIT;
        Some((eye, eye + forward * 0.1 - Vec3::Z * 0.012))
    }
    pub(super) fn chairlift_rider_instance(&self) -> Option<Instance<'_>> {
        let l = self.chairlift.as_ref().filter(|l| l.ride.is_some())?;
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
    pub(super) fn chairlift_blocked(&self, position: Vec3) -> bool {
        self.chairlift.as_ref().is_some_and(|l| l.blocked(position))
    }
    pub fn chairlift_model_meshes(&self) -> Option<[&Mesh; 5]> {
        let lift = self.chairlift.as_ref()?;
        let models = model_assets();
        Some([&models[0], &models[1], &models[2], &models[3], &lift.cable])
    }
    pub fn chairlift_instances(&self) -> Vec<(usize, Mat4)> {
        let Some(lift) = self.chairlift.as_ref() else {
            return Vec::new();
        };
        let center = self.player_position();
        if center.distance(lift.base).min(center.distance(lift.top)) > 1800. {
            return Vec::new();
        }
        let mut out = vec![(4, transform(lift.base, PI / 2., 1.))];
        for (model, p, yaw) in [(2, lift.base, lift.yaw), (3, lift.top, lift.yaw + PI)] {
            if p.distance(center) < 900. {
                out.push((model, transform(p, yaw, 1.)));
            }
        }
        for s in &lift.supports[1..lift.supports.len() - 1] {
            let ground = ground_position(lift, *s);
            if ground.distance(center) < 900. {
                out.push((
                    1,
                    transform(ground, lift.yaw, (s.z - ground.z) / PYLON_CABLE),
                ));
            }
        }
        let total = *lift.distances.last().unwrap();
        let count = (total / 24.).ceil() as usize;
        let offset = if let Some(r) = &lift.ride {
            lift.ride_distance(r.elapsed)
        } else {
            lift.clock * SPEED
        };
        for i in 0..count {
            let distance = if lift.ride.is_some() {
                offset + i as f32 * total / count as f32
            } else {
                offset + i as f32 * total / count as f32
            };
            let (p, yaw) = lift.at(distance);
            if p.distance(center) < 450. {
                out.push((0, transform(p, yaw, 1.)));
            }
        }
        out
    }
}
#[cfg(test)]
mod tests;
