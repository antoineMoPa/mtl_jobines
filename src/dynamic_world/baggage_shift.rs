//! Airport service checks use the first trailer axle, in metres, never the tractor nose.
use super::{DynamicWorld, VehicleId, METERS_PER_UNIT};
use crate::{hermes::Hermes, kairos::SiteId};
use glam::Vec3;

pub(super) struct Shift {
    tug: VehicleId,
    aircraft: usize,
    aircraft_position: Vec3,
    aircraft_yaw: f32,
    pub loaded: bool,
    pub invalid: bool,
    settled: f32,
}

impl DynamicWorld {
    pub fn begin_baggage_shift(&mut self) -> Option<VehicleId> {
        if self.baggage_shift_invalid() {
            return None;
        }
        let tug = self.vehicles.iter().position(|v| v.is_baggage_tug())?;
        let aircraft = self.vehicles.iter().position(|v| {
            v.definition()
                .is_some_and(|d| d.config.research_id == "vehicle-air-estuaire")
        })?;
        if self.vehicles[tug].baggage_links.is_empty() {
            return None;
        }
        self.baggage_shift = Some(Shift {
            tug: VehicleId(tug),
            aircraft,
            aircraft_position: self.vehicles[aircraft].position,
            aircraft_yaw: self.vehicles[aircraft].yaw,
            loaded: false,
            invalid: false,
            settled: 0.,
        });
        Some(VehicleId(tug))
    }
    pub fn restart_baggage_shift(&mut self, hermes: &Hermes) -> bool {
        let Some(s) = &self.baggage_shift else {
            return false;
        };
        let tug = s.tug.0;
        let aircraft = s.aircraft;
        let restored = [tug, aircraft].map(|i| {
            let mut v = super::Vehicle::spawn_at(hermes.city(), self.vehicle_spawns[i]).ok()?;
            v.set_catalog_model(self.vehicles[i].definition);
            if i == tug {
                v.load_airport_train();
            }
            Some(v)
        });
        let [Some(tug_vehicle), Some(aircraft_vehicle)] = restored else {
            return false;
        };
        self.vehicles[tug] = tug_vehicle;
        self.vehicles[aircraft] = aircraft_vehicle;
        self.baggage_shift = None;
        self.begin_baggage_shift().is_some()
    }
    pub fn end_baggage_shift(&mut self) {
        self.baggage_shift = None;
    }
    pub(super) fn invalidate_baggage_shift(&mut self) {
        if let Some(s) = &mut self.baggage_shift {
            s.invalid = true;
            s.loaded = false;
            s.settled = 0.;
        }
    }
    pub fn baggage_shift_invalid(&self) -> bool {
        self.baggage_shift.as_ref().is_some_and(|s| s.invalid)
    }
    pub fn baggage_service_ready(&self) -> bool {
        self.baggage_shift
            .as_ref()
            .is_some_and(|s| !s.invalid && s.settled >= 1.)
    }
    pub fn update_baggage_shift(&mut self, hermes: &Hermes, site: Option<SiteId>, elapsed: f32) {
        let Some(s) = &self.baggage_shift else {
            return;
        };
        let aircraft = &self.vehicles[s.aircraft];
        let tug = &self.vehicles[s.tug.0];
        if aircraft.position.distance(s.aircraft_position) > 0.5
            || (aircraft.yaw - s.aircraft_yaw).abs() > 0.02
            || !tug.is_baggage_tug()
            || tug.baggage_links.len() != 4
        {
            self.invalidate_baggage_shift();
            return;
        }
        let aligned = site
            .and_then(|site| hermes.mission_site(site))
            .is_some_and(|point| {
                tug.carts().next().is_some_and(|cart| {
                    let target = glam::Vec2::from_array(point) * METERS_PER_UNIT;
                    cart.axle().truncate().distance(target) <= 2.
                        && (cart.rotation * Vec3::X).dot(Vec3::Y).abs() >= 25_f32.to_radians().cos()
                })
            });
        let valid = self.is_driving()
            && self.player_vehicle == s.tug
            && self.health > 0
            && self.crash.is_none()
            && self.entry.is_none()
            && tug.grounded
            && tug.velocity.length_squared() < 0.25
            && aligned;
        let s = self.baggage_shift.as_mut().unwrap();
        s.settled = if valid && !s.invalid {
            s.settled + elapsed.min(0.1)
        } else {
            0.
        };
    }
    pub fn confirm_baggage_service(&mut self, loaded: bool) -> bool {
        if !self.baggage_service_ready() {
            return false;
        }
        let s = self.baggage_shift.as_mut().unwrap();
        if s.loaded == loaded {
            return false;
        }
        s.loaded = loaded;
        s.settled = 0.;
        true
    }
    pub fn baggage_bay(&self, hermes: &Hermes, site: Option<SiteId>) -> Option<(Vec3, bool)> {
        self.baggage_shift.as_ref()?;
        let p = hermes.mission_site(site?)?;
        let z = hermes.city().surface_at(p, f32::MAX)?.height;
        Some((
            Vec3::new(p[0], p[1], z) * METERS_PER_UNIT,
            self.baggage_service_ready(),
        ))
    }
    pub fn baggage_markers(&self, hermes: &Hermes, site: Option<SiteId>) -> Vec<Vec3> {
        let Some(s) = &self.baggage_shift else {
            return Vec::new();
        };
        let mut points = vec![
            self.vehicles[s.tug.0].position,
            self.vehicles[s.aircraft].position,
        ];
        if site == Some(SiteId::BaggageAircraft) {
            // Service corridor west of the tower and hangar, within the apron.
            for y in [0.1, -0.4, -0.7] {
                points.push(Vec3::new(-4.65, y, 0.122001) * METERS_PER_UNIT);
            }
        }
        points
    }
    pub(super) fn baggage_cargo_visible(&self, vehicle: usize, cart: usize) -> bool {
        self.baggage_shift
            .as_ref()
            .filter(|s| s.tug.0 == vehicle)
            .map_or(true, |s| cart == 0 && s.loaded && !s.invalid)
    }
}
