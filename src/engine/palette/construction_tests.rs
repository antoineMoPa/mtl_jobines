use super::*;
use crate::{engine::bug_report::Report, hermes::Hermes};
use glam::Vec3;

struct Harness {
    renderer: Renderer,
    palette: Palette,
    sound: crate::audio::Settings,
    hermes: Hermes,
    world: super::super::render::GpuMesh,
    details: super::super::render::GpuMesh,
    labels: HashMap<String, egui::Pos2>,
}
impl Harness {
    fn frame(&mut self, events: Vec<egui::Event>) -> Option<PaletteAction> {
        self.renderer.construction_span = self.palette.orthographic_span();
        let (eye, look, _) = self.palette.build_camera().unwrap_or((
            Vec3::new(2.1, -1.3, 0.2),
            Vec3::new(2.1, -0.9, 0.13),
            [2.1, -1.1],
        ));
        let matrix = self.renderer.view_projection(eye, look);
        let ctx = self.palette.ctx.clone();
        let mut action = None;
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280., 800.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                action = self.palette.show_panels(
                    ui,
                    &mut self.renderer,
                    &mut self.sound,
                    matrix,
                    self.hermes.city(),
                );
            },
        );
        self.labels.clear();
        for s in &output.shapes {
            if let egui::Shape::Text(t) = &s.shape {
                self.labels
                    .insert(t.galley.text().into(), t.visual_bounding_rect().center());
            }
        }
        let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
        self.palette
            .remember_report_overlay(&action, &primitives, output.pixels_per_point);
        self.palette.painter.paint(
            &self.renderer,
            &self.target(),
            primitives,
            output.textures_delta,
            output.pixels_per_point,
        );
        action
    }
    fn target(&self) -> wgpu::TextureView {
        self.renderer
            .device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("construction UI QA"),
                size: wgpu::Extent3d {
                    width: 1280,
                    height: 800,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.renderer.config.format.add_srgb_suffix(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }
    fn click(&mut self, label: &str) -> Option<PaletteAction> {
        let pos = *self
            .labels
            .get(label)
            .unwrap_or_else(|| panic!("missing {label}"));
        let mut result = None;
        for pressed in [true, false] {
            result = self.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
        }
        result
    }
    fn capture_report(&mut self) -> Vec<u8> {
        let (eye, look, center) = self.palette.build_camera().unwrap_or((
            Vec3::new(2.1, -1.3, 0.2),
            Vec3::new(2.1, -0.9, 0.13),
            [2.1, -1.1],
        ));
        self.renderer.construction_span = self.palette.orthographic_span();
        self.renderer
            .report_screenshot_with_overlay(
                &[&self.world, &self.details],
                eye,
                look,
                0.,
                self.hermes.minimap_view(center, 0.),
                |target| self.palette.paint_report_overlay(&self.renderer, target),
            )
            .unwrap()
    }
}
fn key(key: egui::Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}

#[test]
#[ignore = "requires a native GPU adapter"]
fn construction_views_allow_bug_reports_without_editing_under_the_dialog() {
    let authored = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/maps/mtl");
    let directory =
        std::env::temp_dir().join(format!("construction-reports-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("models")).unwrap();
    let path = directory.join("map.road");
    let original = std::fs::read(authored.join("map.road")).unwrap();
    std::fs::write(&path, &original).unwrap();
    std::fs::copy(
        authored.join("elevation.png"),
        directory.join("elevation.png"),
    )
    .unwrap();
    for entry in std::fs::read_dir(authored.join("models")).unwrap() {
        let entry = entry.unwrap();
        if entry.path().is_file() {
            std::fs::copy(
                entry.path(),
                directory.join("models").join(entry.file_name()),
            )
            .unwrap();
        }
    }
    let mut hermes = Hermes::load_with_game_data(&path, None).unwrap();
    let mut renderer = pollster::block_on(Renderer::new(None, 1280, 800)).unwrap();
    renderer.daylight = Some(1.);
    renderer.reserved_bottom = 42;
    if let Some(pixels) = hermes.update_minimap([2.1, -0.9]) {
        renderer.update_minimap(pixels);
    }
    let geometry = hermes.take_geometry();
    let mut h = Harness {
        world: renderer.upload(&geometry.0),
        details: renderer.upload(&geometry.1),
        palette: Palette::headless(&renderer),
        renderer,
        sound: Default::default(),
        hermes,
        labels: Default::default(),
    };
    for building in [true, false] {
        h.palette.dismiss();
        if building {
            h.palette.buildings = Some(
                super::super::buildings::Document::open(&path, Vec3::new(2.1, -0.9, 0.122), 0.3)
                    .unwrap(),
            );
        } else {
            let mut doc = super::super::expansion::Document::open(&path).unwrap();
            doc.enable_views(Vec3::new(2.1, -0.9, 0.122), 0.3);
            h.palette.expansion = Some(doc);
        }
        for iso in [true, false] {
            h.frame(vec![]);
            h.frame(vec![]);
            if !iso {
                h.click("Street");
                h.frame(vec![]);
                assert!(h.palette.build_camera().is_none());
                assert!(h.palette.orthographic_span().is_none());
            }
            assert_eq!(h.click("[bug]"), Some(PaletteAction::BugReport));
            let screenshot = h.capture_report();
            let mut reader = png::Decoder::new(screenshot.as_slice())
                .read_info()
                .unwrap();
            let mut pixels = vec![0; reader.output_buffer_size()];
            reader.next_frame(&mut pixels).unwrap();
            let index = (799 * 1280 + 1100) * 4;
            assert_eq!(
                &pixels[index..index + 3],
                if building {
                    &[9, 14, 18]
                } else {
                    &[14, 21, 17]
                }
            );
            if let Some(directory) = std::env::var_os("CONSTRUCTION_CAPTURE") {
                let name = format!(
                    "{}-{}.png",
                    if building { "lots" } else { "roads" },
                    if iso { "iso" } else { "street" }
                );
                std::fs::create_dir_all(&directory).unwrap();
                std::fs::write(std::path::Path::new(&directory).join(name), &screenshot).unwrap();
            }
            h.palette.bug_report.begin(
                Report::new(h.hermes.city(), [2.1, -0.9, 0.122], [0.; 3], [1.; 3]),
                Ok(screenshot),
            );
            h.frame(vec![]);
            let pos = egui::pos2(1100., 200.);
            h.frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ]);
            let end = pos + egui::vec2(-200., 100.);
            h.frame(vec![
                egui::Event::PointerMoved(end),
                egui::Event::PointerButton {
                    pos: end,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
                key(egui::Key::Enter),
                key(egui::Key::ArrowRight),
            ]);
            h.frame(vec![key(egui::Key::Escape)]);
            assert!(!h.palette.bug_report.is_open());
            assert!(
                h.palette.buildings.as_ref().is_some_and(|d| d.open)
                    || h.palette.expansion.as_ref().is_some_and(|d| d.picker.open)
            );
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}
