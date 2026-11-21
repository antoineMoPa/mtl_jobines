//! Visual catalogue of the same embedded models used by the game.
use super::*;
use crate::engine::tooltip::HoverHint;
use crate::hermes::{external_models, landmark_destinations, landmarks};

#[derive(Default)]
pub(super) struct Picker {
    pub open: bool,
    pub dismissed: bool,
    search: String,
    previews: Vec<(String, &'static str, egui::TextureHandle)>,
}

impl Document {
    pub(super) fn choose_landmark(&mut self, city: &City, id: &str) -> Result<(), String> {
        let index = self.selected.ok_or("Select a building first")?;
        let raw = Arc::new(landmarks::source_mesh(id)?);
        let (lo, hi) = external_models::bounds(&raw);
        let size = hi - lo;
        let mut feature = self.items[index].feature.clone();
        let a = Vec2::from_array(feature.points[0]);
        let b = Vec2::from_array(feature.points[1]);
        let c = Vec2::from_array(feature.points[2]);
        feature.height = Some(size.z * ((b - a).length() / size.x).min((c - b).length() / size.y));
        feature.model = Some(id.into());
        let previous = self.items[index].model.replace(raw);
        if let Err(error) = self.edit(city, index, feature) {
            self.items[index].model = previous;
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn landmarks_ui(&mut self, ctx: &egui::Context, city: &City, enabled: bool) {
        self.landmark_picker.dismissed = false;
        if !self.landmark_picker.open {
            return;
        }
        if self.landmark_picker.previews.is_empty() {
            for number in 1..=30 {
                let id = format!("study_{number:02}");
                let Some(title) = landmark_destinations::title(&id) else {
                    continue;
                };
                match landmarks::source_mesh(&id) {
                    Ok(mesh) => {
                        let texture =
                            ctx.load_texture(&id, thumbnail(&mesh), egui::TextureOptions::LINEAR);
                        self.landmark_picker.previews.push((id, title, texture));
                    }
                    Err(error) => self.error = error,
                }
            }
        }
        let mut choice = None;
        let mut close = false;
        let current = self.selected().and_then(|item| item.feature.model.clone());
        let response = egui::Modal::new(egui::Id::new("building-landmark-picker"))
            .frame(egui::Frame::popup(&egui::Style {
                visuals: egui::Visuals::dark(),
                ..Default::default()
            }))
            .show(ctx, |ui| {
                *ui.visuals_mut() = egui::Visuals::dark();
                if !enabled {
                    ui.disable();
                }
                ui.set_width(590.0_f32.min(ctx.content_rect().width() - 48.0).max(180.0));
                ui.horizontal(|ui| {
                    ui.heading("Choose a landmark");
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                ui.label(
                    "Replace the selected building. Its footprint and position stay in place.",
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.landmark_picker.search)
                        .hint_text("Search landmarks…"),
                );
                let query = self.landmark_picker.search.to_lowercase();
                let columns = ((ui.available_width() / 188.0) as usize).max(1);
                let mut count = 0;
                egui::ScrollArea::vertical()
                    .min_scrolled_height((ctx.content_rect().height() - 190.0).clamp(120.0, 560.0))
                    .max_height((ctx.content_rect().height() - 190.0).max(120.0))
                    .show(ui, |ui| {
                        egui::Grid::new("landmark-cards")
                            .num_columns(columns)
                            .spacing([8., 12.])
                            .show(ui, |ui| {
                                for (id, title, texture) in &self.landmark_picker.previews {
                                    if !title.to_lowercase().contains(&query)
                                        && !id.contains(&query)
                                    {
                                        continue;
                                    }
                                    let selected = current.as_deref() == Some(id);
                                    let card = ui.vertical(|ui| {
                                        ui.set_width(180.);
                                        let image = egui::Image::new((
                                            texture.id(),
                                            egui::vec2(180., 126.),
                                        ));
                                        let clicked = ui
                                            .add(egui::Button::image(image).selected(selected))
                                            .clicked();
                                        let label = ui.add_sized(
                                            [180., 36.],
                                            egui::Button::new(*title).selected(selected),
                                        );
                                        if clicked || label.clicked() {
                                            choice = Some(id.clone());
                                        }
                                    });
                                    card.response.hover_hint(*title);
                                    count += 1;
                                    if count % columns == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        if count == 0 {
                            ui.label("No landmarks match your search.");
                        }
                    });
                if !self.error.is_empty() {
                    crate::engine::tooltip::message(ui, &self.error);
                }
            });
        if let Some(id) = choice {
            match self.choose_landmark(city, &id) {
                Ok(()) => {
                    self.error.clear();
                    close = true;
                }
                Err(error) => self.error = error,
            }
        }
        if enabled && (close || response.should_close()) {
            self.landmark_picker.open = false;
            self.landmark_picker.dismissed = true;
            self.cancel_interaction();
        }
    }
}

/// Rasterize once per catalogue entry; depth testing keeps roofs and small details readable.
fn thumbnail(mesh: &Mesh) -> egui::ColorImage {
    const W: usize = 240;
    const H: usize = 168;
    let mut image = egui::ColorImage::filled([W, H], egui::Color32::from_rgb(28, 36, 43));
    let mut depth = vec![f32::NEG_INFINITY; W * H];
    let project = |p: [f32; 3]| {
        Vec3::new(
            (p[0] - p[1]) * 0.7071,
            -(p[0] + p[1]) * 0.36 - p[2] * 0.86,
            -(p[0] + p[1]) * 0.608 + p[2] * 0.51,
        )
    };
    let points: Vec<_> = mesh.vertices.iter().map(|v| project(v.position)).collect();
    let lo = points
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |a, p| a.min(*p));
    let hi = points
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |a, p| a.max(*p));
    let scale = ((W as f32 - 20.) / (hi.x - lo.x)).min((H as f32 - 20.) / (hi.y - lo.y));
    let offset = Vec2::new(W as f32, H as f32) * 0.5 - (lo + hi).truncate() * (scale * 0.5);
    let cross = |a: Vec2, b: Vec2| a.x * b.y - a.y * b.x;
    for (triangle, vertices) in points.chunks_exact(3).zip(mesh.vertices.chunks_exact(3)) {
        let p: [Vec2; 3] = std::array::from_fn(|i| triangle[i].truncate() * scale + offset);
        let area = cross(p[1] - p[0], p[2] - p[0]);
        if area.abs() < 0.001 {
            continue;
        }
        let min = p[0].min(p[1]).min(p[2]).floor().max(Vec2::ZERO);
        let max = p[0]
            .max(p[1])
            .max(p[2])
            .ceil()
            .min(Vec2::new(W as f32 - 1., H as f32 - 1.));
        for y in min.y as usize..=max.y as usize {
            for x in min.x as usize..=max.x as usize {
                let q = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let b = cross(q - p[0], p[2] - p[0]) / area;
                let c = cross(p[1] - p[0], q - p[0]) / area;
                let a = 1. - b - c;
                if a < 0. || b < 0. || c < 0. {
                    continue;
                }
                let z = a * triangle[0].z + b * triangle[1].z + c * triangle[2].z;
                let index = y * W + x;
                if z <= depth[index] {
                    continue;
                }
                depth[index] = z;
                let n = Vec3::from_array(vertices[0].normal);
                let light = 0.55 + 0.45 * n.dot(Vec3::new(-0.4, -0.5, 0.768)).max(0.);
                let rgb: [u8; 3] = std::array::from_fn(|i| {
                    let v = a * vertices[0].color[i]
                        + b * vertices[1].color[i]
                        + c * vertices[2].color[i];
                    ((v * light).max(0.).powf(1. / 2.2) * 255.).clamp(0., 255.) as u8
                });
                image.pixels[index] = egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
            }
        }
    }
    image
}

#[cfg(test)]
mod tests {
    use super::super::tests::Fixture;
    use super::*;

    #[test]
    fn landmark_choice_persists_proportions_and_reopens_the_same_mesh() {
        let mut f = Fixture::new();
        f.place();
        let before = f.document.items[0].feature.clone();
        f.document.choose_landmark(&f.city, "study_05").unwrap();
        let feature = &f.document.items[0].feature;
        assert_eq!(feature.points, before.points);
        assert_eq!(feature.elevation, before.elevation);
        assert_eq!(feature.name, before.name);
        assert_eq!(feature.model.as_deref(), Some("study_05"));
        let reopened = Document::open(&f.path, Vec3::ZERO, 0.).unwrap();
        let actual = external_models::place(
            reopened.items[0].model.as_ref().unwrap(),
            &reopened.items[0].feature,
        )
        .unwrap();
        assert!(reopened.items[0].mesh.vertices.len() < actual.vertices.len());
        let city = City::from_program(&f.document.program, None).unwrap();
        let expected = landmarks::build(&city).unwrap();
        assert_eq!(actual.vertices.len(), expected.vertices.len());
        for (a, b) in actual.vertices.iter().zip(&expected.vertices) {
            assert!((Vec3::from_array(a.position) - Vec3::from_array(b.position)).length() < 1e-5);
        }
        assert!(f
            .document
            .source
            .starts_with("# Leave this authored comment"));
    }

    #[test]
    fn failed_landmark_save_keeps_original_building_and_model() {
        let mut f = Fixture::new();
        f.place();
        let feature = f.document.items[0].feature.clone();
        std::fs::write(&f.path, "# external edit\n").unwrap();
        assert!(f.document.choose_landmark(&f.city, "study_05").is_err());
        assert_eq!(f.document.items[0].feature, feature);
        assert!(f.document.items[0].model.is_none());
    }

    #[test]
    fn picker_click_selects_and_escape_does_not_deselect_the_building() {
        let mut f = Fixture::new();
        f.place();
        let ctx = egui::Context::default();
        let view = super::super::tests::view(&f.document);
        let frame = |doc: &mut Document, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(view.viewport),
                    events,
                    ..Default::default()
                },
                |ui| doc.show(ui, view.matrix, [900, 558], &f.city),
            );
            output.textures_delta.clear();
            output
        };
        f.document.landmark_picker.open = true;
        f.document.landmark_picker.search = "hillside".into();
        frame(&mut f.document, vec![]);
        frame(&mut f.document, vec![]);
        let output = frame(&mut f.document, vec![]);
        let position = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Hillside domed basilica" => {
                    Some(text.visual_bounding_rect().center())
                }
                _ => None,
            })
            .unwrap();
        for pressed in [true, false] {
            frame(
                &mut f.document,
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert!(!f.document.landmark_picker.open);
        assert_eq!(f.document.selected, Some(0));
        assert_eq!(
            f.document.items[0].feature.model.as_deref(),
            Some("study_05")
        );
        f.document.landmark_picker.open = true;
        frame(&mut f.document, vec![]);
        frame(
            &mut f.document,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(!f.document.landmark_picker.open);
        assert!(f.document.open);
        assert_eq!(f.document.selected, Some(0));
    }

    #[test]
    fn thumbnail_depth_shows_the_roof_instead_of_the_underside() {
        let mut mesh = Mesh::default();
        for (z, color) in [(0., [0., 1., 0.]), (1., [1., 0., 0.])] {
            for [x, y] in [
                [-5., -5.],
                [5., -5.],
                [5., 5.],
                [-5., -5.],
                [5., 5.],
                [-5., 5.],
            ] {
                mesh.vertices.push(opencity::runtime::Vertex {
                    position: [x, y, z],
                    normal: [0., 0., 1.],
                    color,
                    surface: [0.; 4],
                });
            }
        }
        let image = thumbnail(&mesh);
        let center = image.pixels[84 * 240 + 120];
        assert!(
            center.r() > 150 && center.g() == 0,
            "roof should occlude bottom: {center:?}"
        );
    }

    #[test]
    fn every_catalogue_thumbnail_contains_model_pixels() {
        let mut count = 0;
        for number in 1..=30 {
            let id = format!("study_{number:02}");
            if landmark_destinations::title(&id).is_none() {
                continue;
            }
            let mesh = landmarks::source_mesh(&id).unwrap();
            let preview = thumbnail(&mesh);
            let background = preview.pixels[0];
            assert!(
                preview.pixels.iter().filter(|&&p| p != background).count() > 100,
                "{id}"
            );
            count += 1;
        }
        assert_eq!(count, 28);
    }
}
