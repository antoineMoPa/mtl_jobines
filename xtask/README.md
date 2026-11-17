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

The browser release commands are documented in [web/DEPLOY.md](../web/DEPLOY.md).

## Native game build logging

Moon's Game UI target uses `cargo xtask game-build` and `cargo xtask game-run`.
These use the incremental native `dev` profile: the application uses optimization
level 1 and line-table debug information; world simulation and OpenCity geometry
use optimization level 3. This avoids full release optimization of application
code on each edit while keeping simulation and map preparation fast. The first
build prepares this profile's cache. Shipping and web builds continue to use
`cargo build --release` with their existing settings.

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
After the logged build, Cargo verifies freshness and resolves the executable,
including custom target directories, before launching it.
