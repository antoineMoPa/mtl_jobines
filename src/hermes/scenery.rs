//! Regenerable city tiles plus compressed authored overlays. No city-sized render mesh.
use super::{champlain, concrete_edges, metro, Hermes};
use flate2::{read::ZlibDecoder, write::ZlibEncoder, Compression};
use opencity::runtime::{City, Mesh, Region, Vertex};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    sync::Arc,
};
pub const SPAN: f32 = 2.;
pub type Tile = (i32, i32);
type Pages = BTreeMap<Tile, Vec<u8>>;
pub struct Source {
    city: Arc<City>,
    world: Pages,
    details: Pages,
    generated: bool,
    metro: metro::MetroMap,
}
fn bounds((x, y): Tile) -> [f32; 4] {
    [
        x as f32 * SPAN,
        y as f32 * SPAN,
        (x + 1) as f32 * SPAN,
        (y + 1) as f32 * SPAN,
    ]
}
fn mix(a: Vertex, b: Vertex, t: f32) -> Vertex {
    let blend = |a: f32, b: f32| a + (b - a) * t;
    Vertex {
        position: std::array::from_fn(|i| blend(a.position[i], b.position[i])),
        normal: std::array::from_fn(|i| blend(a.normal[i], b.normal[i])),
        color: std::array::from_fn(|i| blend(a.color[i], b.color[i])),
        surface: std::array::from_fn(|i| blend(a.surface[i], b.surface[i])),
    }
}
// Clip complete triangles, including long roads and roofs crossing many tiles.
// Attribute interpolation preserves UVs, slope data and authored material seams.
fn append_clipped(out: &mut Mesh, triangle: &[Vertex], b: [f32; 4]) {
    if (0..2).any(|a| triangle.iter().all(|v| v.position[a] == b[a + 2])) {
        return;
    }
    let mut polygon = triangle.to_vec();
    for (axis, edge, sign) in [(0, b[0], 1.), (0, b[2], -1.), (1, b[1], 1.), (1, b[3], -1.)] {
        let input = std::mem::take(&mut polygon);
        let Some(mut previous) = input.last().copied() else {
            return;
        };
        let mut d0 = (previous.position[axis] - edge) * sign;
        for current in input {
            let d1 = (current.position[axis] - edge) * sign;
            if (d0 < 0.) != (d1 < 0.) {
                let mut crossing = mix(previous, current, d0 / (d0 - d1));
                crossing.position[axis] = edge;
                polygon.push(crossing);
            }
            if d1 >= 0. {
                polygon.push(current);
            }
            previous = current;
            d0 = d1;
        }
    }
    for i in 1..polygon.len().saturating_sub(1) {
        let t = [polygon[0], polygon[i], polygon[i + 1]];
        let p: [glam::Vec3; 3] = std::array::from_fn(|i| glam::Vec3::from_array(t[i].position));
        if (p[1] - p[0]).cross(p[2] - p[0]).length_squared() > 1e-24 {
            out.vertices.extend(t);
        }
    }
}
fn pack(mesh: Mesh) -> Pages {
    let mut tiles = BTreeMap::<Tile, Mesh>::new();
    for t in mesh.vertices.chunks_exact(3) {
        let lo: [i32; 2] = std::array::from_fn(|a| {
            (t.iter()
                .map(|v| v.position[a])
                .fold(f32::INFINITY, f32::min)
                / SPAN)
                .floor() as i32
        });
        let hi: [i32; 2] = std::array::from_fn(|a| {
            (t.iter()
                .map(|v| v.position[a])
                .fold(f32::NEG_INFINITY, f32::max)
                / SPAN)
                .floor() as i32
        });
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                append_clipped(tiles.entry((x, y)).or_default(), t, bounds((x, y)));
            }
        }
    }
    // Release the input before compression, and release each tile after encoding.
    drop(mesh);
    tiles
        .into_iter()
        .map(|(key, mesh)| {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
            for v in &mesh.vertices {
                for value in v
                    .position
                    .into_iter()
                    .chain(v.normal)
                    .chain(v.color)
                    .chain(v.surface)
                {
                    encoder
                        .write_all(&value.to_le_bytes())
                        .expect("encode scenery page");
                }
            }
            (key, encoder.finish().expect("finish scenery page"))
        })
        .collect()
}
fn unpack(pages: &Pages, key: Tile, mesh: &mut Mesh) {
    if let Some(bytes) = pages.get(&key) {
        let mut bytes_out = Vec::new();
        ZlibDecoder::new(bytes.as_slice())
            .read_to_end(&mut bytes_out)
            .expect("decode scenery page");
        mesh.vertices
            .extend(bytes_out.chunks_exact(52).map(|bytes| {
                let v: [f32; 13] = std::array::from_fn(|i| {
                    f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                });
                Vertex {
                    position: v[0..3].try_into().unwrap(),
                    normal: v[3..6].try_into().unwrap(),
                    color: v[6..9].try_into().unwrap(),
                    surface: v[9..13].try_into().unwrap(),
                }
            }));
    }
}
impl Source {
    pub(super) fn new(hermes: &mut Hermes) -> Self {
        let generated = hermes.prepared.is_none();
        let (world, details) = if let Some(meshes) = hermes.prepared.take() {
            meshes
        } else {
            let mut overlays = std::mem::take(&mut hermes.landmark_mesh);
            crate::dynamic_world::airport::append_scenery(&hermes.city, &mut overlays);
            concrete_edges::add_edges(&hermes.city, &mut overlays);
            (overlays, hermes.details.take_scenery(&hermes.city))
        };
        Self {
            city: hermes.city.clone(),
            world: pack(world),
            details: pack(details),
            generated,
            metro: metro::MetroMap::from_city(&hermes.city),
        }
    }
    pub fn tile(&self, key: Tile, include_details: bool) -> (Mesh, Mesh) {
        let mut world = Mesh::default();
        if self.generated {
            let b = bounds(key);
            let raw = self
                .city
                .mesh_with_external_models(
                    Region::Radius {
                        center: [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5],
                        radius: SPAN * std::f32::consts::FRAC_1_SQRT_2 + 0.0001,
                    },
                    0.04,
                )
                .expect("bounded city mesh");
            for triangle in raw.vertices.chunks_exact(3) {
                append_clipped(&mut world, triangle, b);
            }
            champlain::remove_generated_structure(&self.city, &mut world);
            self.metro.open_portals(&mut world);
        }
        unpack(&self.world, key, &mut world);
        let mut details = Mesh::default();
        if include_details {
            unpack(&self.details, key, &mut details);
        }
        // These short-lived allocations are consumed by the renderer immediately.
        (world, details)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regional_tiles_preserve_whole_city_surface_area_and_boundary_walls() {
        let city = Arc::new(
            City::from_program(
                &opencity::parse(
                    "road local: sidewalk carlane sidewalk; end; local(-3,0,5,0);
             home=building(0.3,{use:residential},-1,0.2,2,0.2,2,1,-1,1);",
                )
                .unwrap(),
                None,
            )
            .unwrap(),
        );
        let source = Source {
            city: city.clone(),
            world: Pages::new(),
            details: Pages::new(),
            generated: true,
            metro: metro::MetroMap::from_city(&city),
        };
        let whole = city
            .mesh_with_external_models(Region::WholeMap, 0.04)
            .unwrap();
        let areas = |mesh: &Mesh| {
            let mut areas = BTreeMap::<u32, f64>::new();
            for t in mesh.vertices.chunks_exact(3) {
                let p: [glam::DVec3; 3] =
                    std::array::from_fn(|i| glam::Vec3::from_array(t[i].position).as_dvec3());
                *areas.entry(t[0].surface[0].to_bits()).or_default() +=
                    (p[1] - p[0]).cross(p[2] - p[0]).length() * 0.5;
            }
            areas
        };
        for x in -2..=2 {
            for y in -1..=1 {
                let mut expected = Mesh::default();
                for t in whole.vertices.chunks_exact(3) {
                    append_clipped(&mut expected, t, bounds((x, y)));
                }
                let (actual, _) = source.tile((x, y), false);
                let expected = areas(&expected);
                let actual = areas(&actual);
                assert_eq!(actual.len(), expected.len(), "tile {x},{y}");
                for (kind, area) in expected {
                    assert!(
                        (actual[&kind] - area).abs() < 1e-5,
                        "tile {x},{y}: {kind} {} != {area}",
                        actual[&kind]
                    );
                }
            }
        }
    }
    #[test]
    fn pages_preserve_area_uv_and_winding_across_positive_and_negative_boundaries() {
        let mut mesh = Mesh::default();
        mesh.triangle(
            [[-3., -1., 0.], [5., -1., 0.], [-3., 3., 0.]],
            [1.; 3],
            opencity::runtime::SurfaceKind::Asphalt,
            [0.; 3],
        );
        for v in &mut mesh.vertices {
            v.surface[1] = v.position[0] * 0.25;
            v.surface[2] = v.position[1] * 0.5;
        }
        let pages = pack(mesh);
        let mut area = 0.;
        for key in pages.keys() {
            let mut tile = Mesh::default();
            unpack(&pages, *key, &mut tile);
            let b = bounds(*key);
            for t in tile.vertices.chunks_exact(3) {
                let p: [glam::Vec3; 3] =
                    std::array::from_fn(|i| glam::Vec3::from_array(t[i].position));
                let cross = (p[1] - p[0]).cross(p[2] - p[0]);
                assert!(cross.z > 0.);
                area += cross.z * 0.5;
                for v in t {
                    assert!((v.surface[1] - v.position[0] * 0.25).abs() < 1e-6);
                    assert!((v.surface[2] - v.position[1] * 0.5).abs() < 1e-6);
                    assert!(
                        v.position[0] >= b[0]
                            && v.position[0] <= b[2]
                            && v.position[1] >= b[1]
                            && v.position[1] <= b[3]
                    );
                }
            }
        }
        assert!((area - 16.).abs() < 1e-5);
    }
}
