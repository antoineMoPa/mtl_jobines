//! Native authored-map editing; the existing reload watcher compiles accepted roads.
use opencity::{expansion, expansion_ui::Picker, Program};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
enum ViewMode {
    Street,
    Iso,
}

#[derive(Clone, Copy)]
struct BuildCamera {
    eye: glam::Vec3,
    look: glam::Vec3,
    center: [f32; 2],
}
impl BuildCamera {
    fn at(target: opencity::expansion_ui::FollowTarget) -> Self {
        let tip = glam::Vec3::from_array(target.position);
        let forward = glam::Vec3::new(target.heading.cos(), target.heading.sin(), 0.);
        let distance = (target.length * 0.75).clamp(0.12, 5.);
        Self {
            eye: tip - forward * distance + glam::Vec3::Z * (distance * 0.65).max(0.08),
            look: tip + forward * (distance * 0.35),
            center: [tip.x, tip.y],
        }
    }
}

pub(super) struct Document {
    pub picker: Picker,
    path: PathBuf,
    source: String,
    program: Program,
    camera: Option<BuildCamera>,
    view_mode: Option<ViewMode>,
    iso_center: glam::Vec3,
    iso_span: f32,
    focused_end: Option<glam::Vec3>,
    pan: Option<egui::Pos2>,
}
impl Document {
    pub fn open(path: &Path) -> Result<Self, String> {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let program = opencity::parse(&source).map_err(|e| e.to_string())?;
        let mut picker = Picker::default();
        picker.start();
        Ok(Self {
            picker,
            path: path.into(),
            source,
            program,
            camera: None,
            view_mode: None,
            iso_center: glam::Vec3::ZERO,
            iso_span: 1.2,
            focused_end: None,
            pan: None,
        })
    }
    pub fn camera(&self) -> Option<(glam::Vec3, glam::Vec3, [f32; 2])> {
        if !self.picker.open || self.view_mode == Some(ViewMode::Street) {
            return None;
        }
        if self.view_mode == Some(ViewMode::Iso) {
            return Some((
                self.iso_center + glam::Vec3::new(-0.7, -0.7, 0.85) * self.iso_span,
                self.iso_center,
                [self.iso_center.x, self.iso_center.y],
            ));
        }
        self.camera
            .filter(|_| self.picker.open)
            .map(|camera| (camera.eye, camera.look, camera.center))
    }
    pub fn enable_views(&mut self, player: glam::Vec3, heading: f32) {
        self.view_mode = Some(ViewMode::Iso);
        self.iso_center = player + glam::Vec3::new(heading.cos(), heading.sin(), 0.) * 0.25;
    }
    pub fn orthographic_span(&self) -> Option<f32> {
        (self.picker.open && self.view_mode == Some(ViewMode::Iso)).then_some(self.iso_span)
    }
    #[cfg(test)]
    pub fn show(&mut self, root: &mut egui::Ui, view: glam::Mat4, scene_pixels: [u32; 2]) {
        self.show_with_input(root, view, scene_pixels, true);
    }
    pub fn show_with_input(
        &mut self,
        root: &mut egui::Ui,
        view: glam::Mat4,
        scene_pixels: [u32; 2],
        input_enabled: bool,
    ) {
        let ctx = root.ctx().clone();
        let before = self.program.clone();
        let mut view_mode = self.view_mode;
        let changed = root
            .add_enabled_ui(input_enabled, |root| {
                self.picker
                    .show_with_toolbar(root, &mut self.program, true, |ui| {
                        if let Some(mode) = &mut view_mode {
                            ui.selectable_value(mode, ViewMode::Street, "Street");
                            ui.selectable_value(mode, ViewMode::Iso, "Iso");
                            ui.separator();
                        }
                    })
            })
            .inner;
        if view_mode != self.view_mode {
            self.view_mode = view_mode;
            self.pan = None;
            ctx.request_repaint();
        }
        let viewport = egui::Rect::from_min_size(
            ctx.content_rect().min,
            egui::vec2(scene_pixels[0] as f32, scene_pixels[1] as f32) / ctx.pixels_per_point(),
        );
        if input_enabled {
            self.iso_input(&ctx, view, viewport);
        } else {
            self.pan = None;
        }
        self.picker
            .world_overlay_with_input(&ctx, &self.program, viewport, input_enabled, |p| {
                let v = view * glam::Vec4::new(p[0], p[1], p[2] + 0.003, 1.);
                if v.w <= 0. {
                    return None;
                }
                let p = v.truncate() / v.w;
                let rect = viewport;
                (p.z >= 0. && p.z <= 1.).then(|| {
                    egui::pos2(
                        rect.left() + (p.x + 1.) * rect.width() / 2.,
                        rect.top() + (1. - p.y) * rect.height() / 2.,
                    )
                })
            });
        if self.view_mode.is_some() {
            if let Some(proposal) = self.picker.selected() {
                let end = glam::Vec3::new(
                    proposal.end.point.x,
                    proposal.end.point.y,
                    proposal.end.height,
                );
                if self.focused_end != Some(end) {
                    self.focused_end = Some(end);
                    let tip = proposal
                        .ghosts
                        .last()
                        .map_or(end, |s| glam::Vec3::new(s.end.x, s.end.y, s.end_z));
                    self.iso_center = (end + tip) * 0.5;
                    self.iso_span = self.iso_span.max(end.distance(tip) * 1.4);
                    ctx.request_repaint();
                }
            }
        }
        if !changed {
            return;
        }
        let result = (|| {
            if std::fs::read_to_string(&self.path).map_err(|e| e.to_string())? != self.source {
                return Err("Map changed on disk. Close and reopen Expand road network.".into());
            }
            let source = expansion::edited_source(&self.source, &before, &self.program)?;
            let temp = self.path.with_extension("road.expansion-tmp");
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| e.to_string())?;
            use std::io::Write;
            let saved = (|| {
                file.write_all(source.as_bytes())
                    .map_err(|e| e.to_string())?;
                file.sync_all().map_err(|e| e.to_string())?;
                if std::fs::read_to_string(&self.path).map_err(|e| e.to_string())? != self.source {
                    return Err("Map changed during save; reopen the picker".into());
                }
                std::fs::rename(&temp, &self.path).map_err(|e| e.to_string())
            })();
            if saved.is_err() {
                let _ = std::fs::remove_file(&temp);
            }
            saved?;
            self.source = source;
            Ok::<_, String>(())
        })();
        match result {
            Ok(()) => {
                self.picker.message = "Saved extension; map reload will apply it.".into();
                if let Some(target) = self.picker.take_follow_target() {
                    self.camera = Some(BuildCamera::at(target));
                }
            }
            Err(e) => {
                self.program = before;
                self.picker.refresh(&self.program);
                self.picker.message = e;
            }
        }
        ctx.request_repaint();
    }
    fn iso_input(&mut self, ctx: &egui::Context, matrix: glam::Mat4, viewport: egui::Rect) {
        if self.view_mode != Some(ViewMode::Iso) || !self.picker.open {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.interact_pos()) else {
            self.pan = None;
            return;
        };
        let ground = |p: egui::Pos2| {
            let x = (p.x - viewport.left()) / viewport.width() * 2. - 1.;
            let y = 1. - (p.y - viewport.top()) / viewport.height() * 2.;
            let inverse = matrix.inverse();
            let a = inverse * glam::Vec4::new(x, y, 0., 1.);
            let b = inverse * glam::Vec4::new(x, y, 1., 1.);
            let a = a.truncate() / a.w;
            let d = b.truncate() / b.w - a;
            (d.z.abs() > 1e-8).then(|| (a + d * ((self.iso_center.z - a.z) / d.z)).truncate())
        };
        if viewport.contains(pos) {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0. {
                self.iso_span = (self.iso_span * (-scroll * 0.002).exp()).clamp(0.2, 20.);
                ctx.request_repaint();
            }
            if ctx.input(|i| i.pointer.secondary_pressed()) {
                self.pan = Some(pos);
            }
        }
        if let Some(previous) = self.pan {
            if let (Some(a), Some(b)) = (ground(previous), ground(pos)) {
                self.iso_center += (a - b).extend(0.);
            }
            self.pan = ctx.input(|i| i.pointer.secondary_down()).then_some(pos);
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests;
