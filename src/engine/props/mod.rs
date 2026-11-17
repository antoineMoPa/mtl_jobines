//! Native in-game prop authoring through the same `.road` source and live reload as roads.
mod input;
mod storage;
#[cfg(test)]
mod tests;
mod ui;
use super::{
    buildings::geometry::{self, View},
    render::{GpuMesh, Renderer},
};
use crate::props::{Entry, Kind, Placement, Row, Shape};
use glam::{Vec2, Vec3};
use opencity::{
    runtime::{City, SurfaceKind},
    Program,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Street,
    Iso,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Select,
    Single,
    Row,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Member {
    entry: usize,
    index: usize,
}
struct Sample {
    member: Member,
    placement: Placement,
}
struct PreviewModel {
    mesh: Arc<GpuMesh>,
    transforms: Vec<[[f32; 4]; 4]>,
}
enum Gesture {
    Move {
        before: Vec<Entry>,
        anchor: Vec2,
        press: egui::Pos2,
        members: Vec<Member>,
    },
    Handle {
        before: Vec<Entry>,
        entry: usize,
        handle: usize,
        press: egui::Pos2,
    },
    MoveRow {
        before: Vec<Entry>,
        entry: usize,
        anchor: Vec2,
        press: egui::Pos2,
    },
    NewRow {
        press: egui::Pos2,
    },
    Box {
        start: egui::Pos2,
        current: egui::Pos2,
        initial: Vec<Member>,
    },
    Pan {
        press: egui::Pos2,
        center: Vec3,
    },
}
pub(super) struct Document {
    pub open: bool,
    path: PathBuf,
    source: String,
    program: Program,
    entries: Vec<Entry>,
    samples: Vec<Sample>,
    mode: Mode,
    tool: Tool,
    kind: Kind,
    spacing: f32,
    center: Vec3,
    span: f32,
    player: Vec3,
    selected: Vec<Member>,
    selected_row: Option<usize>,
    gesture: Option<Gesture>,
    draft: Option<Entry>,
    undo: Vec<Vec<Entry>>,
    redo: Vec<Vec<Entry>>,
    error: String,
    revision: u64,
    models: std::collections::HashMap<(Kind, usize), PreviewModel>,
    gpu_revision: Option<u64>,
    last_pointer: Option<egui::Pos2>,
}
impl Document {
    pub fn open(path: &Path, player: Vec3, heading: f32) -> Result<Self, String> {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let program = opencity::parse(&source).map_err(|e| e.to_string())?;
        let entries = crate::props::read(&program)?;
        Ok(Self {
            open: true,
            path: path.into(),
            source,
            program,
            entries,
            samples: Vec::new(),
            mode: Mode::Iso,
            tool: Tool::Single,
            kind: Kind::Cone,
            spacing: 2.5,
            center: player + Vec3::new(heading.cos(), heading.sin(), 0.) * 0.08,
            span: 0.35,
            player,
            selected: Vec::new(),
            selected_row: None,
            gesture: None,
            draft: None,
            undo: Vec::new(),
            redo: Vec::new(),
            error: String::new(),
            revision: 1,
            models: Default::default(),
            gpu_revision: None,
            last_pointer: None,
        })
    }
    pub fn choose_kind(&mut self, kind: Kind) {
        self.kind = kind;
        self.spacing = kind.spacing();
    }
    pub fn camera(&self) -> Option<(Vec3, Vec3, [f32; 2])> {
        (self.open && self.mode == Mode::Iso).then(|| {
            (
                self.center + Vec3::new(-0.7, -0.7, 0.85) * self.span,
                self.center,
                [self.center.x, self.center.y],
            )
        })
    }
    pub fn orthographic_span(&self) -> Option<f32> {
        (self.open && self.mode == Mode::Iso).then_some(self.span)
    }
    pub fn strip_height(&self, width: f32) -> f32 {
        if !self.open {
            0.
        } else {
            (if width < 850. { 84. } else { 42. }) + if self.error.is_empty() { 0. } else { 24. }
        }
    }
    fn ceiling(&self) -> f32 {
        if self.mode == Mode::Street {
            self.player.z + 0.08
        } else {
            self.center.z + 0.5
        }
    }
    fn ground(&self, city: &City, p: Vec2) -> Result<f32, String> {
        let surface = city
            .surface_at(p.to_array(), self.ceiling())
            .ok_or("Props need a supported surface")?;
        if surface.kind == SurfaceKind::Water {
            return Err("Props need a dry surface".into());
        }
        Ok(surface.height)
    }
    fn point(&self, city: &City, view: View, pos: egui::Pos2) -> Option<Vec2> {
        let mut height = self.center.z;
        let mut p = view.ground(pos, height)?;
        for _ in 0..4 {
            height = self.ground(city, p).ok()?;
            p = view.ground(pos, height)?;
        }
        Some(p)
    }
    fn fresh_name(&self) -> String {
        (1..)
            .map(|i| format!("placed_prop_{i}"))
            .find(|name| {
                !self.entries.iter().any(|e| e.name == *name)
                    && !self.program.statements.iter().any(
                        |s| matches!(s,opencity::Statement::Call(c)if c.assigns.contains(name)),
                    )
            })
            .unwrap()
    }
    fn new_single(&self, city: &City, p: Vec2) -> Result<Entry, String> {
        Ok(Entry {
            name: self.fresh_name(),
            shape: Shape::Single(Placement {
                kind: self.kind,
                position: p.extend(self.ground(city, p)?),
                heading: 0.,
                sign: opencity::props::SignDesign::for_kind(self.kind),
            }),
        })
    }
    fn new_row(&self, city: &City, p: Vec2) -> Result<Entry, String> {
        Ok(Entry {
            name: self.fresh_name(),
            shape: Shape::Row {
                kind: self.kind,
                row: Row {
                    a: p,
                    control: p,
                    b: p,
                    elevation: self.ground(city, p)?,
                    spacing: self.spacing,
                    heading: 0.,
                    offsets: BTreeMap::new(),
                    hidden: BTreeSet::new(),
                    sign: opencity::props::SignDesign::for_kind(self.kind),
                },
            },
        })
    }
    fn rebuild(&mut self, city: &City) {
        self.samples.clear();
        let radius = self.span.max(0.3) * 1.5;
        for (entry, e) in self.entries.iter().enumerate() {
            if let Ok(placements) = e.placements(city) {
                for (index, p) in placements {
                    if p.position.truncate().distance(self.center.truncate()) > radius {
                        continue;
                    }
                    self.samples.push(Sample {
                        member: Member { entry, index },
                        placement: p,
                    });
                }
            }
        }
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn preview(&mut self, renderer: &Renderer, city: &City) -> Option<Vec<Arc<GpuMesh>>> {
        if !self.open {
            return None;
        }
        if self.samples.is_empty() && self.gpu_revision.is_none() {
            self.rebuild(city);
        }
        if self.gpu_revision != Some(self.revision) {
            for model in self.models.values_mut() {
                model.transforms.clear();
            }
            let mut placements: Vec<_> = self
                .samples
                .iter()
                .map(|s| (s.placement.clone(), false))
                .collect();
            if let Some(draft) = &self.draft {
                if let Ok(points) = draft.placements(city) {
                    placements.extend(points.into_iter().map(|(_, p)| (p, true)));
                }
            }
            for (placement, ghost) in placements {
                for (index, part) in placement.parts().into_iter().enumerate() {
                    let model_kind = part.template.expect("catalogue part").0;
                    let model =
                        self.models
                            .entry((model_kind, index))
                            .or_insert_with(|| PreviewModel {
                                mesh: Arc::new(Self::upload_model(renderer, model_kind, index)),
                                transforms: Vec::new(),
                            });
                    let mut transform = (part.transform
                        * glam::Mat4::from_scale(Vec3::splat(0.01)))
                    .to_cols_array_2d();
                    transform[3][3] = if ghost { 0.55 } else { 1. };
                    model.transforms.push(transform);
                }
            }
            for (&(kind, index), model) in &mut self.models {
                if Arc::get_mut(&mut model.mesh).is_none() {
                    model.mesh = Arc::new(Self::upload_model(renderer, kind, index));
                }
                renderer
                    .update_instances(Arc::get_mut(&mut model.mesh).unwrap(), &model.transforms);
            }
            self.gpu_revision = Some(self.revision);
        }
        Some(
            self.models
                .values()
                .filter(|m| !m.transforms.is_empty())
                .map(|m| m.mesh.clone())
                .collect(),
        )
    }
    fn upload_model(renderer: &Renderer, kind: Kind, index: usize) -> GpuMesh {
        let mut vertices = kind.parts()[index].vertices.clone();
        for vertex in &mut vertices {
            vertex.position = vertex.position.map(|v| v * crate::props::UNITS);
        }
        renderer.upload_instanced(&vertices)
    }
    fn active(&self) -> Vec<Member> {
        if let Some(entry) = self.selected_row {
            self.samples
                .iter()
                .filter(|s| s.member.entry == entry)
                .map(|s| s.member)
                .collect()
        } else {
            self.selected.clone()
        }
    }
    fn row(&self) -> Option<&Row> {
        self.selected_row
            .and_then(|i| self.entries.get(i))
            .and_then(|e| {
                if let Shape::Row { row, .. } = &e.shape {
                    Some(row)
                } else {
                    None
                }
            })
    }
    fn accept(&mut self, city: &City, before: Vec<Entry>) {
        if self.entries == before {
            return;
        }
        if let Err(error) = self.save(city) {
            self.entries = before;
            self.selected.clear();
            self.selected_row = None;
            self.error = error;
        } else {
            self.undo.push(before);
            if self.undo.len() > 64 {
                self.undo.remove(0);
            }
            self.redo.clear();
            self.error.clear();
        }
        self.rebuild(city);
    }
    fn history(&mut self, city: &City, redo: bool) {
        self.cancel_interaction();
        let value = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        };
        if let Some(value) = value {
            let before = std::mem::replace(&mut self.entries, value);
            if let Err(error) = self.save(city) {
                let rejected = std::mem::replace(&mut self.entries, before);
                if redo {
                    self.redo.push(rejected)
                } else {
                    self.undo.push(rejected)
                }
                self.error = error;
            } else {
                if redo {
                    self.undo.push(before)
                } else {
                    self.redo.push(before)
                }
                self.selected.clear();
                self.selected_row = None;
                self.error.clear();
            }
            self.rebuild(city);
        }
    }
    pub fn cancel_interaction(&mut self) {
        let changed = self.gesture.is_some() || self.draft.is_some();
        if let Some(
            Gesture::Move { before, .. }
            | Gesture::Handle { before, .. }
            | Gesture::MoveRow { before, .. },
        ) = self.gesture.take()
        {
            self.entries = before;
            self.samples.clear();
            self.gpu_revision = None;
        }
        self.draft = None;
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }
    pub fn map_reloaded(&mut self) {
        if let Ok(source) = std::fs::read_to_string(&self.path) {
            if source != self.source {
                self.cancel_interaction();
                match opencity::parse(&source)
                    .map_err(|e| e.to_string())
                    .and_then(|p| crate::props::read(&p).map(|e| (p, e)))
                {
                    Ok((program, entries)) => {
                        self.program = program;
                        self.source = source;
                        self.entries = entries;
                        self.undo.clear();
                        self.redo.clear();
                        self.selected.clear();
                        self.selected_row = None;
                        self.error = "Map changed externally; prop history refreshed".into();
                        self.samples.clear();
                        self.gpu_revision = None;
                    }
                    Err(error) => self.error = error,
                }
            }
        }
    }
    fn delete(&mut self, city: &City) {
        let before = self.entries.clone();
        if let Some(index) = self.selected_row {
            self.entries.remove(index);
        } else {
            for member in self.selected.iter().rev() {
                match &mut self.entries[member.entry].shape {
                    Shape::Single(_) => {}
                    Shape::Row { row, .. } => {
                        row.hidden.insert(member.index);
                    }
                }
            }
            let removed: BTreeSet<_> = self
                .selected
                .iter()
                .filter(|m| matches!(self.entries[m.entry].shape, Shape::Single(_)))
                .map(|m| m.entry)
                .collect();
            self.entries = std::mem::take(&mut self.entries)
                .into_iter()
                .enumerate()
                .filter_map(|(i, e)| (!removed.contains(&i)).then_some(e))
                .collect();
        }
        self.selected.clear();
        self.selected_row = None;
        self.accept(city, before);
    }
    fn duplicate(&mut self, city: &City) {
        let before = self.entries.clone();
        if let Some(index) = self.selected_row {
            let mut copy = self.entries[index].clone();
            copy.name = self.fresh_name();
            copy.translate(Vec2::splat(0.01));
            self.entries.push(copy);
            self.selected_row = Some(self.entries.len() - 1);
        } else {
            let copies: Vec<_> = self
                .samples
                .iter()
                .filter(|s| self.selected.contains(&s.member))
                .map(|s| s.placement.clone())
                .collect();
            self.selected.clear();
            for mut p in copies {
                p.position += Vec3::new(0.01, 0.01, 0.);
                let index = self.entries.len();
                self.entries.push(Entry {
                    name: self.fresh_name(),
                    shape: Shape::Single(p),
                });
                self.selected.push(Member {
                    entry: index,
                    index: 0,
                });
            }
        }
        self.accept(city, before);
    }
}
