use super::*;
use crate::engine::tooltip::HoverHint;
use egui::{Color32, Stroke};
const ORANGE: Color32 = Color32::from_rgb(255, 161, 20);
impl Document {
    #[cfg(test)]
    pub fn show(&mut self, root: &mut egui::Ui, matrix: glam::Mat4, pixels: [u32; 2], city: &City) {
        self.show_with_input(root, matrix, pixels, city, true);
    }
    pub fn show_with_input(
        &mut self,
        root: &mut egui::Ui,
        matrix: glam::Mat4,
        pixels: [u32; 2],
        city: &City,
        input_enabled: bool,
    ) {
        if !self.open {
            return;
        }
        let ctx = root.ctx().clone();
        self.toolbar(root, city, input_enabled);
        let view = View {
            matrix,
            viewport: egui::Rect::from_min_size(
                ctx.content_rect().min,
                egui::vec2(pixels[0] as f32, pixels[1] as f32) / ctx.pixels_per_point(),
            ),
        };
        let candidates = self.candidates(city);
        if input_enabled && !self.landmark_picker.open && !self.landmark_picker.dismissed {
            self.interaction(&ctx, city, view, &candidates);
        }
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("building-overlay"),
            ))
            .with_clip_rect(view.viewport);
        for feature in &candidates {
            footprint(
                &painter,
                view,
                feature,
                Color32::from_rgba_unmultiplied(179, 212, 217, 110),
                1.,
            );
            if let Some(p) = view.project(center(feature).extend(feature.elevation + 0.003)) {
                if view.viewport.shrink(12.).contains(p) {
                    painter.circle_filled(p, 9., Color32::from_rgb(9, 20, 29));
                    painter.circle_stroke(p, 9., Stroke::new(1., Color32::from_rgb(179, 212, 217)));
                    painter.text(
                        p,
                        egui::Align2::CENTER_CENTER,
                        "+",
                        egui::FontId::monospace(13.),
                        Color32::WHITE,
                    );
                }
            }
        }
        if let Some(item) = self.selected() {
            footprint(&painter, view, &item.feature, ORANGE, 2.);
            if self.mode == CameraMode::Build {
                for point in &item.feature.points {
                    if let Some(p) = view.project(Vec3::new(
                        point[0],
                        point[1],
                        item.feature.elevation + 0.003,
                    )) {
                        painter.rect_filled(
                            egui::Rect::from_center_size(p, egui::vec2(6., 6.)),
                            0.,
                            ORANGE,
                        );
                    }
                }
            }
        }
        if let Some(feature) = &self.draft {
            let skip = match self.gesture {
                Some(Gesture::Move { index, .. } | Gesture::Resize { index, .. }) => Some(index),
                _ => None,
            };
            let color = if self.validate(city, feature, skip).is_ok() {
                ORANGE
            } else {
                Color32::from_rgb(255, 100, 80)
            };
            footprint(&painter, view, feature, color, 2.);
            if matches!(
                self.gesture,
                Some(Gesture::Move { .. } | Gesture::Resize { .. })
            ) {
                let top = feature.elevation + feature.height.unwrap_or(0.);
                for point in &feature.points {
                    if let (Some(a), Some(b)) = (
                        view.project(Vec3::new(point[0], point[1], feature.elevation)),
                        view.project(Vec3::new(point[0], point[1], top)),
                    ) {
                        painter.line_segment([a, b], Stroke::new(1., color));
                    }
                }
                let mut roof = feature.clone();
                roof.elevation = top;
                footprint(&painter, view, &roof, color, 1.);
            }
        }
    }
    fn toolbar(&mut self, root: &mut egui::Ui, city: &City, input_enabled: bool) {
        let height = self.strip_height(root.ctx().content_rect().width());
        egui::Panel::bottom("building-tools")
            .exact_size(height)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_rgb(9, 14, 18))
                    .inner_margin(egui::Margin::symmetric(8, 6)),
            )
            .show(root, |ui| {
                *ui.visuals_mut() = egui::Visuals::dark();
                if !input_enabled {
                    ui.disable();
                }
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(237, 245, 242));
                ui.visuals_mut().selection.bg_fill = Color32::from_rgb(31, 71, 82);
                ui.spacing_mut().interact_size.y = 28.;
                ui.horizontal_wrapped(|ui| {
                    for (mode, label) in
                        [(CameraMode::Build, "Build"), (CameraMode::Street, "Street")]
                    {
                        if ui.selectable_label(self.mode == mode, label).clicked() {
                            self.mode = mode;
                            self.gesture = None;
                            self.draft = None;
                            ui.ctx().request_repaint();
                        }
                    }
                    if let Some(index) = self.selected {
                        let item = &self.items[index];
                        let model = item.feature.model.as_ref().is_some();
                        let current = item
                            .feature
                            .building_style
                            .unwrap_or_default()
                            .architecture
                            .family;
                        let mut family = None;
                        egui::ComboBox::from_id_salt("placed-building-style")
                            .width(135.)
                            .selected_text(
                                item.feature
                                    .model
                                    .as_deref()
                                    .and_then(crate::hermes::landmark_destinations::title)
                                    .unwrap_or(if model { "Custom .glb" } else { label(current) }),
                            )
                            .show_ui(ui, |ui| {
                                for value in BuildingFamily::ALL {
                                    if ui
                                        .selectable_label(!model && current == value, label(value))
                                        .clicked()
                                    {
                                        family = Some(value);
                                    }
                                }
                            });
                        if let Some(family) = family {
                            let mut feature = self.items[index].feature.clone();
                            feature.model = None;
                            let style = feature.building_style.get_or_insert_default();
                            style.architecture.family = family;
                            style.material = material(family);
                            let old = self.items[index].model.take();
                            if let Err(error) = self.edit(city, index, feature) {
                                self.items[index].model = old;
                                self.error = error;
                            }
                        }
                        let feature = &self.items[index].feature;
                        let floor_height = feature.building_style.unwrap_or_default().floor_height;
                        let mut floors = (feature.height.unwrap_or(floor_height) / floor_height)
                            .round()
                            .max(1.) as u32;
                        ui.label("Floors");
                        if ui
                            .add(
                                egui::DragValue::new(&mut floors)
                                    .speed(0.1)
                                    .range(1..=300),
                            )
                            .hover_hint(format!(
                                "Floor count; drag or click to type. {:.2} m per floor. Keeps the footprint unchanged.",
                                floor_height * 100.
                            ))
                            .changed()
                        {
                            let mut feature = self.items[index].feature.clone();
                            feature.height = Some(floors as f32 * floor_height);
                            self.error = self.edit(city, index, feature).err().unwrap_or_default();
                        }
                        let source_height = self.items[index]
                            .model
                            .as_ref()
                            .map(|mesh| {
                                let (lo, hi) = crate::hermes::external_models::bounds(mesh);
                                hi.z - lo.z
                            })
                            .unwrap_or(0.1);
                        let current_scale =
                            self.items[index].feature.height.unwrap_or(0.1) / source_height;
                        let mut scale = current_scale;
                        ui.label("Scale");
                        if ui
                            .add(
                                egui::DragValue::new(&mut scale)
                                    .speed(0.01)
                                    .range(0.01..=100.)
                                    .suffix("×"),
                            )
                            .changed()
                            && scale.is_finite()
                            && current_scale > 0.
                        {
                            let feature =
                                geometry::scaled(&self.items[index].feature, scale / current_scale);
                            self.error = self.edit(city, index, feature).err().unwrap_or_default();
                        }
                        if model {
                            let points = &self.items[index].feature.points;
                            let edge = Vec2::from_array(points[1]) - Vec2::from_array(points[0]);
                            let mut angle = edge.y.atan2(edge.x).to_degrees();
                            ui.label("Angle");
                            if ui
                                .add(egui::DragValue::new(&mut angle).speed(5.).suffix("°"))
                                .hover_hint("Drag to rotate; hold Shift for precision or click to type an angle.")
                                .changed()
                                && angle.is_finite()
                            {
                                let feature = geometry::rotated(&self.items[index].feature, angle);
                                self.error =
                                    self.edit(city, index, feature).err().unwrap_or_default();
                            }
                        }
                        let ground = city
                            .surface_at(
                                center(&self.items[index].feature).to_array(),
                                f32::INFINITY,
                            )
                            .map_or(0., |surface| surface.height);
                        let mut vertical_offset =
                            (self.items[index].feature.elevation - ground) * 100.;
                        ui.label("Vertical offset");
                        if ui
                            .add(
                                egui::DragValue::new(&mut vertical_offset)
                                    .speed(0.1)
                                    .suffix(" m"),
                            )
                            .changed()
                            && vertical_offset.is_finite()
                        {
                            let mut feature = self.items[index].feature.clone();
                            feature.elevation = ground + vertical_offset / 100.;
                            self.error = self.edit(city, index, feature).err().unwrap_or_default();
                        }
                        if ui.button("Landmarks…").clicked() {
                            self.landmark_picker.open = true;
                            self.cancel_interaction();
                        }
                        if ui.button(".glb").clicked() {
                            self.importing = true;
                        }
                        if ui
                            .button("Delete")
                            .hover_hint("Backspace / Delete")
                            .clicked()
                        {
                            self.error = self.delete(index).err().unwrap_or_default();
                        }
                    }
                    if ui.button("Done").clicked() {
                        if self.selected.take().is_none() {
                            self.open = false;
                        }
                    }
                    if !self.error.is_empty() {
                        crate::engine::tooltip::message(ui, &self.error);
                    }
                });
            });
        self.landmarks_ui(root.ctx(), city, input_enabled);
        if std::mem::take(&mut self.importing) {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Building model", &["glb"])
                .pick_file()
            {
                self.error = self.import_model(city, &path).err().unwrap_or_default();
                root.ctx().request_repaint();
            }
        }
    }
}
fn footprint(
    painter: &egui::Painter,
    view: View,
    feature: &LandscapeFeature,
    color: Color32,
    width: f32,
) {
    let points: Option<Vec<_>> = feature
        .points
        .iter()
        .map(|p| view.project(Vec3::new(p[0], p[1], feature.elevation + 0.002)))
        .collect();
    if let Some(points) = points {
        painter.add(egui::Shape::closed_line(points, Stroke::new(width, color)));
    }
}
