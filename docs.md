# MTL Jobines

Native and WebGPU driving prototype using OpenCity's default map. Coordinates: X east, Y north,
Z up; **1 map unit = 100 meters**. See [ARCHITECTURE.md](ARCHITECTURE.md) for ownership and
internals.

## Build and run

```sh
git submodule update --init --recursive
cargo xtask assets
cargo run --release
```

Asset generation requires Georgia/Arial and produces ignored PNGs from committed SVGs; run it
before native builds/tests. Web builds run it automatically. See [commerce
artwork](assets/commerce/README.md).
`make fmt` formats Rust; `make lint` checks formatting/source limits; `make hooks` enables pre-
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
| Camera | Tab chase/hood; wheel zooms; right-drag orbits |
| Reset | R returns to the initial spawn |
| Pause/resume | Escape; arrows/Enter select menu actions |
| Command palette | Cmd/Ctrl+Shift+P |
| Time | `[`/`]` adjust daylight; T resumes the automatic cycle |
| Full map | Click minimap; Escape, close button or backdrop dismisses |
| Navigation | Click destination on full map; right-click clears |

Start/Resume menus, map and palette pause simulation and clear held controls. Exit/Q closes native
builds; browsers return to Start. The minimap follows heading; the full map stays north-up. Routes
follow permitted road connections, refresh after 2 m and turn green on arrival within 6 m; red
means disconnected. Bridges and geometric crossings alone do not connect roads.

Traffic keeps up to 36 NPC cars within roughly 180 m, recycling them beyond 240 m. NPCs use
OpenCity lane centres, permitted junction turns and ramp attachments, with physical vehicle/prop
collisions. They brake for traffic, pedestrians and red/amber lights. Four-way signals use
opposing green phases (10 s green, 2 s amber, 2 s all-red per axis). Left turns yield to oncoming
straight/right traffic. The simulation pauses with the game.
Target speeds are 30 km/h on local roads, 50 on wider ordinary roads and ramps, 70 on merges and
100 on highways, reduced for bends and junctions. After displacement, drivers try to rejoin a
reachable nearby lane and stop if recovery fails. Stopped NPC cars can be entered with F.

The palette's **Sky: Atmosphere and stars…** panel offers presets, solar position, haze, sun and
star settings. The automatic day/night cycle lasts 20 minutes and pauses with simulation.

## Maps and editor launch

Native and browser builds start on the work-in-progress map in `data/maps/mtl/map.road`.
Normal play spawns the player vehicle and moving traffic, without the parked catalog lineup.
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

Static map geometry and the 4096² minimap atlas are prepared once. Scenery uses 200 m selection
groups and 50 m draw batches with camera/shadow culling; nearby props upload dynamically.
Vehicle body/wheel geometry uploads once; driving updates instance transforms in both render passes.
Paused menus/maps/palette reuse frozen buffers and redraw only for input or window events.
Full-resolution map geometry stays resident, so large maps increase startup time and GPU memory.

Rendering uses 4× MSAA, 2048² PCF shadows and parallax window interiors. Background, glass and fog
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
`City::components` retains explicit ports/movements. See [OpenCity docs](opencity/docs.md) and
[game integration](opencity/docs/game-runtime.md) for queries, coordinates and shader contracts.
Publish OpenCity changes first, then update this repository's submodule pointer.
