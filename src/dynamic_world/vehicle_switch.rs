//! Replace the controlled vehicle without changing its slot, route or reset point.
use super::{catalog, Controls, DynamicWorld};

impl DynamicWorld {
    pub(crate) fn switch_vehicle(&mut self, model: usize) -> bool {
        if catalog::definitions().get(model).is_none() {
            return false;
        }
        let index = self.player_vehicle.0;
        self.traffic.release(index);
        let vehicle = &mut self.vehicles[index];
        // Adjust chassis height through the shared model setter so tires stay on
        // the same ground, including bridges. Keep heading and driving momentum.
        vehicle.set_catalog_model(Some(model));
        vehicle.sound_controls = Controls::default();
        self.pedestrian = None;
        self.vehicle_notice = 4.;
        self.sound_geography_timer = 0.;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hermes::Hermes;
    use glam::Vec3;

    #[test]
    fn every_vehicle_switch_preserves_ground_heading_motion_and_reset() {
        let hermes = Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        let index = world.player_vehicle.0;
        let count = world.vehicles.len();
        let spawn = world.vehicle_spawns[index];
        let ground = world.player().position.z - world.player().ride_height();
        let position = world.player().position.truncate();
        let yaw = world.player().yaw;
        world.vehicles[index].velocity = Vec3::new(4., 2., 0.);
        for (model, definition) in catalog::definitions().iter().enumerate() {
            assert!(world.switch_vehicle(model));
            assert_eq!(world.player().definition, Some(model));
            assert_eq!(world.player().position.truncate(), position);
            assert!(
                (world.player().position.z - world.player().ride_height() - ground).abs() < 1e-4
            );
            assert_eq!(world.player().yaw, yaw);
            assert_eq!(world.player().velocity, Vec3::new(4., 2., 0.));
            assert_eq!(
                world.vehicle_name_notice(),
                Some(definition.config.name.as_str())
            );
            assert_eq!(world.vehicles.len(), count);
            assert_eq!(world.vehicle_spawns[index], spawn);
        }
        let selected = world.player().definition;
        world.reset_player(&hermes);
        assert_eq!(world.player().definition, selected);
        assert_eq!(world.player_vehicle.0, index);
    }

    #[test]
    fn switching_while_walking_enters_current_car_and_rejects_invalid_models() {
        let hermes = Hermes::test_map();
        let mut world = DynamicWorld::new(&hermes);
        assert!(world.toggle_vehicle(&hermes));
        let position = world.player_position();
        let model = world.player().definition;
        assert!(!world.switch_vehicle(usize::MAX));
        assert!(!world.is_driving());
        assert_eq!(world.player_position(), position);
        assert_eq!(world.player().definition, model);
        let index = world.player_vehicle;
        assert!(world.switch_vehicle(1));
        assert!(world.is_driving());
        assert_eq!(world.player_vehicle, index);
        assert_eq!(world.player().definition, Some(1));
    }
}
