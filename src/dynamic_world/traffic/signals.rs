//! Opposing approaches share green: 10 s green, 2 s amber, 2 s clearance per axis.
use super::Traffic;
use crate::hermes::traffic_paths::SignalApproach;
use glam::Vec3;
use opencity::runtime::{Mesh, SurfaceKind};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aspect {
    Red,
    Amber,
    Green,
}
fn aspect(signal: &SignalApproach, time: f32) -> Aspect {
    // Stable junction offsets avoid every block changing at the same instant.
    let clock = (time + (signal.junction % 11) as f32 * 3.).rem_euclid(28.);
    let phase = (clock / 14.) as usize;
    let elapsed = clock % 14.;
    if phase != signal.phase || elapsed >= 12. {
        Aspect::Red
    } else if elapsed >= 10. {
        Aspect::Amber
    } else {
        Aspect::Green
    }
}
pub(super) fn is_green(signal: &SignalApproach, time: f32) -> bool {
    aspect(signal, time) == Aspect::Green
}
impl Traffic {
    pub fn append_signals(&self, mesh: &mut Mesh, center: Vec3) {
        for signal in &self.graph.signals {
            if signal.position.distance(center) > 360. {
                continue;
            }
            let right = Vec3::new(signal.forward.y, -signal.forward.x, 0.);
            let base = signal.position + right * 2.8;
            let mut cube = |offset: Vec3, size: Vec3, color: [f32; 3], kind| {
                let at = base + offset;
                mesh.cuboid(
                    ((at - size * 0.5) / 100.).to_array(),
                    ((at + size * 0.5) / 100.).to_array(),
                    color,
                    kind,
                );
            };
            cube(
                Vec3::Z * 1.8,
                Vec3::new(0.12, 0.12, 3.6),
                [0.15, 0.17, 0.18],
                SurfaceKind::Prop,
            );
            cube(
                Vec3::Z * 3.5,
                Vec3::new(0.46, 0.46, 1.25),
                [0.08, 0.08, 0.07],
                SurfaceKind::Prop,
            );
            let active = aspect(signal, self.time);
            for (lamp, z, color) in [
                (Aspect::Red, 3.9, [1., 0.04, 0.02]),
                (Aspect::Amber, 3.5, [1., 0.65, 0.01]),
                (Aspect::Green, 3.1, [0.02, 1., 0.12]),
            ] {
                let color = if active == lamp {
                    color
                } else {
                    color.map(|c| c * 0.12)
                };
                cube(
                    Vec3::Z * z - signal.forward * 0.25,
                    Vec3::splat(0.28),
                    color,
                    if active == lamp {
                        SurfaceKind::Light
                    } else {
                        SurfaceKind::Prop
                    },
                );
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opposing_directions_share_green_and_cross_traffic_waits() {
        let signals: Vec<_> = (0..4)
            .map(|approach| SignalApproach {
                position: Vec3::ZERO,
                forward: Vec3::X,
                junction: 0,
                phase: approach / 2,
            })
            .collect();
        for tick in 0..560 {
            let time = tick as f32 * 0.1;
            assert_eq!(aspect(&signals[0], time), aspect(&signals[1], time));
            assert_eq!(aspect(&signals[2], time), aspect(&signals[3], time));
            assert!(!(is_green(&signals[0], time) && is_green(&signals[2], time)));
        }
        assert_eq!(aspect(&signals[0], 0.), Aspect::Green);
        assert_eq!(aspect(&signals[0], 10.), Aspect::Amber);
        assert_eq!(aspect(&signals[0], 12.), Aspect::Red);
        assert_eq!(aspect(&signals[2], 14.), Aspect::Green);
        assert_eq!(aspect(&signals[0], 28.), Aspect::Green);
    }
}
