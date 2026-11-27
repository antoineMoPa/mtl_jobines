//! The player uses a second region of the sun atlas so the bike and rider
//! can cast detailed shadows within the existing texture-binding budget.
use super::{culling::Bounds, GpuMesh, Renderer};
use glam::{Mat4, Vec3};

pub(super) const RESOLUTION: u32 = 1024;
const MIN_HALF_SPAN: f32 = 0.10;

pub(in crate::engine) fn world_shader() -> String {
    let source = opencity::runtime::GAME_SHADER;
    let (before, visibility) = source
        .split_once("\nfn visibility(")
        .expect("sun visibility");
    let (_, after) = visibility.split_once("\n}\n").expect("sun visibility body");
    let before = before.replace(
        "star_color: vec4<f32> };",
        "star_color: vec4<f32>, player_light_view_projection: mat4x4<f32>, shadow_atlas: vec4<f32> };",
    );
    format!("{before}\n{}\n{after}", include_str!("player_shadow.wgsl"))
}

pub(super) fn projection(meshes: &[&GpuMesh], sun: Vec3) -> Option<Mat4> {
    let bounds = meshes
        .iter()
        .filter_map(|mesh| {
            let instances = mesh.instances.as_ref()?;
            instances.bounds.get(instances.player_shadow?).copied()
        })
        .reduce(|a, b| Bounds {
            min: a.min.min(b.min),
            max: a.max.max(b.max),
        })?;
    let focus = (bounds.min + bounds.max) * 0.5;
    let half = MIN_HALF_SPAN.max((bounds.max - bounds.min).length() * 0.6);
    let up = if sun.z.abs() > 0.99 { Vec3::Y } else { Vec3::Z };
    Some(
        Mat4::orthographic_rh(-half, half, -half, half, 0.01, 10.)
            * Mat4::look_at_rh(focus + sun * 4., focus, up),
    )
}

impl Renderer {
    pub(super) fn draw_player_shadows(&self, pass: &mut wgpu::RenderPass<'_>, meshes: &[&GpuMesh]) {
        let x = self.world_shadow_resolution;
        let y = (x - RESOLUTION) / 2;
        let size = RESOLUTION as f32;
        pass.set_viewport(x as f32, y as f32, size, size, 0., 1.);
        pass.set_scissor_rect(x, y, RESOLUTION, RESOLUTION);
        pass.set_bind_group(0, &self.player_shadow_group, &[]);
        for mesh in meshes {
            let Some((draw, index)) = mesh.player_shadow_draw() else {
                continue;
            };
            let instances = mesh.instances.as_ref().unwrap();
            if let Some(skin) = &mesh.skin {
                pass.set_pipeline(&self.pedestrian_shadow_pipeline);
                pass.set_vertex_buffer(2, skin.slice(..));
            } else {
                pass.set_pipeline(&self.vehicle_shadow_pipeline);
            }
            pass.set_vertex_buffer(1, instances.buffer.slice(..));
            draw.draw(pass, 0..draw.paint, index..index + 1);
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::engine::lighting::Lighting;
    use opencity::runtime::{Mesh, SurfaceKind};

    #[test]
    #[ignore = "requires a native GPU adapter"]
    fn player_tile_resolves_a_thin_shadow_on_the_ground() {
        let mut renderer = pollster::block_on(Renderer::new(None, 128, 128)).unwrap();
        renderer.ambient_occlusion = false;
        renderer.sky_settings.solar_hour = Some(9.5);
        let sun = Lighting::with_settings(0., None, renderer.sky_settings).sun;
        let mut ground = Mesh::default();
        ground.cuboid(
            [-0.1, -0.1, -0.001],
            [0.1, 0.1, 0.],
            [0.6; 3],
            SurfaceKind::Prop,
        );
        let ground = renderer.upload(&ground);
        let mut caster = Mesh::default();
        caster.cuboid(
            [-0.04, -0.04, -0.04],
            [0.04, 0.04, 0.04],
            [0.6; 3],
            SurfaceKind::Prop,
        );
        for vertex in &mut caster.vertices {
            vertex.surface[1] = 1.;
        }
        let mut caster = renderer.upload_instanced(&caster.vertices);
        let center = Vec3::Z * 0.02;
        let transform =
            Mat4::from_scale_rotation_translation(Vec3::splat(0.01), glam::Quat::IDENTITY, center)
                .to_cols_array_2d();
        let look = center - sun * (center.z / sun.z);
        let eye = look + Vec3::Z * 0.003 - sun.truncate().extend(0.) * 0.004;
        let sample = |caster: &GpuMesh| {
            let png = renderer
                .capture_png(|target| {
                    renderer.render_scene(target, &[&ground, caster], eye, look, 0., None);
                })
                .unwrap();
            let mut decoder = png::Decoder::new(png.as_slice()).read_info().unwrap();
            let mut pixels = vec![0; decoder.output_buffer_size()];
            decoder.next_frame(&mut pixels).unwrap();
            pixels[(64 * 128 + 64) * 4]
        };

        renderer.update_instances(&mut caster, &[transform]);
        assert!(projection(&[&caster], sun).is_none());
        let coarse = sample(&caster);
        renderer.update_actor_instances(&mut caster, &[transform], Some(0));
        let detailed = sample(&caster);
        assert!(
            u16::from(coarse) > u16::from(detailed) + 20,
            "expected the player's fine shadow to be visible on the ground ({coarse} vs {detailed})"
        );
    }
}
