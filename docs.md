# MTL Jobines

A native and browser driving prototype on OpenCity's authored default map. Rust, winit, wgpu, custom WGSL; no editor UI/runtime dependency. One map unit is **100 meters** (a 0.045-wide lane is 4.5 m). Map coordinates are X east, Y north, Z up.

```sh
git submodule update --init --recursive
cargo run --release
```

Moon’s Project menu offers **Game UI** and **OpenCity UI** targets, each with Build, Run, and Build and Run commands in release mode. OpenCity UI opens the editor with the game’s **Play from here** action configured. `cargo run` also enables development optimization for map geometry and SVG preparation while retaining debug assertions and symbols. Startup logs report map loading, cache preparation and the first presented frame.

For the browser, install the WebAssembly target and the `wasm-bindgen-cli` version matching `Cargo.lock` (currently `0.2.128`), then build and serve, as in OpenCity:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.128 --locked
make build
make serve                 # http://127.0.0.1:8878/
# make serve PORT=9000     # optional port override
```

`make` builds and serves; `make serve` serves the existing `web/dist` build using Node.js. Moontask's Build / Run commands use these targets. The browser requires WebGPU on localhost or HTTPS and a keyboard/mouse; click the canvas to focus it. Driving, cameras, reset and minimap share the native code. The default map and elevation PNG are embedded; custom `--map`, SVG export and GPU smoke commands remain native-only. Static scenery and the map atlas are prepared once at startup on both platforms. Driving selects nearby GPU batches without rebuilding map geometry or rasterizing SVGs.

Native startup persists the prepared geometry and minimap atlas in a compressed disk cache. On macOS it lives in `~/Library/Caches/mtl_jobines`; `MTL_JOBINES_CACHE_DIR` overrides the directory. The cache key includes the map source, elevation image, game/OpenCity source fingerprint and dependency lockfile. Missing, stale or corrupt data is rebuilt; unwritable caches leave the game usable. Physics geography is still loaded from the authored map on each launch, and GPU buffers are recreated for the current device.

The same format can be shipped as game data, generated without opening a window or requiring a GPU:

```sh
cargo run --release -- --build-game-data /path/to/default.game-data.gz
cargo run --release -- --game-data /path/to/default.game-data.gz
# Include prepared default-map data inside the browser build:
BUNDLE_GAME_DATA=1 make build
# Or embed prepared data in a native build:
MTL_JOBINES_BUNDLED_GAME_DATA=/path/to/default.game-data.gz cargo build --release
```

Bundling skips runtime mesh construction and SVG rasterization at the cost of a larger executable/download. It is optional for browser builds; ordinary `make build` retains runtime preparation. A mismatched bundle falls back to fresh data. Android-only packages may appear in Cargo.lock through winit/wgpu; Cargo resolves all platforms in that file. They are absent from the macOS and WebAssembly build graphs.

The initial menu shows Start and keeps simulation paused until clicked, Enter or Escape. Later pauses show Resume. WASD / arrows drive and steer; S brakes while moving forward and then reverses. Space brakes, Shift holds the handbrake, R resets to the initial start, Tab switches chase/hood cameras, wheel zooms, right-drag orbits, Escape pauses. F exits below 3 m/s onto a clear nearby surface, or enters a car within 4 m. On foot the player is a cube: W/S or Up/Down walk forward/backward at 5.5 m/s; A/D or Left/Right turn the player and follow camera at 150°/s, including while stationary. Shift runs at 10 m/s. Right-drag orbits the view independently. The camera and minimap follow the player; unoccupied cars brake. Click Resume, press Enter, or press Escape again to continue. Click the circular minimap to open the nearly full-screen map modal; Escape, its close button, or clicking the backdrop closes it. Both overlays pause driving and clear held controls. `--map /path/to/map.road` loads another authored map; elevation images resolve relative to that file.

Run `make editor` from this repository to open OpenCity with the game launch action configured. In Top or Isometric view, right-click the desired spot and choose **Play from here in MTL Jobines**. The game starts directly at that position using your current edits, including unsaved maps. Unsaved contents are passed through a temporary snapshot beside the original map (or in the startup directory for a new map), preserving relative assets without saving or changing the open document. The snapshot is removed when the game closes. A centered spinner appears during startup and disappears when the first game frame is presented. Process failures, including invalid spawn positions, appear in a centered error dialog. The default city’s embedded elevation image is included with unsaved snapshots. **Copy coordinates** is also available. The game-owned `opencity-launch.json` supplies the command through `OPENCITY_LAUNCH_ACTION`; a standalone OpenCity editor has no built-in game launcher or assumption about where this repository lives. From a terminal, use `cargo run --release -- --map /path/to/map.road --start '-24,3'`. The optional third comma-separated value sets heading in degrees counterclockwise from east, for example `--start '-24,3,90'`; otherwise heading follows a nearby car lane. Browser launches accept `?start=-24,3,90` for the embedded default map. Coordinates use map units, and spawning uses the highest supported surface at that point; water, buildings and unsupported points are rejected. R returns to the selected start.

The box car has cylinder tires, front steering, wheel rotation and suspension. Physics runs at 120 Hz. Each supported tire uses its own surface grip, rolling resistance, position and steering direction. Its contact velocity includes chassis translation and spin; longitudinal and lateral impulses share an elliptical grip budget. Tire forces change linear velocity and supply torque through their contact arms, integrated using chassis yaw inertia. Body heading and travel direction can diverge during a slide; steering builds angular velocity over time. Service brakes act on all wheels and preserve lateral authority, while the handbrake brakes the rear wheels and reduces their lateral grip. Unsupported wheels exert no tire forces, split grip can produce yaw under braking, and airborne vehicles retain spin without steering authority. Asphalt grips and accelerates better than grass; grass/sand increase rolling resistance. From 20 m/s on level asphalt, straight service braking stops in about 1.3 s / 12.4 m. Slope gravity, aerodynamic drag, per-wheel support heights, body pitch/roll and swept building contacts remain active. Wheel queries use a height ceiling to stay below bridges, and clearance correction keeps tires above curbs. Water is intentionally difficult to drive through; R recovers the car. Tire loads use a quarter of the static chassis weight per supported wheel; pitch/roll still follow the support slope rather than full suspension torque, and tire spin is visual rather than a drivetrain simulation. Street furniture is visual detail; building collision uses three footprint circles around the chassis. Building rebounds reflect horizontal travel across the contact surface and reduce speed according to impact strength, so grazing scrapes lose less speed than head-on hits. Oblique contact torque feeds the same persistent yaw velocity as tire forces, and grounded tire grip damps the resulting spin.

Procedural meshes share OpenCity's road cross-sections, tapering and native curved junction bands. Raised curbs, lane/edge stripes and dry grass shoulders connect pavement to terrain. Terrain uses original elevation PNG cells at driving LOD, preserving the collision/render diagonal. Deterministic lamps, benches and forest trees are generated once alongside terrain and buildings. Static triangles are assigned once to 200 m GPU batches, with full triangle bounds used for visibility. The frame selects scenery within 800 m and detail batches within 150 m. This removes the former movement-triggered geometry builds/uploads; the full-resolution map remains resident, so larger custom maps require more GPU memory and startup time.

Rendering uses 4× MSAA, a 2048² directional shadow map with PCF, directional/sky lighting, linear material colors and sRGB output, procedural asphalt aggregate/cracks, grass and soil variation, sidewalk joints, facade floors/windows/brickwork, animated water highlights, distance fog and exposure compression. The palette and floor metadata come from OpenCity; the game shader can be replaced independently.

```sh
cargo test
# Exercise the real map and Metal/Vulkan/DX12 renderer without a window:
cargo run --release -- --smoke /tmp/jobines.ppm
cargo run --release -- --smoke /tmp/jobines-overview.ppm --overview
cargo run --release -- --smoke /tmp/jobines-profile.ppm --profile
```

The circular bottom-left Hermes minimap is generated once from OpenCity’s SVG export into a 4096² atlas. It follows the player and rotates with their heading beneath an upward-pointing marker. T-junction pavement is included in the export. The expanded modal stays north-up and uses the same atlas to show the complete map and player location with a heading marker. The circular map spans 400 m; the bottom bar represents 100 m. Use --export-minimap /tmp/map.svg to export the source SVG without opening a window. See [ARCHITECTURE.md](ARCHITECTURE.md) for Engine / Dynamic World / Hermes ownership and the routing/AI blueprint.

The smoke command simulates five seconds of acceleration on the real default map, checks travel/finite coordinates, creates the GPU pipelines and shadow pass, then writes actual rendered PPM frames for driving, Start, Resume, the expanded map and the on-foot cube. Add `--profile` to measure 240 synchronized GPU frames while driving across the former rebuild thresholds. It requires a GPU device even without a window.

## OpenCity submodule and API

`opencity/` is a real git submodule, using `git@github.com:antoineMoPa/opencity.git`. The game enables only `game-runtime` with default features disabled. `City::load`, `City::query`, `City::surface_at`, `City::mesh`, `City::detail_mesh` and `City::building_collision` provide the integration. Structured queries return borrowed roads, lane directions, route polylines, buildings, landscape and junctions. Complete explicit component ports/movements remain available in `City::components`.

See `opencity/docs/game-runtime.md` for shader bindings, coordinate and query contracts. Optional `runtime::EDITOR_SHADER` and `runtime::GAME_SHADER` exports require no wgpu dependency in the library.

Initial review order: commit/publish the runtime changes in the source OpenCity repo first; update this submodule to that commit, then commit this game repository. Until review, the same runtime changes are present as local modifications inside this submodule so this checkout builds immediately. No source commits were made by the agent.
