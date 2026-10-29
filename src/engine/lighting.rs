use glam::Vec3;

/// A full simulated day takes twenty minutes of unpaused game time.
const DAY_SECONDS: f32 = 20. * 60.;

/// Game-owned solar/weather controls. Directions use east/north/up (Z up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkySettings {
    pub latitude: f32,
    pub day_of_year: u32,
    pub north_rotation: f32,
    pub solar_hour: Option<f32>,
    pub turbidity: f32,
    pub sun_temperature: f32,
    pub sun_intensity: f32,
    pub star_density: f32,
    pub star_size: f32,
    pub star_brightness: f32,
    pub star_color: [f32; 3],
    pub horizon_glow: f32,
}
impl Default for SkySettings {
    fn default() -> Self {
        Self {
            latitude: 45.5,
            day_of_year: 172,
            north_rotation: 0.,
            solar_hour: None,
            turbidity: 2.5,
            sun_temperature: 5778.,
            sun_intensity: 1.,
            star_density: 0.18,
            star_size: 1.,
            star_brightness: 2.5,
            star_color: [0.83, 0.90, 1.],
            horizon_glow: 1.,
        }
    }
}
impl SkySettings {
    pub fn sun_direction(self, solar_hour: f32) -> Vec3 {
        let latitude = self.latitude.to_radians();
        let declination = (23.44_f32.to_radians().sin()
            * ((self.day_of_year as f32 - 80.) * std::f32::consts::TAU / 365.).sin())
        .asin();
        let hour = (solar_hour - 12.) * std::f32::consts::TAU / 24.;
        let east = -declination.cos() * hour.sin();
        let up =
            latitude.sin() * declination.sin() + latitude.cos() * declination.cos() * hour.cos();
        let north =
            latitude.cos() * declination.sin() - latitude.sin() * declination.cos() * hour.cos();
        let rotation = self.north_rotation.to_radians();
        Vec3::new(
            east * rotation.cos() - north * rotation.sin(),
            east * rotation.sin() + north * rotation.cos(),
            up,
        )
    }
    pub fn hour(self, time: f32, manual: Option<f32>) -> f32 {
        self.solar_hour.unwrap_or_else(|| match manual {
            // Keep bracket keys as a continuous noon-to-midnight control.
            Some(value) => 12. + (1. - value.clamp(0., 1.)) * 12.,
            None => (12. + time.rem_euclid(DAY_SECONDS) / DAY_SECONDS * 24.).rem_euclid(24.),
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    pub daylight: f32,
    /// Shadow light remains above the horizon at night as subtle moonlight.
    pub sun: Vec3,
    pub solar: Vec3,
    pub sun_color: [f32; 3],
    pub star_visibility: f32,
}
impl Lighting {
    pub fn with_settings(time: f32, manual: Option<f32>, settings: SkySettings) -> Self {
        let solar = settings.sun_direction(settings.hour(time, manual));
        let t = ((solar.z + 0.14) / 0.34).clamp(0., 1.);
        let daylight = t * t * (3. - 2. * t);
        // Keep the artistic night fill above the horizon continuously. The
        // atmosphere still uses the astronomical solar direction unchanged.
        let direction = Vec3::new(solar.x, solar.y, solar.z.max(0.12));
        let sun = direction / direction.length();
        let temperature = ((settings.sun_temperature - 2000.) / 8000.).clamp(0., 1.);
        let warm = [1., 0.43, 0.15];
        let cool = [0.87, 0.94, 1.];
        let sun_color = std::array::from_fn(|i| warm[i] + (cool[i] - warm[i]) * temperature);
        let star_t = ((solar.z + 0.18) / 0.145).clamp(0., 1.);
        let star_visibility = 1. - star_t * star_t * (3. - 2. * star_t);
        Self {
            daylight,
            sun,
            solar,
            sun_color,
            star_visibility,
        }
    }
    #[cfg(test)]
    pub fn at_time(time: f32, manual: Option<f32>) -> Self {
        Self::with_settings(time, manual, SkySettings::default())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cycle_and_manual_values_cover_day_and_night() {
        let noon = Lighting::at_time(0., None);
        let midnight = Lighting::at_time(DAY_SECONDS / 2., None);
        assert_eq!(noon.daylight, 1.);
        assert_eq!(midnight.daylight, 0.);
        assert_eq!(Lighting::at_time(75., Some(2.)).daylight, 1.);
        assert!(noon.sun.z > midnight.sun.z && midnight.sun.z > 0.);
        assert!(midnight.solar.z < 0.);
        assert_eq!(noon.star_visibility, 0.);
        assert_eq!(midnight.star_visibility, 1.);
    }
    #[test]
    fn solar_mapping_preserves_seasons_and_z_up_north_rotation() {
        let mut settings = SkySettings::default();
        let summer = settings.sun_direction(12.);
        settings.day_of_year = 355;
        assert!(summer.z > settings.sun_direction(12.).z);
        assert!(settings.sun_direction(0.).z < 0.);
        let original = settings.sun_direction(9.);
        settings.north_rotation = 90.;
        let rotated = settings.sun_direction(9.);
        assert!((original.length() - 1.).abs() < 1e-6);
        assert!((rotated.x + original.y).abs() < 1e-6);
        assert!((rotated.y - original.x).abs() < 1e-6);
        assert_eq!(rotated.z, original.z);
    }
    #[test]
    fn shadow_direction_stays_continuous_through_a_full_montreal_day() {
        let mut previous = None;
        for minute in 0..=24 * 60 {
            let settings = SkySettings {
                solar_hour: Some(minute as f32 / 60.),
                ..Default::default()
            };
            let lighting = Lighting::with_settings(0., None, settings);
            assert!(lighting.sun.is_finite());
            assert!((lighting.sun.length() - 1.).abs() < 1e-6);
            assert!(lighting.sun.z > 0.);
            if let Some(previous) = previous {
                assert!(
                    lighting.sun.distance(previous) < 0.01,
                    "shadow direction jumped at minute {minute}"
                );
            }
            assert!(
                lighting
                    .solar
                    .distance(settings.sun_direction(minute as f32 / 60.))
                    < 1e-6
            );
            previous = Some(lighting.sun);
        }
    }
    #[test]
    fn explicit_solar_hour_overrides_cycle_and_brackets() {
        let settings = SkySettings {
            solar_hour: Some(19.),
            ..Default::default()
        };
        assert_eq!(settings.hour(0., None), 19.);
        assert_eq!(settings.hour(500., Some(1.)), 19.);
        let settings = SkySettings::default();
        assert_eq!(settings.hour(0., None), 12.);
        assert_eq!(settings.hour(DAY_SECONDS, None), 12.);
        assert_eq!(settings.hour(0., Some(0.)), 24.);
    }
}
