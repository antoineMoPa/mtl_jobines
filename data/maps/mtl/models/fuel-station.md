# Fuel station

The game asset is exported from the editable `fuel-station.claydash` beside it.
The recipe comes from the original station design attached to the gas-station task.
All 19 text objects (including digits on displays and price signs) were removed.
Box corner radii are zero, avoiding dense tessellation of tiny rounded edges.
Shop, canopy, four pumps, hoses, roadside sign, air station, bench, planting beds,
markings and materials are retained. The two source-only ground slabs were already
excluded from the previous game GLB and remain excluded.

Export from the repository root with Node 18+ and Claydash installed:

```sh
node scripts/export-fuel-station.mjs
# Or use a different binary / preview destination:
CLAYDASH_BIN=/path/to/claydash node scripts/export-fuel-station.mjs /tmp/fuel-station.glb
```

The exporter starts a private window-free MCP session, uses direct geometry with
resolution 16 and a 0.02 m surface-error target, validates the result, then publishes
it atomically. It does not connect to an open Claydash editor. The source's flat
boxes provide the main reduction; native curved hose and shrub primitives remain.
Exports must stay under 10,000 triangles and preserve the station's placement bounds.

Before/after: 59,884 → 9,008 triangles; 1,445,432 → 362,424 GLB bytes.
The three map placements continue sharing one decoded mesh. Source bounds in glTF
axes are X ±15.1000004 m, Y −0.0500000–7.1500001 m, Z −11.1000004–10.0550003 m,
identical to the previous model. Existing map transforms and separate shop/pump
collision shells therefore retain their placement.

Game-renderer before/after captures can be generated without a desktop window:

```sh
FUEL_STATION_BEFORE=/path/to/previous-fuel-station.glb \
FUEL_STATION_CAPTURE=/tmp/fuel-station-captures \
cargo test -p mtl_jobines --lib native_fuel_station_asset_views -- --ignored --nocapture
```
