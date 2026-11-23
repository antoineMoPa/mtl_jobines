# C++ client API design

Open World Engine

## Abstract

An OWE game is a C++20 coroutine. The client controls input, simulation and presentation while the
engine owns the window and world. Sections 1–3 describe the SDK used by Switchback Circuit on native
and WebAssembly targets. Sections 4–6 propose extensions for cameras and weapons, collision queries,
and shader authoring.

## Table of contents

1. [Client loop](#1-client-loop)
2. [Function reference](#2-function-reference)
   - [Skydome textures and shaders](#26-skydome-textures-and-shaders)
3. [Native and WebAssembly bindings](#3-native-and-webassembly-bindings)
4. [Proposed cameras, weapons and aiming](#4-proposed-cameras-weapons-and-aiming)
5. [Proposed collision queries](#5-proposed-collision-queries)
6. [Proposed shader hooks](#6-proposed-shader-hooks)
7. [References](#7-references)

## 1. Client loop

You decide when to read input, step physics, draw, and exit. The engine owns the window.

Define the title, maps, prefabs, placements and named assets in a C++ function taking
`owe::Content&`. The native and WebAssembly hosts run this function once before gameplay. The SDK
copies embedded map, model and shader bytes into the engine. [Switchback content
definition](../examples/switchback-circuit/content.hpp), [Breakwater content
definition](../examples/breakwater/content.hpp)

```cpp
while (auto frame = co_await game.next_frame()) {
    if (frame->input().pressed(owe::Key::Escape)) break;

    for (float dt : frame->fixed_steps()) {
        game.world.step(dt);
    }
    frame->present();
}
```

### 1.1. Example client

`→` moves the car. `P` pauses physics. `Esc` exits.

```cpp
#include "content.hpp" // defines define_game(owe::Content&)

owe::Task play(owe::Game& game) {
    auto map = co_await game.maps.load("circuit");
    if (map != owe::Error::Ok) co_return map;

    auto car = game.world.spawn("comet", {4.f, -46.f, 0.12f});
    if (!car) co_return car.error;

    bool paused = false;
    while (auto frame = co_await game.next_frame()) {
        const auto& keys = frame->input();
        if (keys.pressed(owe::Key::Escape)) break;
        if (keys.pressed(owe::Key::P)) paused = !paused;

        if (!paused) {
            for (float dt : frame->fixed_steps()) {
                if (keys.down(owe::Key::Right)) {
                    auto pos = game.world.position(car.value);
                    if (!pos) co_return pos.error;
                    auto next = pos.value;
                    next.x += 2.f * dt;
                    auto moved = game.world.set_position(car.value, next);
                    if (moved != owe::Error::Ok) co_return moved;
                }
                game.world.step(dt);
            }
        }

        frame->present();
    }
    co_return owe::Error::Ok;
}

OWE_CLIENT(define_game, play)
```

## 2. Function reference

### 2.1. Run, pause, or stop the loop

- `co_await game.next_frame()`

  Resumes with a frame and fresh input. A closed window returns an empty result.

- `frame.fixed_steps()`, `world.step(dt)`

  Each step advances simulation once. Skip the steps to pause. Unused steps expire at the end of the
  frame; catch-up work is capped.

- `frame.present()`

  Draw the current world once, even while paused. Skip to keep the previous image.

- `co_return owe::Error::Ok` or `co_return error`

  End the session. An error goes back to the launcher. Local variables survive an await; frame and
  input references expire at the next await.

### 2.2. Load or replace a map

- `co_await game.maps.load("circuit") → Error`

  The engine prepares the packaged maps at startup. Awaiting a map switch resumes the client with
  its result on the next frame.

- **Success:** use the new world immediately. Old handles expire.
- **Failure:** the previous map stays active. Before the first load, world calls return `NoMap`.
- **Window closed:** the load returns `Closed`.

### Flora prefabs

Use `Content::flora` to register one named plant definition. Choose a bundled maple canopy and its
full height in metres. The host anchors the trunk at the placement position. Placed and spawned
plants use the usual entity API on native and WebAssembly hosts.

```cpp
content.flora("courtyard_maple", {owe::Canopy::BroadMaple, 8.f});
content.place("circuit", "east_tree", "courtyard_maple", {4.f, 12.f, 0.f}, true);
```

The other forms are `UprightMaple` and `YoungMaple`. Flora and box prefabs share a namespace.
Heights must be finite and greater than zero.

### 2.3. Spawn, find, or destroy an object

- `world.spawn(prefab, position) → Result<Entity>`

  Keep the returned handle. An unregistered prefab returns `UnknownPrefab`.

- `world.find("start_line") → Result<Entity>`

  Pass the unique name assigned to an object in the map. In this example the name is `"start_line"`.
  An unknown name returns `NotFound`.

- `world.destroy(entity) → Error`

  Remove a spawned object. Using its old handle returns `StaleEntity`, including after a successful
  map switch.

### 2.4. Read or change a position

- `world.set_facing(entity, x, y) → Error`

  Point the prefab’s local +X axis along a horizontal world direction. Switchback Circuit uses the
  steering heading for the player and the track tangent for each rival.

- `world.position(entity) → Result<Vec3>`

  World position in metres. X and Y span the ground. Z points up.

- `world.set_position(entity, position) → Error`

  Teleport immediately. Setting the position clears velocity. Fixed placements return `ReadOnly`.

### 2.5. Read keyboard input

- `input.down(owe::Key::Right)`

  Pass an `owe::Key` enum value identifying a physical keyboard key. This example is true while the
  right arrow is held. All steps share the frame’s input. Losing focus clears held keys.

- `input.pressed(owe::Key::P)`, `input.released(owe::Key::P)`

  Check once per frame. A quick tap can set both. OS repeat is ignored.

### 2.6. Skydome textures and shaders

Clients can change the visible sky on native and WebGPU. Register named PNG and WGSL assets in the
client’s content definition, then select either asset from `play`. Fog, reflections and scene
lighting use the engine’s atmospheric environment.

```cpp
#include <owe/content.hpp>

constexpr unsigned char harbour[] = {
#embed "sky/harbour.png"
};
constexpr unsigned char dusk[] = {
#embed "sky/dusk.wgsl"
};

void define_game(owe::Content& content) {
    content.sky_texture("harbour", harbour);
    content.sky_shader("dusk", dusk);
    // Register the game's maps and prefabs here too.
}
```

Connect this definition to the game coroutine with `OWE_CLIENT(define_game, play)`. List both files
in the game Makefile’s `MAP_FILES` so edits rebuild the client. The engine prepares the assets at
startup and reports malformed PNG or WGSL by name.

```cpp
auto status = game.skydome.set_texture("harbour");
if (status != owe::Error::Ok) co_return status;
status = game.skydome.set_shader("dusk");
if (status != owe::Error::Ok) co_return status;
```

Both setters return `owe::Error`. Selections take effect on the next presented frame and survive map
loads. Pass `""` to restore that setter’s default. An unknown name returns `owe::Error::NotFound`
and preserves the active selection.

Use a horizontally wrapping sRGB PNG of the upper hemisphere, with the zenith at the top and the
horizon at the bottom. With the default shader, opaque pixels show the panorama and transparent
pixels reveal the engine sky.

Define `owe_skydome` in the WGSL file and return linear display RGB. The renderer applies sRGB
encoding. This example tints the panorama and blends it over the sky using its alpha.

```wgsl
fn owe_skydome(input: SkydomeInput) -> vec3<f32> {
    let dusk = input.panorama.rgb * vec3(1.0, 0.6, 0.45);
    return mix(input.sky, dusk, input.panorama.a);
}
```

- `direction: vec3<f32>`

  Unit view direction in Z-up world coordinates.

- `uv: vec2<f32>`

  Panorama coordinates. World +X lies at U = 0.5.

- `panorama: vec4<f32>`

  Sampled linear RGBA. With the default texture, R and G contain the day and night cloud masks,
  respectively.

- `sky: vec3<f32>`

  Engine atmosphere and stars after exposure and tone mapping. The default texture also contributes
  clouds.

- `daylight: f32`

  Daylight fraction from 0 to 1.

- `time: f32`

  Client simulation time in seconds.

### 2.7. First-person surface impacts

After an accepted shot, position the current view model and call
`game.world.muzzle_flash(owe::MuzzleFlash::Rifle)` or pass `owe::MuzzleFlash::Handgun`. The engine
places the flash at the forward (+X) cap of the displayed model and lights nearby surfaces. Call it
on misses as well as hits.

Breakwater registers each prefab with `content.impact_material("panel", owe::ImpactMaterial::Wood)`
or `owe::ImpactMaterial::Concrete`. After setting the first-person camera,
`game.world.strike_reticle(distance, strength)` finds the nearest prefab or supporting ground face
and emits the material effect. Distance is in metres up to 250 and strength ranges from 0.25 to 2. A
miss returns `Error::NotFound`. A successful `Strike` contains the world-space contact point and the
entity, with ID zero for terrain.

Health belongs to the client. When an upright, movable YZ panel runs out of health, pass its
`Strike` to `game.world.shatter_panel(hit.value)` to remove the collider and fracture its painted
layers together. Dust and detached fragments have no gameplay colliders. Reloading the map clears
the marks and debris.

## 3. Native and WebAssembly bindings

`OWE_CLIENT(define_game, play)` connects a C++20 coroutine to the engine. The SDK supplies the task
type and awaitables.

- **Native:** the engine pumps window events and resumes the client on the main thread. Engine calls
  use a versioned C ABI with fixed-width IDs and a caller-owned command packet.
- **Browser:** awaiting a frame yields to the browser. The loader resumes the client on an animation
  frame or when a map load finishes. Coroutine state stays in client memory.

## 4. Proposed cameras, weapons and aiming

A client equips a weapon on an actor once. The same actor can move, aim and fire in first person or
over the shoulder. The engine keeps the weapon state across camera changes. The client owns input
bindings, loadouts, tuning, HUD and game rules.

### 4.1. One client loop for both views

Proposed C++20 SDK surface. Bind actions in the client and buffer input until a 60 Hz simulation
step is available. Consume pointer deltas and button edges once, including when a render frame
contains several steps. Release aim and clear pending actions on focus loss.

```cpp
auto gun = game.weapons.equip(player, "range-rifle");
if (!gun) co_return;
auto view = owe::View::FirstPerson;
while (auto frame = co_await game.next_frame()) {
    controls.collect(frame->input());  // client bindings and edge buffer
    for (float dt : frame->fixed_steps()) {
        auto input = controls.take_tick();
        if (input.change_view)
            view = view == owe::View::FirstPerson
                ? owe::View::Shoulder : owe::View::FirstPerson;
        game.camera.set_view(player, view);
        game.camera.look(player, input.look_delta);
        game.camera.set_aim(player, input.aim_held);
        if (input.reload_pressed) game.weapons.reload(gun.value);
        if (input.fire_pressed) game.weapons.fire(gun.value);
        game.world.step(dt);
        for (auto event : game.weapons.events())
            on_weapon_event(event);      // score, sounds, HUD
    }
    frame->present();
}
```

### 4.2. What the reticle means

- `FirstPerson`

  Anchor the camera to the eye socket and render the equipped view model. Aim moves its sight socket
  to the reticle and eases toward the authored aim field of view.

- `Shoulder`

  Keep the actor visible and mount the same weapon at the hand socket. Aim brings the camera closer
  to the shoulder. Sweep the camera boom against world geometry each tick to keep the lens in open
  space. Changing view preserves look direction, aim intent, magazine, reserve and reload progress.

- `Shot resolution`

  Cast from the active camera through the reticle to the nearest eligible surface or the range
  limit. Cast from the weapon muzzle toward that point, ignoring the owner and equipped prop. The
  first obstruction on this second segment receives the impact. Emit its actual position and surface
  normal for effects. A shoulder camera can see around cover while the muzzle is still blocked.

### 4.3. Weapon state and destructible props

The C++ client declares a weapon ID, GLB, grip/muzzle/sight offsets, magazine size, reserve ammo
type, shot interval, reload duration, damage and effect IDs. The first two loadouts use a semi-auto
rifle and a handgun. Each accepted trigger press consumes one round. Holding fire produces one shot
until release. A cooling or reloading weapon rejects the press. An empty magazine emits dry fire.

Track magazines per weapon instance and reserve counts per actor and ammo type. At reload
completion, transfer the smaller of available reserve and magazine space. Switching weapons cancels
a pending reload before transfer. Expose loaded, capacity, reserve and reload progress in a
read-only snapshot so the HUD can recover after a pause.

Damageable props declare integrity, surface type and an optional broken prefab. Apply a hit once
during the fixed step. Crossing zero disables the intact collider, emits one destruction event and
spawns the replacement if authored. Impact chips, flash and short-lived debris use a capped effects
pool. Static map placements keep their read-only identity; spawn a runtime damageable instance for
authored breakables.

### 4.4. SDK boundary and acceptance

Add versioned fixed-width records for camera intent, weapon commands, snapshots and ordered shot,
impact, reload and destruction events. Use numeric handles, explicit enums and caller-owned buffers
across native and separate Wasm memories. Drain events per simulation tick. Return an error for a
stale owner or an unknown weapon ID. A map replacement invalidates weapon handles together with
their owners. Existing v1 packets and games keep their current layout.

Breakwater exercises first-person aim, single-shot input, separate magazines, reload and panel
destruction in a native C++ client. Montréal uses its existing third-person camera. For engine
acceptance, replay one input sequence in native and Wasm clients, including zero-step and catch-up
frames, a blocked muzzle, an interrupted reload and a map change. Compare ammo, hit entity and
destruction event counts. Editing a weapon definition rebuilds the client independently of the
engine.

## 5. Proposed collision queries

Expose the same world queries the weapon and camera systems use. Clients can pick an object, check
clearance for a spawn, or detect a checkpoint crossing using authored colliders. Each query reads
the latest completed simulation tick. Queued movement becomes visible after `world.step(dt)`.
Coordinates are world-space metres, with +Z up and +X actor-forward at zero yaw.

```cpp
owe::QueryFilter filter{};
filter.layers = owe::Layer::Scenery | owe::Layer::Damageable;
filter.ignore_owner = player;        // includes attached equipment
auto ray = game.camera.reticle_ray(player, 60.f);
if (ray) {
    auto hit = game.world.raycast(ray.value.segment, filter);
    if (hit && hit.value.found)
        show_marker(hit.value.point, hit.value.normal);
}
auto clearance = game.world.sweep(shoulder_sphere, pivot, desired_eye, filter);
auto occupied = game.world.overlap(spawn_capsule, spawn_pose, filter, hits);
```

- `raycast(segment, filter)`

  Supply two finite endpoints. Return the nearest hit on the closed segment, with collider handle,
  optional owner entity, surface ID, point, outward unit normal, distance in metres and fraction
  along the segment. A miss succeeds with found=false. Reject a zero-length segment. An origin
  inside a solid reports fraction zero and started_inside=true; the normal is absent for this case.

- `sweep(shape, start, end, filter)`

  Translate a sphere, upright capsule or oriented box at fixed orientation. Return the earliest
  blocking hit, including the shape origin at impact and the point on the collider. Initial overlap
  reports started_inside. Use a sphere sweep for a shoulder camera and a capsule sweep for player
  movement.

- `overlap(shape, pose, filter, hits)`

  Return intersecting collider handles in stable handle order. Accept a caller-owned buffer with
  capacity and report written and total counts. A zero-capacity call measures required space. Use
  this for pickups, sensors and spawn clearance.

- `contacts(tick, cursor, buffer)`

  After each step, drain begin, persist and end pairs before stepping again. A pair is keyed by map
  generation and collider handles, ordered by handle. The event carries its tick, both optional
  entity handles and sensor status. Solid contacts can include a point and a normal from A toward B.
  Removing a touching collider emits one end event using its old handle. Replacing a map clears the
  stream and invalidates cursors.

- `Collider and filter rules`

  Prefabs declare shape, local pose, layer, collision mask and solid/sensor role. Static map
  geometry has collider handles even where there is no editable entity. The default query includes
  solids. Callers opt into sensors, choose a layer mask and may ignore an owner and attachments.
  Ignore render-only debris. Resolve equal-distance hits in stable collider-handle order.

Version the query ABI alongside the weapon ABI. Copy request and result records across Wasm
memories, identify colliders by generation-checked handles, and return explicit invalid-input,
stale-handle or unsupported-shape errors. A short contact buffer returns a continuation cursor for
the same tick. Reusing a cursor after the next tick reports expiration.

Engine acceptance fixtures cover misses, exact endpoints, an origin inside a box, owner exclusion,
sensor filtering, a shoulder view over blocked cover, an overlapping spawn, a swept capsule crossing
a thin wall, contact lifecycle, buffer pagination and stale handles after a map reload. Run the
fixtures against the native and Wasm bindings.

## 6. Proposed shader hooks

A game starts with the engine shaders. From its C++ coroutine it can install WGSL for either stage,
replace it during play, or restore the built-in shader. The selection belongs to the game session
and survives map switches.

Switchback Circuit uses the existing `game.retro_lens(strength)` preset for a vignette, violet
shadows and RGB separation. The strength ranges from 0 (standard view) to 1 (full effect). The
following interfaces would let a client supply its own WGSL.

### 6.1. Installing and resetting a shader

Pass the WGSL functions below as UTF-8 string literals named `track_material` and `crt`. In this
excerpt from `play`, pressing P restores the default post-processing while keeping the circuit
palette.

```cpp
using Stage = owe::ShaderStage;
auto status = co_await game.shaders.install(
    Stage::PreLighting, "track-material", track_material, sizeof(track_material) - 1);
if (status != owe::Error::Ok) co_return status;
status = co_await game.shaders.install(
    Stage::PostScene, "track-crt", crt, sizeof(crt) - 1);
if (status != owe::Error::Ok) co_return status;

while (auto frame = co_await game.next_frame()) {
    if (frame->input().pressed(owe::Key::P)) {
        auto err = game.shaders.reset(Stage::PostScene);
        if (err != owe::Error::Ok) co_return err;
    }
    for (float dt : frame->fixed_steps()) game.world.step(dt);
    frame->present();
}
```

- `co_await game.shaders.install(stage, label, utf8, byte_count) → Error`

  Use `ShaderStage::PreLighting` or `ShaderStage::PostScene`. The label identifies the source in
  diagnostics. The engine copies the label and source on submission. The byte count excludes any
  terminator. A literal, generated source or bytes read by the client all use the same call.

- `game.shaders.reset(stage) → Error`

  Restore that stage’s built-in pipeline. Resetting an already-default stage succeeds. The next
  `present()` uses the selection active at that call.

### 6.2. Pre-lighting material hook

For v1, the hook receives the engine’s base color for opaque terrain and opaque prefab surfaces,
world position in metres (Z up), and a world-space unit normal. It returns linear RGB. The default
returns the input color. Coverage, depth and shadow casting stay under engine control. The track
shader maps luminance to a stepped paint palette before lighting.

```cpp
constexpr char track_material[] = R"wgsl(
fn owe_pre_lighting(s: OweSurfaceV1) -> vec3f {
    let luma = dot(s.base_color, vec3f(0.2126, 0.7152, 0.0722));
    let band = floor(clamp(luma, 0.0, 1.0) * 4.0) / 4.0;
    return mix(vec3f(0.08, 0.025, 0.16), vec3f(0.96, 0.44, 0.18), band);
}
)wgsl";
```

### 6.3. Post-scene pixel hook

The post hook runs once per scene pixel after the engine’s ambient occlusion, atmosphere and
snowfall, before UI. Its input is the tone-mapped scene in linear RGB, with UV coordinates running
from the top left (0, 0) to the bottom right (1, 1). It returns linear RGB for the output target to
encode. The engine preserves alpha. The default presents the engine composite directly.

```cpp
constexpr char crt[] = R"wgsl(
fn owe_post_scene(p: OwePixelV1) -> vec3f {
    let row = u32(floor(p.uv.y / p.texel_size.y));
    let beam = select(0.68, 1.0, row % 2u == 0u);
    return floor(clamp(p.color, vec3f(0.0), vec3f(1.0)) * 15.0 + 0.5)
         * (beam / 15.0);
}
)wgsl";
```

For effects such as blur, v1 would also expose `p.texel_size` (inverse scene dimensions) and
`owe_sample_scene(uv)`, which samples the same read-only scene image with linear filtering and
clamps at its edges.

### 6.4. Engine changes and failure behavior

1. Add `Shaders` to the C++ SDK and a versioned shader submission entry point to the native library
   and Wasm imports. Pass stage, label and source as a stage ID and length-delimited byte ranges.
   The browser bridge checks both ranges against client memory and copies them into the engine. Keep
   the existing 180-byte command packet for short control operations such as status and reset.
2. Compile the submitted function against the engine’s v1 WGSL template and bindings. Suspend the
   calling coroutine until validation completes or the window closes. Publish the replacement only
   on success. On failure, resume with a shader error, report the stage, label and source-relative
   location when available, and retain the last working selection. Closing the window completes the
   await with `Closed`.
3. Invoke the material hook in the opaque surface path. When a post hook is active, draw the
   composite and snowfall into a sampleable, single-sample color target, then run the hook before
   the HUD. Resize that target with the scene viewport. The built-in post path presents directly.

### 6.5. Acceptance test with Switchback Circuit

Run the same fixed scene on native and WebGPU with defaults, each hook alone, and both together.
Install a second post shader during play, submit invalid WGSL and confirm the second shader still
draws. Reset post and verify the material tint persists. Exercise a source longer than the command
packet, window closure during compilation, map switches, resize and multisampling. Rebuild an
external client after editing its shader literal and verify that the engine keeps its original
timestamp.

## 7. References

- [C++20 SDK header](../sdk/owe/include/owe/game.hpp)
- [Switchback Circuit client implementation](../examples/switchback-circuit/game.cpp)
- [Breakwater native C++ client](../examples/breakwater/README.md)
