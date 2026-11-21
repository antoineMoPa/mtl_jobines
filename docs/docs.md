# MTL Jobines

Native and WebGPU driving prototype using an editable flat starter map. Coordinates: X east,
Y north, Z up; **1 map unit = 100 meters**. See [ARCHITECTURE.md](ARCHITECTURE.md) for
ownership and internals.

## Build and run

```sh
git submodule update --init --recursive
cargo xtask assets
cargo run --release
```

Asset generation requires Georgia/Arial and produces ignored PNGs from committed SVGs; run it
before native builds/tests. Web builds run it automatically. See [commerce
artwork](../assets/commerce/README.md).
`make fmt` formats Rust; `make lint` checks formatting; `make hooks` enables pre-
commit
checks. Native development builds optimize map preparation while retaining debug assertions.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
make build
make serve                 # http://127.0.0.1:8878/
```

`make` builds and serves; `make serve PORT=9000` overrides the port. Browser builds require WebGPU
on localhost/HTTPS; click the canvas for keyboard input. Custom maps, exports and smoke tests are
native-only. Moon's **Game UI**/**OpenCity UI** targets build/run the game or configured editor.

## Controls

| Action | Control |
| --- | --- |
| Accelerate/reverse, steer | WASD or arrows; opposite throttle brakes before reversing |
| Handbrake turn | Space: all-wheel braking with reduced rear lateral grip |
| Enter/exit vehicle | F; exit below 3 m/s, enter within 4 m |
| Walk/run | W/S moves, A/D turns; Shift runs |
| Camera | Tab chase/hood; wheel zooms; mouse movement orbits while driving |
| Reset | R returns to the initial spawn |
| Pause/resume | Escape; arrows/Enter select menu actions |
| Command palette | Cmd/Ctrl+Shift+P |
| Time | `[`/`]` adjust daylight; T resumes the automatic cycle |
| Full map | Click minimap; Escape, close button or backdrop dismisses |
| Map zoom | Scroll over map; +/- zoom; 0 returns to overview |
| Navigation | Click destination on full map; right-click clears |

Start/Resume menus, map and palette pause simulation and clear held controls. Exit/Q closes native
builds; browsers return to Start. The minimap follows your heading. The full map uses the
`.road` axes directly, with +X (east) to the right and +Y (north) up. YULPET is west of town.
Report replay converts older MTL locations when it can match their recorded junction. For manual
lookup, old `(x, y)` becomes `(y, -x)` and the heading decreases by 90°.
Routes follow permitted road connections, refresh after 2 m and turn green on arrival within 6 m;
red
means disconnected. Bridges and geometric crossings alone do not connect roads.

On a native authored map, choose **Add or edit buildings…** in the command palette. Drag a footprint
in
**Build** view or click an empty lot's **+** to create a building with nearby architectural
defaults.
Click any previously placed building's surfaces to select it; drag to move, drag footprint corners
to resize, or use arrows for quarter-meter adjustments. Backspace/Delete removes the selection;
typing in a field never deletes a building. The style dropdown appears after selection.
Middle/right-drag pans Build view and the wheel zooms toward the cursor, including a wide map
overview; Build has no placement radius. **Street** restores the player camera and limits
placement to 50 m. Simulation pauses while authoring. **Done** deselects, then closes the tool;
Escape cancels a gesture, deselects, or closes.

Proposed lots follow the adjacent road's outside edge and local direction, with a setback beyond
the sidewalk. **Expand road network…** and **Grow network…** also provide **Street / Iso** camera
controls. Iso supports middle/right-drag panning and cursor-centered wheel zoom, and follows the
selected road end.

For a custom building, click **.glb** or drop a GLB onto the game after selecting the building.
Import
validates and copies the model into the current map's `models/` directory, keeps the original, and
saves a relative reference in the `.road` file. **Angle** rotates custom models in degrees. Existing
files are never overwritten; identical model contents reuse the same asset. The current renderer
supports embedded triangle meshes with material colors, without base-color textures. Each accepted
edit saves the authored map and uses live reload; external source edits are detected before saving.

Procedural buildings and imported GLBs bake local ambient occlusion into their vertex colors when
the mesh is built, using four directions for faster authoring. Decoded GLBs are cached by content.
Recesses, overhangs and ground contacts receive soft shading without adding
triangles or per-frame ray tracing. Shared building materials add filtered, seeded dirt, rain
streaks and surface mottling; the authored `weathering` value controls wear on procedural buildings.
The game-data cache is invalidated automatically when the mesh generator changes.
Live map edits prepare a 1024-pixel minimap atlas and skip disk-cache compression so buildings
appear sooner; normal startup and exported packages retain the detailed 4096-pixel atlas.
Roads and sidewalks also receive filtered pavement stains and edge grime. Raised neighbors supply
local curb occlusion; flush lane boundaries stay clear. Concrete barriers, retaining walls, bridge
decks and piers have runoff, aggregate variation and darker bases. Bridge piers bake contact AO;
the remaining infrastructure shading uses material-space distances without screen-space sampling.

In native builds, choose **Bug: Report a bug…** in the command palette to capture the current
scene, then describe the problem and click **Save report**. Reports go into
`bug-reports/<number>/` under the working directory, with `screenshot.png` and `report.json`.
The JSON includes the description, player map coordinates (x/y/z in map units, 100 metres per
unit), `camera_position` and `camera_look_at` (the renderer's absolute eye and target points in
the same map units), and the nearest authored intersection's reference, coordinates and road
names. A map without intersections records `null`. The scene stays paused while typing; Cancel
or Escape discards the draft. Existing reports are preserved and the next number follows the
highest one.
The **[bug]** link remains available during building and road authoring. Construction reports
include the proposed footprints and tool strip; the report dialog blocks authoring input.
Drag directly on the frozen screenshot to leave red freehand marks. The saved PNG includes the
marks; clicking or typing in the report form does not draw on the image. Cancel discards the marks.

Replay saved report locations through the native renderer with
`cargo run --release -- --replay-reports bug-reports --replay-output /tmp/report-replay`.
This writes driving views and area overviews. Recorded camera poses are used when present;
older reports use a reconstructed driving view. Add `--replay-landmark site_model_01` to
capture the result of visiting a palette destination as `landmark-spawn.png` and
the surrounding district as `landmark-overview.png`.

Traffic keeps up to 36 NPC cars within roughly 180 m, recycling them beyond 240 m. NPCs use
OpenCity lane centres, permitted junction turns and ramp attachments, with physical vehicle/prop
collisions. They brake for traffic, pedestrians and red/amber lights. Four-way signals use
opposing green phases (10 s green, 2 s amber, 2 s all-red per axis). Left turns yield to oncoming
straight/right traffic. The simulation pauses with the game.
Target speeds are 30 km/h on local roads, 50 on wider ordinary roads and ramps, 70 on merges and
100 on highways, reduced for bends and junctions. After displacement, drivers try to rejoin a
reachable nearby lane and stop if recovery fails. Stopped NPC cars can be entered with F.

Cars carry pitch, roll and yaw momentum through jumps and falls. Supported wheels supply
suspension torque; unsupported ends tip under gravity. Vehicle and prop impacts transfer spin
at their contact points. Tilted cars can land on the chassis or roof, and overturned cars cannot
drive or automatically right themselves.

The palette's **Sky: Atmosphere and stars…** panel offers presets, solar position, haze, sun and
star settings. The automatic day/night cycle lasts 20 minutes and pauses with simulation.

## Maps and editor launch

Native and browser builds start on the flat, empty map in `data/maps/mtl/map.road`.
The native game watches the active `.road` file and its referenced elevation PNG, including while
paused. Save to reload terrain, scenery, collisions, navigation and the minimap in place. Invalid
edits keep the current map and report the error in the terminal and window title; save a correction
to retry. The player stays at their current position when supported, otherwise the authored spawn
is used. The browser embeds its map at build time. See the [authored
map](../data/maps/mtl/map.road).
Normal play spawns the player vehicle and moving traffic, without the parked catalog lineup.
The sphere landmark lot also has twelve parked visitors in a circular arc facing the sphere,
with the entrance forecourt and street access clear. Parked cars have their engines off,
collide with vehicles and pedestrians, and can be entered with F. Their placements are authored
as `parked_vehicle("vehicle-v2-09", x, y, heading_degrees);` in the `.road` map, using an exact
road vehicle research ID from the catalog. Reloading the map restores these placements; an
explicit player start that occupies a parking spot takes priority over that parked car.
Parked visitors render only within 180 m of the player; farther cars submit no body, shadow,
or lamp geometry. Returning to the lot shows the same cars in their current positions.
Use `--smoke /tmp/vehicle-lineup.ppm --vehicle-lineup` for the explicit lineup QA view.

```sh
make editor
cargo run --release -- --map /path/to/map.road --start '-24,3,90'
```

In OpenCity's Top/Isometric view, right-click and choose **Play from here in MTL Jobines**.
`opencity-launch.json` configures this through `OPENCITY_LAUNCH_ACTION`. Unsaved edits use a
temporary map beside the original, preserving relative assets and the editor's save state.
The snapshot is removed on exit; launch failures appear in the editor.

`--start` takes map-unit X/Y and optional heading, counterclockwise from east; omitted heading
follows a nearby lane. Browser builds accept `?start=-24,3,90` on the embedded map. Spawning uses
the highest supported surface and rejects water, buildings and unsupported positions.
The bundled map also declares `pedestrian_start(x, y, heading_degrees)` for normal launches to
begin on foot. `player_start` keeps the car's parking and reset position; an explicit `--start`
or browser `?start=` begins in the car instead. The optional `boat_start(x, y, heading_degrees)`
sets the shore-search origin and bearing for boat moorings.

## Cache and bundled game data

Native startup caches prepared meshes/minimap in `~/Library/Caches/mtl_jobines` on macOS;
`MTL_JOBINES_CACHE_DIR` overrides it. Map, elevation, source and dependency changes invalidate data.
Missing/corrupt caches rebuild. GPU resources and physics geography are initialized each launch.

```sh
cargo run --release -- --build-game-data /path/to/default.game-data.gz
cargo run --release -- --game-data /path/to/default.game-data.gz
BUNDLE_GAME_DATA=1 make build
MTL_JOBINES_BUNDLED_GAME_DATA=/path/to/default.game-data.gz cargo build --release
```

Generation needs no GPU. Bundles skip mesh/SVG preparation at the cost of download size;
incompatible bundles fall back to rebuilding. Ordinary browser builds prepare data at startup.

## Simulation and rendering

Physics runs at 120 Hz with per-tire support/grip, coupled braking/steering forces and persistent
yaw. Asphalt supports roughly five-second 0–100 km/h launches and a 200 km/h top speed; grass/sand
increase resistance. Body pitch/roll is visual and does not redistribute tire loads. Buildings and
barriers collide with the chassis; strong prop impacts create debris that expires after ten seconds.
Destruction is session-local. Water driving is limited; R recovers the car.

OpenCity expands named frontage zones into one shared editor/game layout. The game adds regional
materials, architectural details, fictional signs and shader rooms. Authored cores remain protected;
legacy `residential_area(...)` maps use the same pipeline. There is no map-name placement heuristic.

Static map geometry and the 4096² minimap atlas are prepared once per map revision. Scenery uses
200 m selection groups and 50 m draw batches with camera/shadow culling; nearby props upload
dynamically.
Sidewalk benches, lamps and trash cans use the approved baked GLBs in `data/props/`, decoded
once. Benches and lamps follow the shared road features; bins fit beside benches only where
their footprint is supported by sidewalk and clears buildings and nearby props. Lamp diffusers
retain night emission. Pole stems and overhead assemblies have separate collision hulls and
break together.
Traffic lights use cached GLB bodies and red/amber/green/unpowered variants, including the
pedestrian head. Crossings with three or four distinct incoming approaches receive signals,
including one-way arms. Lights follow junction timing and turn dark when a signal is hit;
failed junctions retain the all-way-stop behavior.
Vehicle body/wheel geometry uploads once; driving updates instance transforms in both render passes.
Paused menus/maps/palette reuse frozen buffers and redraw for input, window events or map reloads.
Full-resolution map geometry stays resident, so large maps increase startup time and GPU memory.

Rendering uses 4× MSAA, 2048² PCF shadows and parallax window interiors.
Half-resolution screen-space
ambient occlusion adds local contacts between visible surfaces, with depth-aware upsampling before
the HUD and toolbars. The GPU world-pass timings include the AO passes. Background, glass and fog
share an HDR atmosphere cube, refreshed on settings changes or four times per simulated second.
Native-resolution stars appear in background/reflections; fog uses only atmosphere. Camera movement
and star edits do not rebuild the atmosphere. Bridge piers/decks are visual; barriers provide
height-aware collision.

## Validation

```sh
cargo test
cargo run --release -- --smoke /tmp/jobines.ppm
cargo run --release -- --smoke /tmp/profile.ppm --profile --profile-realtime
cargo run --release -- --smoke /tmp/sunset.ppm --sky-hour 19 --sky-haze 5
cargo run --release -- --export-minimap /tmp/map.svg
```

Smoke tests require a GPU but no window. They check simulation, pipelines, day/night, menus, map,
walking and directional sky/reflection parity. Options: `--overview`, `--hood`, `--night`,
`--smoke-size 2560x1600`, `--start 'x,y,heading'`. `--sky-hour` holds time (0–24);
`--sky-haze` sets haze (1–10); `--sky-auto` advances time; `--sky-clock 600` exercises night stars.

`--profile` measures 240 synchronized frames at eight physics ticks/frame; `--profile-realtime`
uses 720 frames at two ticks/frame. Reports include percentiles, budget overruns and cached/sky
refresh timings. These measure throughput, not window presentation latency.

## OpenCity submodule and API

`opencity/` uses `game-runtime` with default features disabled. Main APIs: `City::load`, `query`,
`surface_at`, `mesh`, `detail_objects`, `building_detail_parts`, `building_collision`.
`City::components` retains explicit ports/movements. See [OpenCity docs](../opencity/docs.md) and
[game integration](../opencity/docs/game-runtime.md) for queries, coordinates and shader contracts.
Publish OpenCity changes first, then update this repository's submodule pointer.
