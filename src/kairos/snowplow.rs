use super::{Mission, MissionEvent, MissionId, ServicePlan, SiteId, SnowCell};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct SnowplowMission {
    cells: HashSet<SnowCell>,
}

impl Mission for SnowplowMission {
    fn id(&self) -> MissionId {
        MissionId::Snowplow
    }

    fn objective(&self) -> &'static str {
        if self.complete() {
            "Déneigement terminé"
        } else {
            "Déneige les rues"
        }
    }

    fn progress(&self) -> Option<(u32, u32)> {
        Some(((self.cells.len() as u32 / 4).min(300), 300))
    }

    fn target(&self) -> Option<SiteId> {
        None
    }

    fn service_plan(&self) -> Option<ServicePlan> {
        None
    }

    fn event(&mut self, event: &MissionEvent) -> u64 {
        if self.complete() {
            return 0;
        }
        let MissionEvent::SnowCleared(cells) = event else {
            return 0;
        };
        let before = self.cells.len();
        for cell in cells {
            if self.cells.len() >= 1200 {
                break;
            }
            self.cells.insert(*cell);
        }
        (self.cells.len() - before) as u64 * 5
    }

    fn complete(&self) -> bool {
        self.cells.len() >= 1200
    }
}
