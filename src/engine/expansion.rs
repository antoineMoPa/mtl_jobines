//! Native authored-map editing; the existing reload watcher compiles accepted roads.
use opencity::{expansion, expansion_ui::Picker, Program};
use std::path::{Path, PathBuf};

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
        })
    }
    pub fn camera(&self) -> Option<(glam::Vec3, glam::Vec3, [f32; 2])> {
        self.camera
            .filter(|_| self.picker.open)
            .map(|camera| (camera.eye, camera.look, camera.center))
    }
    pub fn show(&mut self, root: &mut egui::Ui, view: glam::Mat4, scene_pixels: [u32; 2]) {
        let ctx = root.ctx().clone();
        let before = self.program.clone();
        let changed = self.picker.show(root, &mut self.program, true);
        let viewport = egui::Rect::from_min_size(
            ctx.content_rect().min,
            egui::vec2(scene_pixels[0] as f32, scene_pixels[1] as f32) / ctx.pixels_per_point(),
        );
        self.picker
            .world_overlay(&ctx, &self.program, viewport, |p| {
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
}

#[cfg(test)]
mod tests;
