# Map editing

[map.road](map.road) is the scene's source of truth.

Run `cargo run`. Saving the active `.road` or its elevation PNG reloads the native
game automatically. Invalid edits keep the last working map; see the terminal
for errors. Browser builds embed the map and require rebuilding.

Rebake elevation from the `.road` recipe:

```sh
cargo run --manifest-path opencity/Cargo.toml --no-default-features \
  --example bake_elevation -- data/maps/mtl/map.road
```
