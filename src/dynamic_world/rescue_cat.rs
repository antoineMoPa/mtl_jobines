//! The job cat stays at its marked block even when ambient wildlife despawns.
use super::{
    wildlife::{CatCoat, Sighting, Species},
    DynamicWorld,
};
use glam::{Vec2, Vec3};

const LIFT_SECONDS: f32 = 0.5;

#[derive(Clone, Copy)]
enum Pose {
    Waiting,
    Carried { from: Vec3 },
    Returned { from: Vec3 },
}

pub struct RescueCat {
    street: Vec3,
    doorstep: Vec3,
    pose: Pose,
    elapsed: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CatAction {
    Catch,
    Return,
}

impl CatAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Catch => "Attraper le chat",
            Self::Return => "Rendre le chat",
        }
    }
}

impl RescueCat {
    pub fn new(street: Vec3, doorstep: Vec3) -> Self {
        Self {
            street,
            doorstep,
            pose: Pose::Waiting,
            elapsed: 0.,
        }
    }
    pub(super) fn step(&mut self, dt: f32) {
        self.elapsed += dt;
    }
    fn waiting_position(&self) -> Vec3 {
        self.street + (Vec2::from_angle(self.elapsed * 0.65) * 0.65).extend(0.)
    }
    fn lift(&self, from: Vec3, to: Vec3) -> Vec3 {
        let t = (self.elapsed / LIFT_SECONDS).min(1.);
        from.lerp(to, t * t * (3. - 2. * t)) + Vec3::Z * (0.22 * (t * std::f32::consts::PI).sin())
    }
}

impl DynamicWorld {
    pub(super) fn cat_grip(&self) -> Option<(Vec3, f32)> {
        let cat = self.rescue_cat.as_ref()?;
        match cat.pose {
            Pose::Waiting => return None,
            Pose::Returned { .. } if cat.elapsed >= LIFT_SECONDS => return None,
            _ => {}
        }
        let phase = (cat.elapsed / LIFT_SECONDS).min(1.);
        let crouch = (phase * std::f32::consts::PI).sin().max(0.);
        Some((self.rescue_cat_sighting()?.position, crouch))
    }
    pub fn cat_marker(&self) -> Option<Vec3> {
        let cat = self.rescue_cat.as_ref()?;
        match cat.pose {
            Pose::Waiting => Some(cat.waiting_position()),
            Pose::Carried { .. } => Some(cat.doorstep),
            Pose::Returned { .. } => None,
        }
    }

    pub fn cat_action(&self) -> Option<CatAction> {
        let player = self.pedestrian.as_ref()?;
        if self.metro.aboard
            || player.swimming
            || player.fallen.is_some()
            || self.player_health() == 0
            || self.arrest_progress().is_some()
        {
            return None;
        }
        let cat = self.rescue_cat.as_ref()?;
        let (action, point) = match cat.pose {
            Pose::Waiting => (CatAction::Catch, cat.waiting_position()),
            Pose::Carried { .. } if cat.elapsed >= LIFT_SECONDS => {
                (CatAction::Return, cat.doorstep)
            }
            _ => return None,
        };
        ((player.position - Vec3::Z * super::player::HALF_HEIGHT).distance(point) <= 2.)
            .then_some(action)
    }

    pub fn interact_with_cat(&mut self) -> Option<CatAction> {
        let action = self.cat_action()?;
        let from = self.rescue_cat_sighting()?.position;
        let cat = self.rescue_cat.as_mut()?;
        cat.pose = match action {
            CatAction::Catch => Pose::Carried { from },
            CatAction::Return => Pose::Returned { from },
        };
        cat.elapsed = 0.;
        Some(action)
    }

    pub(super) fn rescue_cat_sighting(&self) -> Option<Sighting> {
        let cat = self.rescue_cat.as_ref()?;
        let yaw = self.player_yaw();
        let (position, heading, clip) = match cat.pose {
            Pose::Waiting => (
                cat.waiting_position(),
                cat.elapsed * 0.65 + std::f32::consts::FRAC_PI_2,
                0,
            ),
            Pose::Carried { from } if !self.is_driving() => {
                let chest = self.player_position() + (Vec2::from_angle(yaw) * 0.32).extend(0.13);
                (cat.lift(from, chest), yaw + std::f32::consts::FRAC_PI_2, 1)
            }
            Pose::Returned { from } => (cat.lift(from, cat.doorstep), 0., 1),
            Pose::Carried { .. } => return None,
        };
        Some(Sighting {
            species: Species::Cat,
            cat_coat: Some(CatCoat::White),
            position,
            heading,
            roll: 0.,
            clip,
            seconds: cat.elapsed,
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::{
        dynamic_world::{player::Pedestrian, SpawnPoint, METERS_PER_UNIT},
        hermes::Hermes,
        kairos::SiteId,
    };

    #[test]
    fn cat_job_sites_support_walking_routing_pickup_and_handoff() {
        let path = std::path::Path::new(env!("MTL_WORKSPACE_DIR")).join("data/maps/mtl/map.road");
        let mut hermes = Hermes::load_with_game_data(&path, None).unwrap();
        let mut world = DynamicWorld::new(&hermes);
        let mut origin = (world.player_position() / METERS_PER_UNIT).to_array();
        let mut walkers = [SiteId::MissingCat, SiteId::CatDuplex]
            .map(|site| {
                let point = hermes.mission_site(site).unwrap();
                let p = Pedestrian::at(
                    hermes.city(),
                    SpawnPoint {
                        point,
                        heading: None,
                    },
                )
                .unwrap();
                assert!(hermes.select_mission_destination(site, origin));
                origin = (p.position / METERS_PER_UNIT).to_array();
                p
            })
            .into_iter();
        let finder = walkers.next().unwrap();
        let owner = walkers.next().unwrap();
        world.rescue_cat = Some(RescueCat::new(
            finder.position - Vec3::Z * super::super::player::HALF_HEIGHT,
            owner.position - Vec3::Z * super::super::player::HALF_HEIGHT,
        ));
        world.pedestrian = None;
        assert_eq!(world.cat_action(), None);
        world.pedestrian = Some(finder);
        assert_eq!(world.interact_with_cat(), Some(CatAction::Catch));
        let start = world.rescue_cat_sighting().unwrap().position;
        assert_eq!(world.interact_with_cat(), None);
        world.rescue_cat.as_mut().unwrap().step(LIFT_SECONDS);
        assert!(world.rescue_cat_sighting().unwrap().position.z > start.z + 0.8);
        world.pedestrian = Some(owner);
        assert_eq!(world.interact_with_cat(), Some(CatAction::Return));
        assert_eq!(world.cat_action(), None);
    }
}
