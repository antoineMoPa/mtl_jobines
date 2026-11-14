//! A report keeps the captured scene and map location fixed while the user types.
use opencity::runtime::City;
use serde::Serialize;
mod annotation;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize)]
pub(super) struct Report {
    /// Authored map units: one unit is 100 metres; z is elevation.
    map_coordinates: [f32; 3],
    camera_position: [f32; 3],
    /// Absolute target point passed to the renderer, in the same map units.
    camera_look_at: [f32; 3],
    nearest_intersection: Option<Intersection>,
    description: String,
}

#[derive(Debug, Serialize)]
struct Intersection {
    point_ref: String,
    map_coordinates: [f32; 2],
    roads: Vec<String>,
}

impl Report {
    pub(super) fn new(
        city: &City,
        map_coordinates: [f32; 3],
        camera_position: [f32; 3],
        camera_look_at: [f32; 3],
    ) -> Self {
        let distance = |p: &opencity::map::Intersection| {
            (p.position.x - map_coordinates[0]).powi(2)
                + (p.position.y - map_coordinates[1]).powi(2)
        };
        let nearest_intersection = city
            .junctions
            .iter()
            .min_by(|a, b| distance(a).total_cmp(&distance(b)))
            .map(|junction| {
                let mut roads = vec![junction.road_name.clone()];
                for (_, road) in &junction.arm_roads {
                    if !roads.contains(road) {
                        roads.push(road.clone());
                    }
                }
                Intersection {
                    point_ref: junction.point_ref.clone(),
                    map_coordinates: [junction.position.x, junction.position.y],
                    roads,
                }
            });
        Self {
            map_coordinates,
            camera_position,
            camera_look_at,
            nearest_intersection,
            description: String::new(),
        }
    }

    fn save(&self, root: &Path, screenshot: &[u8]) -> Result<PathBuf, String> {
        if self.description.trim().is_empty() {
            return Err("Describe what you see before saving.".into());
        }
        let json = serde_json::to_vec_pretty(self).map_err(|error| error.to_string())?;
        fs::create_dir_all(root).map_err(|error| error.to_string())?;
        let mut number = 0_u64;
        for directory in [root.to_path_buf(), root.join("DONE")] {
            if !directory.exists() {
                continue;
            }
            for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                if let Some(value) = entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.parse::<u64>().ok())
                {
                    number = number.max(value);
                }
            }
        }
        // create_dir reserves a number atomically, including across running instances.
        let folder = loop {
            number = number
                .checked_add(1)
                .ok_or("Bug report numbers are exhausted.")?;
            let folder = root.join(number.to_string());
            match fs::create_dir(&folder) {
                Ok(()) => break folder,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.to_string()),
            }
        };
        let result = (|| {
            fs::write(folder.join("screenshot.png"), screenshot)?;
            fs::write(folder.join("report.json"), json)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(folder.join("screenshot.png"));
            let _ = fs::remove_file(folder.join("report.json"));
            let _ = fs::remove_dir(&folder);
            return Err(error.to_string());
        }
        Ok(folder)
    }
}

#[derive(Default)]
pub(super) struct Dialog {
    open: bool,
    draft: Option<(Report, Vec<u8>)>,
    message: String,
    focus_description: bool,
    annotation: Option<annotation::Annotation>,
}

impl Dialog {
    #[cfg(test)]
    pub(super) fn draft_snapshot(&self) -> (serde_json::Value, Vec<u8>) {
        let (report, screenshot) = self.draft.as_ref().unwrap();
        (serde_json::to_value(report).unwrap(), screenshot.clone())
    }

    #[cfg(test)]
    pub(super) fn type_description(&mut self, text: &str) {
        self.draft.as_mut().unwrap().0.description.push_str(text);
    }

    pub(super) fn begin(&mut self, report: Report, screenshot: Result<Vec<u8>, String>) {
        self.dismiss();
        self.open = true;
        match screenshot {
            Ok(screenshot) => {
                self.annotation = annotation::Annotation::from_png(&screenshot).ok();
                self.draft = Some((report, screenshot));
                self.focus_description = true;
            }
            Err(error) => self.message = format!("Could not capture screenshot: {error}"),
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn dismiss(&mut self) {
        *self = Self::default();
    }

    pub(super) fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut close = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        if let Some(annotation) = &mut self.annotation {
            annotation.show(ctx);
        }
        let mut saved = false;
        egui::Window::new("Report a bug")
            .open(&mut self.open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.)
            .show(ctx, |ui| {
                if let Some((report, screenshot)) = &mut self.draft {
                    ui.label("What do you see? What did you expect to happen?");
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut report.description)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY),
                    );
                    if self.focus_description {
                        response.request_focus();
                        self.focus_description = false;
                    }
                    ui.label(
                        "The screenshot and map location were captured when you chose the command.",
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !report.description.trim().is_empty(),
                                egui::Button::new("Save report"),
                            )
                            .clicked()
                        {
                            match std::env::current_dir()
                                .map_err(|error| error.to_string())
                                .and_then(|root| {
                                    let screenshot = match &self.annotation {
                                        Some(a) => a.screenshot(screenshot)?,
                                        None => screenshot.clone(),
                                    };
                                    report.save(&root.join("bug-reports"), &screenshot)
                                }) {
                                Ok(folder) => {
                                    self.message = format!("Saved to {}", folder.display());
                                    saved = true;
                                }
                                Err(error) => {
                                    self.message = format!("Could not save report: {error}")
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                } else if ui.button("Close").clicked() {
                    close = true;
                }
                if !self.message.is_empty() {
                    ui.label(&self.message);
                }
            });
        if saved {
            self.dismiss();
            ctx.request_repaint();
        }
        if close || !self.open {
            self.dismiss();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencity::map::{Intersection as MapIntersection, IntersectionKind, WorldPoint};

    fn city() -> City {
        let mut city = City::from_program(&opencity::parse("").unwrap(), None).unwrap();
        for (name, x) in [("far", 8.), ("near", 2.)] {
            city.junctions.push(MapIntersection {
                footprint_half: 0.1,
                point_ref: name.into(),
                position: WorldPoint { x, y: 3. },
                kind: IntersectionKind::FourWay,
                road_name: "avenue".into(),
                arm_roads: vec![
                    (WorldPoint { x: 1., y: 0. }, "avenue".into()),
                    (WorldPoint { x: 0., y: 1. }, "cross_street".into()),
                ],
                ports: vec![],
            });
        }
        city
    }

    #[test]
    fn report_uses_nearest_map_junction_and_handles_maps_without_junctions() {
        let mut city = city();
        let report = Report::new(&city, [2.1, 3., 0.5], [2., 2.8, 0.6], [2.2, 3., 0.5]);
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(
            json["map_coordinates"],
            serde_json::json!([2.1_f32, 3., 0.5])
        );
        assert_eq!(
            json["camera_position"],
            serde_json::json!([2., 2.8_f32, 0.6_f32])
        );
        assert_eq!(
            json["camera_look_at"],
            serde_json::json!([2.2_f32, 3., 0.5])
        );
        assert_eq!(json["nearest_intersection"]["point_ref"], "near");
        assert_eq!(
            json["nearest_intersection"]["map_coordinates"],
            serde_json::json!([2., 3.])
        );
        assert_eq!(
            json["nearest_intersection"]["roads"],
            serde_json::json!(["avenue", "cross_street"])
        );
        city.junctions.clear();
        assert!(Report::new(&city, [0.; 3], [0.; 3], [1.; 3])
            .nearest_intersection
            .is_none());
    }

    #[test]
    fn save_allocates_after_highest_number_without_overwriting_and_preserves_text() {
        let root = std::env::temp_dir().join(format!("bug-report-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let result = std::panic::catch_unwind(|| {
            fs::create_dir(root.join("2")).unwrap();
            fs::write(root.join("2/report.json"), "existing report").unwrap();
            fs::create_dir(root.join("DONE")).unwrap();
            fs::create_dir(root.join("DONE/5")).unwrap();
            fs::write(root.join("DONE/5/report.json"), "archived report").unwrap();
            fs::create_dir(root.join("unrelated")).unwrap();
            let mut report = Report::new(&city(), [2., 3., 0.4], [1.9, 2.8, 0.5], [2., 3.1, 0.4]);
            report.description = " \n ".into();
            assert!(report.save(&root, b"image").is_err());
            assert!(!root.join("6").exists());
            report.description = "A step at the junction.\nExpected a smooth road. 🛣".into();
            std::thread::scope(|scope| {
                let first = scope.spawn(|| report.save(&root, b"captured image").unwrap());
                let second = scope.spawn(|| report.save(&root, b"captured image").unwrap());
                assert_ne!(first.join().unwrap(), second.join().unwrap());
            });
            for number in [6, 7] {
                let folder = root.join(number.to_string());
                let saved: serde_json::Value =
                    serde_json::from_slice(&fs::read(folder.join("report.json")).unwrap()).unwrap();
                assert_eq!(saved["description"], report.description);
                assert_eq!(
                    serde_json::from_value::<[f32; 3]>(saved["camera_position"].clone()).unwrap(),
                    report.camera_position
                );
                assert_eq!(
                    serde_json::from_value::<[f32; 3]>(saved["camera_look_at"].clone()).unwrap(),
                    report.camera_look_at
                );
                assert_eq!(
                    fs::read(folder.join("screenshot.png")).unwrap(),
                    b"captured image"
                );
            }
            assert_eq!(
                fs::read_to_string(root.join("2/report.json")).unwrap(),
                "existing report"
            );
            assert_eq!(
                fs::read_to_string(root.join("DONE/5/report.json")).unwrap(),
                "archived report"
            );
            assert!(report.save(&root.join("2/report.json"), b"image").is_err());
        });
        fs::remove_dir_all(root).unwrap();
        result.unwrap();
    }

    #[test]
    fn dialog_escape_discards_capture_and_capture_error_stays_visible() {
        let ctx = egui::Context::default();
        let mut dialog = Dialog::default();
        dialog.begin(
            Report::new(&city(), [0.; 3], [0.; 3], [1.; 3]),
            Ok(vec![1, 2, 3]),
        );
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ui| dialog.show(ui.ctx()),
        );
        output.textures_delta.clear();
        assert!(!dialog.is_open());
        assert!(dialog.draft.is_none());
        dialog.begin(
            Report::new(&city(), [0.; 3], [0.; 3], [1.; 3]),
            Err("readback failed".into()),
        );
        assert!(dialog.is_open());
        assert!(dialog.draft.is_none());
        assert!(dialog.message.contains("readback failed"));
    }
}
