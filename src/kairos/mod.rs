//! Session missions and durable player progress.
mod bus;
mod save;
mod snowplow;
mod taxi;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MissionId {
    Taxi,
    Bus,
    Snowplow,
}

impl MissionId {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Taxi => "Taxi",
            Self::Bus => "Autobus",
            Self::Snowplow => "Déneigeuse",
        }
    }

    pub(crate) fn vehicle_id(self) -> &'static str {
        match self {
            Self::Taxi => "vehicle-j01",
            Self::Bus => "vehicle-j02",
            Self::Snowplow => "vehicle-j03",
        }
    }

    pub(crate) fn season(self) -> crate::season::Season {
        match self {
            Self::Taxi | Self::Bus => crate::season::Season::Summer,
            Self::Snowplow => crate::season::Season::Winter,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SiteId {
    TaxiPickup,
    TaxiDropoff,
    BusStop1,
    BusStop2,
    BusStop3,
}

impl SiteId {
    pub(crate) const ALL: [Self; 5] = [
        Self::TaxiPickup,
        Self::TaxiDropoff,
        Self::BusStop1,
        Self::BusStop2,
        Self::BusStop3,
    ];

    pub(crate) fn map_name(self) -> &'static str {
        match self {
            Self::TaxiPickup => "mission_taxi_pickup",
            Self::TaxiDropoff => "mission_taxi_dropoff",
            Self::BusStop1 => "mission_bus_stop_1",
            Self::BusStop2 => "mission_bus_stop_2",
            Self::BusStop3 => "mission_bus_stop_3",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SnowCell {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) height_bits: u32,
}

pub(crate) enum MissionEvent {
    PassengerBoarded,
    PassengerAlighted,
    StopServed,
    SnowCleared(Vec<SnowCell>),
}

#[derive(Clone, Copy)]
pub(crate) struct ServicePlan {
    pub(crate) board: &'static [u32],
    pub(crate) alight: &'static [u32],
}

pub(crate) enum MissionProgress {
    ClearedDistanceKm(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MissionObjective {
    TaxiPickup,
    TaxiDropoff,
    TaxiComplete,
    BusStop1,
    BusStop2,
    BusStop3,
    SnowClearing,
}

impl MissionObjective {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::TaxiPickup => "Approche-toi du passager et arrête-toi",
            Self::TaxiDropoff => "Approche-toi de la destination et arrête-toi",
            Self::TaxiComplete => "Course terminée",
            Self::BusStop1 => "Approche-toi de l'arrêt 1/3 et arrête-toi",
            Self::BusStop2 => "Approche-toi de l'arrêt 2/3 et arrête-toi",
            Self::BusStop3 => "Approche-toi de l'arrêt 3/3 et arrête-toi",
            Self::SnowClearing => "Déneige les rues",
        }
    }
}

pub(crate) trait Mission {
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
    taxi: u32,
    bus: u32,
    snowplow: u32,
}

impl Completed {
    fn count(&self, id: MissionId) -> u32 {
        match id {
            MissionId::Taxi => self.taxi,
            MissionId::Bus => self.bus,
            MissionId::Snowplow => self.snowplow,
        }
    }

    fn increment(&mut self, id: MissionId) {
        let count = match id {
            MissionId::Taxi => &mut self.taxi,
            MissionId::Bus => &mut self.bus,
            MissionId::Snowplow => &mut self.snowplow,
        };
        *count = count.saturating_add(1);
    }
}

#[derive(Serialize, Deserialize)]
struct PlayerProgress {
    version: u32,
    money_cents: u64,
    completed: Completed,
    #[serde(default)]
    facts: BTreeMap<String, serde_json::Value>,
}

impl Default for PlayerProgress {
    fn default() -> Self {
        Self {
            version: 1,
            money_cents: 0,
            completed: Completed::default(),
            facts: BTreeMap::new(),
        }
    }
}

pub(crate) struct Kairos {
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
            progress: PlayerProgress::default(),
            dirty: false,
            writable: true,
            last_save: web_time::Instant::now(),
        }
    }
}

impl Kairos {
    #[cfg(test)]
    pub(crate) fn test_session() -> Self {
        Self {
            writable: false,
            ..Self::default()
        }
    }

    pub(crate) fn load() -> Self {
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

    pub(crate) fn accept(&mut self, id: MissionId) {
        self.active = Some(match id {
            MissionId::Taxi => Box::new(taxi::TaxiMission::default()),
            MissionId::Bus => Box::new(bus::BusMission::default()),
            MissionId::Snowplow => Box::new(snowplow::SnowplowMission::default()),
        });
    }

    pub(crate) fn abandon(&mut self) {
        self.active = None;
    }

    pub(crate) fn active_id(&self) -> Option<MissionId> {
        self.active.as_ref().map(|mission| mission.id())
    }

    pub(crate) fn objective(&self) -> Option<MissionObjective> {
        self.active.as_ref().map(|mission| mission.objective())
    }
    pub(crate) fn progress(&self) -> Option<MissionProgress> {
        self.active.as_ref().and_then(|mission| mission.progress())
    }

    pub(crate) fn target(&self) -> Option<SiteId> {
        self.active.as_ref().and_then(|mission| mission.target())
    }

    pub(crate) fn service_plan(&self) -> Option<ServicePlan> {
        self.active
            .as_ref()
            .and_then(|mission| mission.service_plan())
    }

    pub(crate) fn service_finished(&mut self) {
        let event = match (self.active_id(), self.target()) {
            (Some(MissionId::Taxi), Some(SiteId::TaxiPickup)) => MissionEvent::PassengerBoarded,
            (Some(MissionId::Taxi), Some(SiteId::TaxiDropoff)) => MissionEvent::PassengerAlighted,
            (Some(MissionId::Bus), Some(_)) => MissionEvent::StopServed,
            _ => return,
        };
        self.event(event);
    }

    pub(crate) fn is_complete(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|mission| mission.complete())
    }

    pub(crate) fn money_cents(&self) -> u64 {
        self.progress.money_cents
    }

    pub(crate) fn completed(&self, id: MissionId) -> u32 {
        self.progress.completed.count(id)
    }

    pub(crate) fn event(&mut self, event: MissionEvent) {
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

    pub(crate) fn save_if_due(&mut self) {
        if self.dirty && self.last_save.elapsed() >= web_time::Duration::from_secs(1) {
            self.save_now();
        }
    }

    pub(crate) fn save_now(&mut self) {
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
