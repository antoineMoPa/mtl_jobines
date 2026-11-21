use super::{Mission, MissionEvent, MissionId, MissionObjective, ServicePlan, SiteId};

#[derive(Default)]
pub(super) enum CatMission {
    #[default]
    Searching,
    Carrying,
    Home,
}

impl Mission for CatMission {
    fn id(&self) -> MissionId {
        MissionId::Cat
    }
    fn objective(&self) -> MissionObjective {
        match self {
            Self::Searching => MissionObjective::FindCat,
            Self::Carrying => MissionObjective::ReturnCat,
            Self::Home => MissionObjective::CatHome,
        }
    }
    fn target(&self) -> Option<SiteId> {
        match self {
            Self::Searching => Some(SiteId::MissingCat),
            Self::Carrying => Some(SiteId::CatDuplex),
            Self::Home => None,
        }
    }
    fn service_plan(&self) -> Option<ServicePlan> {
        None
    }
    fn event(&mut self, event: &MissionEvent) -> u64 {
        match (&*self, event) {
            (Self::Searching, MissionEvent::CatCaught) => *self = Self::Carrying,
            (Self::Carrying, MissionEvent::CatReturned) => {
                *self = Self::Home;
                return 1000;
            }
            _ => {}
        }
        0
    }
    fn complete(&self) -> bool {
        matches!(self, Self::Home)
    }
}
