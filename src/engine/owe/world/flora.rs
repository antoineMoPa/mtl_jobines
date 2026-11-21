//! Reusable flora prefabs backed by the same canopy models as the city.
use super::SCALE;
use crate::props::Kind;
use serde::Deserialize;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Canopy {
    BroadMaple,
    UprightMaple,
    YoungMaple,
}
#[derive(Deserialize)]
pub(super) struct Definition {
    canopy: Canopy,
    height_metres: f32,
}
pub(super) struct Prefab {
    pub kind: Kind,
    pub scale: f32,
}
impl Definition {
    pub(super) fn from_sdk(values: &[f32]) -> Result<Self, String> {
        let canopy = match values[0] {
            0. => Canopy::BroadMaple,
            1. => Canopy::UprightMaple,
            2. => Canopy::YoungMaple,
            _ => return Err("unknown flora canopy".into()),
        };
        let definition = Self {
            canopy,
            height_metres: values[1],
        };
        definition.validate()?;
        Ok(definition)
    }
    fn validate(&self) -> Result<(), String> {
        if self.height_metres.is_finite() && self.height_metres > 0. {
            Ok(())
        } else {
            Err("flora height must be positive and finite".into())
        }
    }
    pub(super) fn prepare(&self) -> Result<Prefab, String> {
        self.validate()?;
        let kind = match self.canopy {
            Canopy::BroadMaple => Kind::Tree,
            Canopy::UprightMaple => Kind::TreeLinden,
            Canopy::YoungMaple => Kind::TreeLocust,
        };
        // The authored origin is the trunk's ground line. Grass and roots extend below it.
        let top = kind
            .far_parts()
            .iter()
            .map(|part| crate::hermes::external_models::bounds(part).1.z)
            .fold(0., f32::max);
        Ok(Prefab {
            kind,
            scale: self.height_metres / (top * SCALE),
        })
    }
}
