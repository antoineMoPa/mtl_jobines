# MTL Jobines

A native and browser driving prototype on OpenCity's authored default map. Rust, winit, wgpu, custom
WGSL; no editor UI/runtime dependency. One map unit is **100 meters** (a 0.045-wide lane is 4.5 m).
Map coordinates are X east, Y north, Z up.

```sh
git submodule update --init --recursive
cargo run --release
```

Moon’s Project menu offers **Game UI** and **OpenCity UI** targets, each with Build, Run, and Build
and Run commands in release mode. OpenCity UI opens the editor with the game’s **Play from here**
action configured. `cargo run` also enables development optimization for map geometry and SVG
preparation while retaining debug assertions and symbols. Startup logs report map loading, cache
preparation and the first presented frame.

For the browser, install the WebAssembly target and the `wasm-bindgen-cli` version matching
`Cargo.lock` (currently `0.2.128`), then build and serve, as in OpenCity:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
make build
make serve                 # http://127.0.0.1:8878/
# make serve PORT=9000     # optional port override
```

`make` builds and serves; `make serve` serves the existing `web/dist` build using Node.js.
Moontask's Build / Run commands use these targets. The browser requires WebGPU on localhost or HTTPS
and a keyboard/mouse; click the canvas to focus it. Driving, cameras, reset and minimap share the
native code. The default map and elevation PNG are embedded; custom `--map`, SVG export and GPU
smoke commands remain native-only. Static scenery and the map atlas are prepared once at startup on
both platforms. Driving selects nearby GPU batches without rebuilding map geometry or rasterizing
SVGs.

Native startup persists the prepared geometry and minimap atlas in a compressed disk cache. On macOS
it lives in `~/Library/Caches/mtl_jobines`; `MTL_JOBINES_CACHE_DIR` overrides the directory. The
cache key includes the map source, elevation image, game/OpenCity source fingerprint and dependency
lockfile. Missing, stale or corrupt data is rebuilt; unwritable caches leave the game usable.
Physics geography is still loaded from the authored map on each launch, and GPU buffers are
recreated for the current device.

The same format can be shipped as game data, generated without opening a window or requiring a GPU:

```sh
cargo run --release -- --build-game-data /path/to/default.game-data.gz
cargo run --release -- --game-data /path/to/default.game-data.gz
# Include prepared default-map data inside the browser build:
BUNDLE_GAME_DATA=1 make build
# Or embed prepared data in a native build:
MTL_JOBINES_BUNDLED_GAME_DATA=/path/to/default.game-data.gz cargo build --release
```

Bundling skips runtime mesh construction and SVG rasterization at the cost of a larger
executable/download. It is optional for browser builds; ordinary `make build` retains runtime
preparation. A mismatched bundle falls back to fresh data. Android-only packages may appear in
Cargo.lock through winit/wgpu; Cargo resolves all platforms in that file. They are absent from the
macOS and WebAssembly build graphs.

The initial menu shows Start and keeps simulation paused until Start is activated or Escape is
pressed. Later pauses show Resume. Up/Down arrows cycle the highlighted menu option; Enter
activates it. Both menus include Exit (or Q while the menu is open): native builds close the game;
browser builds reset to the Start screen because browsers cannot close an ordinary tab. WASD /
arrows drive and steer. W/Up accelerates forward; S/Down accelerates backward. The opposite
direction brakes until nearly stopped, then accelerates that way. Space holds the handbrake. R
resets to the
initial start, Tab switches chase/hood cameras, wheel zooms,
right-drag orbits, Escape pauses. `[` and `]` adjust daylight from 0 (night) to 1 (day) in 0.1
steps; the window title shows the value. T restores the automatic cycle. Cmd+Shift+P (macOS) or Ctrl+Shift+P opens the same
command palette as OpenCity. Search for “Time” and choose Set day, Set night, or Resume
automatic cycle; arrows and Enter select, Escape or clicking outside dismisses. The palette
pauses simulation and captures game input while open. F exits below 3 m/s onto a
clear nearby surface, or enters a car within 4 m. On foot the player is a cube: W/S or Up/Down walk
forward/backward at 5.5 m/s; A/D or
Left/Right turn the player and follow camera at 150°/s, including while stationary. Shift runs at 10
m/s. Right-drag orbits the view independently. The camera and minimap follow the player; unoccupied
cars brake. Click Resume, select it with arrows and Enter, or press Escape again to continue.
Click the circular minimap to open the nearly full-screen map modal; Escape, its close button, or
clicking the backdrop closes it. Click inside the map to snap a destination to the road network.
Hermes finds the shortest permitted road route and draws it in blue on both maps, refreshing after
2 m of player movement, including on foot, after reset and when changing vehicles. A red destination
means no connected route; green means arrival within 6 m. Right-click the expanded map to clear it.
Road directions, authored junction movements and lane-point ramp attachments constrain routing;
plain XY crossings and bridges do not create connections. Separately named endpoints remain
disconnected even at the same coordinates. Both overlays pause driving and clear held controls.
`--map /path/to/map.road` loads another
authored map; elevation images resolve relative to that file.

Run `make editor` from this repository to open OpenCity with the game launch action configured. In
Top or Isometric view, right-click the desired spot and choose **Play from here in MTL Jobines**.
The game starts directly at that position using your current edits, including unsaved maps. Unsaved
contents are passed through a temporary snapshot beside the original map (or in the startup
directory for a new map), preserving relative assets without saving or changing the open document.
The snapshot is removed when the game closes. A centered spinner appears during startup and
disappears when the first game frame is presented. Process failures, including invalid spawn
positions, appear in a centered error dialog. The default city’s embedded elevation image is
included with unsaved snapshots. **Copy coordinates** is also available. The game-owned
`opencity-launch.json` supplies the command through `OPENCITY_LAUNCH_ACTION`; a standalone OpenCity
editor has no built-in game launcher or assumption about where this repository lives. From a
terminal, use `cargo run --release -- --map /path/to/map.road --start '-24,3'`. The optional third
comma-separated value sets heading in degrees counterclockwise from east, for example `--start
'-24,3,90'`; otherwise heading follows a nearby car lane. Browser launches accept `?start=-24,3,90`
for the embedded default map. Coordinates use map units, and spawning uses the highest supported
surface at that point; water, buildings and unsupported points are rejected. R returns to the
selected start.

The box car has cylinder tires, front steering, wheel rotation and suspension. Physics runs at 120
Hz. Each supported tire uses its own surface grip, rolling resistance, position and steering
direction. Its contact velocity includes chassis translation and spin; longitudinal and lateral
impulses share an elliptical grip budget. Tire forces change linear velocity and supply torque
through their contact arms, integrated using chassis yaw inertia. Body heading and travel direction
can diverge during a slide; steering builds angular velocity over time. Service brakes act on all
wheels and preserve lateral authority, while the handbrake brakes the rear wheels and reduces their
lateral grip. Unsupported wheels exert no tire forces, split grip can produce yaw under braking, and
airborne vehicles retain spin without steering authority. Asphalt grips and accelerates better than
grass; grass/sand increase rolling resistance. From 20 m/s on level asphalt, straight service
braking stops in about 1.3 s / 12.4 m. Slope gravity, aerodynamic drag, per-wheel support heights,
body pitch/roll and swept building contacts remain active. Wheel queries use a height ceiling to
stay below bridges, and clearance correction keeps tires above curbs. Water is intentionally
difficult to drive through; R recovers the car. Tire loads use a quarter of the static chassis
weight per supported wheel; the sprung body has damped angular springs driven by actual longitudinal
and lateral acceleration. Acceleration raises the nose, braking dives it, and cornering rolls the
body outward, with a brief rebound as it settles. This body motion is layered over road slope
alignment while wheels retain their support orientation. It does not redistribute tire loads, and
tire spin is visual rather than a drivetrain simulation. Building collision uses three footprint
circles around the chassis. Building rebounds reflect horizontal
travel across the contact surface and reduce speed according to impact strength. Head-on wall hits
retain 30% of incoming speed; grazing scrapes lose less speed. Oblique contact torque feeds the same
persistent yaw velocity as
tire forces, and grounded tire grip damps the resulting spin.

Procedural meshes share OpenCity's road cross-sections, tapering and native curved junction bands.
Raised curbs, lane/edge stripes and dry grass shoulders connect pavement to terrain. Road spans more
than 2 m above terrain receive 65 cm concrete decks with visible undersides and edge faces, plus
columns, crossheads and footings spaced about 24 m apart. Decks follow the same adaptive curve,
grade and tapered width as the pavement. Piers skip lower roads, sidewalks and buildings, preserving
underpasses; these structures are visual props and do not add collision surfaces. Terrain uses
original elevation PNG cells at driving LOD, preserving the collision/render diagonal. Deterministic
lamps, benches and forest trees are generated once alongside terrain and buildings. Game-owned
weighted patterns add low-rise windows, doors, cornices, parapets, pitched/mansard roofs, balcony
railings and straight, curved or quarter-turn stairs to suitable authored footprints. Garden walls,
fences, hedges, flower pots, benches, bins and bicycles are seeded details. Rapier detects vehicle
and pedestrian contact with individual street furniture, trees, balconies, stairs and garden
objects. Gentle vehicle contact stops at the object; stronger impacts detach it into simulated
debris, reduce speed and remove it after ten seconds. The car's existing tire and suspension model
is unchanged.
Iron and chain-link fence sections use two-triangle planes with procedural cutout linework,
including the shadow pass. Panel coordinates follow moving debris; a solid collision proxy
continues to represent each complete section.
Destruction lasts for the current session. Footprints still provide building collision. Static
triangles are assigned once to 200 m GPU batches, with full triangle bounds used for visibility.
The frame selects scenery within 800 m and static detail batches within 150 m. Nearby intact and
moving props are included in the dynamic mesh uploaded each frame. The full-resolution map remains
resident, so larger custom maps require more GPU memory and startup time.

The game expands residential areas before building the City snapshot. The default areas are in
`assets/default-housing.road`; each uses
`residential_area(seed, min_x, min_y, max_x, max_y, center_x, center_y, core_radius)` in map units.
Within an area, named top-level buildings outside the central circle are replaced with seeded,
street-facing homes. The generator uses sidewalks, terrain and existing footprints to place one- to
four-storey buildings, with varying frontage and brick or stone colors. The area seed and road
geometry keep the result stable across reloads. The minimap, collision and rendering all use the
same expanded snapshot, and the default area file participates in the game-data cache key.
Most neighboring homes fill their lots and share side walls; occasional seeded passages break up
the rows. Setbacks stay consistent within each short run of houses. Most one- and two-storey
homes have a 4–5 m setback for a small planted frontage, fences and stairs; a smaller share of
taller buildings meets the sidewalk. These proportions are art direction, not measured counts.
Custom maps can supply their own area calls; maps without them remain unchanged, except that the
built-in areas are selected for the default map by its two named city centers. Expansion happens
in the game, so the OpenCity editor still shows the original authored map.

Rendering uses 4× MSAA, a 2048² directional shadow map with PCF, directional/sky lighting, linear
material colors and sRGB output, procedural asphalt aggregate/cracks, grass and soil variation,
sidewalk joints, facade floors/windows/brickwork, animated water highlights, distance fog and
exposure compression. The palette and floor metadata come from OpenCity; the game shader can be
replaced independently. A 20-minute day/night cycle advances while the game is unpaused. Sky, fog,
sun direction, ambient light, shadowing, window glow and street-lamp glow follow the daylight value;
this is an art-directed cycle rather than an astronomical calendar.

```sh
cargo test
# Exercise the real map and Metal/Vulkan/DX12 renderer without a window:
cargo run --release -- --smoke /tmp/jobines.ppm
cargo run --release -- --smoke /tmp/jobines-overview.ppm --overview
cargo run --release -- --smoke /tmp/jobines-profile.ppm --profile
```

The circular bottom-left Hermes minimap is generated once from OpenCity’s SVG export into a 4096²
atlas. It follows the player and rotates with their heading beneath an upward-pointing marker.
T-junction pavement is included in the export. The expanded modal stays north-up and uses the same
atlas to show the complete map and player location with a heading marker. The circular map spans 400
m; the bottom bar represents 100 m. Use --export-minimap /tmp/map.svg to export the source SVG
without opening a window. See [ARCHITECTURE.md](ARCHITECTURE.md) for Engine / Dynamic World / Hermes
ownership and the routing/AI blueprint.

The smoke command simulates five seconds of acceleration on the real default map, checks
travel/finite coordinates, creates the GPU pipelines and shadow pass, then writes actual rendered
PPM frames for driving by day and night, Start, Resume, the expanded map and the on-foot cube. Add
`--profile` to measure 240 synchronized GPU frames while driving across the former rebuild
thresholds. It requires
a GPU device even without a window.

## OpenCity submodule and API

`opencity/` is a real git submodule, using `git@github.com:antoineMoPa/opencity.git`. The game
enables only `game-runtime` with default features disabled. `City::load`, `City::query`,
`City::surface_at`, `City::mesh`, `City::detail_objects`, `City::building_detail_parts` and
`City::building_collision` provide the integration. Structured queries return borrowed roads, lane
directions, route polylines, buildings, landscape and junctions. Complete explicit component
ports/movements remain available in
`City::components`.

See `opencity/docs/game-runtime.md` for shader bindings, coordinate and query contracts. Optional
`runtime::EDITOR_SHADER` and `runtime::GAME_SHADER` exports require no wgpu dependency in the
library.

Initial review order: commit/publish the runtime changes in the source OpenCity repo first; update
this submodule to that commit, then commit this game repository. Until review, the same runtime
changes are present as local modifications inside this submodule so this checkout builds
immediately. No source commits were made by the agent.

Elevated roads use paved shoulders and concrete safety walls supplied by OpenCity. Interstate
profiles declare `role highway;`, providing walls at ground level too, including median barriers.
Vehicles use the existing swept chassis contact response against walls; pedestrians are blocked
by them as well. Collision queries respect wall height so roads beneath bridges stay clear.
