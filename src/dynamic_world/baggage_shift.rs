//! Airport service checks use the first trailer axle, in metres, never the tractor nose.
use super::{DynamicWorld, VehicleId, METERS_PER_UNIT};
use crate::{hermes::Hermes, kairos::SiteId};
use glam::Vec3;

pub(super) struct Shift {
    tug: VehicleId,
    aircraft: usize,
    tug_model: Option<usize>,
    aircraft_model: Option<usize>,
    aircraft_position: Vec3,
    aircraft_yaw: f32,
    pub loaded: bool,
    dispatch_ground: Vec3,
    deliveries: u8,
    pub invalid: bool,
    settled: f32,
}

impl DynamicWorld {
    pub fn begin_baggage_shift(&mut self, hermes: &Hermes) -> Option<VehicleId> {
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
        let dispatch = hermes.mission_site(SiteId::BaggageDispatch)?;
        let ground = hermes.city().surface_at(dispatch, f32::MAX)?;
        self.baggage_shift = Some(Shift {
            dispatch_ground: Vec3::new(dispatch[0], dispatch[1], ground.height) * METERS_PER_UNIT,
            deliveries: 0,
            tug: VehicleId(tug),
            aircraft,
            tug_model: self.vehicles[tug].definition,
            aircraft_model: self.vehicles[aircraft].definition,
            aircraft_position: self.vehicles[aircraft].position,
            aircraft_yaw: self.vehicles[aircraft].yaw,
            loaded: false,
            invalid: false,
            settled: 0.,
        });
        Some(VehicleId(tug))
    }
    /// Place the worker at a clear boarding point without moving the airport vehicles.
    pub fn approach_baggage_tug(&mut self, hermes: &Hermes) -> bool {
        let Some(shift) = &self.baggage_shift else {
            return false;
        };
        let Some(person) = super::player::Pedestrian::beside_if(
            hermes.city(),
            &self.vehicles[shift.tug.0],
            |position| {
                self.props.blocked(position)
                    || self
                        .vehicles
                        .iter()
                        .any(|v| super::player::blocked_by_vehicle(position, v))
                    || self
                        .parked_carts
                        .iter()
                        .any(|cart| cart.blocks(position, super::player::RADIUS))
            },
        ) else {
            return false;
        };
        if self.is_driving() {
            let vehicle = &mut self.vehicles[self.player_vehicle.0];
            vehicle.velocity = Vec3::ZERO;
            vehicle.sound_controls = super::Controls::default();
        }
        self.entry = None;
        self.pedestrian = Some(person);
        self.update_riders();
        self.sound_geography_timer = 0.;
        true
    }
    pub fn restart_baggage_shift(&mut self, hermes: &Hermes) -> bool {
        let Some(s) = &self.baggage_shift else {
            return false;
        };
        let tug = s.tug.0;
        let aircraft = s.aircraft;
        let restored = [tug, aircraft].map(|i| {
            let mut v = super::Vehicle::spawn_at(hermes.city(), self.vehicle_spawns[i]).ok()?;
            v.set_catalog_model(if i == tug {
                s.tug_model
            } else {
                s.aircraft_model
            });
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
        self.begin_baggage_shift(hermes).is_some()
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
        if !loaded {
            s.deliveries += 1;
        }
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
        if let Some(site) = site {
            let Some(point) = hermes.mission_site(site) else {
                return Vec::new();
            };
            let Some(ground) = hermes.city().surface_at(point, f32::MAX) else {
                return Vec::new();
            };
            vec![Vec3::new(point[0], point[1], ground.height) * METERS_PER_UNIT]
        } else {
            vec![self.vehicles[s.tug.0].position]
        }
    }
    /// Grounded luggage groups beside the terminal bay, using the same cases as the trailers.
    pub fn baggage_luggage_instances(
        &self,
    ) -> impl Iterator<Item = (usize, usize, glam::Mat4, bool)> + '_ {
        let (model, definition) = super::catalog::definitions()
            .iter()
            .enumerate()
            .find(|(_, d)| d.config.research_id == super::baggage::MODEL_ID)
            .expect("bundled baggage tractor");
        let shift = self.baggage_shift.as_ref();
        let remaining =
            shift.map_or(0, |s| {
                if s.invalid {
                    12
                } else {
                    (3_usize.saturating_sub(s.deliveries as usize) * 4)
                        .saturating_sub(if s.loaded { 4 } else { 0 })
                }
            });
        let visible = shift.is_some_and(|s| {
            s.dispatch_ground.distance_squared(self.player_position())
                < crate::performance::draw_radius(super::parked::RENDER_DISTANCE).powi(2)
        });
        (0..if visible { remaining } else { 0 }).flat_map(move |group| {
            let s = shift.unwrap();
            // The west side stays clear of the terminal building east of the bay.
            let ground = s.dispatch_ground
                + Vec3::new(
                    -3.5 - (group % 3) as f32 * 2.8,
                    (group / 3) as f32 * 2.2 - 3.3,
                    0.,
                );
            let world = glam::Mat4::from_scale_rotation_translation(
                Vec3::splat(1. / METERS_PER_UNIT),
                glam::Quat::IDENTITY,
                ground / METERS_PER_UNIT,
            );
            definition
                .ground_cargo_parts()
                .map(move |(part, local)| (model, part, world * local, false))
        })
    }
    pub(super) fn baggage_cargo_visible(&self, vehicle: usize) -> bool {
        self.baggage_shift
            .as_ref()
            .filter(|s| s.tug.0 == vehicle)
            .map_or(true, |s| s.loaded && !s.invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kairos::{Kairos, MissionEvent, MissionId};
    fn setup() -> (Hermes, DynamicWorld) {
        let h = Hermes::test_map_from_source("pad=concrete(-3,-3,3,-3,3,3,-3,3); elevation(pad,0.122001); player_start(-1,0,0); parked_vehicle(\"vehicle-airport-baggage-tractor\",0,0,90); parked_vehicle(\"vehicle-air-estuaire\",2,0,-90); mission_baggage_dispatch=map_point(0,1); mission_baggage_aircraft=map_point(0,-1);");
        let mut w = DynamicWorld::new(&h);
        w.begin_baggage_shift(&h).unwrap();
        w.player_vehicle = w.baggage_shift.as_ref().unwrap().tug;
        w.pedestrian = None;
        (h, w)
    }
    fn park(w: &mut DynamicWorld, h: &Hermes, site: SiteId, yaw: f32) {
        let id = w.baggage_shift.as_ref().unwrap().tug.0;
        w.vehicles[id].yaw = yaw;
        w.vehicles[id].pitch = 0.;
        w.vehicles[id].roll = 0.;
        w.vehicles[id].baggage_links.fill(Default::default());
        let axle = w.vehicles[id].carts().next().unwrap().axle();
        let p = glam::Vec2::from_array(h.mission_site(site).unwrap()) * METERS_PER_UNIT;
        let delta = p - axle.truncate();
        w.vehicles[id].position += delta.extend(0.);
        w.vehicles[id].velocity = Vec3::ZERO;
        w.vehicles[id].grounded = true;
    }
    fn settle(w: &mut DynamicWorld, h: &Hermes, site: SiteId) {
        for _ in 0..11 {
            w.update_baggage_shift(h, Some(site), 0.1);
        }
    }
    #[test]
    fn baggage_dropoff_has_one_marker_at_the_trailer_bay() {
        let (h, w) = setup();
        let markers = w.baggage_markers(&h, Some(SiteId::BaggageAircraft));
        assert_eq!(markers.len(), 1);
        let target = h.mission_site(SiteId::BaggageAircraft).unwrap();
        assert_eq!(
            markers[0].truncate(),
            glam::Vec2::from_array(target) * METERS_PER_UNIT
        );
        assert_eq!(
            markers[0],
            w.baggage_bay(&h, Some(SiteId::BaggageAircraft)).unwrap().0
        );
    }

    #[test]
    fn baggage_service_requires_cart_alignment_stop_and_assigned_driver() {
        let (h, mut w) = setup();
        let site = SiteId::BaggageDispatch;
        park(&mut w, &h, site, 0.);
        settle(&mut w, &h, site);
        assert!(!w.baggage_service_ready());
        park(&mut w, &h, site, std::f32::consts::FRAC_PI_2);
        w.update_baggage_shift(&h, Some(site), 0.5);
        assert!(!w.baggage_service_ready());
        let tug = w.player_vehicle;
        w.player_vehicle = VehicleId(0);
        settle(&mut w, &h, site);
        assert!(!w.baggage_service_ready());
        w.player_vehicle = tug;
        w.vehicles[tug.0].velocity = Vec3::X;
        settle(&mut w, &h, site);
        assert!(!w.baggage_service_ready());
        w.vehicles[tug.0].velocity = Vec3::ZERO;
        // Tractor alone in the box leaves the first cart more than 4 m away.
        let target = glam::Vec2::from_array(h.mission_site(site).unwrap()) * METERS_PER_UNIT;
        let delta = target - w.vehicles[tug.0].position.truncate();
        w.vehicles[tug.0].position += delta.extend(0.);
        settle(&mut w, &h, site);
        assert!(!w.baggage_service_ready());
        park(&mut w, &h, site, -std::f32::consts::FRAC_PI_2);
        settle(&mut w, &h, site);
        assert!(w.baggage_service_ready());
    }
    #[test]
    fn baggage_rotation_loads_all_trailers_for_three_trips_and_recovers_after_reset() {
        let (h, mut w) = setup();
        let mut k = Kairos::test_session();
        k.accept(MissionId::Baggage);
        assert!(!w.baggage_cargo_visible(w.player_vehicle.0));
        assert!(!w.confirm_baggage_service(false));
        let empty_train_parts = w.baggage_cart_instances().count();
        let cargo_parts = super::super::catalog::definitions()
            .iter()
            .find(|d| d.config.research_id == super::super::baggage::MODEL_ID)
            .unwrap()
            .ground_cargo_parts()
            .count();
        park(
            &mut w,
            &h,
            SiteId::BaggageDispatch,
            std::f32::consts::FRAC_PI_2,
        );
        settle(&mut w, &h, SiteId::BaggageDispatch);
        assert!(w.confirm_baggage_service(true));
        k.event(MissionEvent::BaggageLoaded);
        assert!(w.baggage_cargo_visible(w.player_vehicle.0));
        assert_eq!(
            w.baggage_cart_instances().count(),
            empty_train_parts + cargo_parts * 4
        );
        assert!(!w.confirm_baggage_service(true));
        w.reset_player(&h);
        assert!(w.baggage_shift_invalid());
        assert!(!w.baggage_cargo_visible(w.player_vehicle.0));
        k.event(MissionEvent::BaggageRestarted);
        assert!(w.restart_baggage_shift(&h));
        let full_pile = w.baggage_luggage_instances().count();
        assert!(full_pile > 0);
        for trip in 1..=3 {
            for (site, loaded) in [
                (SiteId::BaggageDispatch, true),
                (SiteId::BaggageAircraft, false),
            ] {
                assert_eq!(k.target(), Some(site));
                park(&mut w, &h, site, -std::f32::consts::FRAC_PI_2);
                settle(&mut w, &h, site);
                assert!(w.confirm_baggage_service(loaded));
                k.event(if loaded {
                    MissionEvent::BaggageLoaded
                } else {
                    MissionEvent::BaggageDelivered
                });
                assert_eq!(
                    w.baggage_cart_instances().count(),
                    empty_train_parts + if loaded { cargo_parts * 4 } else { 0 }
                );
                assert!(!w.confirm_baggage_service(loaded));
                if loaded {
                    assert_eq!(
                        w.baggage_luggage_instances().count(),
                        full_pile * (3 - trip) / 3
                    );
                }
            }
            assert!(!k.is_complete());
            assert_eq!(k.money_cents(), 0);
        }
        assert_eq!(
            k.objective(),
            Some(crate::kairos::MissionObjective::ReturnBaggage)
        );
        park(
            &mut w,
            &h,
            SiteId::BaggageDispatch,
            std::f32::consts::FRAC_PI_2,
        );
        settle(&mut w, &h, SiteId::BaggageDispatch);
        assert!(w.baggage_service_ready());
        k.event(MissionEvent::BaggageReturned);
        k.event(MissionEvent::BaggageReturned);
        k.event(MissionEvent::BaggageDelivered);
        k.event(MissionEvent::BaggageLoaded);
        k.event(MissionEvent::BaggageRestarted);
        assert_eq!(k.money_cents(), 6000);
        assert_eq!(k.completed(MissionId::Baggage), 1);
        assert!(k.is_complete());
        assert!(w.begin_baggage_shift(&h).is_some());
        k.accept(MissionId::Baggage);
        assert_eq!(k.target(), Some(SiteId::BaggageDispatch));
        assert_eq!(k.money_cents(), 6000);
    }

    #[test]
    fn baggage_aircraft_movement_clears_pending_load_and_restarts_at_dispatch() {
        let (h, mut w) = setup();
        w.baggage_shift.as_mut().unwrap().loaded = true;
        let aircraft = w.baggage_shift.as_ref().unwrap().aircraft;
        w.vehicles[aircraft].position.x += 2.;
        w.update_baggage_shift(&h, Some(SiteId::BaggageAircraft), 0.1);
        assert!(w.baggage_shift_invalid());
        assert!(!w.baggage_shift.as_ref().unwrap().loaded);
        assert!(!w.confirm_baggage_service(false));
        assert!(w.restart_baggage_shift(&h));
        assert!(!w.baggage_shift_invalid());
    }
}
