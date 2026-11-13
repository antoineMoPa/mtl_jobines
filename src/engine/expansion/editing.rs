use super::*;
use opencity::{map, Argument, Statement};

impl Document {
    pub(super) fn road_input(
        &mut self,
        ctx: &egui::Context,
        matrix: glam::Mat4,
        viewport: egui::Rect,
        enabled: bool,
    ) -> bool {
        if !enabled || !self.picker.open {
            return false;
        }
        if self.selected_road.is_some()
            && !ctx.egui_wants_keyboard_input()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.selected_road = None;
            self.picker.message.clear();
        }
        let shift_click = ctx.input(|i| i.modifiers.shift && i.pointer.primary_clicked())
            && ctx
                .pointer_hover_pos()
                .is_some_and(|p| viewport.contains(p));
        if shift_click {
            let pos = ctx.pointer_hover_pos().unwrap();
            self.selected_road = self
                .projection
                .segments
                .iter()
                .filter(|s| s.source.editable())
                .filter_map(|s| {
                    let (distance, t) = (0..64)
                        .filter_map(|i| {
                            let a = project(matrix, viewport, s, i as f32 / 64.)?;
                            let b = project(matrix, viewport, s, (i + 1) as f32 / 64.)?;
                            let d = b - a;
                            let t = ((pos - a).dot(d) / d.length_sq().max(1e-8)).clamp(0., 1.);
                            Some((pos.distance(a + d * t), (i as f32 + t) / 64.))
                        })
                        .min_by(|a, b| a.0.total_cmp(&b.0))?;
                    let half = self
                        .program
                        .statements
                        .iter()
                        .find_map(|statement| match statement {
                            Statement::Road(road) if road.name == s.road_name => Some(
                                road.elements
                                    .iter()
                                    .map(|e| map::element_world_width(e.kind))
                                    .sum::<f32>()
                                    * 0.5,
                            ),
                            _ => None,
                        })
                        .unwrap_or(0.);
                    let p = s.point_at(t);
                    let tangent = s.tangent_at(t);
                    let angle = tangent.y.atan2(tangent.x);
                    let z = s.height_at(t) + 0.004;
                    let (center, depth) = screen(matrix, viewport, glam::Vec3::new(p.x, p.y, z))?;
                    let edge = screen(
                        matrix,
                        viewport,
                        glam::Vec3::new(p.x - angle.sin() * half, p.y + angle.cos() * half, z),
                    )?
                    .0;
                    let width = center.distance(edge);
                    (distance < width + 10.).then_some((
                        (distance - width).max(0.),
                        depth,
                        s.source,
                    ))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)))
                .map(|(_, _, s)| s);
            self.picker.message = if self.selected_road.is_some() {
                "Road selected · Backspace removes · Esc clears selection".into()
            } else {
                "Shift-click near an authored road to select it".into()
            };
            ctx.request_repaint();
        }
        if !ctx.egui_wants_keyboard_input()
            && self.selected_road.is_some()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
        {
            if let Some(source) = self.selected_road.take() {
                self.remove_road(source);
                ctx.request_repaint();
            }
        }
        shift_click
    }
    fn remove_road(&mut self, source: map::SegmentSource) {
        let junctions = map::intersections(&self.program, &self.projection);
        if !map::remove(&mut self.program, source) {
            return;
        }
        // Centre-connected junctions must keep an arm count matching their remaining streets.
        for junction in junctions.into_iter().filter(|j| j.ports.is_empty()) {
            let projection = map::project(&self.program);
            let kind = match map::intersection_arms(&projection, &junction.point_ref).len() {
                4 => Some(map::IntersectionKind::FourWay),
                3 => Some(map::IntersectionKind::ThreeWay),
                2 if map::right_angle(&projection, &junction.point_ref) => {
                    Some(map::IntersectionKind::RightTurn)
                }
                _ => None,
            };
            if kind != Some(junction.kind) {
                let _ = map::set_intersection(
                    &mut self.program,
                    &junction.point_ref,
                    kind,
                    Some(&junction.road_name),
                );
            }
        }
        // Remove dangling height/stop declarations for points pruned with the road.
        map::prune_unused_points(&mut self.program);
        let names: Vec<_> = self
            .program
            .statements
            .iter()
            .filter_map(|s| match s {
                Statement::Call(c) => c.assigns.first().cloned(),
                _ => None,
            })
            .collect();
        self.program.statements.retain(|s| {
            !matches!(s, Statement::Call(c)
            if matches!(c.name.as_str(), "grade" | "elevation" | "expansion_stop")
                && matches!(c.args.first(), Some(Argument::Identifier(n)) if !names.contains(n)))
        });
        self.projection = map::project(&self.program);
        self.picker.refresh(&self.program);
    }
    pub(super) fn paint_selected(
        &self,
        ctx: &egui::Context,
        matrix: glam::Mat4,
        viewport: egui::Rect,
    ) {
        let Some(segment) = self
            .projection
            .segments
            .iter()
            .find(|s| Some(s.source) == self.selected_road)
        else {
            return;
        };
        let points: Vec<_> = (0..=64)
            .filter_map(|i| project(matrix, viewport, segment, i as f32 / 64.))
            .collect();
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("selected-road"),
        ))
        .with_clip_rect(viewport)
        .add(egui::Shape::line(
            points,
            egui::Stroke::new(5., egui::Color32::GOLD),
        ));
    }
}
fn project(
    matrix: glam::Mat4,
    viewport: egui::Rect,
    segment: &map::Segment,
    t: f32,
) -> Option<egui::Pos2> {
    let p = segment.point_at(t);
    screen(
        matrix,
        viewport,
        glam::Vec3::new(p.x, p.y, segment.height_at(t) + 0.004),
    )
    .map(|(pos, _)| pos)
}
fn screen(matrix: glam::Mat4, viewport: egui::Rect, p: glam::Vec3) -> Option<(egui::Pos2, f32)> {
    let q = matrix * p.extend(1.);
    if q.w <= 0. {
        return None;
    }
    let q = q.truncate() / q.w;
    (0. ..=1.).contains(&q.z).then(|| {
        (
            egui::pos2(
                viewport.left() + (q.x + 1.) * viewport.width() * 0.5,
                viewport.top() + (1. - q.y) * viewport.height() * 0.5,
            ),
            q.z,
        )
    })
}
