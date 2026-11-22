use super::{
    Mission, MissionEvent, MissionId, MissionObjective, MissionProgress, ServicePlan, SiteId,
};

#[derive(Default)]
enum Stage {
    #[default]
    Assigned,
    Loaded,
    Delivered,
}
#[derive(Default)]
pub(super) struct BaggageMission {
    stage: Stage,
}
impl Mission for BaggageMission {
    fn id(&self) -> MissionId {
        MissionId::Baggage
    }
    fn objective(&self) -> MissionObjective {
        match self.stage {
            Stage::Assigned => MissionObjective::LoadBaggage,
            Stage::Loaded => MissionObjective::DeliverBaggage,
            Stage::Delivered => MissionObjective::BaggageComplete,
        }
    }
    fn progress(&self) -> Option<MissionProgress> {
        Some(match self.stage {
            Stage::Assigned => MissionProgress::Bags {
                loaded: 0,
                delivered: 0,
            },
            Stage::Loaded => MissionProgress::Bags {
                loaded: 4,
                delivered: 0,
            },
            Stage::Delivered => MissionProgress::Bags {
                loaded: 0,
                delivered: 4,
            },
        })
    }
    fn target(&self) -> Option<SiteId> {
        match self.stage {
            Stage::Assigned => Some(SiteId::BaggageDispatch),
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
                self.stage = Stage::Delivered;
                return 2000;
            }
            (Stage::Assigned | Stage::Loaded, MissionEvent::BaggageRestarted) => {
                self.stage = Stage::Assigned
            }
            _ => {}
        }
        0
    }
}
