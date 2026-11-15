use super::{Mission, MissionEvent, MissionId, MissionObjective, ServicePlan, SiteId};

#[derive(Clone, Copy, Default)]
enum BusStop {
    #[default]
    First,
    Second,
    Third,
}

impl BusStop {
    fn next(&self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::Third,
            Self::Third => Self::First,
        }
    }
}

#[derive(Default)]
pub(super) struct BusMission {
    next_stop: BusStop,
}

impl Mission for BusMission {
    fn id(&self) -> MissionId {
        MissionId::Bus
    }

    fn objective(&self) -> MissionObjective {
        match self.next_stop {
            BusStop::First => MissionObjective::BusStop1,
            BusStop::Second => MissionObjective::BusStop2,
            BusStop::Third => MissionObjective::BusStop3,
        }
    }

    fn target(&self) -> Option<SiteId> {
        Some(match self.next_stop {
            BusStop::First => SiteId::BusStop1,
            BusStop::Second => SiteId::BusStop2,
            BusStop::Third => SiteId::BusStop3,
        })
    }

    fn service_plan(&self) -> Option<ServicePlan> {
        Some(match self.next_stop {
            BusStop::First => ServicePlan {
                board: &[1, 2],
                alight: &[],
            },
            BusStop::Second => ServicePlan {
                board: &[3],
                alight: &[1],
            },
            BusStop::Third => ServicePlan {
                board: &[],
                alight: &[2, 3],
            },
        })
    }

    fn event(&mut self, event: &MissionEvent) -> u64 {
        if matches!(event, MissionEvent::StopServed) {
            self.next_stop = self.next_stop.next();
            800
        } else {
            0
        }
    }

    fn complete(&self) -> bool {
        false
    }
}
