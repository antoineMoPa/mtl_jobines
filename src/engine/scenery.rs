//! Immutable map geometry is built per map revision, then drawn in spatial batches.
//! Driving only selects GPU buffers; it never rebuilds or rasterizes the map.
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
    world: Vec<Batch>,
    details: Vec<Batch>,
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
fn batches(renderer: &Renderer, mut mesh: Mesh) -> Vec<Batch> {
    partition(&mut mesh)
        .into_iter()
        .map(|(bounds, ranges, infrastructure)| Batch {
            infrastructure,
            bounds,
            meshes: ranges
                .into_iter()
                .map(|range| {
                    if infrastructure {
                        renderer.upload_vertices(&mesh.vertices[range])
                    } else {
                        renderer.upload_scenery_vertices(&mesh.vertices[range])
                    }
                })
                .collect(),
        })
        .collect()
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
        let _span = mtl_world::telemetry::span("map.scenery.prepare_upload");
        let (world, details) = hermes.take_geometry();
        renderer.snow.set_geometry(&[&world, &details]);
        let paint = super::graffiti::Nearby::new(renderer, &world);
        let models = hermes
            .external_models
            .iter()
            .flat_map(|group| model_batches(renderer, &group.mesh, &group.transforms))
            .collect();
        Self {
            grass: Default::default(),
            metro: crate::hermes::metro::MetroMap::from_city(hermes.city()),
            models,
            world: batches(renderer, world),
            details: batches(renderer, details),
            paint,
        }
    }
    pub fn update_graffiti(&mut self, renderer: &Renderer, center: [f32; 2]) {
        self.paint.update(renderer, center);
    }
    pub fn visible(&self, center: [f32; 2]) -> Vec<&GpuMesh> {
        self.visible_in_view(center, None, 1.)
    }
    pub fn visible_in_view(
        &self,
        center: [f32; 2],
        span: Option<f32>,
        aspect: f32,
    ) -> Vec<&GpuMesh> {
        self.visible_with_road_preview(center, span, aspect, false)
    }
    pub fn visible_with_road_preview(
        &self,
        center: [f32; 2],
        span: Option<f32>,
        aspect: f32,
        preview: bool,
    ) -> Vec<&GpuMesh> {
        let intersects = |b: &Batch, radius: f32| {
            b.bounds[0] <= center[0] + radius
                && b.bounds[2] >= center[0] - radius
                && b.bounds[1] <= center[1] + radius
                && b.bounds[3] >= center[1] - radius
        };
        self.world
            .iter()
            .filter(|b| {
                (!preview || !b.infrastructure)
                    && intersects(
                        b,
                        span.map_or(crate::performance::draw_radius(8.), |span| {
                            (span * aspect.max(1.) * 1.5).max(8.)
                        }),
                    )
            })
            .chain(
                self.details
                    .iter()
                    .filter(|b| intersects(b, crate::performance::draw_radius(1.5))),
            )
            .flat_map(|b| &b.meshes)
            .chain(self.models.iter())
            .chain(self.paint.visible())
            .collect()
    }
}

#[cfg(test)]
mod tests {
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
