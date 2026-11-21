//! Local encounters are rolled on arrival and as the player travels. Animals keep
//! their existing homes and reactions until the player has passed out of range.
use super::{CatCoat, City, Habitats, Home, Species};
use glam::{Vec2, Vec3};
use std::f32::consts::TAU;

struct Encounter {
    limit: usize,
    smallest: usize,
    largest: usize,
    chance: f32,
    spread: f32,
}

impl Species {
    fn encounter(self) -> Encounter {
        // Island-wide live cap, size of one encounter, chance per travel sample,
        // and distance in metres between the flock centre and its outer members.
        let (limit, smallest, largest, chance, spread) = match self {
            Self::Gull => (7, 4, 7, 1.0, 4.5),
            Self::Pigeon => (6, 2, 6, 0.72, 3.8),
            Self::Mallard => (3, 2, 3, 0.68, 2.4),
            Self::Goose => (8, 1, 8, 0.40, 5.0),
            Self::Heron => (1, 1, 1, 0.025, 0.0),
            Self::Squirrel => (3, 1, 1, 0.55, 0.0),
            Self::Chickadee => (2, 1, 2, 0.34, 2.5),
            Self::Crow => (2, 1, 1, 0.22, 0.0),
            Self::Fox => (1, 1, 1, 0.24, 0.0),
            Self::Skunk => (1, 1, 1, 0.09, 0.0),
            Self::Cat => (3, 1, 3, 0.46, 12.0),
        };
        Encounter {
            limit,
            smallest,
            largest,
            chance,
            spread,
        }
    }

    fn on_shore(self) -> bool {
        matches!(self, Self::Mallard | Self::Goose | Self::Heron)
    }
}

pub(in crate::dynamic_world) struct Population {
    seed: u64,
    last_eye: Option<Vec2>,
    scan_delay: f32,
    night: bool,
}

impl Default for Population {
    fn default() -> Self {
        let seed = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map_or(1, |time| time.as_nanos() as u64);
        Self::with_seed(seed)
    }
}

impl Population {
    fn with_seed(seed: u64) -> Self {
        Self {
            seed,
            last_eye: None,
            scan_delay: 0.,
            night: false,
        }
    }

    fn roll(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        (self.seed >> 40) as f32 / 16_777_216.
    }

    pub(in crate::dynamic_world) fn step(
        &mut self,
        homes: &mut Vec<Home>,
        habitats: &Habitats,
        city: &City,
        eye: Vec2,
        night: bool,
        dt: f32,
        blocked: impl Fn(Vec3) -> bool,
    ) {
        let dusk = night && !self.night;
        if self.night && !night {
            homes.retain(|home| home.species != Species::Skunk);
        }
        self.night = night;
        self.scan_delay -= dt;
        let travel = self.last_eye.map(|prev| eye.distance(prev));
        let arrival = travel.is_none_or(|metres| metres > 180.);
        let exploring =
            arrival || (travel.is_some_and(|metres| metres >= 24.) && self.scan_delay <= 0.);
        if !exploring && !dusk {
            return;
        }
        if exploring {
            self.last_eye = Some(eye);
            self.scan_delay = 0.5;
            // A hit animal occupies its population slot during the existing
            // recovery timer, including when the player leaves the area.
            homes.retain(|home| {
                !home.is_active()
                    || home.ground[0].truncate().distance_squared(eye)
                        < (home.species.view_distance() + 48.).powi(2)
            });
        }
        let nearby_shore = exploring
            && habitats
                .nearest_shore(eye)
                .is_some_and(|shore| shore.distance_squared(eye) < 123.0_f32.powi(2));
        for species in Species::ALL {
            if (!exploring && species != Species::Skunk)
                || (species == Species::Skunk && !night)
                || (species.on_shore() && !nearby_shore)
            {
                continue;
            }
            let rule = species.encounter();
            let count = homes.iter().filter(|home| home.species == species).count();
            // Keep a flock together for the encounter's lifetime. Solitary
            // animals can occupy independent ranges up to their species cap.
            if count >= rule.limit || (rule.largest > 1 && count > 0) || self.roll() >= rule.chance
            {
                continue;
            }
            self.place_encounter(
                homes, habitats, city, eye, arrival, species, &rule, &blocked,
            );
        }
    }

    fn place_encounter(
        &mut self,
        homes: &mut Vec<Home>,
        habitats: &Habitats,
        city: &City,
        eye: Vec2,
        arrival: bool,
        species: Species,
        rule: &Encounter,
        blocked: &impl Fn(Vec3) -> bool,
    ) {
        let view = species.view_distance();
        let min_distance = if arrival { 6. } else { view + 6. };
        let outer_distance = match (arrival, species) {
            (true, Species::Gull) => 30.,
            (true, _) => view,
            (false, _) => view + 38.,
        };
        // One roll for the whole search. Retrying a blocked point must not make
        // a flock on an outlying sidewalk as likely as a downtown flock.
        let outer_city_pigeons = species == Species::Pigeon && self.roll() < 1. / 12.;
        let start_angle = self.roll() * TAU;
        let rotation = 2.3999631; // golden angle, to cover different bearings
        let mut remaining_places = 48;
        for attempt in 0..16 {
            let angle = start_angle + rotation * attempt as f32;
            let radius = min_distance + (outer_distance - min_distance) * self.roll();
            let probe = eye + Vec2::from_angle(angle) * radius;
            let center = if species.on_shore() {
                let Some(bank) = habitats.nearest_shore(probe) else {
                    continue;
                };
                bank
            } else {
                probe
            };
            if !(min_distance..=outer_distance + 3.).contains(&center.distance(eye)) {
                continue;
            }
            let habitat = habitats.at(city, center);
            if !habitat.permits(species)
                || (species == Species::Pigeon && !habitat.downtown && !outer_city_pigeons)
                || homes.iter().any(|home| {
                    home.species == species
                        && home.ground[0].truncate().distance_squared(center) < 16.0_f32.powi(2)
                })
            {
                continue;
            }
            let target =
                rule.smallest + (self.roll() * (rule.largest + 1 - rule.smallest) as f32) as usize;
            let mut flock = Vec::with_capacity(target);
            let attempts = if rule.spread == 0. { 1 } else { target * 5 };
            for member in 0..attempts {
                if remaining_places == 0 {
                    break;
                }
                remaining_places -= 1;
                let point = if member == 0 {
                    center
                } else {
                    center
                        + Vec2::from_angle(self.roll() * TAU) * (self.roll().sqrt() * rule.spread)
                };
                if point.distance_squared(eye) < min_distance.powi(2)
                    || (species == Species::Cat
                        && arrival
                        && point.distance(eye) + species.roam_radius() > view)
                    || homes.iter().chain(&flock).any(|home| {
                        if species == Species::Cat && home.species == Species::Cat {
                            let center = (home.ground[0] + home.ground[8]).truncate() * 0.5;
                            return center.distance_squared(point) < 5.0_f32.powi(2);
                        }
                        home.ground[0].truncate().distance_squared(point)
                            < (species.roam_radius() + home.species.roam_radius() + 0.8).powi(2)
                    })
                    || (member != 0 && !habitats.permits(city, species, point))
                {
                    continue;
                }
                let coat =
                    (species == Species::Cat).then(|| CatCoat::ALL[(self.roll() * 2.) as usize]);
                let Some(home) = Home::at(species, point, self.roll() * 24., city, coat) else {
                    continue;
                };
                if !arrival
                    && home
                        .flight
                        .as_ref()
                        .is_some_and(|flight| flight.enters_range(eye, view + 2.))
                {
                    continue;
                }
                // Check the actual orbit at its four extremes as well as its
                // centre. A walking animal at the edge must still fit its habitat.
                let stride = if species == Species::Cat {
                    1 // Yards and sidewalks can border a traffic lane between the extrema.
                } else if species.roam_radius() == 0. {
                    16
                } else {
                    4
                };
                if home.ground.iter().step_by(stride).any(|&p| {
                    blocked(p)
                        || (species.roam_radius() > 0.
                            && !habitats.permits(city, species, p.truncate()))
                }) {
                    continue;
                }
                flock.push(home);
                if flock.len() == target {
                    break;
                }
            }
            if flock.len() >= rule.smallest {
                homes.extend(flock);
                return;
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::dynamic_world::METERS_PER_UNIT;

    #[test]
    fn montreal_arrivals_vary_within_their_habitats_and_flock_sizes() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl");
        let program = opencity::parse(include_str!("../../../data/maps/mtl/map.road")).unwrap();
        let city = City::from_program(&program, Some(&path)).unwrap();
        let habitats = Habitats::read(&program, &city);
        let start = crate::dynamic_world::SpawnPoint::from_program(&program)
            .unwrap()
            .unwrap();
        let player_start = Vec2::from_array(start.point) * METERS_PER_UNIT;
        let mut visits_with_flight = 0;
        for seed in 0..48 {
            let mut homes = Vec::new();
            Population::with_seed(seed).step(
                &mut homes,
                &habitats,
                &city,
                player_start,
                false,
                0.,
                |_| false,
            );
            if (0..24).any(|second| {
                homes.iter().any(|home| {
                    let bird = home.sighting(second as f32);
                    bird.clip == super::super::flight::FLIGHT_CLIP
                        && bird.position.z > home.ground[0].z + 1.5
                        && bird.position.truncate().distance(player_start)
                            < bird.species.view_distance()
                })
            }) {
                visits_with_flight += 1;
            }
        }
        assert!(
            visits_with_flight >= 36,
            "only {visits_with_flight}/48 player starts had visible flight"
        );
        let row = city
            .landscape
            .iter()
            .find(|feature| feature.name.as_deref() == Some("courtyard_street_08"))
            .unwrap();
        let front = row.front_edge.unwrap();
        let doorstep = (Vec2::from_array(row.points[front])
            + Vec2::from_array(row.points[(front + 1) % row.points.len()]))
            * (0.5 * METERS_PER_UNIT);
        let mut cat_counts = [[0usize; 4]; 2];
        let mut coats_seen = std::collections::HashSet::new();
        let lot = city
            .landscape
            .iter()
            .find(|feature| {
                feature.name.as_deref() == Some("sphere_parking")
                    && feature.kind == opencity::LandscapeKind::Concrete
            })
            .unwrap();
        let parking = lot
            .points
            .iter()
            .copied()
            .map(Vec2::from_array)
            .sum::<Vec2>()
            * (METERS_PER_UNIT / lot.points.len() as f32);
        let coast = habitats.nearest_shore(parking).unwrap();
        let mut gull_flocks = Vec::new();
        let mut duck_flocks = 0;
        let mut goose_flocks = 0;
        let mut herons = 0;
        for seed in 0..48 {
            let mut population = Population::with_seed(seed);
            let mut homes = Vec::new();
            let night = seed % 2 == 1;
            population.step(&mut homes, &habitats, &city, doorstep, night, 0., |_| false);
            let cats: Vec<_> = homes.iter().filter(|h| h.species == Species::Cat).collect();
            assert!(cats.len() <= 3);
            cat_counts[usize::from(night)][cats.len()] += 1;
            for (i, cat) in cats.iter().enumerate() {
                coats_seen.insert(cat.sighting(0.).cat_coat.unwrap());
                assert!(cat.ground.iter().all(|p| {
                    habitats.permits(&city, Species::Cat, p.truncate())
                        && p.truncate().distance(doorstep) <= Species::Cat.view_distance()
                }));
                assert!(cats[..i].iter().all(|other| {
                    cat.ground[0]
                        .truncate()
                        .distance(other.ground[0].truncate())
                        >= 4.99
                }));
            }
            homes.clear();
            population = Population::with_seed(seed);
            population.step(&mut homes, &habitats, &city, parking, false, 0., |_| false);
            let gulls: Vec<_> = homes
                .iter()
                .filter(|home| home.species == Species::Gull)
                .collect();
            assert!(
                (4..=7).contains(&gulls.len()),
                "seed {seed}: {} gulls",
                gulls.len()
            );
            assert!(gulls
                .iter()
                .all(|home| habitats.at(&city, home.ground[0].truncate()).parking));
            gull_flocks.push(gulls[0].ground[0]);

            homes.clear();
            population = Population::with_seed(seed);
            population.step(&mut homes, &habitats, &city, coast, false, 0., |_| false);
            let count = |kind| homes.iter().filter(|home| home.species == kind).count();
            assert!(matches!(count(Species::Mallard), 0 | 2 | 3));
            assert!(count(Species::Goose) <= 8);
            assert!(count(Species::Heron) <= 1);
            for home in homes.iter().filter(|home| home.species.on_shore()) {
                assert!(home
                    .ground
                    .iter()
                    .step_by(4)
                    .all(|p| habitats.at(&city, p.truncate()).shore));
            }
            duck_flocks += usize::from(count(Species::Mallard) != 0);
            goose_flocks += usize::from(count(Species::Goose) != 0);
            herons += count(Species::Heron);
        }
        assert!(CatCoat::ALL.iter().all(|coat| coats_seen.contains(coat)));
        for (size, (day, night)) in cat_counts[0].iter().zip(&cat_counts[1]).enumerate() {
            assert!(
                day + night > 0,
                "no residential visits with {size} cats: {cat_counts:?}"
            );
        }
        assert!(cat_counts
            .iter()
            .all(|counts| counts[1..].iter().sum::<usize>() > 0));
        assert!(gull_flocks
            .windows(2)
            .any(|pair| pair[0].distance(pair[1]) > 5.));
        assert!(
            duck_flocks >= 18,
            "{duck_flocks} duck flocks in 48 coastal visits"
        );
        assert!(
            goose_flocks >= 7,
            "{goose_flocks} goose flocks in 48 coastal visits"
        );
        assert!(herons < goose_flocks / 2);
        // Exercise heron placement directly even when none of the rare rolls hits.
        let mut heron = Vec::new();
        Population::with_seed(721).place_encounter(
            &mut heron,
            &habitats,
            &city,
            coast,
            true,
            Species::Heron,
            &Species::Heron.encounter(),
            &|_| false,
        );
        assert_eq!(heron.len(), 1);

        // A blocked parking lot must never receive a partial flock.
        let mut blocked_parking = Vec::new();
        Population::with_seed(721).step(
            &mut blocked_parking,
            &habitats,
            &city,
            parking,
            false,
            0.,
            |_| true,
        );
        assert!(blocked_parking.is_empty());
    }

    #[test]
    fn travel_preserves_hits_caps_and_daylight_rules() {
        let program = opencity::parse("island=land(-20,-20,20,-20,20,20,-20,20);").unwrap();
        let city = City::from_program(&program, None).unwrap();
        let habitats = Habitats::read(&program, &city);
        let mut population = Population::with_seed(13);
        let mut homes = Vec::new();
        let mut eye = Vec2::new(-1560., 0.);
        let mut saw_skunk = false;
        let mut saw_squirrel = false;
        for _ in 0..120 {
            let previous: Vec<_> = homes.iter().map(|h: &Home| h.ground[0]).collect();
            population.step(&mut homes, &habitats, &city, eye, true, 1., |_| false);
            assert!(
                homes.iter().all(|home| home.species != Species::Cat),
                "uninhabited island"
            );
            for home in &homes {
                if !previous.is_empty() && !previous.contains(&home.ground[0]) {
                    assert!(home.ground[0].truncate().distance(eye) > home.species.view_distance());
                }
            }
            for species in Species::ALL {
                assert!(
                    homes.iter().filter(|h| h.species == species).count()
                        <= species.encounter().limit
                );
            }
            saw_skunk |= homes.iter().any(|home| home.species == Species::Skunk);
            saw_squirrel |= homes.iter().any(|home| home.species == Species::Squirrel);
            eye.x += 26.;
        }
        assert!(saw_squirrel && saw_skunk);

        homes.clear();
        let mut skunk = Home::at(Species::Skunk, eye, 0., &city, None).unwrap();
        skunk.knock_over(0.);
        homes.push(skunk);
        let mut squirrel =
            Home::at(Species::Squirrel, eye + Vec2::X * 5., 0., &city, None).unwrap();
        squirrel.knock_over(0.);
        let down = squirrel.ground[0];
        homes.push(squirrel);
        for _ in 0..100 {
            population.step(&mut homes, &habitats, &city, eye, false, 1., |_| false);
            assert!(homes.iter().all(|home| home.species != Species::Skunk));
            assert!(homes
                .iter()
                .any(|home| home.ground[0] == down && home.is_downed()));
        }
        eye.y += 500.;
        population.step(&mut homes, &habitats, &city, eye, false, 1., |_| true);
        assert_eq!(homes.len(), 1);
        assert!(homes[0].is_downed());
    }
}
