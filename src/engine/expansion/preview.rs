//! The road surface is inexpensive to compile; buildings and dressing follow on reload.
use crate::engine::render::{GpuMesh, Renderer};
use opencity::{
    map, network,
    road_geometry::RoadGeometry,
    runtime::{Mesh, SurfaceKind},
    Program, RoadElementKind, Statement,
};
use std::{
    collections::HashSet,
    sync::{mpsc, Arc},
};

#[derive(Default)]
pub(super) struct SurfacePreview {
    worker: Option<mpsc::Receiver<Result<Mesh, String>>>,
    mesh: Option<Arc<GpuMesh>>,
}
impl SurfacePreview {
    pub fn waiting(&self) -> bool {
        self.worker.is_some()
    }
    pub fn start(&mut self, program: Program) {
        let (sender, receiver) = mpsc::channel();
        self.worker = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(surface(&program));
        });
    }
    pub fn poll(&mut self, renderer: &Renderer) -> Option<Arc<GpuMesh>> {
        if let Some(receiver) = &self.worker {
            match receiver.try_recv() {
                Ok(result) => {
                    self.worker = None;
                    match result {
                        Ok(mesh) => self.mesh = Some(Arc::new(renderer.upload(&mesh))),
                        Err(error) => eprintln!("Road surface preview: {error}"),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => self.worker = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        self.mesh.clone()
    }
}
pub(super) fn surface(program: &Program) -> Result<Mesh, String> {
    let definitions: HashSet<_> = program
        .statements
        .iter()
        .filter_map(|s| match s {
            Statement::Road(r) => Some(r.name.as_str()),
            Statement::Block(b) => Some(b.name.as_str()),
            Statement::Network(n) => n.name.as_deref(),
            _ => None,
        })
        .collect();
    let keep = |name: &str| {
        definitions.contains(name)
            || matches!(
                name,
                "map_point"
                    | "lane_point"
                    | "lane_split"
                    | "grade"
                    | "elevation"
                    | "intersection2"
                    | "intersection3"
                    | "intersection4"
            )
    };
    let program = Program {
        statements: program
            .statements
            .iter()
            .filter_map(|s| match s {
                Statement::Call(c) if !keep(&c.name) => None,
                Statement::Block(b) => {
                    let mut b = b.clone();
                    b.calls.retain(|c| keep(&c.name));
                    Some(Statement::Block(b))
                }
                _ => Some(s.clone()),
            })
            .collect(),
    };
    let projection = map::project(&program);
    if projection.unresolved != 0 {
        return Err("Unresolved road preview".into());
    }
    let rows = RoadGeometry::placement_cross_sections_all(&program, &projection);
    let mut mesh = Mesh::default();
    for (segment, rows) in projection.segments.iter().zip(&rows) {
        let Some(road) = program.statements.iter().find_map(|s| match s {
            Statement::Road(r) if r.name == segment.road_name => Some(r),
            _ => None,
        }) else {
            continue;
        };
        for (band, element) in road.elements.iter().enumerate() {
            let sidewalk = element.kind == RoadElementKind::Sidewalk;
            let color = match element.kind {
                RoadElementKind::Sidewalk => [0.52, 0.51, 0.45],
                RoadElementKind::Verge => [0.12, 0.20, 0.09],
                _ => [0.19, 0.20, 0.20],
            };
            for pair in rows.windows(2) {
                let points = [
                    pair[0].boundaries[band],
                    pair[0].boundaries[band + 1],
                    pair[1].boundaries[band + 1],
                    pair[1].boundaries[band],
                ]
                .map(|v| {
                    [
                        v.point.x,
                        v.point.y,
                        v.height + if sidewalk { 0.0015 } else { 0. },
                    ]
                });
                mesh.quad(points, color, SurfaceKind::Prop);
            }
        }
    }
    let mut instances: Vec<_> = network::project(&program)
        .instances
        .into_iter()
        .map(|instance| (instance, None))
        .collect();
    for junction in map::intersections(&program, &projection) {
        let instance = network::default_four_way_instance(&program, &projection, &junction)
            .or_else(|| network::default_three_way_instance(&program, &projection, &junction))
            .or_else(|| network::default_right_turn_instance(&program, &projection, &junction));
        let Some(mut instance) = instance else {
            continue;
        };
        instance.origin.z = opencity::elevation_of(&program, &junction.point_ref)
            .or_else(|| {
                projection.segments.iter().find_map(|s| {
                    if s.start_ref.as_ref() == Some(&junction.point_ref) {
                        Some(s.start_z)
                    } else if s.end_ref.as_ref() == Some(&junction.point_ref) {
                        Some(s.end_z)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or(0.);
        instances.push((instance, Some(junction.footprint_half)));
    }
    for (instance, half) in instances {
        for patch in opencity::junction_mesh::patches(&instance.geometry, half) {
            let offset = if patch.material == network::Material::Sidewalk {
                0.0015
            } else if patch.kind == network::BandKind::Marking {
                0.0002
            } else {
                0.
            };
            let points: Vec<_> = patch
                .points
                .into_iter()
                .map(|p| {
                    let p = instance.world_point(p);
                    [p.x, p.y, p.z + offset]
                })
                .collect();
            let color = patch
                .material
                .rgb()
                .map(|c| (f32::from(c) / 255.).powf(2.2));
            for i in 1..points.len().saturating_sub(1) {
                mesh.triangle(
                    [points[0], points[i], points[i + 1]],
                    color,
                    SurfaceKind::Prop,
                    [0.; 3],
                );
            }
        }
    }
    Ok(mesh)
}
