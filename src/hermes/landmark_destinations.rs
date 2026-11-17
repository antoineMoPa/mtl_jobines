//! Palette destinations follow placed map features, never a second coordinate list.
use super::Hermes;

#[derive(Clone, Debug)]
pub struct Landmark {
    pub id: String,
    pub title: &'static str,
    pub point: [f32; 2],
    pub target_height: f32,
    pub radius: f32,
}

pub fn title(model: &str) -> Option<&'static str> {
    Some(match model {
        "study_01" => "Orange sphere",
        "study_02" => "Stepped downtown skyscraper",
        "study_03" => "Stadium and inclined tower",
        "study_04" => "White office tower",
        "study_05" => "Hillside domed basilica",
        "study_06" => "Twin-tower basilica",
        "study_07" => "Stacked riverside housing",
        "study_08" => "Geodesic dome",
        "study_09" => "Old city hall",
        "study_10" => "Old port market dome",
        "study_11" => "Historic art museum",
        "study_12" => "Domed downtown cathedral",
        "study_13" => "Historic downtown office block",
        "study_14" => "Square financial tower",
        "study_17" => "Central railway station",
        "study_18" => "Canal market",
        "study_19" => "Port clock tower",
        "study_20" => "Old town chateau",
        "study_21" => "Western hospital campus",
        "study_22" => "Downtown hospital campus",
        "study_23" => "Brewery clock building",
        "study_24" => "Port grain silos",
        "study_25" => "Flour mill rooftop sign",
        "study_26" => "Sloped-roof skyscraper",
        "study_27" => "Square residential skyscraper",
        "study_28" => "Twin towers and skybridge",
        "study_29" => "Western downtown office tower",
        "study_30" => "Park-side skyscraper",
        _ => return None,
    })
}

impl Hermes {
    pub fn landmarks(&self) -> Vec<Landmark> {
        self.city
            .landscape
            .iter()
            .filter_map(|feature| {
                let title = title(feature.model.as_deref()?)?;
                let id = feature.name.clone()?;
                if feature.points.is_empty() {
                    return None;
                }
                let sum = feature
                    .points
                    .iter()
                    .fold([0.; 2], |a, p| [a[0] + p[0], a[1] + p[1]]);
                let count = feature.points.len() as f32;
                Some(Landmark {
                    id,
                    title,
                    point: [sum[0] / count, sum[1] / count],
                    target_height: feature.elevation + feature.height.unwrap_or(0.) * 0.75,
                    radius: feature
                        .points
                        .iter()
                        .map(|p| (p[0] - sum[0] / count).hypot(p[1] - sum[1] / count))
                        .fold(0., f32::max),
                })
            })
            .collect()
    }
}
