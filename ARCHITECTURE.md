# MTL Jobines architecture

Engine coordinates Dynamic World and Hermes through explicit calls. OpenCity supplies immutable static geography; the game owns simulation and presentation.

## Ownership

| Domain | Responsibility | Source |
| --- | --- | --- |
| Engine | Window/input, startup, frame scheduling, cameras, static GPU batches, GPU resources, shaders and HUD | `src/engine/` |
| Dynamic World | Vehicles, active `VehicleId`, on-foot player, controls, physics, collision response, spawn/reset and entity meshes | `src/dynamic_world/` |
| Hermes | Map loading/access, SVG minimap and navigation caches; future routing | `src/hermes/` |
| OpenCity | `.road` authoring, terrain/buildings, road profiles/junctions, lane topology, spatial/support queries, static meshes and SVG export | `opencity/` submodule |

`src/lib.rs` shares the game between the native entry point and WebAssembly startup. Hermes holds an immutable `Arc<City>`. Dynamic World queries that snapshot through Hermes and produces CPU meshes. Engine alone owns GPU resources.

## Runtime

Startup loads the map and minimap before spawning the player, then initializes GPU resources before simulation starts. Browser startup uses embedded map/elevation assets and async WebGPU initialization. Both platforms build static map meshes once and partition complete triangles into 200 m GPU batches; driving only selects nearby buffers. Full-resolution geometry remains resident, trading startup work and memory for predictable frame work.

Each frame, Engine reads controls, advances Dynamic World at **120 Hz** with capped elapsed time, follows the player, selects cached scenery and renders shadows, world and HUD. The initial Start menu pauses simulation; Escape later opens the Resume menu; clicking the minimap opens the full-map modal. Both stop physics and the game clock, clear held controls, and stop continuous redraws until dismissed. Input and overlay state belong to Engine.

Dynamic World starts with one player vehicle. F switches between driving and a cube pedestrian near a slow vehicle; unoccupied cars hold the handbrake. Walking checks support, buildings and the vehicle footprint. Explicit OpenCity start coordinates are retained for reset. Physics evaluates each supported tire at its own contact velocity and surface grip, couples longitudinal/lateral impulses with a tire envelope, and integrates their torque through chassis yaw inertia. Tire and building-contact torques share persistent angular velocity; world-space momentum stays independent of body heading. Rear-only handbraking reduces rear lateral authority. Suspension support, gravity, swept building contacts, separation and damped specular rebounds remain in Dynamic World. Tire loads are static quarters of chassis weight; pitch/roll follow support slope.

Hermes parses and rasterizes `City::minimap_svg()` once into a 4096² full-map atlas, mapping world coordinates into that atlas. Engine draws a round north-up 400 m minimap with heading and 100 m scale, or a nearly full-screen map modal using the same texture. No SVG work or static mesh generation occurs while driving.

Prepared CPU meshes and the atlas use a versioned gzip game-data format with explicit little-endian float mappings, size/finite-value checks, checksum verification and atomic writes. Native Hermes checks packaged/bundled data and the platform disk cache before building. Keys cover map/elevation bytes, source fingerprint and dependency versions. Engine uploads recovered meshes into device-local GPU batches; physics still uses the authored City snapshot. Browser builds may opt into embedding the same prepared data with `BUNDLE_GAME_DATA=1 make build`.

## Integration constraints

- Physics uses meters; **1 map unit = 100 m**. World axes are X east, Y north, Z up; SVG/screen Y points down. Use explicit mappings and enums; avoid normalization functions.
- Use OpenCity's lane directions, endpoint references and permitted movements. XY crossings alone do not connect roads; bridges and stacked decks remain separate. Use support-height ceilings to keep tire queries on the correct deck.
- Editor-to-game launching is an external process boundary. OpenCity accepts an explicitly configured executable/argument list with map path and coordinate placeholders; the game owns `opencity-launch.json` and `make editor`. OpenCity does not reference game names, game CLI flags or parent-repository layouts.\n- A map replacement must refresh Hermes caches and Engine scenery from the same immutable snapshot. Collection indices are not persistent IDs; replace append-only `VehicleId` indexing with a stale-ID policy before adding despawning.

## Planned extensions

Routing, AI traffic, missions, movable props and gameplay persistence are unimplemented.

Hermes will snap destinations to suitable lanes/decks and search a directed graph derived from OpenCity topology, initially weighted by sampled path length. Routes should carry snapshot identity, lane/movement sequence, distance and display polyline; map replacement invalidates them.

Engine displays routes; Dynamic World's AI reuses player `Controls`. Dynamic World owns entity lifetimes; a future mission system owns objectives and completion.

## Validation and limits

Tests cover physics, minimap alignment/caching and shaders; OpenCity tests cover map queries and geometry. See [docs.md](docs.md) for test, GPU smoke and SVG export commands, controls and API references.

Current limits include simplified driving physics, procedural building extrusions, visual-only furniture, basic tee geometry and no buoyancy.
