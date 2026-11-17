//! Manual native capture of authored map signs for the task's HTML QA report.
use super::*;
use crate::{
    hermes::Hermes,
    props::{Kind, SignDesign},
};
use glam::Vec3;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
};

fn family(kind: Kind) -> usize {
    match kind {
        Kind::StopSign => 0,
        Kind::SpeedSign => 1,
        Kind::OneWaySign => 2,
        Kind::YieldSign => 3,
        Kind::CurveSign => 4,
        Kind::HighwaySign => 5,
        _ => unreachable!(),
    }
}
#[test]
#[ignore = "manual native GPU report capture; set ROAD_SIGN_QA_DIR"]
fn native_capture_thirty_map_signs_for_qa() {
    let output = PathBuf::from(std::env::var_os("ROAD_SIGN_QA_DIR").expect("ROAD_SIGN_QA_DIR"));
    std::fs::create_dir_all(&output).unwrap();
    let source = std::fs::read_to_string("data/maps/mtl/map.road").unwrap();
    let program = opencity::parse(&source).unwrap();
    let entries = crate::props::read(&program).unwrap();
    let mut hermes =
        Hermes::load_with_game_data(std::path::Path::new("data/maps/mtl/map.road"), None).unwrap();
    let visible = crate::props::visible_placements(&entries, hermes.city());
    let visible_ids: HashSet<_> = visible.iter().map(|(e, i, _)| (*e, *i)).collect();
    let mut buckets: [Vec<_>; 6] = std::array::from_fn(|_| Vec::new());
    let mut suppressed = Vec::new();
    for (entry, e) in entries.iter().enumerate() {
        if SignDesign::for_kind(e.kind()).is_none() {
            continue;
        }
        for (member, p) in e.placements(hermes.city()).unwrap() {
            let hidden = !visible_ids.contains(&(entry, member));
            if hidden {
                suppressed.push(e.name.clone());
            }
            buckets[family(p.kind)].push((entry, member, p, hidden));
        }
    }
    for bucket in &mut buckets {
        bucket.sort_by_key(|(_, _, _, hidden)| *hidden);
    }
    let mut selected = Vec::new();
    let mut cursor = [0; 6];
    while selected.len() < 30 {
        let before = selected.len();
        for f in 0..6 {
            if let Some(p) = buckets[f].get(cursor[f]) {
                selected.push(p.clone());
                cursor[f] += 1;
            }
            if selected.len() == 30 {
                break;
            }
        }
        assert!(selected.len() > before, "map needs 30 sign placements");
    }
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 720)).unwrap();
    renderer.daylight = Some(1.);
    renderer.render_lod = crate::engine::render::RenderLod::Full;
    let scenery = crate::engine::scenery::Scenery::new(&renderer, &mut hermes);
    // Keep only one GPU upload per shared source, using the game's prop instance pipeline.
    let mut models = HashMap::<usize, (Arc<Mesh>, GpuMesh, Vec<[[f32; 4]; 4]>)>::new();
    let mut records = Vec::new();
    for (number, (entry, member, p, hidden)) in selected.into_iter().enumerate() {
        for (_, _, transforms) in models.values_mut() {
            transforms.clear();
        }
        for (_, _, neighbor) in &visible {
            if neighbor.position.truncate().distance(p.position.truncate()) > 0.7 {
                continue;
            }
            for part in neighbor.parts() {
                let id = Arc::as_ptr(&part.mesh) as usize;
                let model = models.entry(id).or_insert_with(|| {
                    let mut vertices = part.mesh.vertices.clone();
                    for v in &mut vertices {
                        v.position = v.position.map(|x| x * crate::props::UNITS);
                    }
                    let gpu = renderer.upload_instanced(&vertices);
                    (part.mesh.clone(), gpu, Vec::new())
                });
                model.2.push(
                    (part.transform * glam::Mat4::from_scale(Vec3::splat(0.01))).to_cols_array_2d(),
                );
            }
        }
        for (_, gpu, transforms) in models.values_mut() {
            renderer.update_instances(gpu, transforms);
        }
        let design = p
            .sign
            .clone()
            .unwrap_or_else(|| SignDesign::for_kind(p.kind).unwrap());
        let panel = design
            .profiles()
            .into_iter()
            .find(|part| !part.pole)
            .unwrap();
        let look = p
            .transform()
            .transform_point3(panel.center / crate::props::UNITS);
        let front = p.transform().transform_vector3(-Vec3::Y);
        let eye = p.position
            + front
                * if p.kind == Kind::HighwaySign {
                    0.14
                } else {
                    0.065
                }
            + Vec3::Z
                * if p.kind == Kind::HighwaySign {
                    0.032
                } else {
                    0.025
                };
        let mut meshes = scenery.visible(p.position.truncate().to_array());
        meshes.extend(
            models
                .values()
                .filter(|(_, _, t)| !t.is_empty())
                .map(|(_, gpu, _)| gpu),
        );
        let png = renderer
            .report_screenshot(
                &meshes,
                eye,
                look,
                0.,
                hermes.minimap_view(p.position.truncate().to_array(), p.heading.to_radians()),
            )
            .unwrap();
        let file = format!("sign-{:02}.png", number + 1);
        std::fs::write(output.join(&file), png).unwrap();
        records.push(serde_json::json!({"number":number+1,"name":entries[entry].name,"member":member,"kind":p.kind.keyword(),"label":p.kind.label(),"design":format!("{design:?}"),"position":p.position.to_array(),"heading":p.heading,"hidden":hidden,"file":file}));
        eprintln!(
            "QA sign {}/30: {}{}",
            number + 1,
            entries[entry].name,
            if hidden {
                " (hidden by visibility guard)"
            } else {
                ""
            }
        );
    }
    std::fs::write(output.join("manifest.json"),serde_json::to_vec_pretty(&serde_json::json!({"signs":records,"suppressed":suppressed,"render":"Native game renderer, authored map positions; traffic and actors omitted","map":"data/maps/mtl/map.road"})).unwrap()).unwrap();
}
