# Pedestrian assets

Six shared medium-poly, textured pedestrian GLBs, in metres: +Y up and +Z forward.
Each contains a 22-joint skin and Idle, Walk, Start and Stop animation clips. The
runtime maps their axes explicitly to the game's +Z up and +X forward convention.

Mario's summer and winter game assets are `player.glb` and `player-winter.glb`.
Both use a 20,000-triangle body with a broader rib cage, rounder shoulders and
fuller upper arms. Each GLB embeds a 1536-square albedo and a fitted auburn
mullet with strand-aligned UVs, tangents and KHR_materials_anisotropy. The winter nape is trimmed to
fit the hood. Hair tips
move before skinning in both the regular and shadow passes. The static models
in `mario-v2/` carry the same build in the source pose and orientation.

Both outfits use the same 22-joint bind pose and switch with the session season.
During gameplay Mario's shoulders, arms and hands follow the procedural step
phase. His resting elbows bend slightly and his feet plant in separate lanes
about 22 cm apart. The GLBs retain the
standard clips for export and compatibility.

The crowd assets retain the shoulder helper joints, narrower foot placement and delayed
forearm/hand motion from the pedestrian research. Their distinct baked cadences are
combined with independently seeded playback phases, walking speeds and pauses.

These are complete exportable GLBs, including their embedded albedo images, skin
weights and animations. No service credentials or remote requests are needed at runtime.
