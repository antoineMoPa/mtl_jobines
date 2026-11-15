//! Taxi stops sampled from the current road and sidewalk network.
use super::Hermes;
use crate::dynamic_world::METERS_PER_UNIT;
use glam::Vec3;

#[derive(Clone, Copy)]
pub(super) struct TaxiStop {
    pub road: Vec3,
    pub sidewalk: Vec3,
}

#[derive(Clone, Copy)]
pub(super) struct TaxiRun {
    pub pickup: TaxiStop,
    pub dropoff: TaxiStop,
}

impl Hermes {
    fn random_taxi_stop(&mut self) -> Option<TaxiStop> {
        let paths = &self.walking_paths.paths;
        if paths.is_empty() {
            return None;
        }
        for _ in 0..32 {
            self.taxi_seed ^= self.taxi_seed << 13;
            self.taxi_seed ^= self.taxi_seed >> 7;
            self.taxi_seed ^= self.taxi_seed << 17;
            let path = &paths[self.taxi_seed as usize % paths.len()];
            if path.connector || path.length < 4. {
                continue;
            }
            let t = ((self.taxi_seed >> 32) as f32 / u32::MAX as f32).clamp(0.1, 0.9);
            let sidewalk = path.sample(path.length * t).0;
            let near = sidewalk / METERS_PER_UNIT;
            if let Some(road) = self.router.road_point(near) {
                return Some(TaxiStop { road, sidewalk });
            }
        }
        None
    }

    pub(crate) fn choose_taxi_run(&mut self, player: Vec3) -> bool {
        let previous = self.taxi_run;
        let mut pickup = None;
        for _ in 0..48 {
            let Some(stop) = self.random_taxi_stop() else {
                continue;
            };
            if previous.is_some_and(|run| stop.road.distance(run.dropoff.road) < 0.5)
                || !self.router.can_route(player, stop.road)
            {
                continue;
            }
            pickup = Some(stop);
            break;
        }
        let Some(pickup) = pickup else {
            return false;
        };
        for _ in 0..64 {
            let Some(dropoff) = self.random_taxi_stop() else {
                continue;
            };
            if dropoff.road.distance(pickup.road) < 0.8
                || !self.router.can_route(pickup.road, dropoff.road)
            {
                continue;
            }
            self.taxi_run = Some(TaxiRun { pickup, dropoff });
            return true;
        }
        false
    }

    pub(crate) fn clear_taxi_run(&mut self) {
        self.taxi_run = None;
    }
}
