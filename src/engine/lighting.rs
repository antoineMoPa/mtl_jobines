use glam::Vec3;

/// A full simulated day takes twenty minutes of unpaused game time.
const DAY_SECONDS: f32 = 20. * 60.;

#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    pub daylight: f32,
    pub sun: Vec3,
    pub sky: [f32; 3],
}

impl Lighting {
    pub fn at_time(time: f32, manual: Option<f32>) -> Self {
        let phase = time.rem_euclid(DAY_SECONDS) / DAY_SECONDS * std::f32::consts::TAU;
        let daylight = manual.unwrap_or(0.5 + 0.5 * phase.cos()).clamp(0., 1.);
        // Keep the light above the horizon at night as a subtle moon light;
        // the manual 0..1 control follows the same low-to-high elevation.
        let direction = Vec3::new(-0.55 + phase.sin() * 0.28, -0.4, 0.12 + daylight * 0.8);
        let sun = direction / direction.length();
        let night = [0.007, 0.013, 0.035];
        let day = [0.70, 0.76, 0.70];
        let sky = std::array::from_fn(|i| night[i] + (day[i] - night[i]) * daylight);
        Self { daylight, sun, sky }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_and_manual_values_cover_day_and_night() {
        let noon = Lighting::at_time(0., None);
        let midnight = Lighting::at_time(DAY_SECONDS / 2., None);
        assert!((noon.daylight - 1.).abs() < 1e-6);
        assert!(midnight.daylight < 1e-6);
        assert_eq!(Lighting::at_time(75., Some(0.3)).daylight, 0.3);
        assert_eq!(Lighting::at_time(75., Some(2.)).daylight, 1.);
        assert!(noon.sky[2] > midnight.sky[2]);
        assert!(noon.sun.z > midnight.sun.z && midnight.sun.z > 0.);
    }
}
