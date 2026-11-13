//! Screen-space error selection. Render detail is independent of traffic physics.
use super::super::culling::Bounds;
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Level {
    Full,
    Medium,
    Coarse,
}
pub(in crate::engine::render) struct View {
    pub eye: Vec3,
    forward: Vec3,
    pixels: f32,
    orthographic: bool,
}
impl View {
    pub(super) fn new(eye: Vec3, look: Vec3, height: u32, span: Option<f32>) -> Self {
        let delta = look - eye;
        Self {
            eye,
            forward: delta / delta.length().max(0.00001),
            pixels: span.map_or(height as f32 / (2. * 29_f32.to_radians().tan()), |s| {
                height as f32 / s
            }),
            orthographic: span.is_some(),
        }
    }
    pub(super) fn projected_error(&self, bounds: Bounds, error: f32) -> f32 {
        if error == 0. {
            return 0.;
        }
        if self.orthographic {
            return error * self.pixels;
        }
        let center = (bounds.min + bounds.max) * 0.5;
        let radius = (bounds.max - bounds.min).length() * 0.5;
        let depth = (center - self.eye).dot(self.forward) - radius;
        if depth <= 0.003 {
            f32::INFINITY
        } else {
            error * self.pixels / depth
        }
    }
    pub(super) fn level(&self, bounds: Bounds, errors: [f32; 2], force: bool) -> Level {
        if force {
            return Level::Full;
        }
        if self.projected_error(bounds, errors[1]) <= 1. {
            Level::Coarse
        } else if self.projected_error(bounds, errors[0]) <= 1. {
            Level::Medium
        } else {
            Level::Full
        }
    }
    pub(super) fn shadow_allowed(
        &self,
        bounds: Bounds,
        error: f32,
        texels: f32,
        force: bool,
    ) -> bool {
        !force
            && self.eye.distance(self.eye.clamp(bounds.min, bounds.max)) >= 0.3
            && error * texels <= 0.75
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn box_at(depth: f32) -> Bounds {
        Bounds {
            min: Vec3::new(-0.02, depth - 0.02, -0.01),
            max: Vec3::new(0.02, depth + 0.02, 0.03),
        }
    }
    #[test]
    fn detail_tracks_pixel_error_resolution_and_projection_without_using_physics() {
        let view = View::new(Vec3::ZERO, Vec3::Y, 800, None);
        assert_eq!(
            view.level(box_at(0.1), [0.0005, 0.0015], false),
            Level::Full
        );
        assert_eq!(
            view.level(box_at(0.6), [0.0005, 0.0015], false),
            Level::Medium
        );
        assert_eq!(
            view.level(box_at(2.), [0.0005, 0.0015], false),
            Level::Coarse
        );
        assert_eq!(view.level(box_at(20.), [0.0005, 0.0015], true), Level::Full);
        let high = View::new(Vec3::ZERO, Vec3::Y, 1600, None);
        assert_eq!(
            high.level(box_at(0.6), [0.0005, 0.0015], false),
            Level::Full
        );
        let iso = View::new(Vec3::ZERO, Vec3::Y, 800, Some(2.));
        assert_eq!(
            iso.level(box_at(0.1), [0.0005, 0.0015], false),
            iso.level(box_at(20.), [0.0005, 0.0015], false)
        );
        assert!(!view.shadow_allowed(box_at(0.1), 0.0005, 640., false));
        assert!(view.shadow_allowed(box_at(2.), 0.0005, 640., false));
        assert!(!view.shadow_allowed(box_at(2.), 0.002, 640., false));
    }
}
