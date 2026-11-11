//! Native authored-map editing; the existing reload watcher compiles accepted roads.
use opencity::{expansion, expansion_ui::Picker, Program};
use std::path::{Path, PathBuf};

pub(super) struct Document {
    pub picker: Picker,
    path: PathBuf,
    source: String,
    program: Program,
}
impl Document {
    pub fn open(path: &Path) -> Result<Self, String> {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let program = opencity::parse(&source).map_err(|e| e.to_string())?;
        let mut picker = Picker::default();
        picker.open = true;
        Ok(Self {
            picker,
            path: path.into(),
            source,
            program,
        })
    }
    pub fn show(&mut self, ctx: &egui::Context, view: glam::Mat4) {
        let before = self.program.clone();
        self.picker.world_overlay(ctx, &self.program, |p| {
            let v = view * glam::Vec4::new(p[0], p[1], p[2] + 0.003, 1.);
            if v.w <= 0. {
                return None;
            }
            let p = v.truncate() / v.w;
            let rect = ctx.content_rect();
            (p.z >= 0. && p.z <= 1.).then(|| {
                egui::pos2(
                    rect.left() + (p.x + 1.) * rect.width() / 2.,
                    rect.top() + (1. - p.y) * rect.height() / 2.,
                )
            })
        });
        if !self.picker.show(ctx, &mut self.program, true) {
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
            Ok(()) => self.picker.message = "Saved extension; map reload will apply it.".into(),
            Err(e) => {
                self.program = before;
                self.picker.refresh(&self.program);
                self.picker.message = e;
            }
        }
    }
}
