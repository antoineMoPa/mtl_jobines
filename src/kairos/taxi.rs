use super::{Mission, MissionEvent, MissionId, ServicePlan, SiteId};

#[derive(Default)]
pub(super) struct TaxiMission {
    stage: u8,
}

impl Mission for TaxiMission {
    fn id(&self) -> MissionId {
        MissionId::Taxi
    }

    fn objective(&self) -> &'static str {
        match self.stage {
            0 => "Passager — arrête-toi et appuie E",
            1 => "Destination — arrête-toi et appuie E",
            _ => "Course terminée",
        }
    }

    fn target(&self) -> Option<SiteId> {
        match self.stage {
            0 => Some(SiteId::TaxiPickup),
            1 => Some(SiteId::TaxiDropoff),
            _ => None,
        }
    }

    fn service_plan(&self) -> Option<ServicePlan> {
        match self.stage {
            0 => Some(ServicePlan {
                board: &[1],
                alight: &[],
            }),
            1 => Some(ServicePlan {
                board: &[],
                alight: &[1],
            }),
            _ => None,
        }
    }

    fn event(&mut self, event: &MissionEvent) -> u64 {
        match (self.stage, event) {
            (0, MissionEvent::PassengerBoarded) => self.stage = 1,
            (1, MissionEvent::PassengerAlighted) => {
                self.stage = 2;
                return 1500;
            }
            _ => {}
        }
        0
    }

    fn complete(&self) -> bool {
        self.stage == 2
    }
}
