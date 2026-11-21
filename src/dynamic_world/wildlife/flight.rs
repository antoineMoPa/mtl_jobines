//! A low circuit begins and ends on the same patch of ground. Check the route
//! once when placing the bird so sampling a visible flock needs no city queries.
use super::{City, Species, METERS_PER_UNIT};
use glam::{Vec2, Vec3};
use std::f32::consts::{FRAC_PI_2, PI, TAU};

pub(super) const CYCLE: f32 = 24.;
const AIRBORNE: f32 = 15.;
const SEGMENTS: usize = 24;
pub(super) const FLIGHT_CLIP: usize = 2;

pub(super) fn surface_height(city: &City, xy: Vec2) -> Option<f32> {
    let surface = city.surface_at((xy / METERS_PER_UNIT).into(), f32::MAX)?;
    let height = if surface.kind == opencity::runtime::SurfaceKind::Water {
        city.elevation.as_ref()?.spec.water_level?
    } else {
        surface.height
    };
    Some(height * METERS_PER_UNIT)
}

impl Species {
    fn circuit(self) -> Option<(f32, f32)> {
        // Turn radius and peak clearance above the street, in metres.
        match self {
            Self::Gull => Some((13., 6.5)),
            Self::Crow => Some((11., 5.)),
            Self::Pigeon => Some((9., 3.8)),
            Self::Chickadee => Some((5., 2.6)),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub(super) struct Flight {
    points: [Vec3; SEGMENTS + 1],
    bearing: f32,
    offset: f32,
}

impl Flight {
    pub(super) fn plan(species: Species, foot: Vec3, phase: f32, city: &City) -> Option<Self> {
        let (radius, height) = species.circuit()?;
        // Try the other side of the perch if the first circuit meets a building.
        [0., PI, FRAC_PI_2, -FRAC_PI_2]
            .into_iter()
            .find_map(|turn| {
                let bearing = phase + turn;
                let radial = Vec2::from_angle(bearing);
                let mut points = [foot; SEGMENTS + 1];
                let mut last_floor = foot.z;
                for i in 1..=SEGMENTS {
                    let f = i as f32 / SEGMENTS as f32;
                    let xy =
                        foot.truncate() + (Vec2::from_angle(bearing + TAU * f) - radial) * radius;
                    let floor = surface_height(city, xy)?;
                    let lift = (PI * f).sin().max(0.);
                    if i < SEGMENTS {
                        points[i] = xy.extend(floor + height * lift);
                    }
                    let padding = Vec3::splat(0.35);
                    let lo = (points[i - 1].min(points[i]) - padding) / METERS_PER_UNIT;
                    let hi = (points[i - 1].max(points[i]) + padding) / METERS_PER_UNIT;
                    if (floor - last_floor).abs() > 1.
                        || city.building_bounds_overlap(lo.into(), hi.into())
                    {
                        return None;
                    }
                    last_floor = floor;
                }
                Some(Self {
                    points,
                    bearing,
                    offset: phase,
                })
            })
    }

    pub(super) fn enters_range(&self, eye: Vec2, range: f32) -> bool {
        self.points
            .iter()
            .any(|p| p.truncate().distance_squared(eye) < range * range)
    }

    fn stage(&self, time: f32) -> f32 {
        (time + self.offset).rem_euclid(CYCLE)
    }

    pub(super) fn start_perched(&mut self) {
        self.offset = AIRBORNE + self.offset.rem_euclid(CYCLE - AIRBORNE);
    }

    pub(super) fn launch(&mut self, time: f32) {
        if self.stage(time) >= AIRBORNE {
            self.offset = -time;
        }
    }

    pub(super) fn pose(&self, time: f32, sighting: &mut super::Sighting) {
        let stage = self.stage(time);
        let progress = (stage / AIRBORNE).min(1.);
        let path = progress * SEGMENTS as f32;
        let i = (path as usize).min(SEGMENTS - 1);
        sighting.position = self.points[i].lerp(self.points[i + 1], path - i as f32);
        sighting.heading = self.bearing + FRAC_PI_2 + TAU * progress;
        if stage < AIRBORNE {
            sighting.roll = -0.18 * (PI * progress).sin();
            sighting.clip = FLIGHT_CLIP;
            sighting.seconds = stage;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::wildlife::Home;

    #[test]
    fn approaching_birds_take_off_and_land_on_the_same_perch_across_clock_wrap() {
        for bird in [
            Species::Gull,
            Species::Crow,
            Species::Pigeon,
            Species::Chickadee,
        ] {
            let program = opencity::parse(&format!(
                "land(-1,-1,1,-1,1,1,-1,1); wildlife_{}_park=map_point(0,0);",
                bird.slug()
            ))
            .unwrap();
            let city = City::from_program(&program, None).unwrap();
            let mut home = Home::read(&program, &city).unwrap().pop().unwrap();
            home.flight.as_mut().unwrap().offset = 0.;
            let foot = home.ground[0];
            let start = CYCLE - 1.;
            assert_ne!(home.sighting(start).clip, FLIGHT_CLIP);
            home.advance(&city, start, 0.1, foot + Vec3::Z);
            let launch = home.sighting(start);
            assert_eq!(launch.clip, FLIGHT_CLIP);
            assert!(launch.position.abs_diff_eq(foot, 0.001));
            let cruise = home.sighting((start + AIRBORNE * 0.5) % CYCLE);
            assert_eq!(cruise.clip, FLIGHT_CLIP);
            assert!(cruise.position.z - foot.z > 2.5);
            assert!(cruise.position.truncate().distance(foot.truncate()) > 9.);
            let landed = home.sighting((start + AIRBORNE) % CYCLE);
            assert!(landed.position.abs_diff_eq(foot, 0.001));
            assert_ne!(landed.clip, FLIGHT_CLIP);
        }
    }
}
