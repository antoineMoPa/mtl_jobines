# Map editing

[map.road](map.road) is the scene's source of truth.

Run `cargo run`. Saving the active `.road`, its elevation PNG or a referenced GLB reloads the native
game automatically. Invalid edits keep the last working map; see the terminal
for errors. Browser builds embed the map and require rebuilding.

Native building GLBs resolve relative to the `.road` file and stay outside compiled
assets and map packages. Material/vertex colors are supported; textures are not.

Rebake elevation from the `.road` recipe:

```sh
cargo run --manifest-path opencity/Cargo.toml --no-default-features \
  --example bake_elevation -- data/maps/mtl/map.road
```
