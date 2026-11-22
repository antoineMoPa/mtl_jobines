//! Immutable map geometry is built per map revision, then drawn in spatial batches.
//! Gameplay uploads visible batches first and fills nearby buffers over subsequent frames.
use super::render::{GpuMesh, Renderer};
use crate::hermes::Hermes;
use opencity::runtime::Mesh;
use std::ops::Range;
// Keep the existing 200 m radius-selection groups, then cull their 50 m
// GPU batches independently. Splitting must not shorten detail draw distance.
const TILE_SPAN: f32 = 2.;
const DRAW_SPAN: f32 = 0.5;
struct Batch {
    infrastructure: bool,
    bounds: [f32; 4],
    meshes: Vec<GpuMesh>,
}
pub struct Scenery {
    pub(super) grass: super::render::SupportCache,
    pub(super) metro: crate::hermes::metro::MetroMap,
    banks: super::snow::banks::Banks,
    winter: bool,
    world: Vec<Batch>,
    details: Vec<Batch>,
    pending_world: Option<PendingLayer>,
    pending_details: Option<PendingLayer>,
    paint: super::graffiti::Nearby,
    models: Vec<GpuMesh>,
}
fn triangle_key(triangle: &[opencity::runtime::Vertex; 3]) -> (i32, i32, bool, i32, i32) {
    let center = [
        triangle.iter().map(|v| v.position[0]).sum::<f32>() / 3.,
        triangle.iter().map(|v| v.position[1]).sum::<f32>() / 3.,
    ];
    (
        (center[0] / TILE_SPAN).floor() as i32,
        (center[1] / TILE_SPAN).floor() as i32,
        infrastructure(triangle),
        (center[0] / DRAW_SPAN).floor() as i32,
        (center[1] / DRAW_SPAN).floor() as i32,
    )
}
fn infrastructure(triangle: &[opencity::runtime::Vertex; 3]) -> bool {
    use opencity::runtime::SurfaceKind;
    let s = triangle[0].surface;
    s[0] == SurfaceKind::Asphalt as u32 as f32
        || s[0] == SurfaceKind::Marking as u32 as f32
        || s[0] == SurfaceKind::Sidewalk as u32 as f32 && s[3] != 0.
        || s[0] == SurfaceKind::Grass as u32 as f32
            && s[3] != 0.
            && s[3] != opencity::runtime::Vertex::DRY_TERRAIN
            && s[3] != opencity::runtime::Vertex::SHORE_PROFILE
        || s[3] == -2.
}
// Sort whole triangles in place. Retain only draw ranges, never a second city-sized mesh.
fn partition(mesh: &mut Mesh) -> Vec<([f32; 4], Vec<Range<usize>>, bool)> {
    let (triangles, remainder) = mesh.vertices.as_chunks_mut::<3>();
    assert!(
        remainder.is_empty(),
        "scenery must contain complete triangles"
    );
    triangles.sort_unstable_by_key(triangle_key);
    let mut groups: Vec<([f32; 4], Vec<Range<usize>>, bool)> = Vec::new();
    let mut previous = None;
    for (index, triangle) in triangles.iter().enumerate() {
        let key = triangle_key(triangle);
        if previous
            .is_none_or(|p: (i32, i32, bool, i32, i32)| (p.0, p.1, p.2) != (key.0, key.1, key.2))
        {
            groups.push((
                [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ],
                Vec::new(),
                key.2,
            ));
        }
        let (bounds, draws, _) = groups.last_mut().unwrap();
        if previous != Some(key) {
            draws.push(index * 3..index * 3);
        }
        draws.last_mut().unwrap().end += 3;
        for vertex in triangle {
            for axis in 0..2 {
                bounds[axis] = bounds[axis].min(vertex.position[axis]);
                bounds[axis + 2] = bounds[axis + 2].max(vertex.position[axis]);
            }
        }
        previous = Some(key);
    }
    groups
}
// Keep one CPU vertex allocation per layer until all its draw ranges reach the GPU.
struct PendingLayer {
    mesh: Mesh,
    groups: Vec<(usize, std::collections::VecDeque<Range<usize>>)>,
}

impl PendingLayer {
    fn partition(mut mesh: Mesh) -> (Vec<Batch>, Self) {
        let mut batches = Vec::new();
        let mut groups = Vec::new();
        for (bounds, ranges, infrastructure) in partition(&mut mesh) {
            groups.push((batches.len(), ranges.into()));
            batches.push(Batch {
                bounds,
                infrastructure,
                meshes: Vec::new(),
            });
        }
        (batches, Self { mesh, groups })
    }

    fn upload(
        &mut self,
        renderer: &Renderer,
        batches: &mut [Batch],
        center: [f32; 2],
        radius: f32,
    ) {
        // Finish every intersecting group before drawing, including on teleport or zoom.
        // Spend at most four spare draw uploads per layer on the next nearest group.
        let mut nearest = None;
        let mut best = f32::INFINITY;
        for (index, _) in &self.groups {
            let distance = batches[*index].distance(center);
            if distance > radius && distance < best {
                nearest = Some(*index);
                best = distance;
            }
        }
        self.groups.retain_mut(|(index, ranges)| {
            let batch = &mut batches[*index];
            let count = if batch.distance(center) <= radius {
                ranges.len()
            } else if Some(*index) == nearest {
                ranges.len().min(4)
            } else {
                0
            };
            for range in ranges.drain(..count) {
                let vertices = &self.mesh.vertices[range];
                batch.meshes.push(if batch.infrastructure {
                    renderer.upload_vertices(vertices)
                } else {
                    renderer.upload_scenery_vertices(vertices)
                });
            }
            !ranges.is_empty()
        });
    }
}

impl Batch {
    // Chebyshev distance preserves the square selection used for drawing.
    fn distance(&self, [x, y]: [f32; 2]) -> f32 {
        (self.bounds[0] - x)
            .max(x - self.bounds[2])
            .max(self.bounds[1] - y)
            .max(y - self.bounds[3])
            .max(0.)
    }
}

fn world_radius(span: Option<f32>, aspect: f32, flight_visibility: f32) -> f32 {
    span.map_or_else(
        || crate::performance::draw_radius(8.) + 8. * (flight_visibility - 1.),
        |span| (span * aspect.max(1.) * 1.5).max(8.),
    )
}
// Identity placements already contain world coordinates. Give each spatial draw its
// own bounds instead of a single box covering the entire metro loop.
pub(super) fn model_batches(
    renderer: &Renderer,
    source: &Mesh,
    transforms: &[glam::Mat4],
) -> Vec<GpuMesh> {
    let upload = |vertices: &[opencity::runtime::Vertex]| {
        let mut gpu = renderer.upload_instanced(vertices);
        gpu.static_model = true;
        let transforms: Vec<_> = transforms
            .iter()
            .map(|matrix| {
                let mut columns = matrix.to_cols_array_2d();
                columns[1][3] = -1.;
                columns
            })
            .collect();
        renderer.update_instances(&mut gpu, &transforms);
        gpu
    };
    if transforms == [glam::Mat4::IDENTITY] {
        let mut mesh = source.clone();
        partition(&mut mesh)
            .into_iter()
            .flat_map(|(_, ranges, _)| ranges)
            .map(|range| upload(&mesh.vertices[range]))
            .collect()
    } else {
        vec![upload(&source.vertices)]
    }
}
impl Scenery {
    pub fn new(renderer: &Renderer, hermes: &mut Hermes) -> Self {
        let mut scenery = Self::streamed(renderer, hermes);
        scenery.upload_neighborhood(renderer, [0.; 2], f32::INFINITY, f32::INFINITY);
        scenery
    }

    pub fn streamed(renderer: &Renderer, hermes: &mut Hermes) -> Self {
        let _span = mtl_world::telemetry::span("map.scenery.prepare_upload");
        let (world, details) = hermes.take_geometry();
        renderer.snow.set_geometry(&[&world, &details]);
        let paint = super::graffiti::Nearby::new(renderer, &world);
        let models = hermes
            .external_models
            .iter()
            .flat_map(|group| model_batches(renderer, &group.mesh, &group.transforms))
            .collect();
        let banks = super::snow::banks::Banks::new(&world);
        let (world, pending_world) = PendingLayer::partition(world);
        let (details, pending_details) = PendingLayer::partition(details);
        Self {
            grass: Default::default(),
            metro: crate::hermes::metro::MetroMap::from_city(hermes.city()),
            models,
            banks,
            winter: false,
            world,
            details,
            pending_world: Some(pending_world),
            pending_details: Some(pending_details),
            paint,
        }
    }
    fn upload_neighborhood(
        &mut self,
        renderer: &Renderer,
        center: [f32; 2],
        world_radius: f32,
        detail_radius: f32,
    ) {
        for (pending, batches, radius) in [
            (&mut self.pending_world, &mut self.world, world_radius),
            (&mut self.pending_details, &mut self.details, detail_radius),
        ] {
            if let Some(layer) = pending {
                layer.upload(renderer, batches, center, radius);
                if layer.groups.is_empty() {
                    *pending = None;
                }
            }
        }
    }

    pub fn for_frame(
        &mut self,
        renderer: &Renderer,
        center: [f32; 2],
        preview: bool,
    ) -> (Vec<&GpuMesh>, &crate::hermes::metro::MetroMap) {
        let aspect = renderer.config.width as f32 / renderer.scene_height() as f32;
        let radius = world_radius(
            renderer.construction_span,
            aspect,
            renderer.flight_visibility,
        );
        self.upload_neighborhood(
            renderer,
            center,
            radius,
            crate::performance::draw_radius(1.5),
        );
        (self.visible_batches(center, radius, preview), &self.metro)
    }

    pub fn update_winter(
        &mut self,
        renderer: &mut Renderer,
        world: &crate::dynamic_world::DynamicWorld,
        city: &opencity::runtime::City,
        eye: glam::Vec3,
        time: f32,
    ) {
        let previous = self.winter;
        self.winter = renderer.season == crate::season::Season::Winter;
        renderer.update_wheel_snow(world, city, eye, time, previous != self.winter);
        if self.winter {
            self.banks.update(renderer, eye);
        }
    }
    pub fn update_graffiti(&mut self, renderer: &Renderer, center: [f32; 2]) {
        self.paint.update(renderer, center);
    }
    pub fn visible(&self, center: [f32; 2]) -> Vec<&GpuMesh> {
        self.visible_batches(center, world_radius(None, 1., 1.), false)
    }

    fn visible_batches(&self, center: [f32; 2], world_radius: f32, preview: bool) -> Vec<&GpuMesh> {
        let intersects = |b: &Batch, radius: f32| b.distance(center) <= radius;
        self.world
            .iter()
            .filter(|b| (!preview || !b.infrastructure) && intersects(b, world_radius))
            .chain(
                self.details
                    .iter()
                    .filter(|b| intersects(b, crate::performance::draw_radius(1.5))),
            )
            .flat_map(|b| &b.meshes)
            .chain(self.banks.visible().filter(|_| self.winter && !preview))
            .chain(self.models.iter())
            .chain(self.paint.visible())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    #[ignore = "requires a native GPU adapter"]
    fn staged_upload_finishes_visible_groups_and_follows_teleports() {
        use super::{PendingLayer, Renderer};
        use opencity::runtime::{Mesh, SurfaceKind};

        let renderer = pollster::block_on(Renderer::new(None, 64, 64)).unwrap();
        let mut mesh = Mesh::default();
        // Each group contains six separate draws, more than one frame of prefetch.
        for x in [0., 10., 20.] {
            for dx in [0., 0.5, 1.] {
                for y in [0., 0.5] {
                    let x = x + dx;
                    mesh.triangle(
                        [[x, y, 0.], [x + 0.1, y, 0.], [x, y + 0.1, 0.]],
                        [1.; 3],
                        SurfaceKind::Asphalt,
                        [0.; 3],
                    );
                }
            }
        }
        let (mut batches, mut pending) = PendingLayer::partition(mesh);
        pending.upload(&renderer, &mut batches, [0., 0.], 2.);
        assert_eq!(
            batches.iter().map(|b| b.meshes.len()).collect::<Vec<_>>(),
            [6, 4, 0]
        );
        pending.upload(&renderer, &mut batches, [20., 0.], 2.);
        assert_eq!(
            batches.iter().map(|b| b.meshes.len()).collect::<Vec<_>>(),
            [6, 6, 6]
        );
        assert!(pending.groups.is_empty());
    }

    #[test]
    fn small_draw_batches_preserve_original_radius_groups_and_triangles() {
        use opencity::runtime::{Mesh, Vertex};
        let mut mesh = Mesh::default();
        for x in [1.7, 0.1, -0.1] {
            for position in [[x, 0.1, 0.], [x + 0.01, 0.1, 0.], [x, 0.11, 0.]] {
                mesh.vertices.push(Vertex {
                    position,
                    normal: [0., 0., 1.],
                    color: [1.; 3],
                    surface: [0.; 4],
                });
            }
        }
        let signature = |vertices: &[Vertex]| {
            let mut triangles: Vec<[u32; 9]> = vertices
                .chunks_exact(3)
                .map(|t| std::array::from_fn(|i| t[i / 3].position[i % 3].to_bits()))
                .collect();
            triangles.sort_unstable();
            triangles
        };
        let original = signature(&mesh.vertices);
        let allocation = mesh.vertices.as_ptr();
        let capacity = mesh.vertices.capacity();
        let groups = super::partition(&mut mesh);
        assert_eq!(allocation, mesh.vertices.as_ptr());
        assert_eq!(capacity, mesh.vertices.capacity());
        assert_eq!(original, signature(&mesh.vertices));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].1.len(), 1);
        assert_eq!(groups[1].1.len(), 2);
        // Both positive-X draws still share the original wide selection bounds.
        assert_eq!(groups[1].0, [0.1, 0.1, 1.71, 0.11]);
        assert_eq!(
            groups
                .iter()
                .flat_map(|(_, meshes, _)| meshes)
                .map(|range| range.len())
                .sum::<usize>(),
            9
        );
        assert!(groups
            .iter()
            .flat_map(|(_, meshes, _)| meshes)
            .all(|range| range.len() == 3));
    }
}
