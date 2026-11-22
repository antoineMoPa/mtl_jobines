//! Session missions and durable player progress.
mod baggage;
mod bus;
mod cat;
mod save;
mod snowplow;
mod taxi;
mod zombie;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionId {
    Baggage,
    Taxi,
    Bus,
    Snowplow,
    Zombie,
    Cat,
}

impl MissionId {
    pub fn title(self) -> &'static str {
        match self {
            Self::Baggage => "Bagagiste",
            Self::Taxi => "Taxi",
            Self::Bus => "Autobus",
            Self::Snowplow => "Déneigeuse",
            Self::Zombie => "Mode zombie",
            Self::Cat => "Chat perdu",
        }
    }

    pub fn vehicle_id(self) -> Option<&'static str> {
        match self {
            Self::Baggage => Some("vehicle-airport-baggage-tractor"),
            Self::Taxi => Some("vehicle-j01"),
            Self::Bus => Some("vehicle-j02"),
            Self::Snowplow => Some("vehicle-j03"),
            Self::Zombie | Self::Cat => None,
        }
    }

    pub fn required_season(self) -> Option<crate::season::Season> {
        match self {
            Self::Baggage | Self::Taxi | Self::Bus => Some(crate::season::Season::Summer),
            Self::Snowplow => Some(crate::season::Season::Winter),
            Self::Zombie | Self::Cat => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SiteId {
    BaggageDispatch,
    BaggageAircraft,
    TaxiPickup,
    TaxiDropoff,
    BusStop1,
    BusStop2,
    BusStop3,
    MissingCat,
    CatDuplex,
}

impl SiteId {
    pub const ALL: [Self; 9] = [
        Self::BaggageDispatch,
        Self::BaggageAircraft,
        Self::TaxiPickup,
        Self::TaxiDropoff,
        Self::BusStop1,
        Self::BusStop2,
        Self::BusStop3,
        Self::MissingCat,
        Self::CatDuplex,
    ];

    pub fn map_name(self) -> &'static str {
        match self {
            Self::BaggageDispatch => "mission_baggage_dispatch",
            Self::BaggageAircraft => "mission_baggage_aircraft",
            Self::TaxiPickup => "mission_taxi_pickup",
            Self::TaxiDropoff => "mission_taxi_dropoff",
            Self::BusStop1 => "mission_bus_stop_1",
            Self::BusStop2 => "mission_bus_stop_2",
            Self::BusStop3 => "mission_bus_stop_3",
            Self::MissingCat => "mission_missing_cat",
            Self::CatDuplex => "mission_cat_duplex",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SnowCell {
    pub x: i32,
    pub y: i32,
    pub height_bits: u32,
}

pub enum MissionEvent {
    BaggageLoaded,
    BaggageDelivered,
    BaggageRestarted,
    CatCaught,
    CatReturned,
    PassengerBoarded,
    PassengerAlighted,
    StopServed,
    SnowCleared(Vec<SnowCell>),
}

#[derive(Clone, Copy)]
pub struct ServicePlan {
    pub board: &'static [u32],
    pub alight: &'static [u32],
}

pub enum MissionProgress {
    ClearedDistanceKm(f64),
    Bags { loaded: u8, delivered: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissionObjective {
    LoadBaggage,
    DeliverBaggage,
    BaggageComplete,
    TaxiPickup,
    TaxiDropoff,
    TaxiComplete,
    BusStop1,
    BusStop2,
    BusStop3,
    SnowClearing,
    SurviveZombies,
    FindCat,
    ReturnCat,
    CatHome,
}

impl MissionObjective {
    pub fn label(self) -> &'static str {
        match self {
            Self::LoadBaggage => "Charge 4 bagages au terminal · remorque parallèle au quai",
            Self::DeliverBaggage => "Livre les 4 bagages à l’Estuaire · reste sur le tablier",
            Self::BaggageComplete => "Vol Estuaire terminé · 4 bagages livrés",
            Self::TaxiPickup => "Approche-toi du passager et arrête-toi",
            Self::TaxiDropoff => "Approche-toi de la destination et arrête-toi",
            Self::TaxiComplete => "Course terminée",
            Self::BusStop1 => "Approche-toi de l'arrêt 1/3 et arrête-toi",
            Self::BusStop2 => "Approche-toi de l'arrêt 2/3 et arrête-toi",
            Self::BusStop3 => "Approche-toi de l'arrêt 3/3 et arrête-toi",
            Self::SnowClearing => "Déneige les rues",
            Self::FindCat => "Retrouve le chat perdu.",
            Self::ReturnCat => "Ramène le chat au duplex.",
            Self::CatHome => "Le chat est rentré chez lui !",
            Self::SurviveZombies => "Survis aux zombies",
        }
    }
}

pub trait Mission {
    fn id(&self) -> MissionId;
    fn objective(&self) -> MissionObjective;
    fn progress(&self) -> Option<MissionProgress> {
        None
    }
    fn target(&self) -> Option<SiteId>;
    fn service_plan(&self) -> Option<ServicePlan>;
    fn event(&mut self, event: &MissionEvent) -> u64;
    fn complete(&self) -> bool;
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Completed {
    baggage: u32,
    taxi: u32,
    bus: u32,
    snowplow: u32,
    zombie: u32,
    cat: u32,
}

impl Completed {
    fn count(&self, id: MissionId) -> u32 {
        match id {
            MissionId::Baggage => self.baggage,
            MissionId::Taxi => self.taxi,
            MissionId::Bus => self.bus,
            MissionId::Snowplow => self.snowplow,
            MissionId::Zombie => self.zombie,
            MissionId::Cat => self.cat,
        }
    }

    fn increment(&mut self, id: MissionId) {
        let count = match id {
            MissionId::Baggage => &mut self.baggage,
            MissionId::Taxi => &mut self.taxi,
            MissionId::Bus => &mut self.bus,
            MissionId::Snowplow => &mut self.snowplow,
            MissionId::Zombie => &mut self.zombie,
            MissionId::Cat => &mut self.cat,
        };
        *count = count.saturating_add(1);
    }
}

#[derive(Serialize, Deserialize)]
struct PlayerProgress {
    version: u32,
    #[serde(default)]
    intro_viewed: bool,
    money_cents: u64,
    completed: Completed,
    #[serde(default)]
    facts: BTreeMap<String, serde_json::Value>,
}

impl Default for PlayerProgress {
    fn default() -> Self {
        Self {
            version: 1,
            intro_viewed: false,
            money_cents: 0,
            completed: Completed::default(),
            facts: BTreeMap::new(),
        }
    }
}

fn default_vehicle_return(away: bool, previous: f32, elapsed: f32) -> f32 {
    if !away {
        return -1.;
    }
    let remaining = if previous < 0. { 10. } else { previous };
    (remaining - elapsed).max(0.)
}

pub struct Kairos {
    return_seconds: Option<f32>,
    vehicle_return_rule: Box<dyn Fn(bool, f32, f32) -> f32>,
    active: Option<Box<dyn Mission>>,
    progress: PlayerProgress,
    dirty: bool,
    writable: bool,
    last_save: web_time::Instant,
}

impl Default for Kairos {
    fn default() -> Self {
        Self {
            active: None,
            return_seconds: None,
            vehicle_return_rule: Box::new(default_vehicle_return),
            progress: PlayerProgress::default(),
            dirty: false,
            writable: true,
            last_save: web_time::Instant::now(),
        }
    }
}

impl Kairos {
    pub fn intro_viewed(&self) -> bool {
        self.progress.intro_viewed
    }

    pub fn mark_intro_viewed(&mut self) {
        if !self.progress.intro_viewed {
            self.progress.intro_viewed = true;
            self.dirty = true;
        }
        self.save_now();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_session() -> Self {
        Self {
            writable: false,
            ..Self::default()
        }
    }

    pub fn load() -> Self {
        match save::read() {
            Ok(None) => Self::default(),
            Ok(Some(progress)) => Self {
                progress,
                ..Self::default()
            },
            Err(error) => {
                eprintln!("Player progress unavailable: {error}");
                Self {
                    writable: false,
                    ..Self::default()
                }
            }
        }
    }

    pub fn accept(&mut self, id: MissionId) {
        self.return_seconds = None;
        self.active = Some(match id {
            MissionId::Baggage => Box::new(baggage::BaggageMission::default()),
            MissionId::Taxi => Box::new(taxi::TaxiMission::default()),
            MissionId::Bus => Box::new(bus::BusMission::default()),
            MissionId::Snowplow => Box::new(snowplow::SnowplowMission::default()),
            MissionId::Cat => Box::new(cat::CatMission::default()),
            MissionId::Zombie => Box::new(zombie::ZombieMission),
        });
    }

    pub fn abandon(&mut self) {
        self.active = None;
        self.return_seconds = None;
    }

    /// Supply the client countdown rule. A negative result clears the timer, zero expires it.
    pub fn set_vehicle_return_rule(&mut self, rule: impl Fn(bool, f32, f32) -> f32 + 'static) {
        self.vehicle_return_rule = Box::new(rule);
    }

    pub fn vehicle_return_expired(&mut self, driving_assigned: bool, elapsed: f32) -> bool {
        let away = self.active_id() != Some(MissionId::Baggage)
            && self.active_id().and_then(MissionId::vehicle_id).is_some()
            && !driving_assigned
            && !self.is_complete();
        let seconds = (self.vehicle_return_rule)(away, self.return_seconds.unwrap_or(-1.), elapsed);
        self.return_seconds = (seconds >= 0.).then_some(seconds);
        seconds == 0.
    }

    pub fn vehicle_return_seconds(&self) -> Option<u32> {
        self.return_seconds.map(|seconds| seconds.ceil() as u32)
    }

    pub fn active_id(&self) -> Option<MissionId> {
        self.active.as_ref().map(|mission| mission.id())
    }

    pub fn objective(&self) -> Option<MissionObjective> {
        self.active.as_ref().map(|mission| mission.objective())
    }
    pub fn progress(&self) -> Option<MissionProgress> {
        self.active.as_ref().and_then(|mission| mission.progress())
    }

    pub fn target(&self) -> Option<SiteId> {
        self.active.as_ref().and_then(|mission| mission.target())
    }

    pub fn service_plan(&self) -> Option<ServicePlan> {
        self.active
            .as_ref()
            .and_then(|mission| mission.service_plan())
    }

    pub fn service_finished(&mut self) {
        let event = match (self.active_id(), self.target()) {
            (Some(MissionId::Taxi), Some(SiteId::TaxiPickup)) => MissionEvent::PassengerBoarded,
            (Some(MissionId::Taxi), Some(SiteId::TaxiDropoff)) => MissionEvent::PassengerAlighted,
            (Some(MissionId::Bus), Some(_)) => MissionEvent::StopServed,
            _ => return,
        };
        self.event(event);
    }

    pub fn is_complete(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|mission| mission.complete())
    }

    pub fn money_cents(&self) -> u64 {
        self.progress.money_cents
    }

    pub fn completed(&self, id: MissionId) -> u32 {
        self.progress.completed.count(id)
    }

    pub fn event(&mut self, event: MissionEvent) {
        let Some(mission) = self.active.as_mut() else {
            return;
        };
        let bus_loop = mission.id() == MissionId::Bus
            && mission.target() == Some(SiteId::BusStop3)
            && matches!(&event, MissionEvent::StopServed);
        let was_complete = mission.complete();
        let cents = mission.event(&event);
        if cents > 0 {
            if let Some(total) = self.progress.money_cents.checked_add(cents) {
                self.progress.money_cents = total;
                self.dirty = true;
            }
        }
        if !was_complete && mission.complete() {
            self.progress.completed.increment(mission.id());
            self.dirty = true;
        }
        if bus_loop {
            self.progress.completed.increment(MissionId::Bus);
            self.dirty = true;
        }
    }

    pub fn save_if_due(&mut self) {
        if self.dirty && self.last_save.elapsed() >= web_time::Duration::from_secs(1) {
            self.save_now();
        }
    }

    pub fn save_now(&mut self) {
        if !self.dirty || !self.writable {
            return;
        }
        match save::write(&self.progress) {
            Ok(()) => {
                self.dirty = false;
                self.last_save = web_time::Instant::now();
            }
            Err(error) => {
                eprintln!("Could not save player progress: {error}");
                self.last_save = web_time::Instant::now();
                #[cfg(target_arch = "wasm32")]
                {
                    self.writable = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_rule_controls_displayed_countdown_and_expiry() {
        let mut kairos = Kairos::test_session();
        kairos.set_vehicle_return_rule(|away, previous, elapsed| {
            if !away {
                -1.
            } else if previous < 0. {
                2.5
            } else {
                (previous - elapsed).max(0.)
            }
        });
        kairos.accept(MissionId::Taxi);
        assert!(!kairos.vehicle_return_expired(false, 0.));
        assert_eq!(kairos.vehicle_return_seconds(), Some(3));
        assert!(!kairos.vehicle_return_expired(true, 1.));
        assert_eq!(kairos.vehicle_return_seconds(), None);
        assert!(!kairos.vehicle_return_expired(false, 0.));
        assert!(kairos.vehicle_return_expired(false, 3.));
    }

    #[test]
    fn intro_viewed_survives_save_and_old_saves_default_to_unseen() {
        let progress =
            save::decode(br#"{"version":1,"money_cents":2375,"completed":{"taxi":2}}"#).unwrap();
        let mut kairos = Kairos {
            progress,
            ..Kairos::test_session()
        };
        assert!(!kairos.intro_viewed());
        kairos.mark_intro_viewed();
        assert!(kairos.dirty);
        let bytes = serde_json::to_vec(&kairos.progress).unwrap();
        let loaded = Kairos {
            progress: save::decode(&bytes).unwrap(),
            ..Kairos::test_session()
        };
        assert!(loaded.intro_viewed());
        assert_eq!(loaded.money_cents(), 2375);
        assert_eq!(loaded.completed(MissionId::Taxi), 2);
    }

    #[test]
    fn mission_completion_and_payout_are_once_only() {
        let mut kairos = Kairos::default();
        kairos.accept(MissionId::Taxi);
        kairos.event(MissionEvent::PassengerBoarded);
        kairos.event(MissionEvent::PassengerAlighted);
        kairos.event(MissionEvent::PassengerAlighted);
        assert_eq!(kairos.money_cents(), 1500);
        assert_eq!(kairos.completed(MissionId::Taxi), 1);
        kairos.accept(MissionId::Snowplow);
        let cells = (0..12)
            .map(|x| SnowCell {
                x,
                y: 2,
                height_bits: 0,
            })
            .collect();
        kairos.event(MissionEvent::SnowCleared(cells));
        kairos.event(MissionEvent::SnowCleared(vec![SnowCell {
            x: 0,
            y: 2,
            height_bits: 0,
        }]));
        assert_eq!(kairos.money_cents(), 1501);
    }

    #[test]
    fn bus_circuits_repeat_and_survive_save_round_trip() {
        let mut kairos = Kairos::default();
        kairos.accept(MissionId::Bus);
        for _ in 0..4 {
            kairos.event(MissionEvent::StopServed);
        }
        assert!(!kairos.is_complete());
        assert_eq!(kairos.target(), Some(SiteId::BusStop2));
        assert_eq!(kairos.money_cents(), 3200);
        assert_eq!(kairos.completed(MissionId::Bus), 1);
        for _ in 0..2 {
            kairos.event(MissionEvent::StopServed);
        }
        assert_eq!(kairos.target(), Some(SiteId::BusStop1));
        assert_eq!(kairos.money_cents(), 4800);
        assert_eq!(kairos.completed(MissionId::Bus), 2);
        let bytes = serde_json::to_vec(&kairos.progress).unwrap();
        let loaded = save::decode(&bytes).unwrap();
        assert_eq!(loaded.money_cents, 4800);
        assert_eq!(loaded.completed.bus, 2);
    }

    #[test]
    fn snow_batches_keep_paying_for_new_cells_without_a_finish_line() {
        let mut kairos = Kairos::test_session();
        kairos.accept(MissionId::Snowplow);
        let cells = (0..1200)
            .map(|x| SnowCell {
                x,
                y: 0,
                height_bits: 0,
            })
            .collect();
        kairos.event(MissionEvent::SnowCleared(cells));
        assert_eq!(kairos.money_cents(), 100);
        assert_eq!(kairos.completed(MissionId::Snowplow), 0);
        assert!(!kairos.is_complete());
        kairos.event(MissionEvent::SnowCleared(vec![SnowCell {
            x: 1200,
            y: 0,
            height_bits: 0,
        }]));
        assert_eq!(kairos.money_cents(), 100);
        kairos.event(MissionEvent::SnowCleared(
            (1201..1212)
                .map(|x| SnowCell {
                    x,
                    y: 0,
                    height_bits: 0,
                })
                .collect(),
        ));
        assert_eq!(kairos.money_cents(), 101);
        assert!(!kairos.is_complete());
    }
}
