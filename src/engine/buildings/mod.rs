//! Native building authoring. All accepted construction remains ordinary `.road` calls.
#[cfg(test)]
mod editing_tests;
pub(super) mod geometry;
#[cfg(test)]
mod gpu_tests;
mod input;
mod landmarks;
#[cfg(test)]
mod perf_tests;
mod roads;
mod storage;
#[cfg(test)]
mod tests;
mod ui;

use geometry::{center, View};
use glam::{Vec2, Vec3};
use opencity::{
    runtime::{architecture::BuildingRecipe, City, Mesh, Region},
    Argument, BuildingFamily, BuildingMaterial, BuildingStyle, Call, LandscapeFeature,
    LandscapeKind, Program, Statement,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CameraMode {
    Build,
    Street,
}
#[derive(Clone)]
struct Item {
    statement: usize,
    feature: LandscapeFeature,
    model: Option<Arc<Mesh>>,
    mesh: Mesh,
}
impl Item {
    fn rebuild(&mut self) -> Result<(), String> {
        self.mesh = match &self.model {
            Some(model) => crate::hermes::external_models::place(model, &self.feature)?,
            None => BuildingRecipe::from_feature(&self.feature)
                .map(|r| r.mesh())
                .unwrap_or_else(|| geometry::prism(&self.feature)),
        };
        Ok(())
    }
}
enum Gesture {
    Draw {
        start: Vec2,
        press: egui::Pos2,
        current: Vec2,
    },
    Move {
        index: usize,
        before: LandscapeFeature,
        press: egui::Pos2,
        active: bool,
    },
    Resize {
        index: usize,
        before: LandscapeFeature,
        opposite: Vec2,
        press: egui::Pos2,
        active: bool,
    },
    Pan {
        press: egui::Pos2,
        center: Vec3,
    },
}
pub(super) struct Document {
    pub open: bool,
    mode: CameraMode,
    path: PathBuf,
    source: String,
    program: Program,
    roads: roads::Roads,
    items: Vec<Item>,
    selected: Option<usize>,
    center: Vec3,
    span: f32,
    player: Vec3,
    heading: f32,
    gesture: Option<Gesture>,
    draft: Option<LandscapeFeature>,
    error: String,
    importing: bool,
    landmark_picker: landmarks::Picker,
    seed: u32,
    revision: u64,
    candidate_cache: Option<roads::CandidateCache>,
}
impl Document {
    pub fn open(path: &Path, player: Vec3, heading: f32) -> Result<Self, String> {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let program = opencity::parse(&source).map_err(|e| e.to_string())?;
        let projected = opencity::project_landscape_features(&program);
        if !projected.errors.is_empty() {
            return Err(projected.errors[0].to_string());
        }
        let mut items = Vec::new();
        for (statement, s) in program.statements.iter().enumerate() {
            let Statement::Call(call) = s else {
                continue;
            };
            if call.name != "building" {
                continue;
            }
            let Some(name) = call.assigns.first() else {
                continue;
            };
            let Some(feature) = projected.features.iter().find(|f| {
                f.name.as_ref() == Some(name)
                    && f.kind == LandscapeKind::Building
                    && f.points.len() == 4
            }) else {
                continue;
            };
            let model = feature
                .model
                .as_ref()
                .filter(|p| {
                    p.ends_with(".glb") || crate::hermes::landmark_destinations::title(p).is_some()
                })
                .map(|relative| {
                    if crate::hermes::landmark_destinations::title(relative).is_some() {
                        return crate::hermes::landmarks::source_mesh(relative).map(Arc::new);
                    }

                    let bytes = storage::read_model(
                        &path.parent().unwrap_or(Path::new(".")).join(relative),
                    )?;
                    crate::hermes::external_models::decode_shared(&bytes)
                })
                .transpose()?;
            let mut item = Item {
                statement,
                feature: feature.clone(),
                model,
                mesh: Mesh::default(),
            };
            item.rebuild()?;
            items.push(item);
        }
        let roads = roads::Roads::new(&program);
        Ok(Self {
            open: true,
            mode: CameraMode::Build,
            path: path.into(),
            source,
            program,
            roads,
            items,
            selected: None,
            center: player + Vec3::new(heading.cos(), heading.sin(), 0.) * 0.25,
            span: 1.2,
            player,
            heading,
            gesture: None,
            draft: None,
            error: String::new(),
            importing: false,
            landmark_picker: landmarks::Picker::default(),
            seed: 317,
            revision: 0,
            candidate_cache: None,
        })
    }
    pub fn camera(&self) -> Option<(Vec3, Vec3, [f32; 2])> {
        (self.open && self.mode == CameraMode::Build).then(|| {
            (
                self.center + Vec3::new(-0.7, -0.7, 0.85) * self.span,
                self.center,
                [self.center.x, self.center.y],
            )
        })
    }
    pub fn orthographic_span(&self) -> Option<f32> {
        (self.open && self.mode == CameraMode::Build).then_some(self.span)
    }
    pub fn strip_height(&self, width: f32) -> f32 {
        if !self.open {
            return 0.;
        }
        let base = if self.selected.is_some() && width < 650. {
            78.
        } else {
            42.
        };
        base + if self.error.is_empty() { 0. } else { 24. }
    }
    pub fn cancel_interaction(&mut self) {
        self.gesture = None;
        self.draft = None;
    }
    pub fn map_reloaded(&mut self) {
        self.candidate_cache = None;
    }
    fn selected(&self) -> Option<&Item> {
        self.selected.and_then(|i| self.items.get(i))
    }
    fn ground_height(&self, city: &City, p: Vec2) -> f32 {
        city.surface_at(p.to_array(), self.surface_ceiling())
            .map_or(self.player.z, |s| s.height)
    }
    fn surface_ceiling(&self) -> f32 {
        if self.mode == CameraMode::Build {
            f32::INFINITY
        } else {
            self.player.z + 0.5
        }
    }
    fn prototype(&mut self, city: &City, points: Vec<[f32; 2]>) -> LandscapeFeature {
        self.seed = self.seed.wrapping_add(17);
        let location =
            points.iter().map(|p| Vec2::from_array(*p)).sum::<Vec2>() / points.len() as f32;
        let mut near: Vec<_> = city
            .landscape
            .iter()
            .chain(self.items.iter().map(|i| &i.feature))
            .filter(|f| f.kind == LandscapeKind::Building && f.height.unwrap_or(0.) >= 0.06)
            .map(|f| ((center(f) - location).length(), f))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.truncate(4);
        let seed = u64::from(self.seed);
        let mut draw = sample(seed, 1) * near.iter().map(|(d, _)| 1. / (d + 0.08)).sum::<f32>();
        let mut neighbor = near.first().map(|(_, f)| *f);
        for (distance, feature) in &near {
            draw -= 1. / (distance + 0.08);
            if draw <= 0. {
                neighbor = Some(*feature);
                break;
            }
        }
        let mut style = neighbor.and_then(|f| f.building_style).unwrap_or_default();
        style.floor_height = 0.0315;
        style.architecture.family = neighbor
            .and_then(BuildingRecipe::from_feature)
            .map_or(BuildingFamily::Heritage, |r| r.family);
        style.architecture.seed = self.seed;
        style.architecture.palette =
            opencity::BuildingPalette::seeded(style.architecture.family, seed);
        style.material = material(style.architecture.family);
        let height = (neighbor.and_then(|f| f.height).unwrap_or(0.095)
            + (sample(seed, 2) * 3.).floor() * 0.0315
            - 0.0315)
            .clamp(0.063, 0.9);
        LandscapeFeature {
            kind: LandscapeKind::Building,
            name: None,
            points,
            elevation: self.ground_height(city, location) + 0.001,
            height: Some(height),
            building_style: Some(style),
            building_use: opencity::BuildingUse::Unspecified,
            front_edge: None,
            terrain_height: None,
            model: None,
            vegetation_type: None,
        }
    }
    fn validate(
        &self,
        city: &City,
        feature: &LandscapeFeature,
        skip: Option<usize>,
    ) -> Result<(), String> {
        let c = center(feature);
        if !feature.points.iter().flatten().all(|v| v.is_finite())
            || feature.height.is_none_or(|h| !h.is_finite() || h <= 0.)
        {
            return Err("Invalid building geometry".into());
        }
        if self.mode == CameraMode::Street && c.distance(self.player.truncate()) > 0.5 {
            return Err("Outside 50 m placement range".into());
        }
        let overlaps = |other: &LandscapeFeature| {
            feature.elevation < other.elevation + other.height.unwrap_or(100.)
                && feature.elevation + feature.height.unwrap_or(0.) > other.elevation
                && geometry::overlap(&feature.points, &other.points)
        };
        for (index, item) in self.items.iter().enumerate() {
            if Some(index) != skip && overlaps(&item.feature) {
                return Err("Building overlap".into());
            }
        }
        let extent = feature
            .points
            .iter()
            .map(|p| Vec2::from_array(*p).distance(c))
            .fold(0_f32, f32::max);
        let query = city.query(Region::Radius {
            center: c.to_array(),
            radius: extent + 0.1,
        });
        for (_, f) in query.buildings {
            if self.items.iter().any(|i| i.feature.name == f.name) {
                continue;
            }
            if overlaps(f) {
                return Err("Building overlap".into());
            }
        }
        for (_, f) in query.landscape {
            if matches!(f.kind, LandscapeKind::Lake)
                && geometry::overlap(&feature.points, &f.points)
            {
                return Err("Water overlap".into());
            }
        }
        if self.roads.overlaps(feature) {
            return Err("Road or sidewalk overlap".into());
        }
        Ok(())
    }
    fn create(&mut self, city: &City, mut feature: LandscapeFeature) -> Result<(), String> {
        self.validate(city, &feature, None)?;
        let mut n = 1;
        let name = loop {
            let name = format!("placed_building_{n}");
            if !self
                .program
                .statements
                .iter()
                .any(|s| matches!(s,Statement::Call(c)if c.assigns.contains(&name)))
            {
                break name;
            }
            n += 1;
        };
        feature.name = Some(name);
        let mut item = Item {
            statement: self.program.statements.len(),
            feature,
            model: None,
            mesh: Mesh::default(),
        };
        item.rebuild()?;
        let mut after = self.program.clone();
        after
            .statements
            .push(Statement::Call(storage::call(&item.feature)?));
        storage::set_height(&mut after, &item.feature);
        self.save(after)?;
        self.items.push(item);
        self.selected = Some(self.items.len() - 1);
        Ok(())
    }
    fn edit(&mut self, city: &City, index: usize, feature: LandscapeFeature) -> Result<(), String> {
        self.validate(city, &feature, Some(index))?;
        let mut item = self.items[index].clone();
        item.feature = feature;
        item.rebuild()?;
        let mut after = self.program.clone();
        after.statements[item.statement] = Statement::Call(storage::call(&item.feature)?);
        storage::set_height(&mut after, &item.feature);
        self.save(after)?;
        self.items[index] = item;
        Ok(())
    }
}
fn material(family: BuildingFamily) -> BuildingMaterial {
    match family {
        BuildingFamily::Heritage => BuildingMaterial::RedBrick,
        BuildingFamily::Civic => BuildingMaterial::Limestone,
        BuildingFamily::CurtainWall => BuildingMaterial::Glass,
        _ => BuildingMaterial::Concrete,
    }
}
fn sample(seed: u64, channel: u64) -> f32 {
    let mut n = seed.wrapping_add(channel.wrapping_mul(0x9e3779b97f4a7c15));
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((n ^ (n >> 31)) >> 40) as f32 / 16777216.
}
fn label(family: BuildingFamily) -> &'static str {
    match family {
        BuildingFamily::Heritage => "Brick",
        BuildingFamily::Civic => "Stone",
        BuildingFamily::CurtainWall => "Glass",
        BuildingFamily::Mixed => "Mixed facade",
        BuildingFamily::Podium => "Tower + podium",
        BuildingFamily::ConcreteFrame => "Concrete frame",
        BuildingFamily::Auto => "Nearby mix",
        BuildingFamily::Legacy => "Original",
    }
}
