# Fabula scenes

Scenes run inside the loaded world. To add one:

1. Copy `opening/scene.json` into a new scene folder and edit its content.
2. Supply an optional PCM16 mono/stereo WAV. Its sample rate is converted when loaded.
3. Add a cached `Scene::load(include_str!(...), Some(include_bytes!(...)))` binding in
   `src/engine/fabula/catalog.rs` for `GameIntro`, `BeforeMission(MissionId)`, or
   `AfterMission(MissionId)`.

The caller handles what follows playback. Finishing and skipping both run that continuation;
cancellation discards it. Unmapped cues continue immediately. The opening runs once per app session,
followed by help. Explicit `--start` / `?start=` launches bypass it.

Use **Mission: Intro** in the command palette to replay the opening at your current location and
show help again.

## Scene data

- `duration`, `voice_start`: seconds. The voice must fit within the scene.
- `shots`: contiguous `start`/`end` intervals covering the duration, with `eye_from`, `eye_to`,
  `look_from`, `look_to`. Camera motion eases between the endpoints.
- `subtitles`: nonoverlapping `{start, end, text}` entries. Text is authored content, including
  place names. Silence and muted audio still show subtitles.
- `characters`: `{source: "player", offset, yaw, pose}` overrides the on-foot player's rendering.
  `{source: "model", index, offset, yaw, pose}` adds a temporary character from the existing
  pedestrian catalog (maximum 16). Named poses currently support `idle` and `walk`.
- `props`: `{kind, offset, yaw}` adds a temporary instance using an existing prop keyword, such as
  `cone`.

All positions are metres in a frame anchored to the player's position and heading at cue time:
forward, left, up. `yaw` is an offset in degrees. Temporary character/prop positions locate their
feet/base; the player override offsets its existing pose. Camera and staging data contain no city
coordinates or names in engine code.

Gameplay is frozen during playback. Staged player poses never alter its physics or rig state.
Temporary instances are removed on exit; the original camera and world remain loaded. Other existing
pedestrians retain their current poses. Voice, subtitles, camera, and staged animation use one
clock; the audio sample clock drives it when output is available. Window focus loss pauses playback.
The existing mixer and egui renderer handle audio and captions on native and web.
