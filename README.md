# mtl_jobine

Run `make editor` for the native map editor, or `cargo editor -- path/to/map.road`.
The game and editor share a Cargo workspace, lockfile, and build cache. Map-only
edits use live reload and do not require rebuilding either executable.