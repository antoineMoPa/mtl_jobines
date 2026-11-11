# Game performance checks

Build the native game in release mode, then use its synchronized GPU driving profiler:

```sh
cargo build --release -p mtl_jobines --bin mtl_jobines
target/release/mtl_jobines --smoke /tmp/game-day.ppm --profile --profile-realtime
target/release/mtl_jobines --smoke /tmp/game-night.ppm --profile --profile-realtime --night
target/release/mtl_jobines --smoke /tmp/game-large.ppm --profile --profile-realtime \
    --smoke-size 2560x1600
target/release/mtl_jobines --smoke /tmp/game-cycle.ppm --profile-120 --sky-auto --sky-clock 300
```

The native process needs access to a real GPU. Each run also writes snapshots of driving,
menus, navigation and vehicle lights beside the requested image.

`--profile --profile-realtime` measures 720 frames with two 120 Hz physics ticks per frame.
`--profile-120` measures 1,440 frames with one tick each. Both simulate twelve seconds with
full throttle and a fixed traffic seed. Plain `--profile` uses eight ticks per frame as a
simulation stress test. None of these runs waits for display refresh; the FPS display is
unpaced throughput, not presented window FPS.

Use `--map path/to/map.road` and `--start x,y,heading-degrees` to repeat a particular route.
Keep the map, elevation, external models, shaders, resolution and starting position identical
between builds. Freeze map inputs when another task may edit them. Run benchmarks serially,
after compilation finishes. Repeated runs help distinguish improvements from system load.

Compare total-frame median, p95, p99 and frames over the stated budget. The stage breakdown
separates simulation, moving meshes/uploads, and rendering plus the GPU wait. GPU timestamps
report shadow, world and HUD passes when supported. Treat those timestamps as diagnostics;
they are not independent wall-clock costs that can always be added together.

The measured loop covers traffic and vehicle physics, moving geometry, cached scenery, sky,
shadows, world rendering and the HUD. It excludes native window presentation, audio output,
interactive navigation updates and asset startup. A straight throttle run can hit scenery or
leave the road, so check the final capture and traveled distance. Use additional starting
positions to cover highway traffic and building-heavy views.

## Runtime invariants

Vehicle catalog geometry stays on the GPU. Each body and wheel instance has conservative
world bounds derived from its full current transform. Camera and shadow frusta select
independent instance ranges, retaining offscreen shadow casters and the existing fade values.
Adjacent surviving instances share draws without copying or re-uploading matrices.

Support queries use a separate spatial index of supporting triangles. The rendering index
still contains every triangle. The support index preserves triangle order, surface tie
breaking, bridge ceilings, terrain gradients and water handling. It adds triangle IDs rather
than another copy of the geometry.

## Regression checks

```sh
cargo test -p mtl_jobines --lib -- --test-threads=1
cargo test -p mtl_jobines --lib -- --ignored --test-threads=1
cargo test -p opencity --no-default-features --features game-runtime --lib support_index
```

The ignored game checks require the native GPU; the startup profile also uses the native
audio device. Culling tests cover transformed part bounds, opacity, near-plane crossings,
independent shadow visibility and contiguous instance ranges. The support regression compares
45,930 queries against the original rendering-index candidate lists, bit for bit.

For a visual comparison, compare matching static PPM captures byte for byte. Final driving
captures contain a measured FPS counter; exclude only that counter's rectangle when comparing
the scene. Keep the simulation tick count and sky clock equal between runs.
