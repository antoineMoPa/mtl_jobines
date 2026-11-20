//! On-foot loadout. Screen picking uses the displayed chase camera projection.
use super::*;
use crate::engine::impact::{Contact, Effects};
use crate::engine::{
    muzzle::{Equipment, FlashStyle},
    render::ImpactVertex,
};
use crate::hermes::external_models;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Weapon {
    Rifle,
    Handgun,
}
impl Weapon {
    fn slot(self) -> usize {
        self as usize
    }
    fn capacity(self) -> u8 {
        match self {
            Self::Rifle => 12,
            Self::Handgun => 7,
        }
    }
    fn bytes(self) -> &'static [u8] {
        match self {
            Self::Rifle => include_bytes!("../../../assets/fps/fps-rifle.glb"),
            Self::Handgun => include_bytes!("../../../assets/fps/fps-handgun.glb"),
        }
    }
    pub(super) fn model(self) -> Mesh {
        let mut mesh =
            external_models::decode_equipment(self.bytes()).expect("packaged weapon GLB");
        let (lo, hi) = external_models::bounds(&mesh);
        let size = hi - lo;
        let length = match self {
            Self::Rifle => 0.92,
            Self::Handgun => 0.24,
        };
        let grip = lo + size * Vec3::new(0.24, 0.5, 0.33);
        for vertex in &mut mesh.vertices {
            vertex.position = ((Vec3::from_array(vertex.position) - grip)
                * (length / size.x / METERS_PER_UNIT))
                .to_array();
        }
        mesh
    }
}

pub(super) struct Loadout {
    pub equipped: Option<Weapon>,
    rounds: [u8; 2],
    reserve: [u8; 2],
    ready_at: f32,
    reload_at: Option<f32>,
    pub projection: Option<Mat4>,
    models: [Option<Equipment>; 2],
    pub(super) effects: Effects,
}

impl Default for Loadout {
    fn default() -> Self {
        Self {
            equipped: None,
            rounds: [12, 7],
            reserve: [36, 21],
            ready_at: 0.,
            reload_at: None,
            projection: None,
            models: [None, None],
            effects: Default::default(),
        }
    }
}

impl Loadout {
    pub fn equip(&mut self, weapon: Option<Weapon>) {
        if self.equipped != weapon {
            self.equipped = weapon;
            self.projection = None;
            self.effects.muzzle.clear();
            self.reload_at = None;
        }
    }
    pub fn reload(&mut self, time: f32) {
        let Some(w) = self.equipped else { return };
        if self.reload_at.is_none()
            && self.rounds[w.slot()] < w.capacity()
            && self.reserve[w.slot()] > 0
        {
            self.reload_at = Some(time + 0.85);
        }
    }
    pub fn advance(&mut self, time: f32) {
        if let Some(w) = self.equipped {
            if self.reload_at.is_some_and(|end| time >= end) {
                let i = w.slot();
                let transferred = (w.capacity() - self.rounds[i]).min(self.reserve[i]);
                self.rounds[i] += transferred;
                self.reserve[i] -= transferred;
                self.reload_at = None;
            }
        }
    }
    fn try_fire(&mut self, time: f32, unlimited: bool) -> bool {
        let Some(w) = self.equipped else { return false };
        if time < self.ready_at {
            return false;
        }
        if !unlimited {
            if self.reload_at.is_some() || self.rounds[w.slot()] == 0 {
                return false;
            }
            self.rounds[w.slot()] -= 1;
        }
        self.effects.muzzle.ignite(match w {
            Weapon::Rifle => FlashStyle::Rifle,
            Weapon::Handgun => FlashStyle::Handgun,
        });
        self.ready_at = time
            + match w {
                Weapon::Rifle => 0.17,
                Weapon::Handgun => 0.22,
            };
        true
    }
    pub fn visible(
        &mut self,
        renderer: &Renderer,
        world: &DynamicWorld,
        target: Vec3,
        camera: Quat,
        sprites: &mut Vec<ImpactVertex>,
    ) -> Option<&GpuMesh> {
        let weapon = self.equipped?;
        let hand = world.weapon_hand()?;
        let model = self.models[weapon.slot()]
            .get_or_insert_with(|| Equipment::upload(renderer, &weapon.model()));
        let pose = weapon_pose(hand, target);
        model.place(renderer, pose, camera, &self.effects.muzzle, sprites);
        Some(&model.gpu)
    }
}

pub(super) fn weapon_pose(hand: Vec3, target: Vec3) -> Mat4 {
    let delta = target - hand;
    let rotation = Quat::from_rotation_z(delta.y.atan2(delta.x))
        * Quat::from_rotation_y(-delta.z.atan2(delta.truncate().length()));
    Mat4::from_rotation_translation(rotation, hand / METERS_PER_UNIT)
}

pub(super) fn sight_line(inverse: Mat4) -> Option<(Vec3, Vec3)> {
    let near = inverse.project_point3(Vec3::ZERO) * METERS_PER_UNIT;
    let far = inverse.project_point3(Vec3::Z * 0.8) * METERS_PER_UNIT;
    let delta = far - near;
    let length = delta.length();
    (length > 0. && length.is_finite()).then(|| (near, near + delta * (80. / length)))
}

impl Game {
    pub(super) fn fire_weapon(&mut self) {
        if self.simulation_paused() || !self.world.can_equip_weapon() || self.orbit_drag.is_some() {
            return;
        }
        let Some((eye, end)) = self.loadout.projection.and_then(sight_line) else {
            return;
        };
        let Some(hand) = self.world.weapon_hand() else {
            return;
        };
        if !self
            .loadout
            .try_fire(self.game_time, self.world.zombie_mode())
        {
            return;
        }
        self.world
            .report_crime(crate::dynamic_world::Crime::Gunshot);
        let city = self.hermes.city();
        let target = self.world.weapon_aim_point(city, eye, end);
        let damage = if self.loadout.equipped == Some(Weapon::Rifle) {
            2
        } else {
            1
        };
        if let Some(hit) = self.world.fire_weapon_at(city, hand, target, damage) {
            let ground = city
                .surface_at(
                    [hit.point.x / METERS_PER_UNIT, hit.point.y / METERS_PER_UNIT],
                    (hit.point.z + 0.02) / METERS_PER_UNIT,
                )
                .map_or(hit.point.z - 2., |s| s.height * METERS_PER_UNIT);
            self.loadout.effects.strike(
                Contact {
                    mark_owner: hit.stationary.then_some(0),
                    point: hit.point,
                    normal: hit.normal,
                    local_min: Vec3::splat(f32::NEG_INFINITY),
                    local_max: Vec3::splat(f32::INFINITY),
                    color: match hit.surface {
                        crate::dynamic_world::ShotSurface::Wood => Vec3::new(0.5, 0.3, 0.12),
                        crate::dynamic_world::ShotSurface::Asphalt => Vec3::splat(0.14),
                        _ => Vec3::splat(0.47),
                    },
                    material: hit.surface,
                },
                None,
                if damage == 2 { 1. } else { 0.73 },
                ground,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn police_report_requires_a_successful_normal_play_shot() {
        use crate::dynamic_world::{ChaseStatus, Crime};
        for zombies in [false, true] {
            let mut game = Game::new(Hermes::test_map());
            game.set_overlay(Overlay::Driving);
            assert!(game.world.toggle_vehicle(&game.hermes));
            game.world.set_zombie_mode(game.hermes.city(), zombies);
            game.loadout.equip(Some(Weapon::Handgun));
            game.loadout.projection = Some(Mat4::IDENTITY);
            game.loadout.ready_at = 1.;
            game.fire_weapon();
            assert_eq!(game.world.chase_status(), ChaseStatus::Clear);
            game.game_time = 2.;
            game.fire_weapon();
            assert_eq!(
                game.world.chase_status(),
                if zombies {
                    ChaseStatus::Clear
                } else {
                    ChaseStatus::Reported(Crime::Gunshot)
                }
            );
        }
    }

    #[test]
    fn weapons_have_valid_models_and_separate_ammunition() {
        let mut loadout = Loadout::default();
        for weapon in [Weapon::Rifle, Weapon::Handgun] {
            let mesh = weapon.model();
            assert!(mesh.vertices.len() > 300);
            assert!(mesh
                .vertices
                .iter()
                .all(|v| v.position.iter().all(|x| x.is_finite())));
        }
        loadout.equip(Some(Weapon::Rifle));
        assert!(loadout.try_fire(0., false));
        assert!(!loadout.try_fire(0.01, false));
        loadout.reload(0.2);
        loadout.equip(Some(Weapon::Handgun));
        loadout.advance(1.2);
        assert_eq!(loadout.rounds, [11, 7]);
        assert_eq!(loadout.reserve, [36, 21]);
        loadout.equip(Some(Weapon::Rifle));
        loadout.reload(1.2);
        loadout.advance(2.2);
        assert_eq!(loadout.rounds, [12, 7]);
        assert_eq!(loadout.reserve, [35, 21]);
    }

    #[test]
    fn zombie_mode_fires_both_empty_weapons_at_their_usual_cadence() {
        let mut loadout = Loadout::default();
        loadout.rounds = [0; 2];
        loadout.reserve = [0; 2];
        let mut time = 0.;
        for weapon in [Weapon::Handgun, Weapon::Rifle] {
            loadout.equip(Some(weapon));
            for _ in 0..100 {
                assert!(loadout.try_fire(time, true));
                assert!(!loadout.try_fire(time + 0.01, true));
                time += 0.25;
            }
            assert!(!loadout.try_fire(time, false));
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
#[ignore = "requires a native GPU adapter"]
fn equipped_weapon_renders_in_the_player_chase_view() {
    let mut game = Game::new(Hermes::test_map());
    assert!(game.world.toggle_vehicle(&game.hermes));
    game.set_overlay(Overlay::Driving);
    let renderer = pollster::block_on(Renderer::new(None, 960, 640)).unwrap();
    let scenery = Scenery::new(&renderer, &mut game.hermes);
    let p = game.world.player_position() / METERS_PER_UNIT;
    let yaw = game.world.player_yaw();
    let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
    let eye = p + (-forward * 0.15 + Vec3::Z * 0.075) * game.zoom;
    let look = p + Vec3::Z * 0.014 + forward * 0.045;
    let target = (look + forward * 0.25) * METERS_PER_UNIT;
    game.world.aim_weapon(Some(target), true);
    let mut actors = VehicleScenery::new(&renderer);
    actors.update(&renderer, &game.world);
    let mut meshes = scenery.visible([p.x, p.y]);
    meshes.extend(actors.visible());
    let capture = |meshes: &[&GpuMesh]| {
        renderer
            .capture_png(|view| renderer.render_scene(view, meshes, eye, look, 10., None, None))
            .unwrap()
    };
    let bare = capture(&meshes);
    game.loadout.equip(Some(Weapon::Rifle));
    meshes.push(
        game.loadout
            .visible(
                &renderer,
                &game.world,
                target,
                Quat::IDENTITY,
                &mut Vec::new(),
            )
            .unwrap(),
    );
    let armed = capture(&meshes);
    assert_ne!(
        armed.len(),
        bare.len(),
        "held model must affect the chase frame"
    );
    if let Some(path) = std::env::var_os("MTL_WEAPON_QA_PNG") {
        std::fs::write(path, armed).unwrap();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
#[ignore = "requires a native GPU adapter"]
fn asphalt_impact_renders_from_the_third_person_cursor() {
    capture_asphalt_shot(false);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
#[ignore = "requires a native GPU adapter"]
fn muzzle_flash_from_a_primary_scene_click() {
    capture_asphalt_shot(true);
}

#[cfg(all(test, not(target_arch = "wasm32")))]
fn capture_asphalt_shot(capture_muzzle: bool) {
    use crate::dynamic_world::ShotSurface;
    use opencity::runtime::SurfaceKind;
    let mut game = Game::new(Hermes::test_map());
    game.world.prepare_player_pedestrian_collision(&game.hermes);
    game.set_overlay(Overlay::Driving);
    game.renderer = Some(pollster::block_on(Renderer::new(None, 960, 640)).unwrap());
    game.loadout.equip(Some(Weapon::Rifle));
    let player = game.world.player_position();
    let city = game.hermes.city();
    let hand = game.world.weapon_hand().unwrap();
    let target = (-10..=10)
        .flat_map(|y| (-10..=10).map(move |x| player + Vec3::new(x as f32, y as f32, 0.)))
        .filter_map(|mut p| {
            let s = city.surface_at([p.x / METERS_PER_UNIT, p.y / METERS_PER_UNIT], 1.)?;
            if s.kind != SurfaceKind::Asphalt {
                return None;
            }
            p.z = s.height * METERS_PER_UNIT;
            let span = hand.distance(p);
            let check = game
                .world
                .weapon_aim_point(city, hand, p + (p - hand) * 0.1);
            ((2.5..8.).contains(&span) && check.distance(p) < 0.05).then_some(p)
        })
        .min_by(|a, b| {
            (a.distance(player) - 4.)
                .abs()
                .total_cmp(&(b.distance(player) - 4.).abs())
        })
        .unwrap_or_else(|| panic!("no clear asphalt near {player:?}, hand {hand:?}"));
    let delta = (target - player).truncate();
    let forward = (delta / delta.length()).extend(0.);
    let eye =
        (player - forward * 4. + Vec3::Z * 2.4 + Vec3::Z.cross(forward) * 1.1) / METERS_PER_UNIT;
    let look = target / METERS_PER_UNIT;
    game.world.aim_weapon(Some(target), true);
    let matrix = game.renderer.as_ref().unwrap().view_projection(eye, look);
    game.loadout.projection = Some(matrix.inverse());
    game.cursor = None;
    game.fire_weapon();
    assert_eq!(game.loadout.rounds[0], 11);
    assert!(game.loadout.effects.has_mark(ShotSurface::Asphalt));
    for _ in 0..if capture_muzzle { 0 } else { 7 } {
        game.loadout.effects.step(1. / 60.);
    }
    let renderer = game.renderer.as_mut().unwrap();
    let scenery = Scenery::new(renderer, &mut game.hermes);
    let mut actors = VehicleScenery::new(renderer);
    actors.update(renderer, &game.world);
    let mut solids = Mesh::default();
    game.loadout.effects.solids(&mut solids);
    let chips = renderer.upload(&solids);
    let mut meshes = scenery.visible([look.x, look.y]);
    meshes.extend(actors.visible());
    meshes.push(&chips);
    let view_rotation = Quat::from_mat4(&Mat4::look_at_rh(eye, look, Vec3::Z).inverse())
        * Quat::from_mat3(&glam::Mat3::from_cols(-Vec3::Z, -Vec3::X, Vec3::Y));
    let mut sprites = game
        .loadout
        .effects
        .sprites(|_| None, view_rotation, eye * METERS_PER_UNIT);
    meshes.push(
        game.loadout
            .visible(renderer, &game.world, target, view_rotation, &mut sprites)
            .unwrap(),
    );
    assert_eq!(renderer.local_lights.flash.get().is_some(), capture_muzzle);
    renderer.update_impacts(&sprites);
    let frame = renderer
        .capture_png(|view| renderer.render_scene(view, &meshes, eye, look, 10., None, None))
        .unwrap();
    if let Some(path) = std::env::var_os(if capture_muzzle {
        "MTL_MUZZLE_QA_PNG"
    } else {
        "MTL_IMPACT_QA_PNG"
    }) {
        std::fs::write(path, frame).unwrap();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
#[ignore = "requires a native GPU adapter"]
fn zombie_knockout_through_the_third_person_cursor() {
    let mut game = Game::new(Hermes::test_map());
    let walker = game.world.prepare_player_pedestrian_collision(&game.hermes);
    let target = walker + Vec3::Z * 1.05;
    game.set_overlay(Overlay::Driving);
    game.renderer = Some(pollster::block_on(Renderer::new(None, 960, 640)).unwrap());
    game.loadout.equip(Some(Weapon::Rifle));
    let player = game.world.player_position();
    let yaw = game.world.player_yaw();
    let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.);
    let eye =
        (player - forward * 4. + Vec3::Z * 1.85 + Vec3::Z.cross(forward) * 1.3) / METERS_PER_UNIT;
    let look = target / METERS_PER_UNIT;
    let matrix = game.renderer.as_ref().unwrap().view_projection(eye, look);
    game.loadout.projection = Some(matrix.inverse());
    game.cursor = Some([480., 320.]);
    game.world.aim_weapon(Some(target), true);
    game.fire_weapon();
    assert_eq!(game.world.pedestrian_reactions()[1], 0);
    game.world.set_zombie_mode(game.hermes.city(), true);
    for shot in 1..=2 {
        game.game_time = shot as f32;
        game.fire_weapon();
        assert_eq!(game.world.pedestrian_reactions()[1], usize::from(shot == 2));
    }
    assert_eq!(game.loadout.rounds[0], 11);
    for _ in 0..90 {
        game.world.step(
            &game.hermes,
            Default::default(),
            Default::default(),
            1. / 60.,
        );
        game.loadout.effects.step(1. / 60.);
    }
    let renderer = game.renderer.as_mut().unwrap();
    let scenery = Scenery::new(renderer, &mut game.hermes);
    let mut actors = VehicleScenery::new(renderer);
    actors.update(renderer, &game.world);
    let mut meshes = scenery.visible([look.x, look.y]);
    meshes.extend(actors.visible());
    meshes.push(
        game.loadout
            .visible(
                renderer,
                &game.world,
                target,
                Quat::IDENTITY,
                &mut Vec::new(),
            )
            .unwrap(),
    );
    let capture = |renderer: &Renderer| {
        renderer
            .capture_png(|view| renderer.render_scene(view, &meshes, eye, look, 10., None, None))
            .unwrap()
    };
    let normal = capture(renderer);
    renderer.zombie_mode = game.world.zombie_mode();
    let frame = capture(renderer);
    assert_ne!(frame, normal);
    renderer.zombie_mode = false;
    assert_eq!(capture(renderer), normal);
    if let Some(path) = std::env::var_os("MTL_ZOMBIE_QA_PNG") {
        std::fs::write(path, frame).unwrap();
    }
}
