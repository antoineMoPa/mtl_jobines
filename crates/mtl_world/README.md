# Cached game world library

`mtl_world` owns map preparation (Hermes), simulation, missions, audio, prop
schemas, seasons, device budgets and browser asset storage. `mtl_jobines` owns
the renderer, UI and platform entry points, and depends on this library.
Renderer/UI and shader edits therefore reuse compiled world code and its
embedded vehicle/pedestrian assets. World changes still rebuild the application.

The library root is `../../src/world.rs`. Modules deliberately retain their
existing paths under `src/`, preserving asset/fixture paths and allowing ongoing
work without a large file move. Each module belongs to exactly one crate.
The audio settings window lives in `src/engine/sound_settings.rs`, keeping egui
out of the world's direct dependencies.

The world build script lives at `../../build.rs`; the application has no build
script. It fingerprints world sources, OpenCity and generator assets for map
cache invalidation. Renderer/UI edits do not invalidate that fingerprint.
Repository-relative test data uses `MTL_WORKSPACE_DIR`, emitted by the script,
because `CARGO_MANIFEST_DIR` now refers to this manifest's directory.

From the repository root:

```sh
cargo build --release --locked --timings
cargo test --locked --lib -- --test-threads=1
cargo check --locked --target wasm32-unknown-unknown
```

The default workspace members include both game crates, so `cargo test` keeps
running the moved tests. `cargo test -p mtl_world --lib` runs only world tests;
`cargo test -p mtl_jobines --lib` runs only renderer/application tests. The
`test-support` feature exposes constructors and scene setup used by game tests;
it is enabled by the application's dev dependency, not normal builds.

To measure a warm renderer rebuild, finish one release build, touch
`src/engine/frame_rate.rs`, then repeat the build with `--timings`. Only the
application should compile. An unchanged build should compile neither crate.
The first build after introducing the split must compile the new library.
