//! Wheel slots follow the contact solver's rear-first order, with +Y to the left.
use super::Vehicle;
use glam::Vec3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::dynamic_world) enum Wheel {
    RearRight,
    RearLeft,
    FrontRight,
    FrontLeft,
}

impl Wheel {
    pub(in crate::dynamic_world) const ALL: [Self; 4] = [
        Self::RearRight,
        Self::RearLeft,
        Self::FrontRight,
        Self::FrontLeft,
    ];

    pub(in crate::dynamic_world) fn from_axle(front: bool, left: bool) -> Self {
        match (front, left) {
            (false, false) => Self::RearRight,
            (false, true) => Self::RearLeft,
            (true, false) => Self::FrontRight,
            (true, true) => Self::FrontLeft,
        }
    }

    pub(super) fn contact(self, vehicle: &Vehicle) -> Vec3 {
        let front = matches!(self, Self::FrontRight | Self::FrontLeft);
        let left = matches!(self, Self::RearLeft | Self::FrontLeft);
        let radius_loss = if vehicle.is_flat(self) {
            vehicle.wheel_radius() * (1. - FLAT_HEIGHT)
        } else {
            0.
        };
        Vec3::new(
            vehicle.wheelbase() * if front { 0.5 } else { -0.5 },
            vehicle.half_track() * if left { 1. } else { -1. },
            radius_loss - vehicle.ride_height(),
        )
    }
}

pub(in crate::dynamic_world) const FLAT_HEIGHT: f32 = 0.62;

impl Vehicle {
    pub(in crate::dynamic_world) fn police_tire_target(&self, observer: Vec3) -> Option<Vec3> {
        self.definition()?.road_wheels?;
        Wheel::ALL
            .into_iter()
            .filter(|&wheel| !self.is_flat(wheel))
            .map(|wheel| {
                let rubber = wheel.contact(self) + Vec3::Z * self.wheel_radius() * 0.15;
                self.position + self.orientation() * rubber
            })
            .min_by(|a, b| {
                a.distance_squared(observer)
                    .total_cmp(&b.distance_squared(observer))
            })
    }

    pub(in crate::dynamic_world) fn is_flat(&self, wheel: Wheel) -> bool {
        self.flat_tires[wheel as usize]
    }

    pub(in crate::dynamic_world) fn pop_tire(&mut self, wheel: Wheel) -> bool {
        !std::mem::replace(&mut self.flat_tires[wheel as usize], true)
    }
}
