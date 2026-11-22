//! The YUL apron tug and its permanently pinned, four-wheel baggage cart.
use super::Vehicle;
use glam::{Vec2, Vec3};

pub(super) const MODEL_ID: &str = "vehicle-airport-baggage-tractor";
const PIN_X: f32 = -1.79;
// Distance from the drawbar eye to the midpoint of the cart's axles, in metres.
const DRAWBAR: f32 = 2.32;

pub(super) fn hitch(tug: &Vehicle) -> Vec3 {
    Vec3::new(PIN_X, 0., 0.52 - tug.ride_height())
}

impl Vehicle {
    pub(super) fn has_baggage_trailer(&self) -> bool {
        self.definition()
            .is_some_and(|d| d.config.research_id == MODEL_ID)
    }

    pub(super) fn contact_reach(&self) -> f32 {
        if self.has_baggage_trailer() {
            -PIN_X + DRAWBAR + 1.7
        } else {
            self.length().hypot(self.width()) * 0.5
        }
    }

    fn hitch_xy(&self) -> Vec2 {
        (self.position + self.orientation() * hitch(self)).truncate()
    }

    pub(super) fn trailer_axle(&self) -> Vec2 {
        self.hitch_xy() - Vec2::from_angle(self.yaw + self.trailer_angle) * DRAWBAR
    }

    pub(super) fn follow_baggage_hitch(&mut self, previous_axle: Vec2) {
        let pull = self.hitch_xy() - previous_axle;
        let relative = pull.to_angle() - self.yaw;
        self.trailer_angle = relative.sin().atan2(relative.cos()).clamp(-1.3, 1.3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{Controls, DynamicWorld, SpawnPoint, WalkControls, METERS_PER_UNIT};
    use crate::hermes::Hermes;

    #[test]
    fn enter_the_airport_tug_drive_a_turn_and_reset_with_the_cart() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let map = Hermes::load_with_game_data(&path, None).unwrap();
        let surface = map
            .city()
            .surface_at([-4.335, -0.34], f32::INFINITY)
            .unwrap();
        assert_eq!(surface.kind, opencity::runtime::SurfaceKind::Sidewalk);
        assert!(!map
            .city()
            .building_collision([-4.335, -0.369], 0.009, 0.123, 0.146));
        assert!(map
            .city()
            .building_collision([-4.42, -0.369], 0.009, 0.123, 0.146));
        let start = SpawnPoint {
            point: [-4.36, -0.53],
            heading: Some(0.),
        };
        let mut world = DynamicWorld::with_start(&map, Some(start)).unwrap();
        let tug = world
            .vehicles
            .iter()
            .find(|v| v.has_baggage_trailer())
            .unwrap();
        let parked = tug.position;
        let point = (parked.truncate() + Vec2::Y * 1.7) / METERS_PER_UNIT;
        world.pedestrian = Some(
            super::super::player::Pedestrian::at(
                map.city(),
                SpawnPoint {
                    point: point.to_array(),
                    heading: Some(0.),
                },
            )
            .unwrap(),
        );
        assert!(world.toggle_vehicle(&map));
        assert!(world.player().has_baggage_trailer());
        let mut swung = false;
        for i in 0..180 {
            world.step(
                &map,
                Controls {
                    throttle: 1.,
                    steering: if i > 75 { 0.65 } else { 0. },
                    ..Default::default()
                },
                WalkControls::default(),
                1. / 60.,
            );
            swung |= world.player().trailer_angle.abs() > 0.06;
        }
        assert!(world.player().position.distance(parked) > 4.);
        assert!(swung);
        world.reset_player(&map);
        assert!(world.player().position.distance(parked) < 0.002);
        assert_eq!(world.player().trailer_angle, 0.);
    }
}
