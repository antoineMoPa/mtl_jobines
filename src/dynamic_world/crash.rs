//! Severe player impacts and the short recovery shot that follows them.
use super::{melee, player, DynamicWorld, Pedestrian, SpawnPoint, Vehicle, METERS_PER_UNIT};
use crate::hermes::Hermes;
use glam::{Vec2, Vec3};

const HOLD_SECONDS: f32 = 4.;
pub(super) const ROAD_IMPACT_MPS: f32 = 16.;
pub(super) const HARD_LANDING_MPS: f32 = 9.;

#[derive(Clone, Copy, Debug)]
pub struct Crash {
    pub origin: Vec3,
    pub age: f32,
    pub scale: f32,
}

impl Vehicle {
    pub(super) fn record_severe_contact(&mut self, point: Vec3, speed: f32, threshold: f32) {
        if speed >= threshold
            && self
                .severe_contact
                .is_none_or(|(_, previous)| speed > previous)
        {
            self.severe_contact = Some((point, speed));
        }
    }
}

impl DynamicWorld {
    pub fn crash(&self) -> Option<Crash> {
        self.crash
    }

    pub(super) fn begin_player_crash(&mut self) {
        let vehicle = &mut self.vehicles[self.player_vehicle.0];
        let Some((origin, _)) = vehicle.severe_contact.take() else {
            return;
        };
        if self.pedestrian.is_some() || vehicle.is_bicycle() || vehicle.is_boat() {
            return;
        }
        let scale = if vehicle.is_aircraft() {
            (vehicle.length() / 8.).clamp(1.7, 3.)
        } else {
            1.
        };
        self.crash = Some(Crash {
            origin,
            age: 0.,
            scale,
        });
        vehicle.velocity = Vec3::ZERO;
        vehicle.angular_momentum = Vec3::ZERO;
        vehicle.sound_running = false;
        self.entry = None;
        self.clear_chase();
    }

    pub(super) fn advance_crash(&mut self, hermes: &Hermes, dt: f32) {
        for hit in &mut self.sound_impacts {
            hit.age += dt;
        }
        let crash = self.crash.as_mut().expect("active crash");
        crash.age += dt;
        if crash.age < HOLD_SECONDS {
            return;
        }
        self.crash = None;
        // Use the authored parking footprint even when a CLI start or map jump moved the player.
        let lot = hermes.city().landscape.iter().find(|feature| {
            feature.name.as_deref() == Some("sphere_parking") && !feature.points.is_empty()
        });
        let start = lot
            .map(|lot| SpawnPoint {
                point: (lot
                    .points
                    .iter()
                    .copied()
                    .map(Vec2::from_array)
                    .sum::<Vec2>()
                    / lot.points.len() as f32)
                    .to_array(),
                heading: Some(std::f32::consts::PI),
            })
            .or(hermes.walking_start())
            .or(hermes.default_start())
            .unwrap_or(self.vehicle_spawns[self.player_vehicle.0]);
        let center = Vec2::from_array(start.point);
        self.props
            .update_focus((center * METERS_PER_UNIT).extend(0.));
        for ring in 0..=6 {
            for direction in 0..(if ring == 0 { 1 } else { 12 }) {
                let offset = Vec2::from_angle(direction as f32 * std::f32::consts::TAU / 12.)
                    * (ring as f32 * 2. / METERS_PER_UNIT);
                let point = SpawnPoint {
                    point: (center + offset).to_array(),
                    ..start
                };
                let Ok(person) = Pedestrian::at(hermes.city(), point) else {
                    continue;
                };
                if self.props.blocked(person.position)
                    || self
                        .vehicles
                        .iter()
                        .any(|v| player::blocked_by_vehicle(person.position, v))
                {
                    continue;
                }
                self.pedestrian = Some(person);
                self.walking_spawn = Some(point);
                self.health = melee::PLAYER_HEALTH;
                self.crowd.cancel_bites();
                self.exit_on_right = false;
                self.sound_geography_timer = 0.;
                self.update_riders();
                return;
            }
        }
        self.reset_player(hermes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{catalog, Controls, WalkControls};

    fn setup(id: &str) -> (Hermes, DynamicWorld) {
        let hermes = Hermes::test_map_from_source(
            "road street: carlane carlane; end street(-1,0,1,0);
            land(-2,-2,2,-2,2,2,-2,2); player_start(0,0,0);
            sphere_parking=concrete(0.45,0.45,0.55,0.45,0.55,0.55,0.45,0.55);
            building(0.2,0.1,-0.2,0.3,-0.2,0.3,0.2,0.1,0.2);",
        );
        let mut world = DynamicWorld::new(&hermes);
        assert!(world.switch_vehicle(
            catalog::definitions()
                .iter()
                .position(|d| d.config.research_id == id)
                .unwrap()
        ));
        (hermes, world)
    }

    #[test]
    fn car_wall_impact_holds_then_recovers_on_foot_at_the_julep() {
        for (speed, severe) in [(7., false), (26., true)] {
            let (hermes, mut world) = setup("vehicle-v2-01");
            world.vehicles[0].velocity = Vec3::X * speed;
            for _ in 0..240 {
                world.step(
                    &hermes,
                    Controls::default(),
                    WalkControls::default(),
                    1. / 120.,
                );
                if world.player().collisions > 0 {
                    break;
                }
            }
            assert!(world.player().collisions > 0);
            assert_eq!(world.crash().is_some(), severe);
            if severe {
                let wreck = world.player().position;
                assert!(!world.toggle_vehicle(&hermes));
                world.step(
                    &hermes,
                    Controls {
                        throttle: 1.,
                        ..Default::default()
                    },
                    WalkControls::default(),
                    3.9,
                );
                assert!(world.crash().is_some() && world.is_driving());
                assert_eq!(world.player().position, wreck);
                world.step(&hermes, Controls::default(), WalkControls::default(), 0.11);
                assert!(world.crash().is_none() && !world.is_driving());
                assert!(
                    world
                        .player_position()
                        .truncate()
                        .distance(Vec2::splat(50.))
                        < 2.
                );
                world.step(
                    &hermes,
                    Controls::default(),
                    WalkControls {
                        forward: 1.,
                        ..Default::default()
                    },
                    0.05,
                );
                assert!(world.player_position().x < 50.);
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn playable_map_crash_returns_to_clear_ground_in_the_orange_julep_lot() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let mut world = DynamicWorld::new(&hermes);
        let model = catalog::definitions()
            .iter()
            .position(|d| d.config.research_id == "vehicle-air-fleche")
            .unwrap();
        assert!(world.switch_vehicle(model));
        let plane = &mut world.vehicles[0];
        plane.position.x -= 200.;
        plane.record_severe_contact(plane.position, 20., HARD_LANDING_MPS);
        world.begin_player_crash();
        world.step(
            &hermes,
            Controls::default(),
            WalkControls::default(),
            HOLD_SECONDS,
        );
        let point = world
            .pedestrian
            .as_ref()
            .expect("on foot at the lot")
            .position;
        let lot = hermes
            .city()
            .landscape
            .iter()
            .find(|feature| feature.name.as_deref() == Some("sphere_parking"))
            .unwrap();
        let center = lot
            .points
            .iter()
            .copied()
            .map(Vec2::from_array)
            .sum::<Vec2>()
            / lot.points.len() as f32;
        assert!(point.truncate().distance(center * METERS_PER_UNIT) <= 12.1);
        assert!(!world.props.blocked(point));
        assert!(world
            .vehicles
            .iter()
            .all(|v| !player::blocked_by_vehicle(point, v)));
    }

    #[test]
    fn aircraft_needs_a_hard_contact_to_trigger_recovery() {
        for (sink, severe) in [(2., false), (14., true)] {
            let (hermes, mut world) = setup("vehicle-air-fleche");
            let plane = &mut world.vehicles[0];
            plane.position.x = -30.;
            plane.position.z += 0.6;
            plane.grounded = false;
            plane.velocity = Vec3::new(7., 0., -sink);
            for _ in 0..90 {
                world.step(
                    &hermes,
                    Controls::default(),
                    WalkControls::default(),
                    1. / 120.,
                );
                if world.player().grounded {
                    break;
                }
                assert!(world.crash().is_none());
            }
            assert!(world.player().grounded);
            assert_eq!(world.crash().is_some(), severe);
        }
    }
}
