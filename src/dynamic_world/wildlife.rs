//! Habitat encounters and ground-following routes. Simulation positions use metres.
mod contact;
mod habitat;
mod population;
pub(crate) use habitat::Habitats;
pub(super) use population::Population;
mod response;
use super::METERS_PER_UNIT;
use glam::{Vec2, Vec3};
use opencity::{runtime::City, Argument, Program, Statement};
use response::Reaction;
use std::f32::consts::TAU;

macro_rules! species_catalog {
    ($( $kind:ident => ($slug:literal, $radius:literal, $clips:expr) ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Species { $($kind),+ }
        impl Species {
            pub const ALL: [Self; 11] = [$(Self::$kind),+];
            pub fn slug(self) -> &'static str { match self { $(Self::$kind => $slug),+ } }
            pub fn clips(self) -> [&'static str; 2] { match self { $(Self::$kind => $clips),+ } }
            fn roam_radius(self) -> f32 { match self { $(Self::$kind => $radius),+ } }
            pub fn model(self) -> &'static [u8] {
                match self { $(Self::$kind => crate::web_asset!(
                    concat!("../../data/wildlife/", $slug, ".glb"),
                    concat!("data/wildlife/", $slug, ".glb")
                )),+ }
            }
        }
    }
}
species_catalog! {
    Squirrel => ("squirrel", 0.65, ["bound", "forage"]),
    Pigeon => ("pigeon", 0.30, ["HeadBob", "GroundPeck"]),
    Crow => ("crow", 0.0, ["LookAround", "Caw"]),
    Gull => ("gull", 0.0, ["Survey", "WingStretch"]),
    Mallard => ("mallard", 0.55, ["Waddle", "Dabble"]),
    Goose => ("goose", 0.90, ["Walk", "Graze"]),
    Heron => ("heron", 0.35, ["Stalk", "Strike"]),
    Chickadee => ("chickadee", 0.0, ["TailFlick", "Hop"]),
    Fox => ("fox", 1.40, ["trot", "listen"]),
    Skunk => ("skunk", 0.60, ["waddle", "alert"]),
    Cat => ("cat", 1.0, ["walk", "groom"]),
}

impl Species {
    fn foot_height(self, city: &City, sample: opencity::runtime::SurfaceSample) -> Option<f32> {
        if sample.kind == opencity::runtime::SurfaceKind::Water {
            let level = city.elevation.as_ref()?.spec.water_level?;
            return match self {
                Self::Mallard | Self::Goose => Some(level * METERS_PER_UNIT),
                Self::Heron if (level - sample.height) * METERS_PER_UNIT <= 0.35 => {
                    Some(sample.height * METERS_PER_UNIT)
                }
                _ => None,
            };
        }
        Some(sample.height * METERS_PER_UNIT)
    }

    fn view_distance(self) -> f32 {
        match self {
            Self::Chickadee => 22.,
            Self::Pigeon | Self::Squirrel => 38.,
            Self::Cat | Self::Skunk | Self::Gull | Self::Crow | Self::Mallard => 52.,
            Self::Fox | Self::Heron | Self::Goose => 80.,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Home {
    species: Species,
    ground: [Vec3; 16],
    phase: f32,
    reaction: Reaction,
}
#[derive(Clone, Copy)]
pub struct Sighting {
    pub species: Species,
    pub position: Vec3,
    pub heading: f32,
    pub roll: f32,
    pub clip: usize,
    /// Time since the current behaviour began.
    pub seconds: f32,
}
impl Home {
    pub(crate) fn read(program: &Program, city: &City) -> Result<Vec<Self>, String> {
        let mut homes = Vec::new();
        for statement in &program.statements {
            let Statement::Call(call) = statement else {
                continue;
            };
            let [name] = call.assigns.as_slice() else {
                continue;
            };
            let Some(key) = name.strip_prefix("wildlife_") else {
                continue;
            };
            let (slug, _) = key
                .split_once('_')
                .ok_or_else(|| format!("expected wildlife_SPECIES_LOCATION in {name}"))?;
            let species = Species::ALL
                .into_iter()
                .find(|s| s.slug() == slug)
                .ok_or_else(|| format!("unknown wildlife species in {name}"))?;
            let [Argument::Number(x), Argument::Number(y)] = call.args.as_slice() else {
                return Err(format!("{name} needs a map_point(x,y)"));
            };
            if call.name != "map_point" {
                return Err(format!("{name} needs a map_point(x,y)"));
            }
            let center = Vec2::new(
                x.parse().map_err(|_| name.clone())?,
                y.parse().map_err(|_| name.clone())?,
            ) * METERS_PER_UNIT;
            let seed = name.bytes().fold(2166136261u32, |h, b| {
                (h ^ u32::from(b)).wrapping_mul(16777619)
            });
            let phase = (seed % 4096) as f32 / 97.;
            homes.push(
                Self::at(species, center, phase, city)
                    .ok_or_else(|| format!("{name} has no clear ground"))?,
            );
        }
        Ok(homes)
    }
    fn at(species: Species, center: Vec2, phase: f32, city: &City) -> Option<Self> {
        let mut ground = [Vec3::ZERO; 16];
        let body = species.body_radii();
        let width = body.x.max(body.y);
        let foot = Vec3::new(width, width, 0.);
        let mut first_height = None;
        let samples = if species.roam_radius() == 0. {
            1
        } else {
            ground.len()
        };
        for (i, p) in ground[..samples].iter_mut().enumerate() {
            let xy = center + Vec2::from_angle(TAU * i as f32 / 16.) * species.roam_radius();
            let sample = city.surface_at((xy / METERS_PER_UNIT).to_array(), f32::MAX)?;
            let height = species.foot_height(city, sample)?;
            let position = xy.extend(height);
            let low = (position - foot + Vec3::Z * 0.02) / METERS_PER_UNIT;
            let high = (position + foot + Vec3::Z * (body.z * 2.)) / METERS_PER_UNIT;
            if (height - *first_height.get_or_insert(height)).abs() > 0.5
                || city.building_bounds_overlap(low.into(), high.into())
            {
                return None;
            }
            *p = position;
        }
        if samples == 1 {
            ground = [ground[0]; 16];
        }
        Some(Self {
            species,
            ground,
            phase,
            reaction: Reaction::Roaming,
        })
    }

    fn roaming(&self, time: f32) -> Sighting {
        let t = time + self.phase;
        let stage = t.rem_euclid(12.);
        let moving = stage < 5.;
        let progress = ((t / 12.).floor() + stage.min(5.) / 5.) * 0.5;
        let path = progress.fract() * 16.;
        let index = path as usize;
        let a = self.ground[index];
        let b = self.ground[(index + 1) % 16];
        Sighting {
            species: self.species,
            position: a.lerp(b, path.fract()),
            roll: 0.,
            heading: if self.species.roam_radius() > 0. {
                (progress * TAU + TAU / 4.).rem_euclid(TAU)
            } else {
                self.phase
            },
            clip: usize::from(!moving),
            seconds: if moving { stage } else { stage - 5. },
        }
    }
}

impl super::DynamicWorld {
    pub fn set_wildlife_night(&mut self, night: bool) {
        self.wildlife_night = night;
    }

    pub(super) fn update_wildlife_population(&mut self, hermes: &crate::hermes::Hermes, dt: f32) {
        if hermes.wildlife.is_empty() {
            let eye = self.player_position().truncate();
            self.wildlife_population.step(
                &mut self.wildlife_homes,
                &hermes.wildlife_habitats,
                hermes.city(),
                eye,
                self.wildlife_night,
                dt,
                |point| {
                    self.props.blocked(point + Vec3::Z * 0.4)
                        || self
                            .vehicles
                            .iter()
                            .any(|car| super::player::blocked_by_vehicle(point, car))
                },
            );
        }
    }

    pub fn wildlife(&self) -> impl Iterator<Item = Sighting> + '_ {
        let eye = self.player_position();
        self.wildlife_homes
            .iter()
            .filter(move |h| h.ground[0].distance_squared(eye) < h.species.view_distance().powi(2))
            .map(|h| h.sighting(self.wildlife_time))
    }
}
