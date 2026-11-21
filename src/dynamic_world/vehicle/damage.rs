//! Collision cosmetics in chassis-local metres. The rigid-body solver owns handling.
use super::Vehicle;
use glam::{Mat4, Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Zone {
    Nose,
    Tail,
    LeftFront,
    LeftRear,
    RightFront,
    RightRear,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Piece {
    Panel(Zone),
    Glass(Zone, u8),
}

impl Piece {
    pub(crate) fn zone(self) -> Zone {
        match self {
            Self::Panel(z) | Self::Glass(z, _) => z,
        }
    }
}

#[derive(Default, Clone)]
pub(crate) struct Damage {
    strain: [f32; 6],
    depth: [f32; 6],
    cooldown: [f32; 6],
    pub(crate) loose: Vec<Loose>,
}

#[derive(Clone)]
pub(crate) struct Loose {
    pub(crate) part: usize,
    pivot: Vec3,
    origin: Vec3,
    orientation: Quat,
    velocity: Vec3,
    spin: Vec3,
    floor: f32,
    pub(crate) age: f32,
    pub(crate) dents: u32,
}

impl Loose {
    pub(crate) fn transform(&self) -> Mat4 {
        let fall = (self.velocity.z
            + (self.velocity.z.powi(2) + 19.62 * (self.origin.z - self.floor).max(0.)).sqrt())
            / 9.81;
        let t = self.age.min(fall);
        let center = self.origin + self.velocity * t - Vec3::Z * (4.905 * t * t);
        Mat4::from_scale(Vec3::splat(1. / super::METERS_PER_UNIT))
            * Mat4::from_rotation_translation(
                Quat::from_scaled_axis(self.spin * t) * self.orientation,
                center,
            )
            * Mat4::from_translation(-self.pivot)
    }
}

impl Damage {
    pub(crate) fn tick(&mut self, dt: f32) {
        for time in &mut self.cooldown {
            *time = (*time - dt).max(0.);
        }
        for part in &mut self.loose {
            part.age = (part.age + dt).min(5.);
        }
    }
    pub(crate) fn packed(&self) -> u32 {
        self.depth.iter().enumerate().fold(0, |bits, (i, depth)| {
            bits | ((depth * 7.).round() as u32) << (i * 3)
        })
    }
    pub(crate) fn strain(&self, zone: Zone) -> f32 {
        self.strain[zone as usize]
    }
}

impl Vehicle {
    /// Resolve a visible part after player camera smoothing. Loose pieces use world-space poses.
    pub fn damaged_instance(&self, part: usize, attached: Mat4) -> Option<(Mat4, u32)> {
        let Some(damage) = &self.damage else {
            return Some((attached, 0));
        };
        let loose = damage.loose.iter().find(|p| p.part == part);
        if loose.is_some_and(|p| p.age >= 5.) {
            return None;
        }
        let dents = if self.definition()?.receives_dents(part) {
            loose.map_or_else(|| damage.packed(), |p| p.dents)
        } else {
            0
        };
        Some((loose.map_or_else(|| attached, Loose::transform), dents))
    }

    pub(in crate::dynamic_world) fn crash_damage(&mut self, arm: Vec3, outward: Vec3, speed: f32) {
        if speed < 2.5 {
            return;
        }
        let Some(def) = self.definition() else { return };
        if !matches!(
            def.config.research_id.as_str(),
            "vehicle-v2-01" | "vehicle-12" | "vehicle-v2-09"
        ) {
            return;
        }
        let q = self.orientation();
        let local = q.conjugate() * arm;
        let face = q.conjugate() * outward;
        let zone = if face.x.abs() >= face.y.abs() {
            if face.x >= 0. {
                Zone::Nose
            } else {
                Zone::Tail
            }
        } else {
            match (face.y >= 0., local.x >= 0.) {
                (true, true) => Zone::LeftFront,
                (true, false) => Zone::LeftRear,
                (false, true) => Zone::RightFront,
                (false, false) => Zone::RightRear,
            }
        };
        let floor = self.position.z - self.ride_height();
        let orientation = q * self.cabin_lean();
        let damage = self.damage.get_or_insert_with(Default::default);
        let i = zone as usize;
        if damage.cooldown[i] > 0. {
            return;
        }
        damage.cooldown[i] = 0.15;
        damage.depth[i] = (damage.depth[i] + (speed / 22.22).powf(1.5)).min(1.);
        damage.strain[i] += (speed / (62. / 3.6)).powi(2);
        for (part, piece, pivot) in def.crash_pieces() {
            if piece.zone() != zone || damage.loose.iter().any(|p| p.part == part) {
                continue;
            }
            let spread = match piece {
                Piece::Panel(_) if damage.strain[i] >= 1. => 0.,
                Piece::Glass(_, shard) if speed >= 30. / 3.6 => shard as f32 - 1.5,
                _ => continue,
            };
            damage.loose.push(Loose {
                part,
                pivot,
                origin: self.position + orientation * pivot,
                orientation,
                velocity: self.velocity * 0.35
                    + outward * 2.4
                    + q * Vec3::Y * spread
                    + Vec3::Z * 1.3,
                spin: q * Vec3::new(face.y * 3., face.x * 3., spread + 0.45),
                floor: floor + 0.08,
                age: 0.,
                dents: damage.packed(),
            });
        }
    }
}
