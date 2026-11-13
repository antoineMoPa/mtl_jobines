use super::*;
use opencity::runtime::Mesh;

fn signature(v: &Vertex) -> [u32; 13] {
    let mut values = v
        .position
        .iter()
        .chain(&v.normal)
        .chain(&v.color)
        .chain(&v.surface);
    std::array::from_fn(|_| values.next().unwrap().to_bits())
}
#[test]
fn clustering_preserves_protected_art_contacts_cutouts_lamps_and_bounds() {
    let mut source = Mesh::default();
    for x in 0..20 {
        for y in 0..20 {
            source.cuboid(
                [x as f32 * 0.01, y as f32 * 0.01, 0.],
                [x as f32 * 0.01 + 0.009, y as f32 * 0.01 + 0.009, 0.009],
                [0.4, 0.3, 0.2],
                SurfaceKind::Prop,
            );
        }
    }
    for kind in [0., 1., 2., 4., 5., 6., 8., 9., 11., 14., 40., 41., 42.] {
        let mut triangle = source.vertices[..3].to_vec();
        for v in &mut triangle {
            v.surface = [kind, 0.21, 0.73, -4.2];
            v.position[2] += kind * 0.01;
        }
        source.vertices.extend(triangle);
    }
    let reduced = reduce(&source.vertices, 0.05, Policy::Scenery);
    let repeated = reduce(&source.vertices, 0.05, Policy::Scenery);
    assert_eq!(
        reduced.vertices.iter().map(signature).collect::<Vec<_>>(),
        repeated.vertices.iter().map(signature).collect::<Vec<_>>()
    );
    assert!(reduced.vertices.len() < source.vertices.len() / 2);
    let preserved: Vec<_> = source
        .vertices
        .iter()
        .filter(|v| v.surface[0] != 7.)
        .map(signature)
        .collect();
    let actual: Vec<_> = reduced
        .vertices
        .iter()
        .filter(|v| v.surface[0] != 7.)
        .map(signature)
        .collect();
    assert_eq!(preserved, actual);
    assert!(reduced.error <= 3_f32.sqrt() * 0.05 + 0.000001);
    for axis in 0..3 {
        let extent = |vertices: &[Vertex]| {
            vertices
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
                    (lo.min(v.position[axis]), hi.max(v.position[axis]))
                })
        };
        assert_eq!(extent(&source.vertices), extent(&reduced.vertices));
    }
    assert_eq!(reduced.vertices.len() % 3, 0);
}
#[test]
fn indexed_shadows_weld_color_and_normal_seams_but_keep_coverage_and_material_ranges() {
    let mut source = Mesh::default();
    source.cuboid(
        [0., 0., 0.],
        [1., 1., 1.],
        [0.2, 0.4, 0.6],
        SurfaceKind::Prop,
    );
    let camera = Indexed::new(&source.vertices, 36, 36, Purpose::Camera);
    let shadow = Indexed::new(&source.vertices, 36, 36, Purpose::Shadow);
    assert_eq!(camera.indices.len(), 36);
    assert_eq!(shadow.indices.len(), 36);
    assert!(shadow.vertices.len() < camera.vertices.len());
    let original: Vec<_> = source.vertices.iter().map(signature).collect();
    assert_eq!(
        camera
            .indices
            .iter()
            .map(|&i| signature(&camera.vertices[i as usize]))
            .collect::<Vec<_>>(),
        original
    );
    for (i, kind) in [11., 42.].into_iter().enumerate() {
        let mut triangle = source.vertices[..3].to_vec();
        for v in &mut triangle {
            v.surface = [kind, 0.4 + i as f32, 0.2, 3.];
        }
        source.vertices.extend(triangle);
    }
    let indexed = Indexed::new(&source.vertices, 36, 39, Purpose::Camera);
    assert_eq!(
        (indexed.opaque, indexed.paint, indexed.indices.len()),
        (36, 39, 42)
    );
    let no_paint = Indexed::new(&source.vertices[..39], 36, 39, Purpose::Shadow);
    assert_eq!(no_paint.paint as usize, no_paint.indices.len());
    assert!(no_paint.vertices.iter().all(|v| v.surface[0] != 42.));
}
#[test]
fn every_authored_vehicle_reduces_without_moving_glass_or_headlight_vertices() {
    for definition in crate::dynamic_world::catalog::definitions() {
        for source in definition.part_vertices() {
            let reduced = reduce(source, 0.10, Policy::Vehicle);
            assert!(reduced.vertices.len() <= source.len());
            assert!(reduced
                .vertices
                .iter()
                .all(|v| v.position.iter().all(|f| f.is_finite())));
            assert!(reduced.error <= 0.10 * 3_f32.sqrt() + 0.00001);
            let exact = |vertices: &[Vertex]| {
                vertices
                    .chunks_exact(3)
                    .filter(|t| protected(t, Policy::Vehicle))
                    .flatten()
                    .map(signature)
                    .collect::<Vec<_>>()
            };
            assert_eq!(exact(source), exact(&reduced.vertices));
        }
    }
}
