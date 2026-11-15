# Pedestrian assets

Six shared medium-poly, textured pedestrian GLBs, in metres: +Y up and +Z forward.
Each contains a 22-joint skin and Idle, Walk, Start and Stop animation clips. The
runtime maps their axes explicitly to the game's +Z up and +X forward convention.

`player.glb` is the separately skinned main character. Its mesh and embedded
texture come from the supplied T pose; it uses the same 22-joint skeleton and
four animation clips as the crowd assets. Only the player selects this model.
The summer albedo has its baked rear shadows removed so the white tank top and
skin remain consistent front-to-back under the renderer’s dynamic lighting.
`player-winter.glb` uses the supplied winter outfit on the identical rig. The
player selects it when the session season is winter, including a live season
change while walking; the crowd keeps the six shared models.

The assets retain the shoulder helper joints, narrower foot placement and delayed
forearm/hand motion from the pedestrian research. Their distinct baked cadences are
combined with independently seeded playback phases, walking speeds and pauses.

These are complete exportable GLBs, including their embedded albedo images, skin
weights and animations. No service credentials or remote requests are needed at runtime.

The summer shorts extend to just above the knees. Their mesh is split at level
waist/hem boundaries and mapped to opaque fabric across the front, back and inner
legs, avoiding skin-colored gaps at the crotch and uneven UV-island hems.
