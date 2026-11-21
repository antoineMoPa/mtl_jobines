# Cleanup task

```sh
cargo xtask                    # Format Rust and wrap Markdown prose.
cargo xtask --staged           # Format staged source snapshots and update the index.
cargo xtask --check            # Read-only check of working-tree sources.
cargo xtask --staged --check   # Read-only check of staged snapshots.
```

Rust sources use rustfmt, including nested research tools. Markdown prose wraps at
100 columns while preserving frontmatter, raw HTML, code fences, indented code,
tables and inline code.
Line width and Rust file length do not cause cleanup or commits to fail.

Staged formatting updates the working copy when it matches the staged contents.
For partially staged files, only the staged snapshot is formatted; unstaged bytes
remain untouched. Source-file modes are preserved. The pre-commit hook already
runs this command, so it now applies safe formatting fixes automatically.

`make fmt` applies formatting; `make lint` checks without writing. Asset generation
is unchanged: `cargo xtask assets`.

The browser release commands are documented in [the browser release guide](browser-release.md).

## Native game build logging

Moon's Game UI build command is `true` (a no-op). Its run command,
`cargo xtask game-run`, compiles once and launches the resulting executable
directly. This avoids compiling in both Moon's build phase and its run phase.
For an explicit standalone build, use `cargo xtask game-build`.

Game compilation uses the incremental native `dev` profile: the application uses
optimization level 1 and line-table debug information; world simulation and
OpenCity geometry use optimization level 3. This avoids full release optimization
of application code on each edit while keeping simulation and map preparation
fast. The first build prepares this profile's cache. Shipping and web builds
continue to use `cargo build --release` with their existing settings.

Moon game builds use `target/moon-game`, independently of ordinary agent
checks/tests and editor builds, so those commands can compile concurrently
without waiting on the game's Cargo lock. When `CARGO_TARGET_DIR` is set,
the game cache is its `moon-game` child directory. The first game build
populates this separate cache; subsequent launches reuse it.

Native game and OpenCity libraries emit only `rlib` artifacts, allowing Cargo to
keep hashed filenames for different feature/profile variants in the shared
target directory. The web build scripts explicitly request `cdylib` output with
`cargo rustc --lib --crate-type cdylib` to generate the WebAssembly modules.

The logger prints elapsed time on phase changes and every five seconds while
Cargo builds. On systems with `ps`, the update identifies an active Rust compiler
or native linker belonging to that Cargo process. If process inspection is
unavailable, it reports the combined Cargo/build-script/waiting phase. A linker
that finishes between the one-second samples may not appear in the updates.

Normal Cargo warnings and errors remain visible. `--timings` saves Cargo's HTML
report, with the path printed in the build output, for comparing library and
executable compilation times. Progress logging does not change compiler flags or
invalidate the game build cache. A failed build does not launch the game.

`cargo xtask game-run -- --map path/to/map.road` passes arguments to the game.
`game-run` launches the executable reported by Cargo directly, including custom
target directories, and preserves game arguments and exit status. It does not
start another Cargo invocation or retry compilation when agents edit files.
Edits made during compilation may require the next launch.
