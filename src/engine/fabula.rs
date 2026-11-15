//! Temporary presentation inside the loaded world. Gameplay owns cue continuations.
use crate::dynamic_world::METERS_PER_UNIT;
use glam::{Mat4, Quat, Vec3};
use serde::Deserialize;
use std::sync::Arc;

mod catalog;
pub(super) mod staging;
mod ui;
pub(super) use catalog::{scene_for, Cue};
pub(super) use staging::Stage;
pub(super) use ui::show;
#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Definition {
    pub duration: f32,
    pub voice_start: f32,
    pub shots: Vec<Shot>,
    pub subtitles: Vec<Subtitle>,
    #[serde(default)]
    pub characters: Vec<staging::Character>,
    #[serde(default)]
    pub props: Vec<staging::Prop>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Shot {
    pub start: f32,
    pub end: f32,
    pub eye_from: [f32; 3],
    pub eye_to: [f32; 3],
    pub look_from: [f32; 3],
    pub look_to: [f32; 3],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Subtitle {
    pub start: f32,
    pub end: f32,
    pub text: String,
}
pub(super) struct Scene {
    pub definition: Definition,
    pub voice: Option<Arc<crate::audio::VoiceClip>>,
}
impl Scene {
    pub fn load(json: &str, wav: Option<&[u8]>) -> Result<Self, String> {
        let definition: Definition = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let d = &definition;
        let interval = |start: f32, end: f32| {
            start.is_finite() && end.is_finite() && start >= 0. && end > start && end <= d.duration
        };
        if !d.duration.is_finite()
            || d.duration <= 0.
            || !d.voice_start.is_finite()
            || d.voice_start < 0.
            || d.voice_start > d.duration
            || d.shots.is_empty()
        {
            return Err("Invalid cutscene duration or camera track".into());
        }
        let mut end = 0.;
        for shot in &d.shots {
            if !interval(shot.start, shot.end)
                || shot.start != end
                || [shot.eye_from, shot.eye_to, shot.look_from, shot.look_to]
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite())
                || shot.eye_from == shot.look_from
                || shot.eye_to == shot.look_to
            {
                return Err("Camera shots must cover the scene with valid poses".into());
            }
            end = shot.end;
        }
        if end != d.duration {
            return Err("Camera track must cover scene duration".into());
        }
        end = 0.;
        for subtitle in &d.subtitles {
            if !interval(subtitle.start, subtitle.end)
                || subtitle.start < end
                || subtitle.text.is_empty()
            {
                return Err("Invalid or overlapping subtitles".into());
            }
            end = subtitle.end;
        }
        staging::validate(d)?;
        let voice = wav
            .map(crate::audio::VoiceClip::from_wav)
            .transpose()?
            .map(Arc::new);
        if voice
            .as_ref()
            .is_some_and(|v| d.voice_start + v.duration() > d.duration)
        {
            return Err("Voice extends beyond the cutscene".into());
        }
        Ok(Self { definition, voice })
    }
}

#[derive(Clone, Copy)]
pub(super) struct Anchor {
    /// Physics metres; scene offsets use forward, left, up relative to this heading.
    pub position: Vec3,
    pub yaw: f32,
}
impl Anchor {
    pub fn point(self, offset: [f32; 3]) -> Vec3 {
        self.position + Quat::from_rotation_z(self.yaw) * Vec3::from_array(offset)
    }
    pub fn render_delta(self, offset: [f32; 3], yaw_degrees: f32) -> Mat4 {
        let origin = self.position / METERS_PER_UNIT;
        Mat4::from_translation(self.point(offset) / METERS_PER_UNIT)
            * Mat4::from_rotation_z(yaw_degrees.to_radians())
            * Mat4::from_translation(-origin)
    }
}

pub(super) struct Playback {
    pub scene: &'static Scene,
    pub anchor: Anchor,
    pub elapsed: f32,
    pub presented: bool,
}
impl Playback {
    pub fn camera(&self) -> (Vec3, Vec3) {
        let shots = &self.scene.definition.shots;
        let shot = shots
            .iter()
            .find(|s| self.elapsed < s.end)
            .unwrap_or_else(|| shots.last().unwrap());
        let t = ((self.elapsed - shot.start) / (shot.end - shot.start)).clamp(0., 1.);
        let t = t * t * (3. - 2. * t);
        let point = |from, to| {
            self.anchor.point(
                Vec3::from_array(from)
                    .lerp(Vec3::from_array(to), t)
                    .to_array(),
            ) / METERS_PER_UNIT
        };
        (
            point(shot.eye_from, shot.eye_to),
            point(shot.look_from, shot.look_to),
        )
    }
    pub fn subtitle(&self) -> Option<&str> {
        self.scene
            .definition
            .subtitles
            .iter()
            .find(|s| self.elapsed >= s.start && self.elapsed < s.end)
            .map(|s| s.text.as_str())
    }
    pub fn stage(&self) -> Stage<'_> {
        Stage {
            definition: &self.scene.definition,
            anchor: self.anchor,
            elapsed: self.elapsed,
        }
    }
}

#[derive(Default)]
pub(super) struct Fabula {
    pub playback: Option<Playback>,
}
impl Fabula {
    pub fn active(&self) -> bool {
        self.playback.is_some()
    }
    pub fn play(&mut self, scene: &'static Scene, anchor: Anchor) {
        self.playback = Some(Playback {
            scene,
            anchor,
            elapsed: 0.,
            presented: false,
        });
    }
    /// Audio supplies the clock when available; silent playback uses visible frame time.
    pub fn advance(&mut self, dt: f32, audio_time: Option<f32>) -> bool {
        let Some(p) = &mut self.playback else {
            return false;
        };
        if !p.presented {
            return false;
        }
        p.elapsed = audio_time
            .unwrap_or(p.elapsed + dt)
            .max(p.elapsed)
            .min(p.scene.definition.duration);
        p.elapsed >= p.scene.definition.duration
    }
    pub fn clear(&mut self) {
        self.playback = None;
    }
}
