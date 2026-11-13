use super::*;
use crate::hermes::traffic_paths::{SpeedClass, TrafficPaths};
use opencity::{expansion::Kind, runtime::City};
use std::sync::atomic::{AtomicU64, Ordering};
static ID: AtomicU64 = AtomicU64::new(0);
const SOURCE: &str = include_str!("../../../opencity/tests/fixtures/expansion_junctions.road");

fn select(document: &mut Document, kind: Kind) {
    // Pick the seed's actual open end through the existing world overlay so
    // capture ordering and selection match the production icon strip.
    let ctx = egui::Context::default();
    let pos = egui::pos2(640., 379.);
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 800.),
            )),
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            ..Default::default()
        },
        |ui| document.show(ui, glam::Mat4::IDENTITY, [1280, 758]),
    );
    output.textures_delta.clear();
    document.picker.selected = document
        .picker
        .proposals
        .iter()
        .position(|p| p.kind == kind)
        .unwrap();
}
fn frame(document: &mut Document, ctx: &egui::Context, enter: bool) {
    let events = if enter {
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }]
    } else {
        vec![]
    };
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280., 800.),
            )),
            events,
            ..Default::default()
        },
        |ui| document.show(ui, glam::Mat4::IDENTITY, [1280, 758]),
    );
    output.textures_delta.clear();
}

#[test]
fn existing_confirm_flow_saves_signalized_junctions_in_the_original_strip() {
    for kind in [Kind::ThreeWay, Kind::FourWay] {
        let directory = std::env::temp_dir().join(format!(
            "junction-icons-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.road");
        std::fs::write(&path, SOURCE).unwrap();
        let mut document = Document::open(&path).unwrap();
        let ctx = egui::Context::default();
        frame(&mut document, &ctx, false);
        select(&mut document, kind);
        let before = document.program.clone();
        frame(&mut document, &ctx, false);
        assert_eq!(document.program, before, "selecting an icon only previews");
        let bounds = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 800.));
        assert_eq!(document.picker.viewport_rect(bounds).height(), 758.);
        frame(&mut document, &ctx, true);
        assert!(document.picker.message.starts_with("Saved"));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.starts_with("# Keep this authored street"));
        let program = opencity::parse(&saved).unwrap();
        let city = City::from_program(&program, None).unwrap();
        let graph = TrafficPaths::new(&city, &program);
        let count = if kind == Kind::ThreeWay { 3 } else { 4 };
        assert_eq!(graph.signals.len(), count);
        assert!(graph.signals.iter().all(|s| s.mast_base.is_some()));
        assert_eq!(
            graph
                .paths
                .iter()
                .filter(|p| p.speed != SpeedClass::Junction
                    && p.next
                        .iter()
                        .any(|&next| graph.paths[next].speed == SpeedClass::Junction))
                .count(),
            count
        );
        assert!(graph
            .paths
            .iter()
            .filter(|p| p.speed == SpeedClass::Junction)
            .all(|p| p.signal.is_some() && p.next.len() == 1));
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn zero_metre_junction_keeps_signals_and_can_be_extended_from_each_exit() {
    for kind in [Kind::ThreeWay, Kind::FourWay] {
        let mut program = opencity::parse(SOURCE).unwrap();
        let end = expansion::ends(&program)
            .into_iter()
            .find(|e| e.at_end)
            .unwrap();
        let proposal = expansion::propose(
            &program,
            &end,
            expansion::Options {
                junction_extension: 0.,
                ..Default::default()
            },
        )
        .into_iter()
        .find(|p| p.kind == kind)
        .unwrap();
        proposal.apply(&mut program).unwrap();
        let city = City::from_program(&program, None).unwrap();
        let graph = TrafficPaths::new(&city, &program);
        let count = if kind == Kind::ThreeWay { 3 } else { 4 };
        assert_eq!(graph.signals.len(), count);
        assert!(graph.signals.iter().all(|s| s.mast_base.is_some()));
        let exits: Vec<_> = expansion::ends(&program)
            .into_iter()
            .filter(|e| e.point.x > -0.5)
            .collect();
        assert_eq!(exits.len(), count - 1);
        for exit in exits {
            expansion::possibilities(&program, &exit, 1.)
                .into_iter()
                .find(|p| p.kind == Kind::Continue)
                .expect("zero-length exit remains extendable")
                .apply(&mut program)
                .unwrap();
        }
        let city = City::from_program(&program, None).unwrap();
        let graph = TrafficPaths::new(&city, &program);
        assert!(graph
            .paths
            .iter()
            .filter(|p| p.speed == SpeedClass::Junction)
            .all(|p| p.signal.is_some() && p.next.len() == 1));
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn original_icon_strip_with_city_junction_previews_and_signals() {
    use crate::{
        dynamic_world::DynamicWorld,
        engine::{palette::Painter, render::Renderer},
        hermes::Hermes,
    };
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.reserved_bottom = 42;
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("junction icons QA"),
        size: wgpu::Extent3d {
            width: 1280,
            height: 800,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: renderer.config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target = texture.create_view(&Default::default());
    for kind in [Kind::ThreeWay, Kind::FourWay] {
        let directory = std::env::temp_dir().join(format!(
            "junction-icons-gpu-{}-{kind:?}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("map.road");
        std::fs::write(&path, SOURCE).unwrap();
        let mut document = Document::open(&path).unwrap();
        document.enable_views(glam::Vec3::ZERO, 0.);
        document.iso_center = glam::Vec3::new(0.2, 0., 0.);
        document.iso_span = 1.4;
        select(&mut document, kind);
        let ctx = egui::Context::default();
        let mut painter = Painter::new(&renderer.device, renderer.config.format);
        for accepted in [false, true] {
            if accepted {
                frame(&mut document, &ctx, true);
            }
            document.iso_center = glam::Vec3::new(0.2, 0., 0.);
            let mut hermes = Hermes::test_map_from_source(&document.source);
            let simulation = DynamicWorld::new(&hermes);
            let (world, details) = hermes.take_geometry();
            let world = renderer.upload(&world);
            let details = renderer.upload(&details);
            let mut actors = opencity::runtime::Mesh::default();
            simulation.write_moving_mesh(&mut actors);
            let actors = renderer.upload(&actors);
            let mut props = crate::engine::prop_scenery::PropScenery::default();
            props.update(&renderer, &simulation);
            let mut meshes = vec![&world, &details, &actors];
            meshes.extend(props.visible());
            if let Some(pixels) = hermes.update_minimap([0.65, 0.]) {
                renderer.update_minimap(pixels);
            }
            let (eye, look, _) = document.camera().unwrap();
            renderer.construction_span = document.orthographic_span();
            let matrix = renderer.view_projection(eye, look);
            for _ in 0..3 {
                renderer.render(
                    &target,
                    &meshes,
                    eye,
                    look,
                    0.,
                    hermes.minimap_view([0.65, 0.], 0.),
                );
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280., 800.),
                        )),
                        ..Default::default()
                    },
                    |ui| document.show(ui, matrix, [1280, 758]),
                );
                painter.paint(
                    &renderer,
                    &target,
                    ctx.tessellate(output.shapes, output.pixels_per_point),
                    output.textures_delta,
                    output.pixels_per_point,
                );
            }
            let pixels = renderer.rgba_pixels(&texture).unwrap();
            let index = ((799 * 1280 + 1100) * 4) as usize;
            assert_eq!(
                &pixels[index..index + 3],
                &[27, 41, 34],
                "original green strip"
            );
            if let Some(output) = std::env::var_os("JUNCTION_ICON_CAPTURE") {
                std::fs::create_dir_all(&output).unwrap();
                let name = format!(
                    "{}-{}.png",
                    if kind == Kind::ThreeWay {
                        "three-way"
                    } else {
                        "four-way"
                    },
                    if accepted { "placed" } else { "icons" }
                );
                let mut encoder = png::Encoder::new(
                    std::fs::File::create(Path::new(&output).join(name)).unwrap(),
                    1280,
                    800,
                );
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder
                    .write_header()
                    .unwrap()
                    .write_image_data(&pixels)
                    .unwrap();
            }
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
