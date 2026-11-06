# Vehicle blockouts

The game uses 19 checked-in models: ten new consumer studies and nine preserved
taxi, municipal, emergency, transit and rail assets. Consumer models retain detailed
geometry (13,740–117,144 triangles each), transparent glass and interiors.
Names and performance are fictional arcade tuning, not manufacturer specifications.
The original research page retains its earlier embedded model snapshots.

Assets live in `data/vehicle/<category>/<fictive-name>/`:

- `model.claydash`: native editable source scene.
- `model.glb`: Claydash CLI export with axle-local wheel pivots packaged losslessly.
- `vehicle.json`: identity, dimensions, SI physics settings and QA metadata.

### Consumer-study runtime integration

Nine completed second-pass scenes and exports come from
`research/montreal-vehicle-2/studies/`. The compact study 09 now uses the detailed
replacement delivered in this task's notes. Three existing identities match the original
research headings; seven new fictional identities replace unmatched consumer entries.
Old asset folders remain available locally but are absent from the active catalog.

| Current asset folder | Research ID | Study |
| --- | --- | --- |
| `pickup/castor-rive` | `vehicle-v2-01` | `01` |
| `suv/laurent-traverse` | `vehicle-05` | `02` |
| `suv/boreal-crete` | `vehicle-v2-03` | `03` |
| `suv/laurent-bosquet` | `vehicle-v2-04` | `04` |
| `pickup/castor-chene` | `vehicle-v2-05` | `05` |
| `suv/boreal-colline` | `vehicle-12` | `06` |
| `pickup/castor-granit` | `vehicle-v2-07` | `07` |
| `suv/boreal-rivage` | `vehicle-v2-08` | `08` |
| `compact/laurent-clair` | `vehicle-v2-09` | `09` |
| `suv/boreal-sentier` | `vehicle-04` | `10` |

For the nine research exports, the Rust
[runtime packager](../../research/montreal-vehicle-2/tools/runtime-packager/README.md)
rotates each source into the game frame, scales it uniformly, grounds its tires and
assembles four axle-local wheels including every rim, hub and lug. Its `audit.json`
records bounds, pivots and component membership. Geometry fields update to match;
the three existing vehicles retain their handling and sound settings. The saved
scene is authoritative; embedded DSL may describe an earlier foundation.

`compact/laurent-clair` keeps its catalog identity, research ID and arcade handling.
Its replacement GLB has 54,748 triangles in 28 mesh nodes, four axle-local wheel
assemblies, eight interior primitives, detailed grille and wipers, and glass opacity
0.65. Its dimensions, wheel radius, wheelbase, track and chassis offset match the new
geometry. The editable scene is replaced alongside the packaged GLB; no external
textures or authoring runtime are required to play.

`researchId` links each model to a stable card anchor in the exploration page.
For example, `vehicle-j03` links to `research/montreal-vehicles/index.html#vehicle-j03`.
These neutral IDs are independent of headings and vehicle names.
Real names are retained only in raw frequency research; runtime data uses fictional names.
`catalog.json` orders the ten studies followed by preserved special assets.
`src/dynamic_world/catalog.rs` embeds
these files into both native and web builds; add its `asset!` entry for a new vehicle.
17 road vehicles park on supported, unoccupied ground near the selected spawn.
Restricted custom maps can have fewer parking spaces.
Subway and commuter rail models are display assets;
rail driving and vehicle missions are future work.

## GLB part contract

Coordinates are metres: +X forward, +Y up, +Z right. Ground is Y = 0.
The game explicitly maps this to +X forward, +Y left, +Z up.

| Node name | Role |
| --- | --- |
| `wheel_front_left`, `wheel_front_right` | Steering and rolling |
| `wheel_rear_left`, `wheel_rear_right` | Rolling |
| `wheel_middle_left`, `wheel_middle_right` | Additional rolling axle |
| Other names | Sprung body parts |

Every wheel mesh is centered on its axle, with its cylinder axis along local Z.
The node translation positions that axle in the vehicle. The export packager moves
baked positions into wheel-local coordinates and records `role`, `steer` and
`axleAxis` in node extras. Node names are the runtime contract; a misspelled
`wheel_` name is rejected. New consumer wheels contain multiple original material
primitives under one named mesh; preserved special vehicles use simple cylinders.

Four tire contacts at the front/rear axles support the arcade chassis, including
six-wheel trucks. Middle wheels animate visually. Body lean does not move wheels.
Vehicle contacts use mass and full body dimensions; this is not a truck simulator.

## Physics and controls

`mass` is kg; `maxSpeed` is m/s; `acceleration` is m/s². Dimensions, `heightOffset`,
`wheelRadius`, `wheelbase` and `track` are metres. `maxSteerAngle` is radians.
`friction` is dimensionless. Height offset positions the game chassis reference
above ground; it does not raise the GLB above ground.

F exits or enters a nearby stopped vehicle. Walk to any vehicle in the lineup.
R restores the selected vehicle to its own initial parking position.
Catalog vehicles use firm, critically damped suspension. Heavy vehicles have smaller
body-lean angles and arcade acceleration of 4.5–6 m/s². Small road vehicles use
10–13 m/s² motor acceleration, subject to tire grip and speed taper.

## Asset authoring

The game loads the checked-in GLBs and JSON directly; running it requires no authoring scripts.
Vehicle generation and editable panel recipes live with the vehicle-modeling task:

`.moontasks/lets-model-very-low-poly-version-of-all-v-f9ac79c7-ba82-4b5c-9adb-f5c6d3046cd9/`

See that task's notes for Claydash export commands, filtering and page-preview refreshes.
The research page builder and browser QA helper live in the original vehicle-research task.
These are local task tools, separate from the Rust game and its normal build.

The generator creates geometry through Claydash, exports GLBs, rebases wheel pivots and
preserves existing physics settings. Editable source scenes remain under `data/vehicle/`.

The research page embeds models, source downloads, JSON and WebP images.
Interactive 3D imports model-viewer 4.1.0 from unpkg and requires internet access.
Saved previews and embedded downloads remain available if the CDN cannot load.
Viewer license: https://github.com/google/model-viewer/blob/v4.1.0/LICENSE.
Disposable QA images and caches remain in `research/montreal-vehicles/qa/`.

## Validation

```sh
cargo test --lib
cargo check --target wasm32-unknown-unknown
cargo run -- --smoke /tmp/vehicle-lineup.ppm --vehicle-lineup
```

The smoke command writes PPM images regardless of filename extension. The focused
catalog/lineup tests load every GLB and check wheel pivots, steering animation,
acceleration, contacts, reset behavior and parking availability on the default map.
Browser reports in the QA directory record direct Firefox and Chrome `file://` loads.

## Vehicle sound

Hold **Enter** (or keypad Enter) to sound the current vehicle's horn. Menus keep
Enter for selection; walking, pausing and focus loss cannot leave a horn held.
The command palette's **Sound: Volume…** includes a separate horn level.

Each `vehicle.json` now has an optional `sound` object, used for both player and
NPC vehicles. These assets are embedded: rebuild the game after editing JSON.
Preserve the `sound` object when re-exporting vehicle assets. Omitted fields use
the ordinary petrol-car defaults; choosing `diesel` alone does not apply a preset.
The checked-in files explicitly tune the fire, refuse, snow and grading trucks
and the bus, independently of physical mass. The electric taxi avoids idle
combustion noise and simulated gear shifts. No emergency siren is triggered by
Enter.

Example tuning for a large vehicle:

```json
"sound": {
  "engine": "diesel",
  "idleRpm": 600,
  "redlineRpm": 2500,
  "engineGain": 2.0,
  "firingOrder": 3,
  "exhaustGain": 0.95,
  "bodyGain": 0.55,
  "roughness": 0.65,
  "mufflerHz": 450,
  "machineryGain": 0.35,
  "gearRatios": [3.8, 2.3, 1.5, 1.1, 0.84, 0.67],
  "finalDrive": 5.0,
  "horn": { "kind": "air", "frequenciesHz": [175, 230], "gain": 1.0 }
}
```

| Setting | Effect / accepted range |
| --- | --- |
| `engine` | `petrol`, `diesel` or `electric`; changes excitation character |
| `idleRpm`, `redlineRpm` | 100–2000 / 500–10000 RPM; redline must exceed idle |
| `engineGain`, `exhaustGain`, `bodyGain` | Motor 0–4; exhaust 0–2; low body order 0–1 |
| `firingOrder` | 1–6 firing events per crank revolution; 3 for six-cylinder four-stroke sound |
| `roughness` | 0–1 irregular combustion texture |
| `mufflerHz` | 100–3000 Hz low-pass cutoff, brightening slightly with load |
| `machineryGain` | 0–1 load-driven accessory/rattle noise; not a simulation of working equipment |
| `gearRatios`, `finalDrive` | Six decreasing positive ratios, each 0.1–8; final drive 1–10 |
| `horn.kind` | `dual` electric-style horn or `air` with stronger upper harmonics and air texture |
| `horn.frequenciesHz`, `horn.gain` | Two fundamentals, each 80–1200 Hz; gain 0–2 |

The wheel radius comes from `physics.wheelRadius`. Gear ratios/RPM affect audio
only; handling remains controlled by the existing physics settings. Invalid or
unknown sound fields fail asset validation rather than silently changing sound.
Profiles are procedural approximations, without recorded engine samples.

Audio output uses CPAL on both native and web. Start/resume the browser game with
a click or Enter to unlock sound; subsequent key/pointer gestures can retry a
browser-suspended context. Menus, focus loss and document visibility changes
silence output. Web and native share vehicle settings, synthesis, horn/collision
events and the bounded voice pool. Native uses the Ambisonic crate; web uses a
codec-free first-order stereo adapter with matching steady-state direction and
distance behavior, plus Doppler. The adapter avoids the crate's mandatory MP3
decoder, whose memory allocator does not support WASM. Neither stereo path is an
HRTF renderer. CPAL's browser path uses scheduled audio buffers on the main thread;
this first version does not use an AudioWorklet.
