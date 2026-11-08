//! Game-facing road selection, rendering and driving-recorder lifecycle.
use super::{
    render::{GpuMesh, Renderer},
    road_ends::RoadEnds,
};
use glam::Mat4;
use opencity::runtime::City;
#[cfg(not(target_arch = "wasm32"))]
use {
    super::road_drawing::{file::Recording, Pose},
    std::path::Path,
    web_time::Instant,
};

pub(super) struct RoadTool {
    pub ends: RoadEnds,
    pub mesh: Option<GpuMesh>,
    pub view: Option<Mat4>,
    signature: Option<(bool, u64)>,
    pub notice: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    recording: Option<Recording>,
    #[cfg(not(target_arch = "wasm32"))]
    saved_source: Option<String>,
}
impl RoadTool {
    pub fn new(city: &City) -> Self {
        Self {
            ends: RoadEnds::new(city),
            mesh: None,
            view: None,
            signature: None,
            notice: None,
            #[cfg(not(target_arch = "wasm32"))]
            recording: None,
            #[cfg(not(target_arch = "wasm32"))]
            saved_source: None,
        }
    }
    pub fn update_mesh(&mut self, renderer: &Renderer, reveal: bool) {
        let signature = (reveal, self.ends.revision);
        if self.signature != Some(signature) {
            self.mesh = Some(renderer.upload_dynamic(&self.ends.mesh(reveal)));
            self.signature = Some(signature);
        }
    }
    pub fn hint(&self, reveal: bool) -> Option<String> {
        if let Some(notice) = &self.notice {
            return Some(notice.clone());
        }
        if reveal || !self.ends.selected.is_empty() {
            return Some(format!(
                "{} lane ends selected · Shift-click cubes to toggle",
                self.ends.selected.len()
            ));
        }
        None
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn select(
        &mut self,
        city: &City,
        path: Option<&Path>,
        size: [f32; 2],
        cursor: [f32; 2],
        pose: Pose,
    ) {
        if self
            .recording
            .as_ref()
            .is_some_and(|r| r.drawing.revision > 0)
        {
            self.notice = Some(
                "Recording · use Roads: Stop extending roads before changing selection".into(),
            );
            return;
        }
        let Some(id) = self.view.and_then(|v| self.ends.pick(v, size, cursor)) else {
            return;
        };
        self.ends.toggle(id);
        self.notice = None;
        self.recording = None;
        if self.ends.selected.is_empty() {
            return;
        }
        if let Some(path) = path {
            match Recording::new(path, city, &self.ends.selected, pose) {
                Ok(recording) => self.recording = Some(recording),
                Err(error) => self.notice = Some(error),
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn select(&mut self, size: [f32; 2], cursor: [f32; 2]) {
        if let Some(id) = self.view.and_then(|v| self.ends.pick(v, size, cursor)) {
            self.ends.toggle(id);
            self.notice =
                Some("Lane ends selected · saving .road files requires the native game".into());
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn record(&mut self, pose: Pose, ready: bool) {
        let Some(recording) = &mut self.recording else {
            return;
        };
        recording.drawing.sample(pose);
        if recording.drawing.revision > 0 {
            self.notice = Some(format!(
                "Recording {:.0} m · auto-saving .road · palette: Roads: Stop extending roads",
                recording.drawing.length_meters()
            ));
        }
        if ready {
            self.save(false);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&mut self, force: bool) {
        let Some(recording) = &mut self.recording else {
            return;
        };
        match recording.save(Instant::now(), force) {
            Ok(true) => self.saved_source = Some(recording.saved_source().to_owned()),
            Ok(false) => {}
            Err(error) => {
                eprintln!("Road recording: {error}");
                self.notice = Some(error);
                self.recording = None;
            }
        }
    }
    pub fn stop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.save(true);
            self.recording = None;
        }
        self.ends.clear();
        if self
            .notice
            .as_ref()
            .is_some_and(|s| s.starts_with("Recording"))
        {
            self.notice = None;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn owns_source(&self, source: &str) -> bool {
        self.saved_source.as_deref() == Some(source)
            || self
                .recording
                .as_ref()
                .is_some_and(|r| r.owns_source(source))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn reload(&mut self, city: &City, own_save: bool) {
        if own_save {
            let selected = self.ends.selected.clone();
            self.ends = RoadEnds::new(city);
            self.ends.selected = selected;
            self.signature = None;
        } else {
            *self = Self::new(city);
        }
    }
}
