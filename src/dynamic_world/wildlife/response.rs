//! Contact reactions share the exact positions used to draw and pick each animal.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum Reaction {
    Roaming,
    Startled {
        direction: Vec2,
        speed: f32,
        elapsed: f32,
        offset: Vec3,
    },
    Down {
        pose: Sighting,
        elapsed: f32,
    },
}

impl Species {
    /// Gameplay envelopes in local metres, measured from the exported meshes.
    pub(super) fn body_radii(self) -> Vec3 {
        let dimensions = match self {
            Self::Squirrel => [0.19, 0.075, 0.17],
            Self::Pigeon => [0.15, 0.072, 0.10],
            Self::Crow => [0.24, 0.095, 0.14],
            Self::Gull => [0.25, 0.10, 0.15],
            Self::Mallard => [0.24, 0.12, 0.18],
            Self::Goose => [0.44, 0.20, 0.39],
            Self::Heron => [0.38, 0.16, 0.55],
            Self::Chickadee => [0.075, 0.038, 0.053],
            Self::Fox => [0.50, 0.13, 0.28],
            Self::Skunk => [0.34, 0.11, 0.25],
            Self::Cat => [0.245, 0.083, 0.17],
        };
        Vec3::from_array(dimensions)
    }
}
impl Home {
    pub(super) fn knock_over(&mut self, time: f32) {
        self.reaction = Reaction::Down {
            pose: self.sighting(time),
            elapsed: 0.,
        };
    }
    pub(super) fn is_downed(&self) -> bool {
        matches!(self.reaction, Reaction::Down { .. })
    }
    pub(super) fn is_active(&self) -> bool {
        matches!(self.reaction, Reaction::Roaming)
    }
    pub(super) fn bump(&mut self, direction: Vec2, speed: f32) {
        if self.is_active() {
            self.reaction = Reaction::Startled {
                direction,
                speed: (speed + 1.2).clamp(2., 5.),
                elapsed: 0.,
                offset: Vec3::ZERO,
            };
        }
    }
    pub(super) fn sighting(&self, time: f32) -> Sighting {
        match self.reaction {
            Reaction::Roaming => self.roaming(time),
            Reaction::Startled {
                direction,
                elapsed,
                offset,
                ..
            } => {
                let mut pose = self.roaming(time);
                pose.position += offset;
                let facing = if elapsed < 1.4 { direction } else { -direction };
                pose.heading = facing.y.atan2(facing.x);
                pose.clip = match self.species {
                    Species::Chickadee | Species::Gull => 1,
                    _ => 0,
                };
                pose.seconds = elapsed * 1.5;
                pose
            }
            Reaction::Down { mut pose, elapsed } => {
                let f = (elapsed / 0.30).min(1.);
                pose.roll = 1.52 * f * (2. - f);
                // Rest the side of the model on the ground after the fall.
                pose.position.z += self.species.body_radii().y * pose.roll.sin();
                pose
            }
        }
    }
    fn advance(&mut self, city: &City, time: f32, dt: f32, eye: Vec3) {
        match self.reaction {
            Reaction::Roaming => {}
            Reaction::Down { pose, elapsed } => {
                let elapsed = (elapsed + dt).min(40.);
                self.reaction = if elapsed >= 40.
                    && pose.position.distance_squared(eye)
                        > (self.species.view_distance() + 8.).powi(2)
                {
                    Reaction::Roaming
                } else {
                    Reaction::Down { pose, elapsed }
                };
            }
            Reaction::Startled {
                direction,
                speed,
                elapsed,
                mut offset,
            } => {
                let base = self.roaming(time).position;
                let elapsed = elapsed + dt;
                let travel = if elapsed < 0.85 {
                    speed * dt
                } else if elapsed < 1.4 {
                    0.
                } else {
                    -(1.5 * dt).min(offset.truncate().length())
                };
                let next = offset.truncate() + direction * travel;
                if elapsed >= 1.4 && next.length_squared() < 0.0001 {
                    self.reaction = Reaction::Roaming;
                    return;
                }
                let xy = base.truncate() + next;
                if let Some(s) = city
                    .surface_at(
                        (xy / METERS_PER_UNIT).to_array(),
                        (base.z + offset.z + 0.5) / METERS_PER_UNIT,
                    )
                    .and_then(|sample| self.species.foot_height(city, sample))
                {
                    let at = xy.extend(s);
                    let lo = (at + Vec3::new(-0.06, -0.06, 0.04)) / METERS_PER_UNIT;
                    let hi = (at + Vec3::new(0.06, 0.06, 0.25)) / METERS_PER_UNIT;
                    if (at.z - base.z - offset.z).abs() < 0.48
                        && !city.building_bounds_overlap(lo.into(), hi.into())
                    {
                        offset = at - base;
                    }
                }
                // A map edit or obstruction can cut off the return route. Resume at home out of view.
                if elapsed > 6.
                    && base.distance_squared(eye) > (self.species.view_distance() + 8.).powi(2)
                {
                    self.reaction = Reaction::Roaming;
                } else {
                    self.reaction = Reaction::Startled {
                        direction,
                        speed,
                        elapsed,
                        offset,
                    };
                }
            }
        }
    }
}
impl super::super::DynamicWorld {
    pub(in crate::dynamic_world) fn step_wildlife_reactions(&mut self, city: &City, dt: f32) {
        self.wildlife_time = (self.wildlife_time + dt).rem_euclid(24.);
        let eye = self.player_position();
        for home in &mut self.wildlife_homes {
            home.advance(city, self.wildlife_time, dt, eye);
        }
    }
    pub(in crate::dynamic_world) fn wildlife_shot_candidate(
        &self,
        origin: Vec3,
        direction: Vec3,
        range: f32,
    ) -> Option<(usize, f32)> {
        let eye = self.player_position();
        self.wildlife_homes
            .iter()
            .enumerate()
            .filter_map(|(id, h)| {
                if h.is_downed()
                    || h.ground[0].distance_squared(eye) >= h.species.view_distance().powi(2)
                {
                    return None;
                }
                let pose = h.sighting(self.wildlife_time);
                let radii = h.species.body_radii();
                let forward = Vec2::from_angle(pose.heading);
                let local = |p: Vec3| {
                    Vec3::new(
                        p.truncate().dot(forward),
                        p.truncate().dot(forward.perp()),
                        p.z,
                    ) / radii
                };
                let start = local(origin - pose.position - Vec3::Z * radii.z);
                let ray = local(direction);
                let len = ray.length_squared();
                if len <= f32::EPSILON {
                    return None;
                }
                let midpoint = -start.dot(ray) / len;
                let clearance = 1. - (start + ray * midpoint).length_squared();
                if clearance < 0. {
                    return None;
                }
                let half = (clearance / len).sqrt();
                let entry = (midpoint - half).max(0.);
                (midpoint + half >= 0. && entry < range).then_some((id, entry))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
    pub(in crate::dynamic_world) fn hit_wildlife(&mut self, id: usize) {
        let home = &mut self.wildlife_homes[id];
        let pose = home.sighting(self.wildlife_time);
        home.knock_over(self.wildlife_time);
        for other in &mut self.wildlife_homes {
            let delta = other.sighting(self.wildlife_time).position - pose.position;
            let planar = delta.truncate();
            if delta.length_squared() < 4. * 4. && planar.length_squared() > 0.0001 {
                other.bump(planar / planar.length(), 2.);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disturbed_animals_move_and_downed_animals_recover_offscreen() {
        let program =
            opencity::parse("land(-1,-1,1,-1,1,1,-1,1); wildlife_pigeon_test=map_point(0,0);")
                .unwrap();
        let city = City::from_program(&program, None).unwrap();
        let mut home = Home::read(&program, &city).unwrap().pop().unwrap();
        let eye = home.ground[0];
        home.bump(Vec2::X, 3.);
        for frame in 1..=12 {
            home.advance(&city, frame as f32 / 60., 1. / 60., eye);
        }
        let fleeing = home.sighting(0.2);
        assert!(fleeing.position.x - home.roaming(0.2).position.x > 0.5);
        for frame in 13..=360 {
            home.advance(&city, frame as f32 / 60., 1. / 60., eye);
        }
        assert!(home.is_active());

        let pose = home.sighting(6.);
        home.reaction = Reaction::Down { pose, elapsed: 0. };
        home.advance(&city, 6.3, 0.3, eye);
        assert!(home.sighting(6.3).roll > 1.5);
        home.advance(&city, 10., 45., eye);
        assert!(
            !home.is_active(),
            "a downed animal stays down while the player watches"
        );
        home.advance(&city, 10.1, 0.1, eye + Vec3::X * 100.);
        assert!(home.is_active());
    }
}
