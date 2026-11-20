//! Contact and posing share the same simulation clock, in seconds.
use super::pedestrian_catalog::{Definition, Transform};
use glam::{EulerRot, Quat};

pub(super) const PLAYER_HEALTH: u8 = 100;
pub(super) const ZOMBIE_HEALTH: u8 = 3;
pub(super) const BITE_DAMAGE: u8 = 25;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Attack {
    Punch,
    Bite,
}

impl Attack {
    fn contact(self) -> f32 {
        match self {
            Self::Punch => 18. / 60.,
            Self::Bite => 32. / 60.,
        }
    }

    fn duration(self) -> f32 {
        match self {
            Self::Punch => 54. / 60.,
            Self::Bite => 85. / 60.,
        }
    }
}

#[derive(Default, Clone, Copy)]
pub(super) struct Melee {
    swing: Option<(Attack, f32)>,
    flinch: f32,
}

impl Melee {
    pub fn active(&self) -> bool {
        self.swing.is_some()
    }

    pub fn start(&mut self, attack: Attack) -> bool {
        if self.active() || self.flinch > 0. {
            return false;
        }
        self.swing = Some((attack, 0.));
        true
    }

    pub fn hurt(&mut self) {
        self.swing = None;
        self.flinch = 0.32;
    }

    pub fn advance(&mut self, dt: f32) -> bool {
        self.flinch = (self.flinch - dt).max(0.);
        let Some((attack, time)) = self.swing else {
            return false;
        };
        let next = time + dt;
        self.swing = (next < attack.duration()).then_some((attack, next));
        time < attack.contact() && next >= attack.contact()
    }

    pub fn pose(&self, model: &Definition, locals: &mut [Transform]) {
        if self.flinch > 0. {
            let recoil = sample(0.32 - self.flinch, &[(0., 0.), (0.07, 1.), (0.32, 0.)]);
            locals[model.node("Chest")].rotation *= Quat::from_rotation_x(-0.32 * recoil);
            locals[model.node("Neck")].rotation *= Quat::from_rotation_z(0.20 * recoil);
        }
        let Some((attack, time)) = self.swing else {
            return;
        };
        let frame = time * 60.;
        let blend = (frame / 6.).min(1.) * ((attack.duration() * 60. - frame) / 10.).clamp(0., 1.);
        let bite = attack == Attack::Bite;
        let drive = if bite {
            sample(
                frame,
                &[
                    (0., 0.),
                    (19., -0.22),
                    (23., -0.18),
                    (32., 1.),
                    (38., 0.96),
                    (47., 1.),
                    (80., 0.),
                ],
            )
        } else {
            sample(
                frame,
                &[
                    (0., 0.),
                    (10., -0.24),
                    (18., 1.),
                    (21., 0.97),
                    (30., 0.32),
                    (48., 0.),
                ],
            )
        };
        let hips = &mut locals[model.node("Hips")];
        hips.translation.z += drive * if bite { 0.32 } else { 0.11 };
        hips.translation.y -= drive.max(0.) * if bite { 0.11 } else { 0.024 };
        let twist = if bite { 0.29 } else { 0.65 } * drive;
        locals[model.node("Spine")].rotation *= Quat::from_euler(
            EulerRot::YXZ,
            twist * 0.42,
            drive * if bite { 0.45 } else { 0.14 },
            0.,
        );
        locals[model.node("Chest")].rotation *= Quat::from_rotation_y(twist * 0.58);
        locals[model.node("Neck")].rotation *= Quat::from_euler(
            EulerRot::YXZ,
            -0.45 * twist,
            if bite { 0.38 * drive } else { 0. },
            if bite { 0.3 * drive } else { 0. },
        );
        for (side, sign) in [("R", -1.), ("L", 1.)] {
            let strike = if side == "R" { drive } else { 0. };
            let (pitch, yaw, roll, elbow) = if bite {
                (
                    -0.95 - 1.52 * drive,
                    sign * (-0.11 - 0.20 * drive),
                    sign * 0.21,
                    -0.41 - 0.11 * drive,
                )
            } else {
                (
                    -0.48 - 1.31 * strike,
                    sign * -0.18 - 0.37 * strike,
                    sign * (0.21 - 0.10 * strike),
                    -1.99 + 1.90 * strike,
                )
            };
            let arm = &mut locals[model.node(&format!("UpperArm_{side}"))];
            arm.rotation = arm
                .rotation
                .slerp(Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll), blend);
            let forearm = &mut locals[model.node(&format!("Forearm_{side}"))];
            forearm.rotation = forearm.rotation.slerp(Quat::from_rotation_x(elbow), blend);
        }
    }
}

fn sample(frame: f32, keys: &[(f32, f32)]) -> f32 {
    for pair in keys.windows(2) {
        let [(a, start), (b, end)] = [pair[0], pair[1]];
        if frame <= b {
            let t = ((frame - a) / (b - a)).clamp(0., 1.);
            return start + (end - start) * t * t * (3. - 2. * t);
        }
    }
    keys.last().unwrap().1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic_world::{
        player::{Pedestrian, HALF_HEIGHT},
        Controls, DynamicWorld, WalkControls,
    };
    use crate::hermes::Hermes;
    use glam::{Vec2, Vec3};

    fn encounter(h: &Hermes) -> DynamicWorld {
        let mut w = DynamicWorld::new(h);
        let target = w.prepare_player_pedestrian_collision(h);
        let yaw = w.player_yaw();
        let forward = Vec2::from_angle(yaw).extend(0.);
        w.pedestrian = Some(Pedestrian::new(
            h.city(),
            target + forward * 0.83 + Vec3::Z * HALF_HEIGHT,
            yaw + std::f32::consts::PI,
        ));
        w.set_zombie_mode(h.city(), true);
        w
    }

    fn walk(w: &mut DynamicWorld, h: &Hermes, frames: usize, forward: f32) {
        for _ in 0..frames {
            w.step(
                h,
                Controls::default(),
                WalkControls {
                    forward,
                    ..Default::default()
                },
                1. / 60.,
            );
        }
    }

    #[test]
    fn zombie_punch_contacts_once_interrupts_and_knocks_out() {
        let h = Hermes::test_map();
        let mut w = encounter(&h);
        for health in (1..=ZOMBIE_HEALTH).rev() {
            assert!(w.start_punch());
            assert!(!w.start_punch());
            walk(&mut w, &h, 17, 0.);
            assert_eq!(
                w.zombie_health_meters().next().unwrap().1,
                health as f32 / 3.
            );
            walk(&mut w, &h, 38, 0.);
        }
        assert_eq!(w.pedestrian_reactions()[1], 1);
        assert_eq!(w.player_health(), PLAYER_HEALTH);
    }

    #[test]
    fn zombie_bite_can_be_evaded_with_regular_movement_and_can_kill() {
        let h = Hermes::test_map();
        let mut w = encounter(&h);
        walk(&mut w, &h, 12, 0.);
        walk(&mut w, &h, 30, -1.);
        assert_eq!(w.player_health(), PLAYER_HEALTH);
        let mut w = encounter(&h);
        walk(&mut w, &h, 31, 0.);
        assert_eq!(w.player_health(), PLAYER_HEALTH);
        walk(&mut w, &h, 12, 0.);
        assert_eq!(w.player_health(), PLAYER_HEALTH - BITE_DAMAGE);
        walk(&mut w, &h, 300, 0.);
        assert_eq!(w.player_health(), 0);
        assert!(w.pedestrian.as_ref().unwrap().fallen.is_some());
        assert!(!w.start_punch() && !w.can_equip_weapon() && !w.toggle_vehicle(&h));
        w.reset_player(&h);
        assert!(w.zombie_mode());
        assert_eq!(w.player_health(), PLAYER_HEALTH);
    }

    #[test]
    fn zombie_mission_exit_cancels_damage_and_revives_fallen_player() {
        let h = Hermes::test_map();
        let mut w = encounter(&h);
        walk(&mut w, &h, 330, 0.);
        assert_eq!(w.player_health(), 0);
        w.set_zombie_mode(h.city(), false);
        assert!(w.start_punch());
        walk(&mut w, &h, 100, 0.);
        assert_eq!(w.player_health(), PLAYER_HEALTH);
        assert_eq!(w.pedestrian_reactions()[1], 0);
        assert!(w.pedestrian.as_ref().unwrap().fallen.is_none());
    }
}
