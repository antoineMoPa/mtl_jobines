//! Resident render variants, selected independently for camera and shadow visibility.
mod mesh;
mod selection;
use super::*;
use mesh::{Indexed, Policy, Purpose};
use selection::{Level, View};
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::engine) enum RenderLod {
    Full,
    Shadows,
    Adaptive,
}
pub(super) struct Geometry {
    buffer: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    vertices: u32,
    opaque: u32,
    paint: u32,
    error: f32,
}
pub(super) struct Variants {
    medium: Geometry,
    coarse: Geometry,
    shadow: Geometry,
}
pub(super) struct Draw<'a> {
    buffer: &'a wgpu::Buffer,
    indices: Option<&'a wgpu::Buffer>,
    pub count: u32,
    pub vertices: u32,
    pub opaque: u32,
    pub paint: u32,
}
impl Draw<'_> {
    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        range: Range<u32>,
        instances: Range<u32>,
    ) {
        if range.is_empty() {
            return;
        }
        pass.set_vertex_buffer(0, self.buffer.slice(..));
        if let Some(indices) = self.indices {
            pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(range, 0, instances);
        } else {
            pass.draw(range, instances);
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Choice {
    Full,
    Medium,
    Coarse,
    Shadow,
}
impl GpuMesh {
    fn geometry(&self, choice: Choice) -> Draw<'_> {
        let variant = self.lod.as_ref().and_then(|lod| match choice {
            Choice::Full => None,
            Choice::Medium => Some(&lod.medium),
            Choice::Coarse => Some(&lod.coarse),
            Choice::Shadow => Some(&lod.shadow),
        });
        if let Some(g) = variant {
            Draw {
                buffer: &g.buffer,
                indices: Some(&g.indices),
                count: g.count,
                vertices: g.vertices,
                opaque: g.opaque,
                paint: g.paint,
            }
        } else {
            Draw {
                buffer: &self.buffer,
                indices: None,
                count: self.count,
                vertices: self.count,
                opaque: self.opaque_count,
                paint: self.paint_start,
            }
        }
    }
    fn choice(
        &self,
        view: &View,
        bounds: Bounds,
        scale: f32,
        force: bool,
        mode: RenderLod,
        shadow: bool,
    ) -> Choice {
        let Some(lod) = &self.lod else {
            return Choice::Full;
        };
        if mode == RenderLod::Full {
            return Choice::Full;
        }
        if shadow {
            if view.shadow_allowed(
                bounds,
                lod.shadow.error * scale,
                super::SHADOW_RESOLUTION as f32 / super::SHADOW_SPAN,
                force,
            ) {
                Choice::Shadow
            } else {
                Choice::Full
            }
        } else if mode == RenderLod::Shadows {
            Choice::Full
        } else {
            match view.level(
                bounds,
                [lod.medium.error * scale, lod.coarse.error * scale],
                force,
            ) {
                Level::Full => Choice::Full,
                Level::Medium => Choice::Medium,
                Level::Coarse => Choice::Coarse,
            }
        }
    }
    pub(super) fn static_draw(&self, view: &View, mode: RenderLod, shadow: bool) -> Draw<'_> {
        let choice = self.bounds.map_or(Choice::Full, |bounds| {
            self.choice(view, bounds, 1., false, mode, shadow)
        });
        self.geometry(choice)
    }
    pub(super) fn instance_draws<'a>(
        &'a self,
        frustum: &'a Frustum,
        view: &'a View,
        mode: RenderLod,
        shadow: bool,
    ) -> impl Iterator<Item = (Draw<'a>, Range<u32>)> + 'a {
        let instances = self.instances.as_ref().expect("instanced draw");
        let mut index = 0;
        std::iter::from_fn(move || {
            while index < instances.bounds.len() && !frustum.intersects(instances.bounds[index]) {
                index += 1;
            }
            if index == instances.bounds.len() {
                return None;
            }
            let start = index;
            let choice = self.choice(
                view,
                instances.bounds[index],
                instances.scales[index],
                instances.force_full[index],
                mode,
                shadow,
            );
            index += 1;
            while index < instances.bounds.len()
                && frustum.intersects(instances.bounds[index])
                && self.choice(
                    view,
                    instances.bounds[index],
                    instances.scales[index],
                    instances.force_full[index],
                    mode,
                    shadow,
                ) == choice
            {
                index += 1;
            }
            Some((self.geometry(choice), start as u32..index as u32))
        })
    }
}
impl Renderer {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn visibility_stats(
        &self,
        meshes: &[&GpuMesh],
        eye: Vec3,
        look: Vec3,
        time: f32,
    ) -> VisibilityStats {
        let (view, light) = self.view_matrices(
            eye,
            look,
            Lighting::with_settings(time, self.daylight, self.sky_settings).sun,
        );
        let camera = Frustum::from_matrix(view);
        let shadow = Frustum::from_matrix(light);
        let detail = self.detail_view(eye, look);
        let mut stats = VisibilityStats {
            candidates: meshes.len(),
            candidate_vertices: 0,
            camera_draws: 0,
            camera_vertices: 0,
            shadow_draws: 0,
            shadow_vertices: 0,
            camera_vertex_buffers: 0,
            shadow_vertex_buffers: 0,
        };
        for mesh in meshes {
            stats.candidate_vertices += u64::from(mesh.count)
                * mesh.instances.as_ref().map_or(1, |i| i.bounds.len() as u64);
            if mesh.instances.is_some() {
                for is_shadow in [false, true] {
                    let frustum = if is_shadow { &shadow } else { &camera };
                    for (draw, range) in
                        mesh.instance_draws(frustum, &detail, self.render_lod, is_shadow)
                    {
                        let count = if is_shadow { draw.paint } else { draw.count };
                        let buffer = if is_shadow && draw.indices.is_none() {
                            draw.paint
                        } else {
                            draw.vertices
                        };
                        let copies = u64::from(range.end - range.start);
                        if is_shadow {
                            stats.shadow_draws += 1;
                            stats.shadow_vertices += u64::from(count) * copies;
                            stats.shadow_vertex_buffers += u64::from(buffer) * copies;
                        } else {
                            stats.camera_draws += 1;
                            stats.camera_vertices += u64::from(count) * copies;
                            stats.camera_vertex_buffers += u64::from(buffer) * copies;
                        }
                    }
                }
            } else {
                if camera.includes(mesh.bounds) {
                    let draw = mesh.static_draw(&detail, self.render_lod, false);
                    stats.camera_draws += 1;
                    stats.camera_vertices += u64::from(draw.count);
                    stats.camera_vertex_buffers += u64::from(draw.vertices);
                }
                if shadow.includes(mesh.bounds) {
                    let draw = mesh.static_draw(&detail, self.render_lod, true);
                    if draw.paint > 0 {
                        stats.shadow_draws += 1;
                        stats.shadow_vertices += u64::from(draw.paint);
                        stats.shadow_vertex_buffers += u64::from(if draw.indices.is_none() {
                            draw.paint
                        } else {
                            draw.vertices
                        });
                    }
                }
            }
        }
        stats
    }
    fn lod_geometry(&self, source: &[Vertex], error: f32, purpose: Purpose) -> Geometry {
        let (sorted, opaque, paint) = super::material_vertices(source);
        let source = if matches!(purpose, Purpose::Shadow) {
            &sorted[..paint as usize]
        } else {
            &sorted
        };
        let indexed = Indexed::new(source, opaque, paint, purpose);
        let bytes = |vertices: &[Vertex]| unsafe {
            std::slice::from_raw_parts(
                vertices.as_ptr().cast::<u8>(),
                std::mem::size_of_val(vertices),
            )
        };
        let contents = bytes(&indexed.vertices);
        Geometry {
            buffer: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("resident LOD vertices"),
                    contents: if contents.is_empty() {
                        &[0; 4]
                    } else {
                        contents
                    },
                    usage: wgpu::BufferUsages::VERTEX,
                }),
            indices: self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("resident LOD indices"),
                    contents: if indexed.indices.is_empty() {
                        &[0; 4]
                    } else {
                        bytemuck::cast_slice(&indexed.indices)
                    },
                    usage: wgpu::BufferUsages::INDEX,
                }),
            count: indexed.indices.len() as u32,
            vertices: indexed.vertices.len() as u32,
            opaque: indexed.opaque,
            paint: indexed.paint,
            error,
        }
    }
    fn variants(&self, source: &[Vertex], shadow_source: &[Vertex], policy: Policy) -> Variants {
        let cells = match policy {
            Policy::Vehicle => [0.04, 0.10, 0.06],
            Policy::Scenery => [0.0005, 0.0015, 0.0005],
        };
        let medium = mesh::reduce(source, cells[0], policy);
        let coarse = mesh::reduce(source, cells[1], policy);
        let shadow = mesh::reduce(shadow_source, cells[2], policy);
        Variants {
            medium: self.lod_geometry(&medium.vertices, medium.error, Purpose::Camera),
            coarse: self.lod_geometry(&coarse.vertices, coarse.error, Purpose::Camera),
            shadow: self.lod_geometry(&shadow.vertices, shadow.error, Purpose::Shadow),
        }
    }
    pub(in crate::engine) fn upload_vehicle_lod(
        &self,
        source: &[Vertex],
        shadow: &[Vertex],
    ) -> GpuMesh {
        let mut mesh = self.upload_instanced(source);
        mesh.lod = Some(Box::new(self.variants(source, shadow, Policy::Vehicle)));
        mesh
    }
    pub(in crate::engine) fn upload_scenery(&self, mesh: &Mesh) -> GpuMesh {
        self.upload_scenery_vertices(&mesh.vertices)
    }
    pub(in crate::engine) fn upload_scenery_vertices(&self, source: &[Vertex]) -> GpuMesh {
        let mut mesh = self.upload_vertices(source);
        if source.len() > 384 && mesh::reducible(source, Policy::Scenery) {
            mesh.lod = Some(Box::new(self.variants(source, source, Policy::Scenery)));
        }
        mesh
    }
    pub(super) fn detail_view(&self, eye: Vec3, look: Vec3) -> View {
        View::new(eye, look, self.scene_height(), self.construction_span)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod gpu_tests;
