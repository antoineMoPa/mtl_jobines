//! Named mission points authored in the .road map.
use crate::kairos::SiteId;
use glam::Vec3;
use opencity::{Argument, Program, Statement};
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct MissionSites {
    points: HashMap<SiteId, [f32; 2]>,
}

impl MissionSites {
    pub(super) fn from_program(program: &Program) -> Self {
        let mut points = HashMap::new();
        for statement in &program.statements {
            let Statement::Call(call) = statement else {
                continue;
            };
            if call.name != "map_point" || call.assigns.len() != 1 || call.args.len() != 2 {
                continue;
            }
            let Some(site) = SiteId::ALL
                .into_iter()
                .find(|site| call.assigns[0] == site.map_name())
            else {
                continue;
            };
            let [Argument::Number(x), Argument::Number(y)] = call.args.as_slice() else {
                continue;
            };
            if let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>()) {
                if x.is_finite() && y.is_finite() {
                    points.insert(site, [x, y]);
                }
            }
        }
        Self { points }
    }

    pub(super) fn get(&self, site: SiteId) -> Option<[f32; 2]> {
        self.points.get(&site).copied()
    }

    pub(super) fn waiting_point(
        &self,
        graph: &super::walking_paths::WalkingPaths,
        point: [f32; 2],
    ) -> Option<Vec3> {
        let center = glam::Vec2::from_array(point) * crate::dynamic_world::METERS_PER_UNIT;
        graph
            .paths
            .iter()
            .filter(|path| !path.connector)
            .flat_map(|path| {
                let samples = (path.length / 2.).ceil().max(1.) as usize;
                (0..=samples)
                    .map(move |step| path.sample(path.length * step as f32 / samples as f32).0)
            })
            .filter(|candidate| candidate.truncate().distance(center) < 12.)
            .min_by(|a, b| {
                a.truncate()
                    .distance_squared(center)
                    .total_cmp(&b.truncate().distance_squared(center))
            })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn authored_bus_sites_have_sidewalks_and_directed_routes() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let mut hermes = super::super::Hermes::load_with_game_data(&path, None).unwrap();
        for site in [SiteId::BusStop1, SiteId::BusStop2, SiteId::BusStop3] {
            let point = hermes.mission_site(site).expect(site.map_name());
            let waiting = hermes
                .mission_waiting_point(site)
                .expect("mission passenger needs a sidewalk");
            let map_waiting = waiting.truncate() / crate::dynamic_world::METERS_PER_UNIT;
            assert!(map_waiting.distance(glam::Vec2::from_array(point)) < 0.12);
        }
        let mut from = [1.1318498, -0.94828856, 0.102];
        for site in [SiteId::BusStop1, SiteId::BusStop2, SiteId::BusStop3] {
            assert!(hermes.select_mission_destination(site, from), "{:?}", site);
            let point = hermes.mission_site(site).unwrap();
            from = [point[0], point[1], 0.102];
        }
    }
}
