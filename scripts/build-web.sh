#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dist_dir="$repo_dir/web/dist"

rm -rf "$dist_dir"

cargo run --quiet --locked --manifest-path "$repo_dir/xtask/Cargo.toml" -- assets

# Opt in to a larger download that skips runtime mesh/SVG preparation.
if [[ "${BUNDLE_GAME_DATA:-0}" == "1" ]]; then
  mkdir -p "$dist_dir"
  cargo run --manifest-path "$repo_dir/Cargo.toml" --locked --release -- \
    --build-game-data "$dist_dir/default.game-data.gz"
  export MTL_JOBINES_BUNDLED_GAME_DATA="$dist_dir/default.game-data.gz"
fi

cargo build --manifest-path "$repo_dir/Cargo.toml" --locked --release \
  --target wasm32-unknown-unknown --lib

mkdir -p "$dist_dir/pkg"
wasm-bindgen "$repo_dir/target/wasm32-unknown-unknown/release/mtl_jobines.wasm" \
  --out-dir "$dist_dir/pkg" --target web --no-typescript
cp "$repo_dir/web/index.html" "$dist_dir/index.html"
cp "$repo_dir/web/boot.png" "$dist_dir/boot.png"
while IFS= read -r asset; do
  mkdir -p "$dist_dir/$(dirname "$asset")"
  cp "$repo_dir/$asset" "$dist_dir/$asset"
done < "$repo_dir/web/assets.list"
touch "$dist_dir/.nojekyll"

printf 'Web build ready: %s\n' "$dist_dir"
