//! Opt-in native tooling: MTL_FOOT_MOTION=1. F8 toggles the diagnostic view.
use crate::dynamic_world::pedestrian_gait::FootMotion;
use egui::{Color32, Pos2, Stroke};
use glam::{Mat4, Vec3};
use std::collections::VecDeque;

pub(super) struct FootMotionPanel {
    pub enabled: bool,
    open: bool,
    history: VecDeque<FootMotion>,
}
impl Default for FootMotionPanel {
    fn default() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let enabled = std::env::var("MTL_FOOT_MOTION").as_deref() == Ok("1");
        #[cfg(target_arch = "wasm32")]
        let enabled = false;
        Self {
            enabled,
            open: enabled,
            history: VecDeque::new(),
        }
    }
}
impl FootMotionPanel {
    pub fn show(&mut self, ctx: &egui::Context, sample: Option<FootMotion>, vp: Mat4) {
        if !self.enabled {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F8)) {
            self.open = !self.open;
        }
        if !self.open {
            self.history.clear();
            return;
        }
        let Some(sample) = sample else {
            self.history.clear();
            egui::Window::new("Foot motion (F8)")
                .open(&mut self.open)
                .show(ctx, |ui| {
                    ui.label("Leave the vehicle to inspect foot motion.");
                });
            return;
        };
        if self
            .history
            .back()
            .is_some_and(|p| p.root.distance(sample.root) > 3.)
        {
            self.history.clear();
        }
        let drift: [f32; 2] = std::array::from_fn(|i| {
            self.history.back().map_or(0., |p| {
                if p.contact[i] && sample.contact[i] {
                    p.anchors[i].distance(sample.anchors[i]) * 1000.
                } else {
                    0.
                }
            })
        });
        self.history.push_back(sample);
        while self.history.len() > 180 {
            self.history.pop_front();
        }
        let colors = [
            Color32::from_rgb(56, 190, 255),
            Color32::from_rgb(255, 170, 65),
        ];
        // World-space trails stay attached to the actual rendered feet as the camera moves.
        let screen = ctx.content_rect();
        let project = |p: Vec3| -> Option<Pos2> {
            let clip = vp * (p / crate::dynamic_world::METERS_PER_UNIT).extend(1.);
            if clip.w <= 0. || clip.z < 0. {
                return None;
            }
            let ndc = clip.truncate() / clip.w;
            Some(Pos2::new(
                screen.left() + (ndc.x + 1.) * screen.width() * 0.5,
                screen.top() + (1. - ndc.y) * screen.height() * 0.5,
            ))
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("foot_trails"),
        ));
        for side in 0..2 {
            for (a, b) in self.history.iter().zip(self.history.iter().skip(1)) {
                if let (Some(a), Some(b)) = (project(a.ankles[side]), project(b.ankles[side])) {
                    painter.line_segment([a, b], Stroke::new(2., colors[side]));
                }
            }
            if let Some(p) = project(sample.ankles[side]) {
                if sample.contact[side] {
                    painter.circle_filled(p, 5., colors[side]);
                } else {
                    painter.circle_stroke(p, 5., Stroke::new(2., colors[side]));
                }
            }
        }
        egui::Window::new("Foot motion (F8)").open(&mut self.open).default_pos([20., 110.]).show(ctx, |ui| {
            ui.label(format!("{:.2} m/s   {:.0} steps/min", sample.speed, sample.cadence));
            ui.label("Blue: left   Orange: right   Solid: contact   Ring: swing");
            for side in 0..2 {
                ui.colored_label(colors[side], format!("{}: {} | anchor drift {:.2} mm/frame", if side == 0 { "L" } else { "R" }, if sample.contact[side] { "contact" } else { "air" }, drift[side]));
            }
            ui.label("Side view: ankle paths relative to player (metres)");
            let (rect, _) = ui.allocate_exact_size(egui::vec2(340., 170.), egui::Sense::hover());
            let p = ui.painter_at(rect);
            let origin = Pos2::new(rect.center().x, rect.bottom() - 20.);
            p.line_segment([Pos2::new(rect.left(), origin.y), Pos2::new(rect.right(), origin.y)], Stroke::new(1., ui.visuals().weak_text_color()));
            for side in 0..2 {
                let points: Vec<_> = self.history.iter().map(|s| {
                    let d = s.ankles[side] - s.root;
                    Pos2::new(origin.x + (d.x * s.yaw.cos() + d.y * s.yaw.sin()) * 100., origin.y - d.z * 100.)
                }).collect();
                if points.len() > 1 { p.add(egui::Shape::line(points, Stroke::new(2., colors[side]))); }
            }
            ui.small("World trails show rendered ankles. Drift measures the IK anchor, allowing heel/toe roll. History: last 180 frames.");
            if ui.button("Clear trails").clicked() { self.history.clear(); }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_only_draw_when_explicitly_enabled() {
        let ctx = egui::Context::default();
        let mut panel = FootMotionPanel {
            enabled: false,
            open: true,
            history: VecDeque::new(),
        };
        let sample = FootMotion {
            root: Vec3::ZERO,
            yaw: 0.,
            ankles: [Vec3::X * 0.1, -Vec3::X * 0.1],
            anchors: [Vec3::ZERO; 2],
            contact: [true, false],
            speed: 1.8,
            cadence: 140.,
        };
        let mut output = ctx.run_ui(Default::default(), |ui| {
            panel.show(ui.ctx(), Some(sample), Mat4::IDENTITY)
        });
        output.textures_delta.clear();
        assert!(panel.history.is_empty());
        panel.enabled = true;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            panel.show(ui.ctx(), Some(sample), Mat4::IDENTITY)
        });
        output.textures_delta.clear();
        assert_eq!(panel.history.len(), 1);
        assert!(!output.shapes.is_empty());
        let mut output = ctx.run_ui(Default::default(), |ui| {
            panel.show(ui.ctx(), None, Mat4::IDENTITY)
        });
        output.textures_delta.clear();
        assert!(panel.history.is_empty());
    }
}
