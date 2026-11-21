# Pedestrian assets

The catalog contains six crowd models, two seasonal Mario models and one patrol officer.
All use metres with +Y up and +Z forward.
Each contains a 22-joint skin and Idle, Walk, Start and Stop animation clips. The
runtime maps their axes explicitly to the game's +Z up and +X forward convention.

Mario's summer and winter game assets are `player.glb` and `player-winter.glb`.
Both use a 20,000-triangle body with a broader rib cage, rounder shoulders and
fuller upper arms, thighs and calves. Each GLB embeds a 1536-square albedo and a
fitted auburn mullet with strand-aligned UVs, tangents and KHR_materials_anisotropy.
The winter nape is trimmed to fit the hood. Hair tips move before skinning in
both the regular and shadow passes. The static models
in `mario-v2/` carry the same build in the source pose and orientation.

Both outfits use the same 22-joint bind pose and switch with the session season.
During gameplay Mario's shoulders, arms and hands follow the procedural step
phase. He walks on 22 cm foot lanes and settles into a 36 cm standing stance.
While idle he rests his right hand at his pocket, then raises his left wrist
and glances down. The GLBs retain the standard clips for export and compatibility.

Cruiser drivers and dismounted officers share `officer.glb`. Its 7,800 triangles
use a 1536-square albedo baked from the colored source mesh.

The crowd assets retain the shoulder helper joints, narrower foot placement and delayed
forearm/hand motion from the pedestrian research. Their distinct baked cadences are
combined with independently seeded playback phases, walking speeds and pauses.

These are complete exportable GLBs, including their embedded albedo images, skin
weights and animations. No service credentials or remote requests are needed at runtime.
