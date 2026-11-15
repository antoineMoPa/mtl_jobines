use super::{Mission, MissionEvent, MissionId, ServicePlan, SiteId};

#[derive(Default)]
pub(super) struct BusMission {
    served: u8,
}

impl Mission for BusMission {
    fn id(&self) -> MissionId {
        MissionId::Bus
    }

    fn objective(&self) -> &'static str {
        match self.served {
            0 => "Arrêt 1/3 — arrête-toi et appuie E",
            1 => "Arrêt 2/3 — arrête-toi et appuie E",
            2 => "Arrêt 3/3 — arrête-toi et appuie E",
            _ => "Ligne terminée",
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
