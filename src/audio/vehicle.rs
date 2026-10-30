//! Authored per-vehicle sound; independent of handling and physical mass.
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum EngineKind {
    Petrol,
    Diesel,
    Electric,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum HornKind {
    Dual,
    Air,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HornSound {
    pub kind: HornKind,
    pub frequencies_hz: [f32; 2],
    pub gain: f32,
}
impl Default for HornSound {
    fn default() -> Self {
        Self {
            kind: HornKind::Dual,
            frequencies_hz: [380., 470.],
            gain: 0.65,
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VehicleSound {
    pub engine: EngineKind,
    pub idle_rpm: f32,
    pub redline_rpm: f32,
    pub engine_gain: f32,
    pub firing_order: f32,
    pub exhaust_gain: f32,
    pub body_gain: f32,
    pub roughness: f32,
    pub muffler_hz: f32,
    pub machinery_gain: f32,
    pub gear_ratios: [f32; 6],
    pub final_drive: f32,
    pub horn: HornSound,
}
impl Default for VehicleSound {
    fn default() -> Self {
        Self {
            engine: EngineKind::Petrol,
            idle_rpm: 780.,
            redline_rpm: 4400.,
            engine_gain: 1.6,
            firing_order: 2.,
            exhaust_gain: 0.72,
            body_gain: 0.23,
            roughness: 0.2,
            muffler_hz: 600.,
            machinery_gain: 0.,
            gear_ratios: [3.42, 2.12, 1.45, 1.03, 0.84, 0.69],
            final_drive: 3.65,
            horn: HornSound::default(),
        }
    }
}
impl VehicleSound {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let in_range = |name: &str, value: f32, low: f32, high: f32| {
            if value.is_finite() && (low..=high).contains(&value) {
                Ok(())
            } else {
                Err(format!("sound.{name} must be between {low} and {high}"))
            }
        };
        for (name, value, low, high) in [
            ("idleRpm", self.idle_rpm, 100., 2000.),
            ("redlineRpm", self.redline_rpm, 500., 10000.),
            ("engineGain", self.engine_gain, 0., 4.),
            ("firingOrder", self.firing_order, 1., 6.),
            ("exhaustGain", self.exhaust_gain, 0., 2.),
            ("bodyGain", self.body_gain, 0., 1.),
            ("roughness", self.roughness, 0., 1.),
            ("mufflerHz", self.muffler_hz, 100., 3000.),
            ("machineryGain", self.machinery_gain, 0., 1.),
            ("finalDrive", self.final_drive, 1., 10.),
            ("horn.gain", self.horn.gain, 0., 2.),
        ] {
            in_range(name, value, low, high)?;
        }
        if self.redline_rpm <= self.idle_rpm {
            return Err("sound.redlineRpm must exceed idleRpm".into());
        }
        for value in self.gear_ratios {
            in_range("gearRatios", value, 0.1, 8.)?;
        }
        if self.gear_ratios.windows(2).any(|pair| pair[0] <= pair[1]) {
            return Err("sound.gearRatios must contain six decreasing ratios".into());
        }
        for value in self.horn.frequencies_hz {
            in_range("horn.frequenciesHz", value, 80., 1200.)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_sound_rejects_invalid_audio_parameters_and_unknown_fields() {
        for bad in [
            r#"{"mufflerHz":0}"#,
            r#"{"idleRpm":900,"redlineRpm":800}"#,
            r#"{"horn":{"frequenciesHz":[0,440]}}"#,
            r#"{"gearRatios":[1,2,3,4,5,6]}"#,
        ] {
            assert!(serde_json::from_str::<VehicleSound>(bad)
                .unwrap()
                .validate()
                .is_err());
        }
        assert!(serde_json::from_str::<VehicleSound>(r#"{"unknownSetting":1}"#).is_err());
        assert!(serde_json::from_str::<VehicleSound>(r#"{"engine":"unknown"}"#).is_err());
        let mut sound = VehicleSound::default();
        sound.engine_gain = f32::NAN;
        assert!(sound.validate().is_err());
        let fallback: VehicleSound = serde_json::from_str("{}").unwrap();
        fallback.validate().unwrap();
    }
}
