use super::*;
use egui::{Color32, Stroke};
const ORANGE: Color32 = Color32::from_rgb(255, 161, 20);
impl Document {
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
        let revision = self.revision;
        if self.samples.is_empty() && self.gpu_revision.is_none() {
            self.rebuild(city);
        }
        self.toolbar(root, city, input_enabled);
        let ctx = root.ctx().clone();
        let view = View {
            matrix,
            viewport: egui::Rect::from_min_size(
                ctx.content_rect().min,
                egui::vec2(pixels[0] as f32, pixels[1] as f32) / ctx.pixels_per_point(),
            ),
        };
        if input_enabled {
            self.interaction(&ctx, city, view);
        }
        if revision != self.revision {
            ctx.request_repaint();
        }
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Background,
                egui::Id::new("prop-overlay"),
            ))
            .with_clip_rect(view.viewport);
        for member in self.active() {
            if let Some(s) = self.samples.iter().find(|s| s.member == member) {
                let r = s.placement.kind.radius() + 0.0005;
                let points: Option<Vec<_>> = (0..24)
                    .map(|i| {
                        let angle = i as f32 * std::f32::consts::TAU / 24.;
                        view.project(
                            s.placement.position
                                + Vec3::new(angle.cos() * r, angle.sin() * r, 0.001),
                        )
                    })
                    .collect();
                if let Some(points) = points {
                    painter.add(egui::Shape::closed_line(points, Stroke::new(2., ORANGE)));
                }
            }
        }
        if let Some(row) = self.row() {
            let points: Option<Vec<_>> = (0..65)
                .map(|i| view.project(row.point(i as f32 / 64.).extend(row.elevation + 0.002)))
                .collect();
            if let Some(points) = points {
                painter.add(egui::Shape::line(points, Stroke::new(1., ORANGE)));
            }
            for (i, p) in [row.a, row.b, row.control].into_iter().enumerate() {
                if let Some(p) = view.project(p.extend(row.elevation + 0.002)) {
                    painter.circle_filled(p, 5., Color32::from_rgb(9, 14, 18));
                    painter.circle_stroke(p, 5., Stroke::new(2., ORANGE));
                    if i == 2 {
                        painter.text(
                            p + egui::vec2(9., 0.),
                            egui::Align2::LEFT_CENTER,
                            "Bend",
                            egui::FontId::proportional(11.),
                            ORANGE,
                        );
                    }
                }
            }
        }
        if let Some(Gesture::Box { start, current, .. }) = &self.gesture {
            painter.rect_stroke(
                egui::Rect::from_two_pos(*start, *current),
                0.,
                Stroke::new(1., ORANGE),
                egui::StrokeKind::Inside,
            );
        }
        if let Some(draft) = &self.draft {
            if let Ok(placements) = draft.placements(city) {
                for (_, p) in placements {
                    if let Some(at) = view.project(p.position + Vec3::Z * 0.002) {
                        painter.circle_stroke(at, 4., Stroke::new(1.5, ORANGE));
                    }
                }
            }
        }
    }
    fn toolbar(&mut self, root: &mut egui::Ui, city: &City, input_enabled: bool) {
        let height = self.strip_height(root.ctx().content_rect().width());
        egui::Panel::bottom("prop-tools")
            .exact_size(height)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(Color32::from_rgb(14, 21, 17))
                    .inner_margin(egui::Margin::symmetric(8, 6)),
            )
            .show(root, |ui| {
                *ui.visuals_mut() = egui::Visuals::dark();
                ui.visuals_mut().override_text_color = Some(Color32::from_rgb(237, 245, 242));
                ui.visuals_mut().selection.bg_fill = Color32::from_rgb(31, 71, 82);
                ui.spacing_mut().interact_size.y = 28.;
                if !input_enabled {
                    ui.disable();
                }
                ui.horizontal_wrapped(|ui| {
                    for (mode, label) in [(Mode::Street, "Street"), (Mode::Iso, "Iso")] {
                        if ui.selectable_label(self.mode == mode, label).clicked() {
                            self.cancel_interaction();
                            self.mode = mode;
                            self.rebuild(city);
                        }
                    }
                    ui.separator();
                    egui::ComboBox::from_id_salt("prop-kind")
                        .width(75.)
                        .selected_text(self.kind.label())
                        .show_ui(ui, |ui| {
                            for kind in Kind::ALL {
                                if ui
                                    .selectable_label(self.kind == kind, kind.label())
                                    .clicked()
                                {
                                    self.kind = kind;
                                    self.spacing = kind.spacing();
                                    self.cancel_interaction();
                                }
                            }
                        });
                    for (tool, label) in [
                        (Tool::Select, "Select"),
                        (Tool::Single, "Single"),
                        (Tool::Row, "Row"),
                    ] {
                        if ui.selectable_label(self.tool == tool, label).clicked() {
                            self.cancel_interaction();
                            self.tool = tool;
                            self.selected.clear();
                            self.selected_row = None;
                        }
                    }
                    if let Some(index) = self.selected_row {
                        if let Shape::Row { row, .. } = &self.entries[index].shape {
                            let mut spacing = row.spacing;
                            ui.label("Spacing");
                            let before = self.entries.clone();
                            if ui
                                .add(
                                    egui::DragValue::new(&mut spacing)
                                        .speed(0.1)
                                        .range(0.6..=50.)
                                        .suffix(" m"),
                                )
                                .changed()
                            {
                                if let Shape::Row { row, .. } = &mut self.entries[index].shape {
                                    row.spacing = spacing;
                                }
                                self.accept(city, before);
                            }
                        }
                    } else if self.tool == Tool::Row {
                        ui.label("Spacing");
                        ui.add(
                            egui::DragValue::new(&mut self.spacing)
                                .speed(0.1)
                                .range(0.6..=50.)
                                .suffix(" m"),
                        );
                    }
                    if self.selected.len() == 1 {
                        let member = self.selected[0];
                        if matches!(self.entries[member.entry].shape, Shape::Row { .. })
                            && ui.button("Select row").clicked()
                        {
                            self.selected_row = Some(member.entry);
                            self.selected.clear();
                        }
                        let mut angle = self
                            .samples
                            .iter()
                            .find(|s| s.member == member)
                            .map_or(0., |s| s.placement.heading);
                        let old = angle;
                        if ui
                            .add(egui::DragValue::new(&mut angle).speed(1.).suffix("°"))
                            .changed()
                            && angle.is_finite()
                        {
                            let before = self.entries.clone();
                            match &mut self.entries[member.entry].shape {
                                Shape::Single(p) => p.heading = angle,
                                Shape::Row { row, .. } => {
                                    row.offsets.entry(member.index).or_default().z += angle - old
                                }
                            }
                            self.accept(city, before);
                        }
                    }
                    if !self.active().is_empty() {
                        if ui.button("Duplicate").clicked() {
                            self.duplicate(city);
                        }
                        if ui.button("Delete").clicked() {
                            self.delete(city);
                        }
                    }
                    if ui
                        .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                        .clicked()
                    {
                        self.history(city, false);
                    }
                    if ui
                        .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                        .clicked()
                    {
                        self.history(city, true);
                    }
                    if ui.button("Done").clicked() {
                        self.cancel_interaction();
                        self.open = false;
                    }
                    if !self.error.is_empty() {
                        crate::engine::tooltip::message(ui, &self.error);
                    }
                });
                let entry = self
                    .selected_row
                    .or_else(|| (self.selected.len() == 1).then(|| self.selected[0].entry));
                if let Some(index) = entry {
                    if let Some(current) = self.entries[index].sign_mut().cloned() {
                        if self.sign_draft.as_ref().is_none_or(|(i, _)| *i != index) {
                            self.sign_draft = Some((index, current.clone()));
                        }
                        let draft = &mut self.sign_draft.as_mut().unwrap().1;
                        let apply = ui
                            .horizontal_wrapped(|ui| {
                                draft.edit(ui);
                                let error = draft.validate().err();
                                let apply = ui
                                    .add_enabled(
                                        *draft != current && error.is_none(),
                                        egui::Button::new("Apply sign"),
                                    )
                                    .clicked();
                                if let Some(error) = error {
                                    crate::engine::tooltip::message(ui, &error);
                                }
                                apply
                            })
                            .inner;
                        if apply {
                            let before = self.entries.clone();
                            *self.entries[index].sign_mut().unwrap() = draft.clone();
                            self.accept(city, before);
                            self.sign_draft = None;
                        }
                    } else {
                        self.sign_draft = None;
                    }
                } else {
                    self.sign_draft = None;
                }
            });
    }
}
