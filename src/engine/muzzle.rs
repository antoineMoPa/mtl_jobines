//! Shot presentation shared by the chase-camera and native client render paths.
use super::{impact::quad, render::ImpactVertex};
use crate::dynamic_world::{LocalLight, METERS_PER_UNIT};
use glam::{Mat4, Quat, Vec2, Vec3};
use opencity::runtime::Mesh;

/// Equipment faces +X. Locate the centre of its forward cap in displayed model space.
fn barrel_tip(mesh: &Mesh) -> Vec3 {
    let (lo, hi) = crate::hermes::external_models::bounds(mesh);
    let cutoff = hi.x - (hi.x - lo.x) * 0.002;
    let mut bottom = hi;
    let mut top = lo;
    for p in mesh
        .vertices
        .iter()
        .map(|v| Vec3::from(v.position))
        .filter(|p| p.x >= cutoff)
    {
        bottom = bottom.min(p);
        top = top.max(p);
    }
    Vec3::new(hi.x, (bottom.y + top.y) * 0.5, (bottom.z + top.z) * 0.5)
}

pub(super) struct Equipment {
    pub gpu: super::render::GpuMesh,
    tip: Vec3,
}
impl Equipment {
    pub fn upload(renderer: &super::render::Renderer, mesh: &Mesh) -> Self {
        Self {
            gpu: renderer.upload_equipment(&mesh.vertices),
            tip: barrel_tip(mesh),
        }
    }
    pub fn place(
        &mut self,
        renderer: &super::render::Renderer,
        pose: Mat4,
        camera: Quat,
        flash: &MuzzleFlash,
        sprites: &mut Vec<ImpactVertex>,
    ) {
        renderer.update_instances(&mut self.gpu, &[pose.to_cols_array_2d()]);
        if let Some(light) = flash.draw(sprites, pose, self.tip, camera) {
            renderer.local_lights.flash.set(Some(light));
        }
    }
}

pub(super) enum FlashStyle {
    Rifle,
    Handgun,
}

#[derive(Default)]
pub(super) struct MuzzleFlash {
    length: f32,
    age: f32,
    seed: u32,
    fresh: bool,
}
impl MuzzleFlash {
    pub fn handgun_at(age: f32) -> Self {
        let mut flash = Self::default();
        flash.ignite(FlashStyle::Handgun);
        flash.age = age;
        flash
    }
    pub fn ignite(&mut self, style: FlashStyle) {
        self.length = match style {
            FlashStyle::Rifle => 0.48,
            FlashStyle::Handgun => 0.28,
        };
        self.age = 0.;
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        self.fresh = true;
    }
    pub fn clear(&mut self) {
        self.length = 0.;
    }
    pub fn step(&mut self, dt: f32) {
        // A slow input frame can advance simulation before the first draw.
        self.age += if self.fresh { dt.min(0.012) } else { dt };
        self.fresh = false;
    }
    pub fn draw(
        &self,
        sprites: &mut Vec<ImpactVertex>,
        model: Mat4,
        tip: Vec3,
        camera: Quat,
    ) -> Option<LocalLight> {
        if self.length == 0. || self.age >= 0.19 {
            return None;
        }
        let origin = model.transform_point3(tip);
        let axis = model.transform_vector3(Vec3::X);
        let seed = (self.seed >> 16) as f32 / 2048.;
        let span = self.length / METERS_PER_UNIT;
        let camera_up = camera * Vec3::Z;
        let camera_side = camera * Vec3::Y;
        let mut card = |center: Vec3, across: Vec3, up: Vec3, kind, opacity| {
            quad(sprites, |uv: Vec2| {
                ImpactVertex::new(
                    center + across * uv.x + up * uv.y,
                    uv.into(),
                    Vec3::splat(0.57),
                    [kind, seed, opacity, self.age / 0.19],
                )
            });
        };
        if self.age > 0.026 {
            let t = self.age / 0.19;
            let radius = span * (0.10 + 0.42 * t);
            let center = origin + axis * span * (0.21 + t * 0.64) + Vec3::Z * span * t * 0.45;
            card(
                center,
                camera_side * radius,
                camera_up * radius,
                0.,
                0.37 * (1. - t),
            );
        }
        let intensity = (1. - self.age / 0.059).max(0.);
        if intensity == 0. {
            return None;
        }
        let length = span * (0.76 + seed.fract() * 0.36) * (0.58 + 0.42 * intensity);
        let lateral = axis.cross(camera * Vec3::X);
        let width = if lateral.length_squared() > 0.0001 {
            lateral / lateral.length()
        } else {
            camera_up
        };
        card(
            origin + axis * length * 0.50,
            axis * length * 0.50,
            width * length * 0.26,
            7.,
            intensity.sqrt(),
        );
        let radius = length * 0.31;
        card(
            origin + axis * length * 0.17,
            camera_side * radius,
            camera_up * radius,
            8.,
            intensity.sqrt(),
        );
        Some(LocalLight {
            position_range: (origin + axis * length * 0.18).extend(span * 10.).into(),
            direction_outer: [0., 0., 0., -1.],
            color_intensity: [1., 0.45, 0.12, intensity * 5.5],
            beam: [0., 0.55, 0., 1.],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_lights_the_transformed_barrel_then_leaves_only_smoke() {
        let pose =
            Mat4::from_rotation_translation(Quat::from_rotation_z(1.1), Vec3::new(2., 3., 1.));
        let tip = Vec3::new(0.005, 0., 0.001);
        let mut flash = MuzzleFlash::default();
        let mut sprites = Vec::new();
        for style in [FlashStyle::Handgun, FlashStyle::Rifle] {
            flash.ignite(style);
            flash.step(0.1);
            let light = flash.draw(&mut sprites, pose, tip, Quat::IDENTITY).unwrap();
            let local = pose.inverse().transform_point3(light.position());
            assert!(local.x > tip.x && local.x - tip.x < 0.002);
            assert!((local.y - tip.y).abs() < 1.0e-6);
            assert!((local.z - tip.z).abs() < 1.0e-6);
            assert_eq!(sprites.len(), 12);
            sprites.clear();
            flash.step(0.06);
            assert!(flash
                .draw(&mut sprites, pose, tip, Quat::IDENTITY)
                .is_none());
            assert_eq!(sprites.len(), 6);
            sprites.clear();
            flash.step(0.18);
            assert!(flash
                .draw(&mut sprites, pose, tip, Quat::IDENTITY)
                .is_none());
            assert!(sprites.is_empty());
        }
        flash.ignite(FlashStyle::Rifle);
        flash.clear();
        assert!(flash
            .draw(&mut sprites, pose, tip, Quat::IDENTITY)
            .is_none());
        assert!(sprites.is_empty());
    }
}
