use super::{Mission, MissionEvent, MissionId, MissionObjective, ServicePlan, SiteId};

pub(super) struct SkiingMission;

impl Mission for SkiingMission {
    fn id(&self) -> MissionId {
        MissionId::Skiing
    }

    fn objective(&self) -> MissionObjective {
        MissionObjective::SkiBromine
    }

    fn target(&self) -> Option<SiteId> {
        None
    }

    fn service_plan(&self) -> Option<ServicePlan> {
        None
    }

    fn event(&mut self, _: &MissionEvent) -> u64 {
        0
    }

    fn complete(&self) -> bool {
        false
    }
}
