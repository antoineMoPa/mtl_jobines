use super::{Anchor, Definition};
use crate::dynamic_world::{pedestrian_catalog, METERS_PER_UNIT};
use glam::{Mat4, Quat, Vec3};
use serde::Deserialize;

pub(in crate::engine) const EXTRA_ACTORS: usize = 16;
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::engine) enum Pose {
    Idle,
    Walk,
}
impl Pose {
    pub fn palette(self, model: usize, elapsed: f32) -> [Mat4; pedestrian_catalog::JOINT_LIMIT] {
        let movement = match self {
            Self::Idle => 0.,
            Self::Walk => 1.,
        };
        pedestrian_catalog::definitions()[model].pose(elapsed, elapsed, movement)
    }
}
#[derive(Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub(in crate::engine) enum CharacterSource {
    Player,
    Model { index: usize },
}
#[derive(Deserialize)]
pub(in crate::engine) struct Character {
    #[serde(flatten)]
    pub source: CharacterSource,
    pub offset: [f32; 3],
    pub yaw: f32,
    pub pose: Pose,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::engine) struct Prop {
    pub kind: String,
    pub offset: [f32; 3],
    pub yaw: f32,
}
#[derive(Clone, Copy)]
pub(in crate::engine) struct Stage<'a> {
    pub definition: &'a Definition,
    pub anchor: Anchor,
    pub elapsed: f32,
}
impl Stage<'_> {
    pub fn player(&self) -> Option<&Character> {
        self.definition
            .characters
            .iter()
            .find(|c| matches!(c.source, CharacterSource::Player))
    }
    pub fn extra_characters(&self) -> impl Iterator<Item = (usize, Mat4, Pose)> + '_ {
        self.definition
            .characters
            .iter()
            .filter_map(|c| match c.source {
                CharacterSource::Player => None,
                CharacterSource::Model { index } => Some((
                    index,
                    pedestrian_catalog::instance_transform(
                        self.anchor.point(c.offset),
                        self.anchor.yaw + c.yaw.to_radians(),
                    ),
                    c.pose,
                )),
            })
    }
    pub fn props(&self) -> Vec<crate::props::Instance> {
        self.definition
            .props
            .iter()
            .flat_map(|p| {
                let kind = crate::props::Kind::from_keyword(&p.kind).expect("validated scene prop");
                let transform = Mat4::from_scale_rotation_translation(
                    Vec3::splat(1. / METERS_PER_UNIT),
                    Quat::from_rotation_z(self.anchor.yaw + p.yaw.to_radians()),
                    self.anchor.point(p.offset) / METERS_PER_UNIT,
                );
                kind.parts()
                    .iter()
                    .enumerate()
                    .map(move |(index, mesh)| crate::props::Instance {
                        kind,
                        index,
                        mesh: mesh.clone(),
                        transform,
                        automatic: false,
                    })
            })
            .collect()
    }
}
pub(super) fn validate(d: &Definition) -> Result<(), String> {
    let valid =
        |offset: [f32; 3], yaw: f32| offset.iter().all(|v| v.is_finite()) && yaw.is_finite();
    let mut players = 0;
    let mut extras = 0;
    for c in &d.characters {
        match c.source {
            CharacterSource::Player => players += 1,
            CharacterSource::Model { index } => {
                extras += 1;
                if index >= pedestrian_catalog::definitions().len() {
                    return Err("Unknown character model".into());
                }
            }
        }
        if !valid(c.offset, c.yaw) {
            return Err("Invalid character placement".into());
        }
    }
    if players > 1 || extras > EXTRA_ACTORS {
        return Err("Too many staged characters".into());
    }
    for p in &d.props {
        if !valid(p.offset, p.yaw) || crate::props::Kind::from_keyword(&p.kind).is_none() {
            return Err("Invalid staged prop".into());
        }
    }
    Ok(())
}
