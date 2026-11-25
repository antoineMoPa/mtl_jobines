//! Arc-length frames for the two authored belts; travel is signed wheel distance.
use glam::{Quat, Vec3};
use std::f32::consts::{FRAC_PI_2, PI};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Belt {
    Motoneige(u8),
    Alpine(u8),
}
impl Belt {
    pub fn from_node(name: &str) -> Option<Self> {
        if let Some(index) = name.strip_prefix("snow_track_shoe_") {
            return index
                .parse::<u8>()
                .ok()
                .filter(|i| *i < 56)
                .map(Self::Alpine);
        }
        let rest = name.strip_prefix("track_belt_")?;
        let (index, finish) = rest.split_once('_')?;
        if !matches!(finish, "cleat" | "band") {
            return None;
        }
        index
            .parse::<u8>()
            .ok()
            .filter(|i| *i < 44)
            .map(Self::Motoneige)
    }
    fn frame(self, travel: f32) -> (Vec3, f32) {
        let (index, count, rear, front, center, radius) = match self {
            Self::Motoneige(i) => (i, 44., -1.120, -0.260, 0.230, 0.221),
            Self::Alpine(i) => (
                i,
                56.,
                -2.020 * super::ALPINE_SCALE,
                0.320 * super::ALPINE_SCALE,
                0.259 * super::ALPINE_SCALE,
                0.241 * super::ALPINE_SCALE,
            ),
        };
        let straight = front - rear;
        let arc = PI * radius;
        let length = 2. * (straight + arc);
        let s = (f32::from(index) * length / count + travel).rem_euclid(length);
        let (x, z, angle) = if s < straight {
            (front - s, center - radius, PI)
        } else if s < straight + arc {
            let a = -FRAC_PI_2 - (s - straight) / radius;
            (
                rear + radius * a.cos(),
                center + radius * a.sin(),
                a - FRAC_PI_2,
            )
        } else if s < 2. * straight + arc {
            (rear + s - straight - arc, center + radius, 0.)
        } else {
            let a = FRAC_PI_2 - (s - 2. * straight - arc) / radius;
            (
                front + radius * a.cos(),
                center + radius * a.sin(),
                a - FRAC_PI_2,
            )
        };
        (Vec3::new(x, 0., z), angle)
    }
    pub fn rest(self) -> Vec3 {
        self.frame(0.).0
    }
    pub fn motion(self, travel: f32) -> (Vec3, Quat) {
        let (rest, a) = self.frame(0.);
        let (point, b) = self.frame(travel);
        (point - rest, Quat::from_rotation_y(a - b))
    }
}
