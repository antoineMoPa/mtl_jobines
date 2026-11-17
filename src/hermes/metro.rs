//! Underground transit uses authored .road curves and docks, shared by scenery and simulation.
mod scenery;
use glam::{Vec2, Vec3};
use opencity::runtime::City;
use std::fmt::Write;
pub const TRACK_HEIGHT: f32 = -0.035;
pub const FLOOR_HEIGHT: f32 = -0.02475;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StationId {
    Acadie,
    JeanTalon,
    Bonaventure,
}
impl StationId {
    pub const ALL: [Self; 3] = [Self::Acadie, Self::JeanTalon, Self::Bonaventure];
    pub fn key(self) -> &'static str {
        match self {
            Self::Acadie => "acadie",
            Self::JeanTalon => "jean_talon",
            Self::Bonaventure => "bonaventure",
        }
    }
    pub fn label_png(self) -> &'static [u8] {
        match self {
            Self::Acadie => include_bytes!("../../data/metro/acadie.png"),
            Self::JeanTalon => include_bytes!("../../data/metro/jean-talon.png"),
            Self::Bonaventure => include_bytes!("../../data/metro/bonaventure.png"),
        }
    }
    fn label_path(self) -> &'static str {
        match self {
            Self::Acadie => include_str!("../../data/metro/acadie-map.svg"),
            Self::JeanTalon => include_str!("../../data/metro/jean-talon-map.svg"),
            Self::Bonaventure => include_str!("../../data/metro/bonaventure-map.svg"),
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Acadie => "Acadie",
            Self::JeanTalon => "Jean-Talon",
            Self::Bonaventure => "Bonaventure",
        }
    }
}
#[derive(Clone, Debug)]
pub struct Station {
    pub id: StationId,
    pub center: Vec2,
    pub forward: Vec2,
    pub distance: f32,
}
impl Station {
    pub fn point(&self, along: f32, across: f32, height: f32) -> Vec3 {
        (self.center + self.forward * along + Vec2::new(-self.forward.y, self.forward.x) * across)
            .extend(height)
    }
}
#[derive(Clone, Default)]
pub struct MetroMap {
    pub stations: Vec<Station>,
    pub samples: Vec<(f32, Vec3)>,
    pub length: f32,
}
impl MetroMap {
    pub fn from_city(city: &City) -> Self {
        let mut roads: Vec<_> = city
            .roads
            .iter()
            .filter(|r| {
                r.segment
                    .name
                    .as_deref()
                    .is_some_and(|n| n.starts_with("metro_loop_"))
            })
            .collect();
        roads.sort_by(|a, b| a.segment.name.cmp(&b.segment.name));
        let mut map = Self::default();
        for road in roads {
            for i in 0..=128 {
                if i == 0 && !map.samples.is_empty() {
                    continue;
                }
                let p = road.segment.point_at(i as f32 / 128.);
                let point = Vec3::new(p.x, p.y, TRACK_HEIGHT);
                if let Some((_, previous)) = map.samples.last() {
                    map.length += point.distance(*previous) * 100.;
                }
                map.samples.push((map.length, point));
            }
        }
        for id in StationId::ALL {
            let name = format!("metro_dock_{}_0", id.key());
            let Some(road) = city
                .roads
                .iter()
                .find(|r| r.segment.name.as_deref() == Some(&name))
            else {
                continue;
            };
            let p = road.segment.point_at(0.5);
            let t = road.segment.tangent_at(0.5);
            let length = t.x.hypot(t.y);
            let forward = Vec2::new(t.x / length, t.y / length);
            let center = Vec2::new(p.x, p.y) - Vec2::new(-forward.y, forward.x) * 0.03;
            let distance = map
                .samples
                .iter()
                .min_by(|a, b| {
                    a.1.truncate()
                        .distance_squared(center)
                        .total_cmp(&b.1.truncate().distance_squared(center))
                })
                .map_or(0., |s| s.0);
            map.stations.push(Station {
                id,
                center,
                forward,
                distance,
            });
        }
        map
    }
    pub fn sample(&self, distance: f32) -> (Vec3, f32) {
        if self.samples.len() < 2 {
            return (Vec3::ZERO, 0.);
        }
        let d = distance.rem_euclid(self.length);
        let i = self
            .samples
            .partition_point(|s| s.0 <= d)
            .clamp(1, self.samples.len() - 1);
        let (a, p) = self.samples[i - 1];
        let (b, q) = self.samples[i];
        let t = (d - a) / (b - a).max(0.00001);
        let v = q - p;
        (p.lerp(q, t), v.y.atan2(v.x))
    }
    pub fn tunnel_fixtures(&self) -> impl Iterator<Item = Vec3> + '_ {
        (0..(self.length / 12.) as usize)
            .map(|i| self.sample(i as f32 * 12.).0 + Vec3::Z * 0.043)
            .filter(|p| {
                !self
                    .stations
                    .iter()
                    .any(|s| s.center.distance(p.truncate()) < 0.24)
            })
    }
    pub fn scenery(
        &self,
    ) -> Result<Option<super::external_models::instances::ModelInstances>, String> {
        scenery::build(self)
    }
}
/// All map presentations share this overlay, including SVG export and GPU vectors.
pub fn navigation_svg(city: &City) -> Result<String, String> {
    let mut svg = city.minimap_svg()?;
    let metro = MetroMap::from_city(city);
    if metro.samples.is_empty() {
        return Ok(svg);
    }
    let at = svg.rfind("</svg>").ok_or("Invalid navigation SVG")?;
    svg.truncate(at);
    write!(svg,"<g transform=\"translate(0,{}) scale(1,-1)\"><path id=\"metro-line\" fill=\"none\" stroke=\"#2367b5\" stroke-width=\"0.065\" stroke-linejoin=\"round\" d=\"",city.bounds[1]+city.bounds[3]).unwrap();
    for (i, (_, p)) in metro.samples.iter().enumerate() {
        write!(svg, "{} {} {} ", if i == 0 { "M" } else { "L" }, p.x, p.y).unwrap();
    }
    svg.push_str("\"/>");
    for station in &metro.stations {
        let p = station.center;
        write!(svg,"<circle cx=\"{}\" cy=\"{}\" r=\"0.09\" fill=\"white\" stroke=\"#2367b5\" stroke-width=\"0.04\"/><g transform=\"translate({}, {}) scale(0.004,-0.004)\">{}</g><title>{}</title>",p.x,p.y,p.x+0.16,p.y+0.09,station.id.label_path(),station.id.title()).unwrap();
    }
    svg.push_str("</g></svg>");
    Ok(svg)
}
#[cfg(test)]
mod tests;
