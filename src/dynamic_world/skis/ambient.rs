//! Winter skiers loop between the piste, the loading queue, and real lift chairs.
use super::{
    super::{
        chairlift::{seat_offset, seated_pose, Lift, LOAD_DISTANCE, SEATS, SEAT_WIDTH},
        crowd::Instance,
        pedestrian_catalog::{self, JOINT_LIMIT},
        pedestrian_gait::{self, Ground},
        player, METERS_PER_UNIT,
    },
    append, meshes,
    movement::Motion,
    pose::Pose,
};
use crate::{
    hermes::{ski_hill::Area, Hermes},
    season::Season,
};
use glam::{Mat4, Vec2, Vec3};
use opencity::runtime::{City, Mesh, SurfaceKind};
use std::{collections::VecDeque, f32::consts::TAU, sync::OnceLock};

pub(in crate::dynamic_world) const CAPACITY: usize = 16;
const VIEW_RADIUS: f32 = 280.;

// Centerlines of the two painted pistes. The first and last points connect to the lift ramps.
const CHALET: &[[f32; 2]] = &[
    [-1.90, -85.66],
    [-1.60, -85.81],
    [-1.22, -85.85],
    [-0.85, -85.72],
    [-0.45, -85.38],
    [-0.05, -85.13],
    [0.50, -84.74],
    [0.93, -84.40],
    [1.55, -83.90],
    [2.00, -83.63],
    [2.65, -83.30],
    [3.15, -83.10],
    [3.48, -82.75],
    [3.53, -82.48],
];
// The Chalet piste starts across the summit terminal from its unloading ramp.
const CHALET_EXIT: &[[f32; 2]] = &[[-2.10, -85.49], [-2.00, -85.69]];
const SOUTH: &[[f32; 2]] = &[
    [-2.02, -85.94],
    [-1.84, -86.21],
    [-1.86, -86.57],
    [-2.05, -86.96],
    [-2.46, -87.40],
    [-2.88, -87.80],
    [-3.25, -88.20],
    [-3.43, -88.60],
    [-3.44, -89.00],
    [-3.30, -89.42],
    [-3.06, -89.80],
    [-3.04, -90.22],
    [-3.07, -90.60],
    [-3.24, -90.96],
    [-3.52, -91.42],
    [-3.88, -91.79],
    [-4.28, -92.06],
    [-4.60, -92.13],
];

fn ground(city: &City, lift: &Lift, point: Vec3) -> Option<Ground> {
    if let Some(deck) = lift.platform_surface(point) {
        return Some(deck);
    }
    let surface = city.surface_at(
        (point.truncate() / METERS_PER_UNIT).to_array(),
        f32::INFINITY,
    )?;
    if !matches!(surface.kind, SurfaceKind::Grass | SurfaceKind::Sand) {
        return None;
    }
    let point = point.truncate().extend(surface.height * METERS_PER_UNIT);
    (!player::Pedestrian::blocked(city, point + Vec3::Z * player::HALF_HEIGHT)).then_some(Ground {
        point,
        gradient: surface.gradient,
    })
}

struct Piste {
    base: [f32; 2],
    exit: &'static [[f32; 2]],
    centerline: &'static [[f32; 2]],
}

const PISTES: [Piste; 2] = [
    Piste {
        base: [3.8, -82.6],
        exit: CHALET_EXIT,
        centerline: CHALET,
    },
    Piste {
        base: [-4.85, -92.05],
        exit: &[],
        centerline: SOUTH,
    },
];

struct Run {
    samples: Vec<(f32, Ground)>,
    length: f32,
}

impl Run {
    fn build(
        hermes: &Hermes,
        lift: &Lift,
        piste: &Piste,
        variant: usize,
        width: f32,
    ) -> Option<Self> {
        let hill = hermes.ski_hill.as_ref()?;
        let top = lift.ambient_gate(true);
        let base = lift.ambient_gate(false);
        let points: Vec<_> = top
            .into_iter()
            .map(|p| p.truncate())
            .chain(
                piste
                    .exit
                    .iter()
                    .chain(piste.centerline)
                    .map(|p| Vec2::from_array(*p) * METERS_PER_UNIT),
            )
            .chain([base[2].truncate()])
            .collect();
        let slalom_start = top.len() + piste.exit.len();
        let slalom_end = points.len() - 2;
        let piste_length: f32 = points[slalom_start..=slalom_end]
            .windows(2)
            .map(|p| p[0].distance(p[1]))
            .sum();
        let mut piste_distance = 0.;
        let mut samples: Vec<(f32, Ground)> = Vec::new();
        let mut length = 0.;
        for segment in 0..points.len() - 1 {
            let a = points[segment];
            let b = points[segment + 1];
            let before = points[segment.saturating_sub(1)];
            let after = points[(segment + 2).min(points.len() - 1)];
            let m = (b - before) * 0.5;
            let n = (after - a) * 0.5;
            let span = a.distance(b);
            let steps = (span / 1.25).ceil() as usize;
            let on_piste = (slalom_start..slalom_end).contains(&segment);
            for step in 0..steps + usize::from(segment == points.len() - 2) {
                let t = step as f32 / steps as f32;
                let t2 = t * t;
                let t3 = t2 * t;
                let mut p = if segment < 2 {
                    a.lerp(b, t)
                } else {
                    a * (2. * t3 - 3. * t2 + 1.)
                        + m * (t3 - 2. * t2 + t)
                        + b * (3. * t2 - 2. * t3)
                        + n * (t3 - t2)
                };
                if on_piste {
                    let d = piste_distance + span * t;
                    let taper = (d.min(piste_length - d) / 40.).clamp(0., 1.);
                    let angle = d * TAU / (76. + variant as f32 * 4.) + variant as f32 * 1.6;
                    let tangent = (b - a) * (6. * t - 6. * t2)
                        + m * (3. * t2 - 4. * t + 1.)
                        + n * (3. * t2 - 2. * t);
                    p += tangent.perp()
                        * (angle.sin() * width * taper * taper / tangent.length().max(0.01));
                    if hill.area_at((p / METERS_PER_UNIT).to_array()) != Some(Area::Piste) {
                        return None;
                    }
                }
                let support = ground(hermes.city(), lift, p.extend(0.))?;
                if lift.blocked(support.point + Vec3::Z * player::HALF_HEIGHT) {
                    return None;
                }
                if let Some((_, prior)) = samples.last() {
                    length += prior.point.distance(support.point);
                }
                samples.push((length, support));
            }
            if on_piste {
                piste_distance += span;
            }
        }
        Some(Self { samples, length })
    }

    fn sample(&self, distance: f32) -> Ground {
        let index = self
            .samples
            .partition_point(|(s, _)| *s <= distance)
            .saturating_sub(1)
            .min(self.samples.len() - 2);
        let (start, a) = self.samples[index];
        let (end, b) = self.samples[index + 1];
        let t = ((distance - start) / (end - start)).clamp(0., 1.);
        Ground {
            point: a.point.lerp(b.point, t),
            gradient: Vec2::from_array(a.gradient)
                .lerp(Vec2::from_array(b.gradient), t)
                .to_array(),
        }
    }

    fn advance(&self, distance: f32, pace: f32, dt: f32) -> f32 {
        let grade = self.sample(distance + 4.).point.z - self.sample(distance - 4.).point.z;
        let speed = (9.5 - grade * 2.0).clamp(5., 19.) * pace;
        // Clear the unloading ramp and brake into the lift queue at walking speed.
        let station_speed = 2.5 + (distance - 10.).min(self.length - distance).max(0.) * 0.45;
        (distance + speed.min(station_speed) * dt).min(self.length)
    }
}

#[derive(Clone, Copy)]
enum Phase {
    Downhill(f32),
    Queue(Vec3),
    Riding { chair: usize, seat: usize },
}

struct Skier {
    model: usize,
    variant: usize,
    pace: f32,
    phase: Phase,
    frame: Option<Frame>,
}

struct Frame {
    transform: Mat4,
    palette: [Mat4; JOINT_LIMIT],
    gear: [Mat4; 6],
}

struct SkiModel {
    seated: [Mat4; JOINT_LIMIT],
    gear: [Mat4; 6],
}

impl SkiModel {
    fn new(model: usize) -> Self {
        let seated = seated_pose(model);
        let definition = &pedestrian_catalog::definitions()[model];
        let joints = definition.joint_worlds(&seated);
        let gear = std::array::from_fn(|i| {
            let left = i % 2 == 0;
            let pole = i >= 4;
            let joint = definition.joint(match (pole, left) {
                (false, true) => "Foot_L",
                (false, false) => "Foot_R",
                (true, true) => "Hand_L",
                (true, false) => "Hand_R",
            });
            let p = joints[joint].w_axis.truncate();
            let side = if left { 1. } else { -1. };
            let (height, across, lean) = if pole {
                (1.13, 0.30, 0.28)
            } else {
                (0.09, 0.13, 0.)
            };
            Mat4::from_translation(Vec3::new(p.z, p.x, p.y - if pole { 0. } else { height }))
                * Mat4::from_rotation_y(lean)
                * Mat4::from_translation(Vec3::new(
                    0.,
                    -side * across,
                    if pole { -height } else { 0. },
                ))
        });
        Self { seated, gear }
    }
}

struct Line {
    lift: usize,
    runs: Vec<Run>,
    skiers: Vec<Skier>,
    queue: VecDeque<usize>,
    clock: f32,
}

impl Line {
    fn new(hermes: &Hermes, lift: &Lift, id: usize, piste: &Piste, count: usize) -> Option<Self> {
        let runs: Vec<_> = (0..4)
            .filter_map(|variant| {
                [9., 6., 3.]
                    .into_iter()
                    .find_map(|width| Run::build(hermes, lift, piste, variant, width))
            })
            .collect();
        runs.first()?;
        let chairs = lift.ambient_chairs();
        let ascending: Vec<_> = (0..chairs.count)
            .filter(|&chair| {
                let distance = chairs.distance(chairs.clock, chair);
                (0.78 * chairs.uphill_end..0.97 * chairs.uphill_end).contains(&distance)
                    && lift.ambient_loading_chair() != Some(chair)
            })
            .collect();
        let queued = (count / 2).min(SEATS);
        let skiers = (0..count)
            .map(|i| {
                let phase = if i < queued {
                    Phase::Queue(lift.ambient_queue(i).0)
                } else if let Some(&chair) = ascending
                    .get((i - queued) * ascending.len() / (count - queued - 1).max(1))
                    .filter(|_| i < count - 2)
                {
                    Phase::Riding {
                        chair,
                        seat: i % SEATS,
                    }
                } else {
                    Phase::Downhill(runs[i % runs.len()].length * (0.47 + 0.17 * (i % 3) as f32))
                };
                Skier {
                    model: (i + id * 3) % pedestrian_catalog::CROWD_MODEL_COUNT,
                    variant: i % runs.len(),
                    pace: 0.84 + 0.055 * ((i * 3 + id) % 7) as f32,
                    phase,
                    frame: None,
                }
            })
            .collect();
        Some(Self {
            lift: id,
            runs,
            skiers,
            queue: (0..queued).collect(),
            clock: chairs.clock,
        })
    }

    fn step(&mut self, city: &City, lift: &Lift, models: &[SkiModel], eye: Vec3, dt: f32) {
        let chairs = lift.ambient_chairs();
        for (i, skier) in self.skiers.iter_mut().enumerate() {
            match skier.phase {
                Phase::Riding { chair, seat }
                    if chairs.passed(self.clock, chair, chairs.uphill_end) =>
                {
                    skier.phase = Phase::Downhill(seat as f32 * SEAT_WIDTH);
                }
                Phase::Downhill(distance) => {
                    let run = &self.runs[skier.variant];
                    let next = run.advance(distance, skier.pace, dt);
                    if next >= run.length {
                        skier.phase = Phase::Queue(run.sample(run.length).point);
                        self.queue.push_back(i);
                    } else {
                        skier.phase = Phase::Downhill(next);
                    }
                }
                _ => {}
            }
        }
        // Only capture a chair at its real loading position. The player ride
        // can advance the cable through the middle of the hill in one frame.
        let load = (0..chairs.count).find(|&chair| {
            let distance = chairs.distance(chairs.clock, chair);
            chairs.passed(self.clock, chair, LOAD_DISTANCE)
                && (LOAD_DISTANCE..LOAD_DISTANCE + 0.7).contains(&distance)
                && lift.ambient_loading_chair() != Some(chair)
        });
        let mut boarded = 0;
        for (slot, &id) in self.queue.iter().enumerate() {
            let skier = &mut self.skiers[id];
            let Phase::Queue(position) = skier.phase else {
                unreachable!()
            };
            let target = lift.ambient_queue(slot).0;
            let delta = target.truncate() - position.truncate();
            let step = delta.length().min(2.8 * dt);
            let next = position + (delta * (step / delta.length().max(0.001))).extend(0.);
            skier.phase = Phase::Queue(next);
            if let Some(chair) =
                load.filter(|_| slot == boarded && slot < SEATS && delta.length() <= 0.65)
            {
                skier.phase = Phase::Riding { chair, seat: slot };
                boarded += 1;
            }
        }
        self.queue.drain(..boarded);
        self.clock = chairs.clock;

        for skier in &mut self.skiers {
            skier.frame = match skier.phase {
                Phase::Riding { chair, seat } => {
                    let (grip, yaw) = lift.at(chairs.distance(chairs.clock, chair));
                    let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.);
                    let point = grip + right * seat_offset(seat) - Vec3::Z * 2.;
                    let model = &models[skier.model];
                    (point.distance_squared(eye) < VIEW_RADIUS * VIEW_RADIUS).then(|| {
                        let transform = Mat4::from_rotation_translation(
                            glam::Quat::from_rotation_z(yaw),
                            point,
                        );
                        Frame {
                            transform: pedestrian_catalog::instance_transform(point, yaw),
                            palette: model.seated,
                            gear: model.gear.map(|part| transform * part),
                        }
                    })
                }
                Phase::Downhill(distance) => {
                    let run = &self.runs[skier.variant];
                    let center = run.sample(distance);
                    let before = run.sample(distance - 3.).point;
                    let after = run.sample(distance + 3.).point;
                    let heading = after - before;
                    let a = (center.point - before).truncate();
                    let b = (after - center.point).truncate();
                    let turn = a.perp_dot(b).atan2(a.dot(b));
                    let speed = (run.advance(distance, skier.pace, 0.05) - distance) / 0.05;
                    skier.ski_frame(
                        city,
                        lift,
                        eye,
                        center,
                        heading.y.atan2(heading.x),
                        speed,
                        turn,
                    )
                }
                Phase::Queue(position) => ground(city, lift, position).and_then(|center| {
                    skier.ski_frame(city, lift, eye, center, lift.ambient_queue(0).1, 0., 0.)
                }),
            };
        }
    }
}

impl Skier {
    fn ski_frame(
        &self,
        city: &City,
        lift: &Lift,
        eye: Vec3,
        center: Ground,
        yaw: f32,
        speed: f32,
        turn: f32,
    ) -> Option<Frame> {
        if center.point.distance_squared(eye) >= VIEW_RADIUS * VIEW_RADIUS {
            return None;
        }
        let support = |p| {
            lift.platform_surface(p)
                .or_else(|| pedestrian_gait::footing(city, p, false))
        };
        let center = support(center.point).unwrap_or(center);
        let mut motion = Motion::new(center.point, yaw, center);
        motion.velocity = Vec3::new(yaw.cos(), yaw.sin(), 0.) * speed;
        motion.tuck_blend = ((speed - 2.) / 24.).clamp(0., 0.6);
        motion.turn_edge = (-turn * (speed / 8.).min(1.) * 1.5).clamp(-0.45, 0.45);
        let pose = Pose::for_model(&mut motion, &support, self.model);
        Some(Frame {
            transform: pose.transform,
            palette: pose.palette,
            gear: pose.gear,
        })
    }
}

#[derive(Default)]
pub(in crate::dynamic_world) struct Skiers {
    lines: Option<Vec<Line>>,
    models: Vec<SkiModel>,
}

impl Skiers {
    pub fn clear(&mut self) {
        self.lines = None;
    }

    pub fn step(&mut self, hermes: &Hermes, lifts: &[Lift], season: Season, eye: Vec3, dt: f32) {
        let near = |lift: &Lift| {
            lift.ambient_stations()
                .iter()
                .any(|p| p.truncate().distance_squared(eye.truncate()) < 850_f32.powi(2))
        };
        if season != Season::Winter || hermes.ski_hill.is_none() || !lifts.iter().any(near) {
            self.lines = None;
            return;
        }
        let lines = self.lines.get_or_insert_with(|| {
            let count = crate::performance::npc_budget(CAPACITY) / PISTES.len();
            PISTES
                .iter()
                .filter_map(|piste| {
                    let base = Vec2::from_array(piste.base) * METERS_PER_UNIT;
                    let (i, lift) = lifts.iter().enumerate().find(|(_, lift)| {
                        lift.ambient_stations()[0].truncate().distance(base) < 40.
                    })?;
                    Line::new(hermes, lift, i, piste, count)
                })
                .collect()
        });
        if !lines.is_empty() && self.models.is_empty() {
            self.models = (0..pedestrian_catalog::CROWD_MODEL_COUNT)
                .map(SkiModel::new)
                .collect();
        }
        for line in lines {
            line.step(hermes.city(), &lifts[line.lift], &self.models, eye, dt);
        }
    }

    fn visible(&self) -> impl Iterator<Item = (&Skier, &Frame)> {
        self.lines
            .iter()
            .flatten()
            .flat_map(|line| &line.skiers)
            .filter_map(|skier| Some((skier, skier.frame.as_ref()?)))
    }

    pub fn instances(&self) -> impl Iterator<Item = Instance<'_>> {
        self.visible().map(|(skier, frame)| Instance {
            model: skier.model,
            transform: frame.transform,
            pose: Some(&frame.palette),
            zombie: false,
            opacity: 1.,
            idle_time: 0.,
            walk_time: 0.,
            movement: 0.,
        })
    }

    pub fn append_gear(&self, output: &mut Mesh) {
        let scale = Mat4::from_scale(Vec3::splat(1. / METERS_PER_UNIT));
        for (_, frame) in self.visible() {
            for (mesh, transform) in meshes()[..4].iter().zip(frame.gear) {
                append(mesh, output, scale * transform);
            }
            for (mesh, transform) in pole_meshes().iter().zip(&frame.gear[4..]) {
                append(mesh, output, scale * *transform);
            }
        }
    }
}

// The player asset has tens of thousands of pole vertices. Preserve the grip
// and basket positions with simpler geometry for the ambient population.
fn pole_meshes() -> &'static [Mesh; 2] {
    static POLES: OnceLock<[Mesh; 2]> = OnceLock::new();
    POLES.get_or_init(|| {
        [0.30, -0.30].map(|y| {
            let mut mesh = Mesh::default();
            let kind = SurfaceKind::Prop;
            mesh.cylinder([0., y, 0.], 0.007, 1.16, [0.45, 0.49, 0.53], kind);
            mesh.cylinder([0., y, 1.05], 0.017, 0.15, [0.08, 0.09, 0.10], kind);
            mesh.cylinder([0., y, 0.06], 0.046, 0.014, [0.08, 0.09, 0.10], kind);
            mesh
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_skiers_carve_both_pistes_and_board_in_groups() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let mut world = super::super::super::DynamicWorld::new(&hermes);
        world.prepare_chairlift_boarding(&hermes);
        world.set_season(&hermes, Season::Winter);
        assert_eq!(world.traffic.count(), 3);
        world.step(&hermes, Default::default(), Default::default(), 0.01);
        assert!(world.ambient_skiers.instances().count() >= SEATS);
        let mut gear = Mesh::default();
        world.ambient_skiers.append_gear(&mut gear);
        assert!(
            !gear.vertices.is_empty()
                && gear
                    .vertices
                    .iter()
                    .all(|v| Vec3::from_array(v.position).is_finite())
        );
        world.reset_player(&hermes);

        let lifts = &world.chairlifts;
        let mut skiers = Skiers::default();
        let eye = lifts[0].ambient_stations()[0];
        skiers.step(&hermes, lifts, Season::Summer, eye, 0.);
        assert!(skiers.lines.is_none());

        // Advance the actual lift clock, leaving the player off the loading lane.
        world.step_chairlift(&hermes, 0.1);
        skiers.step(&hermes, &world.chairlifts, Season::Winter, eye, 0.1);
        {
            let lines = skiers.lines.as_ref().unwrap();
            assert_eq!(lines.len(), 2);
            assert_eq!(
                lines.iter().map(|line| line.skiers.len()).sum::<usize>(),
                CAPACITY
            );
            for line in lines {
                let run = &line.runs[0];
                assert!(line.runs.len() >= 2);
                assert!(run.length > 500.);
                assert!(run.sample(0.).point.z - run.sample(run.length).point.z > 80.);
                let sign = |d| {
                    let a = run.sample(d - 8.).point;
                    let b = run.sample(d).point;
                    let c = run.sample(d + 8.).point;
                    (b - a).truncate().perp_dot((c - b).truncate())
                };
                let turns: Vec<_> = (40..run.length as usize - 40)
                    .step_by(10)
                    .map(|d| sign(d as f32))
                    .collect();
                assert!(turns.iter().filter(|&&s| s > 2.).count() > 6);
                assert!(turns.iter().filter(|&&s| s < -2.).count() > 6);
            }
        }

        let mut group_sizes = [false; SEATS + 1];
        let mut saw_unload = false;
        let mut saw_return = false;
        for _ in 0..1400 {
            world.step_chairlift(&hermes, 0.25);
            let lines = skiers.lines.as_ref().unwrap();
            let previous: Vec<_> = lines
                .iter()
                .flat_map(|line| line.skiers.iter().map(|skier| skier.phase))
                .collect();
            skiers.step(&hermes, &world.chairlifts, Season::Winter, eye, 0.25);
            let lines = skiers.lines.as_ref().unwrap();
            let mut phases = previous.into_iter();
            for line in lines {
                let mut boarded = 0;
                let mut seats = std::collections::HashSet::new();
                for skier in &line.skiers {
                    let before = phases.next().unwrap();
                    if let Phase::Riding { chair, seat } = skier.phase {
                        assert!(seat < SEATS && seats.insert((chair, seat)));
                    }
                    match (before, skier.phase) {
                        (Phase::Queue(_), Phase::Riding { .. }) => boarded += 1,
                        (Phase::Riding { .. }, Phase::Downhill(_)) => saw_unload = true,
                        (Phase::Downhill(_), Phase::Queue(_)) => saw_return = true,
                        _ => {}
                    }
                }
                group_sizes[boarded] = true;
            }
            if group_sizes[1] && group_sizes[4] && saw_unload && saw_return {
                break;
            }
        }
        assert!(
            group_sizes[1] && group_sizes[4] && saw_unload && saw_return,
            "{group_sizes:?}"
        );
        assert!(skiers
            .instances()
            .all(|instance| instance.transform.is_finite()
                && instance.pose.unwrap().iter().all(|joint| joint.is_finite())));
    }
}
