# MTL Jobines architecture

Engine coordinates Dynamic World and Hermes through explicit calls. OpenCity supplies immutable
static geography; the game owns simulation and presentation.

## Ownership

- **Engine** (`src/engine/`): window/input, frame scheduling, cameras, GPU resources, shaders and
  HUD.
- **Dynamic World** (`src/dynamic_world/`): vehicles, player, physics, props and spawn/reset.
- **Hermes** (`src/hermes/`): map loading, game-owned building dressing, minimap, routing and
  caches.
- **OpenCity** (`opencity/`): map authoring, zoning, building scaffolds, terrain, roads, static
  meshes,
  spatial queries and SVG export.

`src/lib.rs` shares native and WebAssembly startup. Hermes holds an immutable `Arc<City>`;
Dynamic World queries it and supplies vehicle part transforms plus pedestrian/debris meshes.
Engine alone owns GPU resources; catalog vehicle vertices upload once and use instance matrices.

OpenCity expands zoning into deterministic building placements shared by its editor and Hermes.
Authored buildings remain protected. Hermes adds regional materials, architectural details,
fictional signs and shader interiors; Engine binds their textures and shader extensions.
Rendering, collision, routing and minimap use the same snapshot. OpenCity owns no game artwork.
Fixed signs are static scenery; removable details and canopies use Dynamic World's Rapier bodies.

Engine owns the clock, weather and HDR sky cubemap shared by background, glass and fog. The cube
refreshes on settings changes or four times per simulated second. Background and glass also share
stellar detail, whose controls do not invalidate the atmosphere cache.

## Runtime

Startup loads the map and minimap, spawns the player, then initializes GPU resources. Browser
startup embeds map/elevation assets and initializes WebGPU asynchronously. Static meshes are built
once into spatial GPU batches; driving selects nearby buffers, updates vehicle instance transforms,
and uploads pedestrian/debris meshes. Static partitioning sorts triangles in place.

Engine advances simulation at **120 Hz** with capped elapsed time, follows the player and renders
shadows, world and HUD. Start/Resume menus and the full-map modal pause physics and the game clock,
clear held controls and stop continuous redraws. The palette uses the same pause policy. Paused
redraws reuse frozen scene buffers; navigation, vehicle transforms and prop selection also stop.
Engine owns input and overlays.

Dynamic World starts with the player vehicle, a parked catalog and nearby NPC traffic. F switches
between driving and walking near a slow vehicle; unoccupied parked cars hold the handbrake.
Tire impulses use contact velocity, surface grip and a combined
longitudinal/lateral envelope. Contact torques drive persistent yaw; momentum is independent of
body heading. Space brakes all wheels and reduces rear lateral grip. Suspension and visual body
pitch/roll remain game-owned; tire loads use equal quarters of chassis weight.

Rapier handles collisions with individual street/frontage props. Strong impacts detach them;
detached bodies disappear after ten seconds. Prop placement is deterministic, destruction
session-local. OpenCity supplies terrain support and building footprints. A game-owned spatial
index limits vehicle building contact tests to nearby footprints.

Hermes rasterizes the minimap once into a 4096² atlas. Engine reuses it for the heading-up minimap
and north-up map modal. Routing uses a directed graph of roads, ramp attachments and authored
movements, with Dijkstra searches over sampled 3D lengths. Crossings alone do not connect roads;
height-aware source snapping separates decks.

Navigation refreshes after 2 m of player movement. Engine uploads the route ribbon only when its
revision changes; routing never reuploads the atlas. Arrival within 6 m clears the route and marks
the destination green; disconnected destinations are red. Right-click clears navigation.

Prepared meshes and the atlas use versioned gzip game data with size, finite-value and checksum
checks plus atomic writes. Native Hermes checks packaged data and disk cache before rebuilding;
cache keys cover map/elevation bytes, source and dependencies. Browser builds can embed prepared
data with `BUNDLE_GAME_DATA=1 make build`. Engine recreates device-local GPU resources.

## Integration constraints

- Physics uses meters: **1 map unit = 100 m**. World axes are X east, Y north, Z up; SVG Y points
  down. Use explicit mappings and enums; avoid normalization functions.
- Honor lane directions, endpoint references, permitted movements and support-height ceilings.
  Bridges and stacked decks remain separate.
- Editor launching is an external process boundary configured by the game's `opencity-launch.json`
  and `make editor`. OpenCity accepts executable/argument placeholders without game-specific paths
  or CLI flags.
- Map replacement rebuilds Hermes caches and Engine scenery from one immutable snapshot and clears
  navigation. Collection indices are not persistent IDs; add a stale-ID policy before despawning
  entities indexed by `VehicleId`.

## Extensions and limits

NPCs use a shared lane/movement graph compiled from OpenCity and drive through player `Controls`.
The population targets 36 cars within 180 m and reuses distant slots beyond 240 m. Four-way lights
alternate opposing approaches over 28 seconds; left turns yield to oncoming straight/right traffic.
Missions and gameplay persistence remain unimplemented. Dynamic World owns entity lifetimes;
a mission system would own objectives.

Driving physics, prop hulls, tee geometry, shader rooms and lighting are approximate; buoyancy is
absent. Tests cover physics, minimap/caching and shaders; OpenCity covers map queries and geometry.
See [docs.md](docs.md) for commands, controls and API references.
