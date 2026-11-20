use super::{Mission, MissionEvent, MissionId, MissionObjective, ServicePlan, SiteId};

pub(super) struct ZombieMission;

impl Mission for ZombieMission {
    fn id(&self) -> MissionId {
        MissionId::Zombie
    }

    fn objective(&self) -> MissionObjective {
        MissionObjective::SurviveZombies
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
