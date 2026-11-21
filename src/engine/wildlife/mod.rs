use super::render::{GpuMesh, Renderer};
use crate::dynamic_world::{
    wildlife::{CatCoat, Species},
    DynamicWorld, METERS_PER_UNIT,
};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
mod model;

struct Part {
    joint: usize,
    mesh: GpuMesh,
    instances: Vec<[[f32; 4]; 4]>,
}
struct Rig {
    model: model::Model,
    parts: Vec<Part>,
}
#[derive(Default)]
pub(super) struct WildlifeScenery {
    rigs: HashMap<(Species, Option<CatCoat>), Rig>,
    matrices: Vec<Mat4>,
}
impl WildlifeScenery {
    pub(super) fn update(&mut self, renderer: &Renderer, world: &DynamicWorld) {
        for rig in self.rigs.values_mut() {
            for part in &mut rig.parts {
                part.instances.clear();
            }
        }
        let to_game = Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let from_game = to_game.transpose();
        for animal in world.wildlife() {
            let appearance = (animal.species, animal.cat_coat);
            let rig = self.rigs.entry(appearance).or_insert_with(|| {
                let (model, meshes) = model::Model::read(animal.species, animal.cat_coat)
                    .expect("bundled wildlife model");
                let parts = meshes
                    .into_iter()
                    .enumerate()
                    .filter(|(_, m)| !m.vertices.is_empty())
                    .map(|(joint, m)| Part {
                        joint,
                        mesh: if m.vertices.len() > 384 {
                            renderer.upload_vehicle_lod(&m.vertices, &m.vertices)
                        } else {
                            renderer.upload_instanced(&m.vertices)
                        },
                        instances: Vec::new(),
                    })
                    .collect();
                Rig { model, parts }
            });
            let scale = animal.species.model_scale();
            let location = Mat4::from_translation(animal.position / METERS_PER_UNIT)
                * Mat4::from_rotation_z(animal.heading)
                * Mat4::from_rotation_x(animal.roll)
                * Mat4::from_scale(Vec3::splat(scale / METERS_PER_UNIT))
                * to_game;
            // A larger stride covers the same route at a lower cadence.
            let seconds = if animal.clip == 0 {
                animal.seconds / scale
            } else {
                animal.seconds
            };
            rig.model.sample(animal.clip, seconds, &mut self.matrices);
            for part in &mut rig.parts {
                let mut columns =
                    (location * self.matrices[part.joint] * from_game).to_cols_array_2d();
                columns[1][3] = -1.;
                part.instances.push(columns);
            }
        }
        for rig in self.rigs.values_mut() {
            for part in &mut rig.parts {
                renderer.update_instances(&mut part.mesh, &part.instances);
            }
        }
    }
    pub(super) fn visible(&self) -> impl Iterator<Item = &GpuMesh> {
        self.rigs
            .values()
            .flat_map(|r| &r.parts)
            .filter(|p| !p.instances.is_empty())
            .map(|p| &p.mesh)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_wildlife_clips_move_finite_joints() {
        for s in Species::ALL {
            let (mut model, meshes) =
                model::Model::read(s, None).unwrap_or_else(|e| panic!("{s:?}: {e}"));
            assert!(meshes.iter().any(|m| !m.vertices.is_empty()));
            for clip in 0..s.clips().len() {
                let (mut start, mut pose) = (Vec::new(), Vec::new());
                model.sample(clip, 0., &mut start);
                let mut changed = false;
                for i in 1..31 {
                    model.sample(clip, i as f32 * 0.079, &mut pose);
                    assert!(pose.iter().all(|m| m.is_finite()));
                    changed |= start
                        .iter()
                        .zip(&pose)
                        .any(|(a, b)| !a.abs_diff_eq(*b, 1e-6));
                }
                assert!(changed, "{s:?} clip {clip}");
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    #[ignore = "requires native GPU and the playable map"]
    fn capture_wildlife_on_the_playable_map() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl/map.road");
        let mut hermes = crate::hermes::Hermes::load_with_game_data(&path, None).unwrap();
        let lot = hermes
            .city()
            .landscape
            .iter()
            .find(|feature| feature.name.as_deref() == Some("sphere_parking"))
            .expect("Orange Julep parking footprint");
        let middle = lot
            .points
            .iter()
            .copied()
            .map(glam::Vec2::from_array)
            .sum::<glam::Vec2>()
            / lot.points.len() as f32;
        let start = crate::dynamic_world::SpawnPoint {
            point: middle.to_array(),
            heading: None,
        };
        let mut world = DynamicWorld::with_start(&hermes, Some(start)).unwrap();
        let animal = (0..=240)
            .find_map(|_| {
                let airborne = world.wildlife().find(|a| {
                    a.species == Species::Gull
                        && a.species.clips()[a.clip] == "Flight"
                        && a.position.z > world.player_position().z + 2.
                });
                if airborne.is_none() {
                    world.step(&hermes, Default::default(), Default::default(), 0.1);
                }
                airborne
            })
            .expect("a gull flying above the Orange Julep lot");

        let mut renderer = pollster::block_on(Renderer::new(None, 1200, 800)).unwrap();
        renderer.daylight = Some(1.);
        renderer.render_lod = crate::engine::render::RenderLod::Full;
        let scenery = crate::engine::scenery::Scenery::new(&renderer, &mut hermes);
        let mut actors = WildlifeScenery::default();
        actors.update(&renderer, &world);
        let look = (animal.position + Vec3::Z * 0.15) / METERS_PER_UNIT;
        let eye = look
            + Mat4::from_rotation_z(animal.heading).transform_vector3(Vec3::new(0.7, -2.5, -0.65))
                / METERS_PER_UNIT;
        let mut meshes = scenery.visible([look.x, look.y]);
        let nav = hermes.minimap_view([look.x, look.y], animal.heading);
        let without = renderer
            .report_screenshot(&meshes, eye, look, 0., nav)
            .unwrap();
        meshes.extend(actors.visible());
        let with = renderer
            .report_screenshot(&meshes, eye, look, 0., nav)
            .unwrap();
        assert_ne!(with, without, "the animal must reach the main render pass");
        let target = animal.position + Vec3::Z * 0.10;
        let hit = world
            .fire_weapon_at(hermes.city(), eye * METERS_PER_UNIT, target, 1)
            .unwrap();
        assert_eq!(hit.surface, crate::dynamic_world::ShotSurface::Wildlife);
        for _ in 0..24 {
            world.step(&hermes, Default::default(), Default::default(), 1. / 60.);
        }
        assert!(world
            .wildlife()
            .any(|a| a.species == Species::Gull && a.roll > 1.5));
        actors.update(&renderer, &world);
        let mut meshes = scenery.visible([look.x, look.y]);
        meshes.extend(actors.visible());
        let after = renderer
            .report_screenshot(&meshes, eye, look, 0., nav)
            .unwrap();
        assert_ne!(with, after);
        if let Some(path) = std::env::var_os("WILDLIFE_CAPTURE") {
            let path = std::path::PathBuf::from(path);
            std::fs::write(&path, with).unwrap();
            std::fs::write(path.with_file_name("in-game-hit-response.png"), after).unwrap();
        }
    }
}
