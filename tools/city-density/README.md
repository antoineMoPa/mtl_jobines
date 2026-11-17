# City density authoring

This Rust tool packs existing developed-area rectangles toward their neighbours and adds
street-facing apartment rows and block interiors on vacant land. It writes ordinary `.road`
buildings; the game
does no additional population work at startup. Protected green-space geometry lives in
`data/maps/mtl/green-spaces.road` and is an authoring reservation layer, not a second terrain
surface.

Create a working directory containing frozen inputs `before.road`, `elevation.png`, and
`green-spaces.road`. Copy the canonical map, terrain, and reservation file there before starting.
From the repository root, run:

```sh
CARGO_TARGET_DIR=opencity/target cargo run --locked \
  --manifest-path tools/city-density/Cargo.toml -- pack WORK_DIRECTORY
```

Review `dense.road` and `report.json` before installing the map. Packing preserves road geometry,
terrain metadata, starts, authored buildings and landmark models. The tool checks all generated
buildings against streets, junction pavement, other buildings and protected green space after
serializing the result. Enlarged and new lots also sample terrain across their interiors. Existing
lots that fail the stricter terrain check remain unchanged and are counted in the report.

Junction reservations use the same pavement and sidewalk patches as the renderer. The land outside
curved curbs can receive buildings. Special junctions without those patches keep their conservative
footprint reservation. Street rows try several setbacks to accommodate curved approaches.

The `fill WORK_DIRECTORY` command adds street rows and block interiors without enlarging buildings.
It uses the same frozen input names and produces the same output files. Use a new working directory
with the previous `dense.road` copied to `before.road` when continuing an already packed map.

The pass grows rectangle sides in map units down to a 0.0005-unit step, then insets enlarged
footprints by 5 cm at game scale to keep collision geometry stable after decimal serialization.
New frontage rows use a 0.001-unit separation. Foundation bottoms and
heights are refitted to sampled terrain. Roof heights are retained unless more clearance is needed.
The infill budget replaces the buildings removed from protected land plus at most 1,500 buildings;
check the final runtime scene against its existing cache limit before installing. A separate
fill pass permits at most 10,000 additional buildings and requires the same runtime check.

Interior lots stay within 1.8 map units of a surface street and follow the nearest street
direction. Several footprint sizes fit the remaining gaps. Central blocks are filled first when
the building budget is reached. Every accepted lot reserves its footprint before the next is tried.
Foundation steps may span up to six game metres across a lot; the building shell reaches above the
highest sample while its bottom stays below the lowest sample. Water and larger drops remain
excluded.

To refresh the reservation layer from a local PBF:

```sh
CARGO_TARGET_DIR=opencity/target cargo run --locked \
  --manifest-path tools/city-density/Cargo.toml -- \
  extract-green SOURCE_PBF data/maps/mtl/georeference.json OUTPUT_ROAD
```

Only anonymous polygons are exported. Protected tags cover parks, gardens, nature reserves, golf
courses, forests, woods, wetlands, cemeteries, recreation grounds and allotments. Closed ways and
stitched relation outer rings are supported. Relation interiors are conservatively reserved with
their outer ring. The geometry represents source land use in the compressed game frame, rather
than surveyed parcels. Attribution: OpenStreetMap contributors, ODbL.

## Playable inner loop

Use `inner-loop WORK_DIRECTORY` for the current compact Montreal map, with the same frozen
input names. Copy `data/maps/mtl/green-spaces.road` alongside the map and elevation image.
This explicit plan maps `cross_street` and `cycling_street` to residential frontage and
block interiors, with 2–4 storey brick apartments and 6–20 storey mixed-use downtown buildings
around Bonaventure. It stays inside the interstate ring and leaves the mountain footprint
and the large Parc Jarry block west of Jean-Talon station clear. Existing buildings that
intersect either reservation are removed, including named buildings inside authored blocks.
Roads, terrain, props, and other buildings retain their authored geometry.

Parking and pedestrian paving, station excavations/roofs, trees and other authored fixtures
also reserve land. Every serialized new footprint is checked against those reservations,
roads and buildings. The final inner-loop audit checks **all** buildings against both parks.
Review the removed building names in `report.json` before installing the result. Keep source
comments and concurrent map edits when transferring the approved building changes.

The runtime cache has a 20 million vertex limit; check the generated scene before installing.
This tool enables OpenCity's render-materials feature because current runtime tree texture
support requires it. Set `CARGO_TARGET_DIR=target` to share the root workspace's build cache.
