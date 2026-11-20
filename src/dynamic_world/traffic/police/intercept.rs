//! Short arcade driving plays. The normal vehicle solver owns all movement and contact.
use crate::dynamic_world::{Controls, Vehicle, METERS_PER_UNIT};
use glam::Vec2;
use opencity::runtime::{City, SurfaceKind};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Play {
    #[default]
    Block,
    Bump,
}

#[derive(Default)]
pub(super) struct Intercept {
    pub(super) suspect: Option<usize>,
    play: Play,
    side: f32,
    active: f32,
    committed: bool,
    rest: f32,
}

impl Intercept {
    fn finish(&mut self) {
        self.play = match self.play {
            Play::Block => Play::Bump,
            Play::Bump => Play::Block,
        };
        self.active = 0.;
        self.committed = false;
        self.side = 0.;
        self.rest = 2.5;
    }

    pub(super) fn contact(&mut self) {
        if self.active > 0. {
            self.finish();
        }
    }

    pub(super) fn controls(
        &mut self,
        city: &City,
        cars: &[Vehicle],
        unit: usize,
        dt: f32,
    ) -> Option<Controls> {
        self.rest = (self.rest - dt).max(0.);
        let Some(suspect) = self.suspect else {
            if self.active > 0. {
                self.finish();
            }
            return None;
        };
        let car = &cars[unit];
        let player = &cars[suspect];
        let f = Vec2::from_angle(player.yaw);
        let side = f.perp();
        let relative = (car.position - player.position).truncate();
        let speed = player.velocity.truncate().dot(f);
        if self.rest > 0.
            || relative.length() > 44.
            || speed < 3.
            || (car.yaw - player.yaw).cos() < 0.55
            || (car.position.z - player.position.z).abs() > 2.
        {
            if self.active > 0. {
                self.finish();
            }
            return None;
        }
        let ahead = relative.dot(f);
        let across = relative.dot(side);
        let spacing = (car.width() + player.width()) * 0.5 + 1.2;
        let on_road = |p: Vec2| {
            city.surface_at(
                (p / METERS_PER_UNIT).to_array(),
                (car.position.z + 1.) / METERS_PER_UNIT,
            )
            .is_some_and(|s| {
                matches!(s.kind, SurfaceKind::Asphalt | SurfaceKind::Marking)
                    && (s.height * METERS_PER_UNIT - car.position.z + car.ride_height()).abs() < 1.5
            })
        };
        if self.active == 0. {
            let preferred = if across < -0.1 { -1. } else { 1. };
            self.side = [preferred, -preferred]
                .into_iter()
                .find(|s| on_road(player.position.truncate() + side * (s * spacing)))?;
        }
        self.active += dt;
        if self.active > 9. {
            self.finish();
            return None;
        }

        let lengths = (car.length() + player.length()) * 0.5;
        self.committed |= match self.play {
            Play::Block => ahead > lengths + 2.,
            Play::Bump => (-lengths..0.).contains(&ahead) && across.abs() > spacing * 0.45,
        };
        let (offset, limit) = match self.play {
            Play::Block if self.committed => (0., (speed - 5.).max(0.)),
            Play::Bump if self.committed => (0., speed + 3.),
            Play::Bump if ahead > lengths => {
                self.finish();
                return None;
            }
            _ => (self.side * spacing, (speed + 9.).max(20.).min(40.)),
        };
        let look = (car.velocity.length() * 0.48).clamp(4., 13.);
        let point = player.position.truncate() + f * (ahead + look) + side * offset;
        let from = car.position.truncate();
        if !on_road(from.lerp(point, 0.5)) || !on_road(point) {
            self.finish();
            return None;
        }
        let delta = point - from;
        let heading = Vec2::from_angle(car.yaw);
        let angle = heading.perp_dot(delta).atan2(heading.dot(delta));
        let limit = if cars.iter().enumerate().any(|(id, other)| {
            if id == unit || id == suspect || (other.position.z - car.position.z).abs() > 2. {
                return false;
            }
            let gap = other.position.truncate() - from;
            let t = gap.dot(delta) / delta.length_squared().max(0.01);
            (0. ..1.).contains(&t)
                && (gap - t * delta).length() < (car.width() + other.width()) * 0.5 + 0.4
        }) {
            0.
        } else {
            limit
        };
        Some(Controls {
            steering: (2.4 * angle).clamp(-1., 1.),
            throttle: ((limit - car.velocity.length()) * 0.5).clamp(0., 1.),
            brake: car.velocity.length() > limit + 0.4 || limit == 0.,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{catalog, contacts, SpawnPoint};
    use crate::hermes::Hermes;
    use glam::Vec3;

    #[test]
    fn cruiser_completes_a_moving_block_and_a_physical_side_contact() {
        let map = Hermes::test_map_from_source("road avenue: sidewalk carlane carlane carlane middle carlane carlane carlane sidewalk; end
            avenue(-4,0,4,0); land(-5,-2,5,-2,5,2,-5,2);");
        for play in [Play::Block, Play::Bump] {
            let mut ai = Intercept {
                suspect: Some(0),
                play,
                ..Default::default()
            };
            let mut cars: Vec<_> = [("vehicle-v2-09", 0.), ("vehicle-j06", -14.)]
                .into_iter()
                .map(|(id, x)| {
                    let mut car = Vehicle::spawn_at(
                        map.city(),
                        SpawnPoint {
                            point: [x / METERS_PER_UNIT, -0.04],
                            heading: Some(0.),
                        },
                    )
                    .unwrap();
                    car.set_catalog_model(
                        catalog::definitions()
                            .iter()
                            .position(|m| m.config.research_id == id),
                    );
                    car.velocity = Vec3::X * 8.;
                    car
                })
                .collect();
            let mut succeeded = false;
            for _ in 0..1080 {
                let input = ai
                    .controls(map.city(), &cars, 1, 1. / 120.)
                    .unwrap_or_default();
                let gas = (8. - cars[0].velocity.x).clamp(0., 1.);
                cars[0].step(
                    map.city(),
                    Controls {
                        throttle: gas,
                        ..Default::default()
                    },
                    1. / 120.,
                );
                cars[1].step(map.city(), input, 1. / 120.);
                let gap = cars[1].position - cars[0].position;
                let contact = !contacts::resolve_with_contacts(&mut cars).is_empty();
                succeeded = match play {
                    Play::Block => gap.x > 4. && gap.y.abs() < 1.5 && input.brake,
                    Play::Bump => contact && gap.x < 0. && gap.y.abs() > 0.8,
                };
                if succeeded {
                    break;
                }
            }
            assert!(
                succeeded,
                "{play:?} failed, final offset {:?}",
                cars[1].position - cars[0].position
            );
        }
    }
}
