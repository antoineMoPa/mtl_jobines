use super::{Mission, MissionEvent, MissionId, MissionObjective, ServicePlan, SiteId};

#[derive(Default)]
pub(super) struct BusMission {
    served: u8,
}

impl Mission for BusMission {
    fn id(&self) -> MissionId {
        MissionId::Bus
    }

    fn objective(&self) -> MissionObjective {
        match self.served {
            0 => MissionObjective::BusStop1,
            1 => MissionObjective::BusStop2,
            2 => MissionObjective::BusStop3,
            _ => MissionObjective::BusComplete,
        }
    }

    fn target(&self) -> Option<SiteId> {
        match self.served {
            0 => Some(SiteId::BusStop1),
            1 => Some(SiteId::BusStop2),
            2 => Some(SiteId::BusStop3),
            _ => None,
        }
    }

    fn service_plan(&self) -> Option<ServicePlan> {
        match self.served {
            0 => Some(ServicePlan {
                board: &[1, 2],
                alight: &[],
            }),
            1 => Some(ServicePlan {
                board: &[3],
                alight: &[1],
            }),
            2 => Some(ServicePlan {
                board: &[],
                alight: &[2, 3],
            }),
            _ => None,
        }
    }

    fn event(&mut self, event: &MissionEvent) -> u64 {
        if self.served < 3 && matches!(event, MissionEvent::StopServed) {
            self.served += 1;
            800
        } else {
            0
        }
    }

    fn complete(&self) -> bool {
        self.served == 3
    }
}
