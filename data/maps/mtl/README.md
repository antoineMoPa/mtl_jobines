# Fresh map

`map.road` is the active native and browser map. It starts with a flat ground
plane, no roads, buildings, water, vegetation, landmarks or authored props, and
an explicit player spawn at `(0, 0)` facing east.

`elevation.png` is a constant-black, 1024 × 1024, 16-bit grayscale height map.
Its bounds are `(-76.8, -76.8)` to `(76.8, 76.8)` map units and its height is
zero everywhere. One map unit represents 100 metres. The declaration retains
an editable 0–1.2 elevation range so the height map can be sculpted later.
The existing geographic reference metadata remains available for authoring.

## Live editing

Run `cargo run` (or launch the native game with `--map path/to/map.road`).
Save the active `.road` file or its referenced elevation PNG to update the game
without restarting. Changes are debounced and compiled in the background;
reload also works from the start screen, pause menu and expanded map.
The static mesh, terrain collisions, buildings, props, traffic paths, routing,
minimap and landmark commands are replaced together. The current player position
and vehicle model are retained when supported by the new map; otherwise the
map's authored spawn is used. Motion and active navigation reset on reload.

Invalid edits keep the last working map. The window title indicates a failed
reload and the terminal provides the error. Fix and save to retry. The browser
embeds the map at build time and must be rebuilt for map changes.

The previous populated map is preserved in Git history before this reset.
`tests/fixtures/junction_regressions.road` retains only the road geometry needed
for the earlier driving regression tests; it is not loaded by the game.
