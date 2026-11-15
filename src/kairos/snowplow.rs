use super::{
    Mission, MissionEvent, MissionId, MissionObjective, MissionProgress, ServicePlan, SiteId,
    SnowCell,
};
use std::collections::HashSet;

// Each cleared cell covers 0.25 m². A 3 m swath gives 12,000 cells per km.
const CELLS_PER_KILOMETER: f64 = 12_000.;
const CELLS_PER_CENT: usize = 12;

#[derive(Default)]
pub(super) struct SnowplowMission {
    cells: HashSet<SnowCell>,
}

impl Mission for SnowplowMission {
    fn id(&self) -> MissionId {
        MissionId::Snowplow
    }

    fn objective(&self) -> MissionObjective {
        MissionObjective::SnowClearing
    }

    fn progress(&self) -> Option<MissionProgress> {
        Some(MissionProgress::ClearedDistanceKm(
            self.cells.len() as f64 / CELLS_PER_KILOMETER,
        ))
    }

    fn target(&self) -> Option<SiteId> {
        None
    }

    fn service_plan(&self) -> Option<ServicePlan> {
        None
    }

    fn event(&mut self, event: &MissionEvent) -> u64 {
        let MissionEvent::SnowCleared(cells) = event else {
            return 0;
        };
        let before = self.cells.len();
        for cell in cells {
            self.cells.insert(*cell);
        }
        (self.cells.len() / CELLS_PER_CENT - before / CELLS_PER_CENT) as u64
    }

    fn complete(&self) -> bool {
        false
    }
}
