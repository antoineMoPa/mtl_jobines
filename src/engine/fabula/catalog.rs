//! Content bindings. Add a scene asset and one cue arm; the player stays generic.
use super::Scene;
use crate::kairos::MissionId;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::engine) enum Cue {
    GameIntro,
    BeforeMission(MissionId),
    AfterMission(MissionId),
}
pub(in crate::engine) fn scene_for(cue: Cue) -> Option<&'static Scene> {
    static OPENING: OnceLock<Result<Scene, String>> = OnceLock::new();
    let scene = match cue {
        Cue::GameIntro => OPENING.get_or_init(|| {
            Scene::load(
                include_str!("../../../assets/cutscenes/opening/scene.json"),
                Some(include_bytes!(
                    "../../../assets/cutscenes/opening/voice.wav"
                )),
            )
        }),
        Cue::BeforeMission(_) | Cue::AfterMission(_) => return None,
    };
    match scene {
        Ok(scene) => Some(scene),
        Err(error) => {
            eprintln!("Cutscene unavailable: {error}");
            None
        }
    }
}
