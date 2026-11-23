use super::{
    Mission, MissionEvent, MissionId, MissionObjective, MissionProgress, ServicePlan, SiteId,
};

#[derive(Default)]
enum Stage {
    #[default]
    Assigned,
    Loaded,
    Returning,
    Delivered,
}
#[derive(Default)]
pub(super) struct BaggageMission {
    stage: Stage,
    deliveries: u8,
}
impl Mission for BaggageMission {
    fn id(&self) -> MissionId {
        MissionId::Baggage
    }
    fn objective(&self) -> MissionObjective {
        match self.stage {
            Stage::Assigned => MissionObjective::LoadBaggage,
            Stage::Loaded => MissionObjective::DeliverBaggage,
            Stage::Returning => MissionObjective::ReturnBaggage,
            Stage::Delivered => MissionObjective::BaggageComplete,
        }
    }
    fn progress(&self) -> Option<MissionProgress> {
        Some(MissionProgress::Baggage {
            loaded_trailers: if matches!(self.stage, Stage::Loaded) {
                4
            } else {
                0
            },
            deliveries: self.deliveries,
        })
    }
    fn target(&self) -> Option<SiteId> {
        match self.stage {
            Stage::Assigned | Stage::Returning => Some(SiteId::BaggageDispatch),
            Stage::Loaded => Some(SiteId::BaggageAircraft),
            Stage::Delivered => None,
        }
    }
    fn service_plan(&self) -> Option<ServicePlan> {
        None
    }
    fn complete(&self) -> bool {
        matches!(self.stage, Stage::Delivered)
    }
    fn event(&mut self, event: &MissionEvent) -> u64 {
        match (&self.stage, event) {
            (Stage::Assigned, MissionEvent::BaggageLoaded) => self.stage = Stage::Loaded,
            (Stage::Loaded, MissionEvent::BaggageDelivered) => {
                self.deliveries += 1;
                self.stage = if self.deliveries == 3 {
                    Stage::Returning
                } else {
                    Stage::Assigned
                };
            }
            (Stage::Returning, MissionEvent::BaggageReturned) => {
                self.stage = Stage::Delivered;
                return 6000;
            }
            (
                Stage::Assigned | Stage::Loaded | Stage::Returning,
                MissionEvent::BaggageRestarted,
            ) => {
                self.stage = Stage::Assigned;
                self.deliveries = 0;
            }
            _ => {}
        }
        0
    }
}
