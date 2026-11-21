//! Render-only impact debris. Positions and velocities are in metres until submission.
use crate::dynamic_world::{ShotSurface as Material, METERS_PER_UNIT as SCALE};
use crate::engine::render::ImpactVertex;
use glam::{Quat, Vec2, Vec3};
use opencity::runtime::{Mesh, SurfaceKind};
use std::collections::VecDeque;

#[derive(serde::Deserialize)]
pub(super) struct PanelLayer {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub color: [f32; 3],
}

pub(super) struct Contact {
    pub mark_owner: Option<u32>,
    pub point: Vec3,
    pub normal: Vec3,
    pub local_min: Vec3,
    pub local_max: Vec3,
    pub color: Vec3,
    pub material: Material,
}

struct Dust {
    origin: Vec3,
    velocity: Vec3,
    radius: f32,
    life: f32,
    age: f32,
    opacity: f32,
    seed: f32,
    color: Vec3,
}
struct Mark {
    entity: u32,
    center: Vec3,
    normal: Vec3,
    min: Vec3,
    max: Vec3,
    radius: Vec2,
    seed: f32,
    material: Material,
}
struct Face {
    points: [Vec3; 3],
    color: Vec3,
}
struct Fragment {
    faces: Vec<Face>,
    position: Vec3,
    rotation: Quat,
    velocity: Vec3,
    spin: Vec3,
    floor: f32,
    age: f32,
    life: f32,
    rest_height: f32,
    settled: bool,
}
struct Random(u32);
impl Random {
    fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.0 >> 8) as f32 / 16777216.
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.unit()
    }
}
pub(super) struct Effects {
    pub muzzle: super::muzzle::MuzzleFlash,
    dust: VecDeque<Dust>,
    marks: VecDeque<Mark>,
    fragments: VecDeque<Fragment>,
    rng: Random,
}
impl Default for Effects {
    fn default() -> Self {
        Self {
            muzzle: Default::default(),
            dust: Default::default(),
            marks: Default::default(),
            fragments: Default::default(),
            rng: Random(9137),
        }
    }
}
fn push<T>(items: &mut VecDeque<T>, item: T, limit: usize) {
    if items.len() == limit {
        items.pop_front();
    }
    items.push_back(item);
}

impl Effects {
    pub fn tire_pop(&mut self, point: Vec3, normal: Vec3) {
        let outward = normal.truncate().extend(0.);
        for _ in 0..7 {
            let rng = &mut self.rng;
            push(
                &mut self.dust,
                Dust {
                    origin: point + outward * 0.06,
                    velocity: outward * rng.range(1.8, 3.8)
                        + Vec3::new(
                            rng.range(-1.3, 1.3),
                            rng.range(-1.3, 1.3),
                            rng.range(1.5, 3.),
                        ),
                    radius: rng.range(0.32, 0.50),
                    life: rng.range(0.65, 0.95),
                    age: 0.,
                    opacity: 0.75,
                    seed: rng.range(1., 90.),
                    color: Vec3::splat(rng.range(0.48, 0.72)),
                },
                192,
            );
        }
    }

    pub fn strike(
        &mut self,
        hit: Contact,
        attachment: Option<(Vec3, f32)>,
        force: f32,
        floor: f32,
    ) {
        let mat = hit.material;
        let rng = &mut self.rng;
        let inverse = Quat::from_rotation_z(-attachment.map_or(0., |o| o.1));
        let local = inverse * (hit.point - attachment.map_or(Vec3::ZERO, |o| o.0));
        let radius = rng.range(0.085, 0.135) * force;
        if let Some(entity) = hit.mark_owner {
            push(
                &mut self.marks,
                Mark {
                    entity,
                    center: local,
                    normal: inverse * hit.normal,
                    min: hit.local_min,
                    max: hit.local_max,
                    radius: Vec2::new(
                        radius,
                        radius * if mat == Material::Wood { 1.65 } else { 0.93 },
                    ),
                    seed: rng.range(1., 90.),
                    material: mat,
                },
                96,
            );
        }
        let raw = match mat {
            Material::Wood => Vec3::new(0.67, 0.39, 0.17),
            Material::Concrete => Vec3::new(0.52, 0.53, 0.48),
            Material::Earth => Vec3::new(0.26, 0.19, 0.11),
            Material::Brick(rgb) => Vec3::from(rgb.map(|v| (v as f32 / 255.).powf(2.2))),
            Material::Glass => Vec3::new(0.64, 0.75, 0.77),
            Material::Asphalt => Vec3::new(0.23, 0.235, 0.24),
            Material::Metal => Vec3::new(0.61, 0.64, 0.67),
            Material::Rubber => Vec3::new(0.19, 0.15, 0.13),
            Material::Spectral => Vec3::new(0.18, 0.42, 0.31),
            Material::Wildlife => Vec3::new(0.52, 0.44, 0.33),
            Material::Water => Vec3::new(0.52, 0.65, 0.71),
        };
        let (tangent, up) = hit.normal.any_orthonormal_pair();
        // Several expanding lobes share a fast outward impulse. Finer dust hangs and lifts.
        for i in 0..match mat {
            Material::Glass => 1,
            Material::Metal | Material::Rubber | Material::Wildlife => 3,
            _ => 10,
        } {
            let angle = rng.range(0., std::f32::consts::TAU);
            let radial = tangent * angle.cos() + up * angle.sin();
            let spread = rng.range(0.8, 3.7) * force;
            let slow = i < 4;
            push(
                &mut self.dust,
                Dust {
                    origin: hit.point + hit.normal * 0.035 + radial * rng.range(0., 0.075),
                    velocity: (hit.normal * rng.range(1.4, 4.6) + radial * spread) * force,
                    radius: rng.range(0.22, 0.38) * force * if slow { 1.3 } else { 0.83 },
                    life: if slow {
                        rng.range(0.65, 1.0)
                    } else {
                        rng.range(0.26, 0.54)
                    },
                    opacity: if slow { 0.40 } else { 0.75 },
                    age: 0.,
                    seed: rng.range(1., 90.),
                    color: raw.lerp(
                        Vec3::splat(0.7),
                        if mat == Material::Concrete { 0.55 } else { 0.2 },
                    ) * rng.range(0.75, 1.05),
                },
                192,
            );
        }
        for i in 0..(match mat {
            Material::Rubber | Material::Spectral | Material::Wildlife => 0.,
            Material::Metal => 9.,
            _ => 22.,
        } * force) as usize
        {
            let axis = if mat == Material::Wood { Vec3::Z } else { up };
            let size = rng.range(0.012, 0.058) * force;
            let tip = axis
                * size
                * if mat == Material::Wood {
                    rng.range(1.9, 3.7)
                } else {
                    1.2
                };
            let wide = if mat == Material::Wood {
                size * 0.37
            } else {
                size
            };
            let (u, v) = axis.any_orthonormal_pair();
            let points = [
                tip,
                u * wide - tip * 0.4,
                v * wide - tip * 0.65,
                -u * wide * 0.6 - tip * 0.85,
            ];
            let color = if i % 3 == 0 { hit.color } else { raw } * rng.range(0.57, 1.12);
            let faces = [[0, 1, 2], [0, 2, 3], [0, 3, 1], [1, 3, 2]].map(|ix| Face {
                points: ix.map(|k| points[k]),
                color,
            });
            let angle = rng.range(0., std::f32::consts::TAU);
            let spread = rng.range(1.3, 5.1) * force;
            push(
                &mut self.fragments,
                Fragment {
                    faces: faces.into(),
                    position: hit.point + hit.normal * 0.04,
                    rotation: Quat::from_rotation_x(rng.range(-1.2, 1.2)),
                    velocity: hit.normal * rng.range(1.1, 4.1) * force
                        + (tangent * angle.cos() + up * angle.sin()) * spread
                        + Vec3::Z * 0.5,
                    spin: Vec3::new(
                        rng.range(-14., 14.),
                        rng.range(-15., 15.),
                        rng.range(-9., 9.),
                    ),
                    floor,
                    age: 0.,
                    life: rng.range(1.3, 2.8),
                    rest_height: size * 0.35,
                    settled: false,
                },
                384,
            );
        }
    }

    #[cfg(test)]
    pub fn has_mark(&self, material: Material) -> bool {
        self.marks.iter().any(|m| m.material == material)
    }

    pub fn remove_marks(&mut self, entity: u32) {
        self.marks.retain(|m| m.entity != entity);
    }

    pub fn step(&mut self, dt: f32) {
        self.muzzle.step(dt);
        for p in &mut self.dust {
            p.age += dt;
        }
        self.dust.retain(|p| p.age < p.life);
        for f in &mut self.fragments {
            f.age += dt;
            if f.settled {
                continue;
            }
            f.velocity.z -= 9.81 * dt;
            f.position += f.velocity * dt;
            f.rotation = Quat::from_scaled_axis(f.spin * dt) * f.rotation;
            let bottom = f.floor + f.rest_height;
            if f.position.z < bottom && f.velocity.z < 0. {
                f.position.z = bottom;
                f.velocity *= Vec3::new(0.55, 0.55, -0.25);
                f.spin *= 0.42;
                f.settled = f.velocity.length_squared() < 0.32;
            }
        }
        self.fragments.retain(|f| f.age < f.life);
    }

    pub fn solids(&self, mesh: &mut Mesh) {
        for f in &self.fragments {
            let fade = ((f.life - f.age) * 3.).clamp(0., 1.);
            for face in &f.faces {
                mesh.triangle(
                    face.points
                        .map(|p| ((f.position + f.rotation * p * fade) / SCALE).into()),
                    face.color.into(),
                    SurfaceKind::Prop,
                    [0.; 3],
                );
            }
        }
    }

    pub fn sprites(
        &self,
        pose: impl Fn(u32) -> Option<(Vec3, f32)>,
        rotation: Quat,
        eye: Vec3,
    ) -> Vec<ImpactVertex> {
        let mut out = Vec::with_capacity(6 * (self.marks.len() + self.dust.len()));
        for m in &self.marks {
            let attachment = pose(m.entity);
            let rot = Quat::from_rotation_z(attachment.map_or(0., |o| o.1));
            let base = attachment.map_or(Vec3::ZERO, |o| o.0);
            let tangent = if m.normal.z.abs() > 0.99 {
                Vec3::X
            } else {
                let t = Vec3::Z.cross(m.normal);
                t / t.length()
            };
            let up = m.normal.cross(tangent);
            let kind = match m.material {
                Material::Wood => 1.,
                Material::Concrete => 2.,
                Material::Earth => 3.,
                Material::Asphalt => 4.,
                Material::Brick(_) => 5.,
                Material::Glass => 6.,
                _ => 2.,
            };
            let make = |uv: Vec2| {
                let p = (m.center + tangent * uv.x * m.radius.x + up * uv.y * m.radius.y)
                    .clamp(m.min, m.max);
                let d = p - m.center;
                ImpactVertex::new(
                    (base + rot * (p + m.normal * 0.004)) / SCALE,
                    [d.dot(tangent) / m.radius.x, d.dot(up) / m.radius.y],
                    if let Material::Brick(rgb) = m.material {
                        Vec3::from(rgb.map(|c| (c as f32 / 255.).powf(2.2)))
                    } else {
                        Vec3::ONE
                    },
                    [kind, m.seed, 1., 0.],
                )
            };
            quad(&mut out, make);
        }
        let forward = rotation * Vec3::X;
        let mut dust: Vec<_> = self
            .dust
            .iter()
            .map(|d| {
                let t = d.age;
                let pos = d.origin
                    + d.velocity * ((1. - (-4.3 * t).exp()) / 4.3)
                    + Vec3::new(0., 0.13, 0.38) * t * t;
                ((pos - eye).dot(forward), pos, d)
            })
            .collect();
        dust.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (_, p, d) in dust {
            let t = d.age / d.life;
            let size = d.radius * (0.28 + 1.9 * t.sqrt());
            let roll = rotation * Quat::from_rotation_x(d.seed + t * 0.55);
            quad(&mut out, |uv| {
                ImpactVertex::new(
                    (p + roll * (Vec3::Y * uv.x * size * 1.14 + Vec3::Z * uv.y * size)) / SCALE,
                    uv.into(),
                    d.color,
                    [0., d.seed, d.opacity * (1. - t) * (1. - t), t],
                )
            });
        }
        out
    }
}
pub(super) fn quad(out: &mut Vec<ImpactVertex>, make: impl Fn(Vec2) -> ImpactVertex) {
    let [a, b, c, d] =
        [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)].map(|(x, y)| make(Vec2::new(x, y)));
    out.extend([a, b, c, a, c, d]);
}

// Clip each painted layer against the same fracture cells, so paint stays on its shard.
fn clip(poly: &[Vec2], normal: Vec2, offset: f32) -> Vec<Vec2> {
    let mut out = Vec::new();
    if let Some(&last) = poly.last() {
        let mut a = last;
        for &b in poly {
            let da = a.dot(normal) - offset;
            let db = b.dot(normal) - offset;
            if (da < 0.) != (db < 0.) {
                out.push(a.lerp(b, da / (da - db)));
            }
            if db <= 0. {
                out.push(b);
            }
            a = b;
        }
    }
    out
}
impl Effects {
    pub fn shatter(
        &mut self,
        position: Vec3,
        yaw: f32,
        parts: &[PanelLayer],
        point: Vec3,
        floor: f32,
    ) {
        let rng = &mut self.rng;
        let rot = Quat::from_rotation_z(yaw);
        let local = rot.conjugate() * (point - position);
        let lo = parts.iter().fold(Vec2::splat(f32::INFINITY), |a, p| {
            a.min(Vec2::new(p.min[1], p.min[2]))
        });
        let hi = parts.iter().fold(Vec2::splat(f32::NEG_INFINITY), |a, p| {
            a.max(Vec2::new(p.max[1], p.max[2]))
        });
        let site = Vec2::new(local.y, local.z);
        let mut seeds = vec![site.clamp(lo, hi)];
        for row in 0..3 {
            for col in 0..5 {
                seeds.push(
                    lo + (hi - lo)
                        * Vec2::new(
                            col as f32 + rng.range(0.08, 0.92),
                            row as f32 + rng.range(0.08, 0.92),
                        )
                        / Vec2::new(5., 3.),
                );
            }
        }
        for &seed in &seeds {
            let mut faces = Vec::new();
            let center = Vec3::new((parts[0].min[0] + parts[0].max[0]) * 0.5, seed.x, seed.y);
            for part in parts {
                let [x, y, z] = part.min;
                let [xx, yy, zz] = part.max;
                let mut cell = vec![
                    Vec2::new(y, z),
                    Vec2::new(yy, z),
                    Vec2::new(yy, zz),
                    Vec2::new(y, zz),
                ];
                for &other in &seeds {
                    if other != seed {
                        let grain = (other - seed) * Vec2::new(1.7, 0.6);
                        cell = clip(&cell, grain, grain.dot((seed + other) * 0.5));
                    }
                    if cell.len() < 3 {
                        break;
                    }
                }
                if cell.len() < 3 {
                    continue;
                }
                let vertex = |x: f32, p: Vec2| Vec3::new(x, p.x, p.y) - center;
                let color = Vec3::from(part.color);
                for i in 1..cell.len() - 1 {
                    for (depth, ids) in [(x, [0, i + 1, i]), (xx, [0, i, i + 1])] {
                        faces.push(Face {
                            points: ids.map(|j| vertex(depth, cell[j])),
                            color,
                        });
                    }
                }
                for i in 0..cell.len() {
                    let (a, b) = (cell[i], cell[(i + 1) % cell.len()]);
                    let [p, q, r, s] = [vertex(x, a), vertex(x, b), vertex(xx, b), vertex(xx, a)];
                    let edge = Vec3::new(0.53, 0.31, 0.14) * rng.range(0.77, 1.15);
                    for points in [[p, q, r], [p, r, s]] {
                        faces.push(Face {
                            points,
                            color: edge,
                        });
                    }
                }
            }
            if faces.is_empty() {
                continue;
            }
            let offset = seed - site;
            let impulse = 1. / (0.28 + offset.length());
            let push_x = if local.x <= center.x { 1. } else { -1. };
            push(
                &mut self.fragments,
                Fragment {
                    faces,
                    position: position + rot * center,
                    rotation: rot,
                    velocity: rot
                        * Vec3::new(
                            push_x * rng.range(0.7, 1.4) * impulse,
                            offset.x * impulse * 3.2,
                            offset.y * impulse * 3. + 1.4,
                        ),
                    spin: Vec3::new(rng.range(-4., 4.), rng.range(-9., 9.), rng.range(-6., 6.)),
                    floor,
                    rest_height: 0.09,
                    age: 0.,
                    life: rng.range(3.5, 5.),
                    settled: false,
                },
                384,
            );
        }
    }
}
