# MTL Jobines architecture

Engine coordinates Dynamic World and Hermes through explicit calls. OpenCity supplies immutable
static geography; the game owns simulation and presentation.

## Ownership

<table>
  <thead>
    <tr>
      <th>
        Domain
      </th>
      <th>
        Responsibility
      </th>
      <th>
        Source
      </th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>
        Engine
      </td>
      <td>
        Window/input, startup, frame scheduling, cameras, static GPU batches, GPU resources, shaders
        and HUD
      </td>
      <td>
        <code>src/engine/</code>
      </td>
    </tr>
    <tr>
      <td>
        Dynamic World
      </td>
      <td>
        Vehicles, active <code>VehicleId</code>, on-foot player, controls, physics, collision
        response, spawn/reset and entity meshes
      </td>
      <td>
        <code>src/dynamic_world/</code>
      </td>
    </tr>
    <tr>
      <td>
        Hermes
      </td>
      <td>
        Map loading/access, SVG minimap, directed road routing and navigation caches
      </td>
      <td>
        <code>src/hermes/</code>
      </td>
    </tr>
    <tr>
      <td>
        OpenCity
      </td>
      <td>
        <code>.road</code> authoring, terrain/buildings, road profiles/junctions, lane topology,
        spatial/support queries, static meshes and SVG export
      </td>
      <td>
        <code>opencity/</code> submodule
      </td>
    </tr>
  </tbody>
</table>

`src/lib.rs` shares the game between the native entry point and WebAssembly startup. Hermes holds an
immutable `Arc<City>`. Dynamic World queries that snapshot through Hermes and produces CPU meshes.
Engine alone owns GPU resources. Hermes supplies application-owned residential pattern weights to
OpenCity's generic building-detail generator. OpenCity separates removable detail meshes from
facades; Dynamic World owns their Rapier colliders and destruction state. OpenCity footprint and
surface queries continue to provide building and road contacts.

## Runtime

Startup loads the map and minimap before spawning the player, then initializes GPU resources before
simulation starts. Browser startup uses embedded map/elevation assets and async WebGPU
initialization. Both platforms build static map meshes once and partition complete triangles into
200 m GPU batches; driving selects nearby static buffers and uploads nearby props with the dynamic
mesh. Full-resolution static geometry remains resident, trading startup work and memory for
predictable frame work.

Each frame, Engine reads controls, advances Dynamic World at **120 Hz** with capped elapsed time,
follows the player, selects cached scenery and renders shadows, world and HUD. The initial Start
menu pauses simulation; Escape later opens the Resume menu; clicking the minimap opens the full-map
modal. Both stop physics and the game clock, clear held controls, and stop continuous redraws until
dismissed. Input and overlay state belong to Engine.

Dynamic World starts with one player vehicle. F switches between driving and a cube pedestrian near
a slow vehicle; unoccupied cars hold the handbrake. Walking checks support, buildings and the
vehicle footprint. Explicit OpenCity start coordinates are retained for reset. Physics evaluates
each supported tire at its own contact velocity and surface grip, couples longitudinal/lateral
impulses with a tire envelope, and integrates their torque through chassis yaw inertia. Tire and
building-contact torques share persistent angular velocity; world-space momentum stays independent
of body heading. Rear-only handbraking reduces rear lateral authority. Suspension support, gravity,
swept building contacts, separation and damped specular rebounds remain in Dynamic World. Tire loads
are static quarters of chassis weight; wheel pitch/roll follow support slope, while independent
damped sprung-body angles respond to longitudinal/lateral acceleration without changing wheel
contact forces.

Rapier sweeps the vehicle against individual street and frontage objects and checks pedestrian
overlap. Slow impacts stop the car; stronger impacts detach the object and apply a modest speed
loss. Detached meshes follow Rapier rigid bodies and disappear after ten seconds. The intact
objects are generated deterministically from the City, and destruction is session-local.

Hermes parses and rasterizes `City::minimap_svg()` once into a 4096² full-map atlas, mapping world
coordinates into that atlas. Engine draws a round heading-up 400 m minimap with heading and 100 m
scale, or a nearly full-screen map modal using the same texture. No SVG work or static mesh
generation occurs while driving. Hermes builds a directed graph from road directions, referenced
lane-point ramp attachments and authored vehicle movements. Dijkstra searches sampled 3D path
lengths, with virtual endpoints for partial road segments. Ramp cuts keep opposite carriageways
separate, and geometric crossings do not connect. Source snapping uses player height to distinguish
decks; destination clicks select the nearest projected road or movement.

Navigation refreshes after 2 m of player movement, including resets and vehicle changes. Engine
uploads a small route ribbon only when the route revision changes; the HUD projects the same atlas
coordinates into the heading-up minimap and north-up modal, with a constant screen-space width.
Arrival within 6 m clears the ribbon and turns the destination green; disconnected routes show a red
marker. Right-clicking the map clears navigation. The static atlas is never reuploaded for routing.

Prepared CPU meshes and the atlas use a versioned gzip game-data format with explicit little-endian
float mappings, size/finite-value checks, checksum verification and atomic writes. Native Hermes
checks packaged/bundled data and the platform disk cache before building. Keys cover map/elevation
bytes, source fingerprint and dependency versions. Engine uploads recovered meshes into device-local
GPU batches; physics uses the same compiled City snapshot. Browser builds may opt into embedding
the same prepared data with `BUNDLE_GAME_DATA=1 make build`.

Before compiling the City, game-owned residential-area definitions replace outer named buildings
and fill suitable sidewalk frontage with seeded homes. Downtown cores retain their existing
buildings. Rendering, collision, routing and the minimap share the resulting snapshot; OpenCity
provides general building materials and detail geometry without assigning regional styles.

## Integration constraints

- Physics uses meters; **1 map unit = 100 m**. World axes are X east, Y north, Z up; SVG/screen Y
  points down. Use explicit mappings and enums; avoid normalization functions.
- Use OpenCity's lane directions, endpoint references and permitted movements. XY crossings alone do
  not connect roads; bridges and stacked decks remain separate. Use support-height ceilings to keep
  tire queries on the correct deck.
- Editor-to-game launching is an external process boundary. OpenCity accepts an explicitly
  configured executable/argument list with map path and coordinate placeholders; the game owns
  `opencity-launch.json` and `make editor`. OpenCity does not reference game names, game CLI flags
  or parent-repository layouts.
- A map replacement must refresh Hermes caches and Engine scenery
  from the same immutable snapshot. Collection indices are not persistent IDs; replace append-only
  `VehicleId` indexing with a stale-ID policy before adding despawning.

## Planned extensions

AI traffic, missions and gameplay persistence are unimplemented.

Routing currently uses road centerlines, with directional lane attachments for ramps and authored
junction movement geometry. A future lane-level planner can retain lane/movement sequences for AI.
Map replacement must rebuild the graph and clear the destination with the immutable City snapshot.

Engine displays routes; Dynamic World's AI reuses player `Controls`. Dynamic World owns entity
lifetimes; a future mission system owns objectives and completion.

## Validation and limits

Tests cover physics, minimap alignment/caching and shaders; OpenCity tests cover map queries and
geometry. See [docs.md](docs.md) for test, GPU smoke and SVG export commands, controls and API
references.

Current limits include simplified driving physics, approximate prop collision hulls, basic tee
geometry and no buoyancy. Daylight follows a game-owned automatic cycle or a
manual 0–1
value; the GPU shader receives the corresponding sun, sky and daylight values.
