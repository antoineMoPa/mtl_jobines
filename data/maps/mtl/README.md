# WIP geographic terrain and independent corridors

## Geographic frame

`georeference.json` maps WGS84 longitude/latitude to world XY and back. The frame
is north-up with equal scale in local Web Mercator, centered at (-73.73, 45.53).
The old screenshot shear and northern correction have been removed. Frame corners
define the transform; their numerical residuals are not surveyed accuracy.

The terrain spans 42 km at the center in 153.6 map units. One unit represents
273.4375 ground metres here; the game uses 100 metres per unit, so geography is
compressed approximately 2.734375 times. Ground scale varies slightly with latitude.
Lane widths remain authored: 0.045 units (4.5 game metres), sidewalks 0.032 (3.2 m).

```sh
opencity/target/debug/opencity map-coordinates \
  --calibration data/maps/mtl/georeference.json --lonlat "-73.5897111,45.5085881"
opencity/target/debug/opencity map-coordinates \
  --calibration data/maps/mtl/georeference.json \
  --input /tmp/street.geojson --output /tmp/street-world.json
opencity/target/debug/opencity map-coordinates \
  --calibration data/maps/mtl/georeference.json --report
```

Build with `cargo build --manifest-path opencity/Cargo.toml --no-default-features
--features geodata --bin opencity`. Use `--world X,Y` for reverse conversion. Bulk
mapping retains source GeoJSON and OSM IDs; it does not simplify or join roads.

## Terrain

`elevation.png` is 1024-square, 16-bit grayscale, with bounds ±76.8, heights
-0.18 to 1.2, and water at zero. Coastlines come from water multipolygons and
closed water ways in the user's local `~/Documents/maps/Montreal.osm.pbf`.
Only polygon relations are filled; river centerline collections are excluded.
Inner rings preserve islands and river gaps. Partial polygons are closed only
where the entire closing edge lies outside the map bounds. Pixel spacing is
approximately 41 ground metres; very narrow water features can disappear.

The rough mountain retains the user's painted relief and peak height. Its summit
is anchored to OSM node 2517010971 at world (40.009832, 8.715367). The crossing
road and western-flank road guide its location; source IDs are saved in the
calibration. Heights are artistic approximations, not a DEM or street surveys.
Attribution: © OpenStreetMap contributors, ODbL.

## Roads

The eight original screenshot-traced corridors comprise route 15 and southern
span, bridge 134, routes 40, 20, 136, and western, central and eastern arterials.
Their trajectories were mapped through the old inverse calibration into the new
frame. Two curves were split at the old northern correction's branch boundary;
shared tangent directions smooth those seams, deviating at most 0.071405 units
from exact reprojection. Profiles and lane widths are unchanged; bridge 134
remains undivided, while the expressways retain their medians.

The initial geographic migration retained 189 segments in eight separate chains.
The current map has 243 segments, eleven one-way ramp movements and a divided
20–136 through link. Only existing roads connect:

- North interchange: 15 north to 40 east, 40 west to 15 south, 40 east to 15 south.
- Southern interchange: 15 north to 20 west and the reverse movement;
  15 south to 136 east and the reverse movement; 20 and 136 connect both ways.
- Central arterial: four entry/exit ramps connect both highway 40 carriageways.

The A15 northern stub is removed: all three northbound lanes continue onto 40,
and all three southbound lanes originate from 40. The second 40 entry merges
into the incoming collector before A15. All six terminal lanes meet exactly in
position, height and travel direction. See the task’s a15-terminal.html comparison.

Other interchange turns and exits to unbuilt roads remain omitted. Five original
corridors are now connected; bridge 134 and the western/eastern arterials remain
separate, giving four connected components. All 22 ramp ends attach to explicit
car lanes with matching heights and one-way travel. Maximum ramp grade is 11%.
Surface roads clear terrain across their full width. Highway 40 has a raised deck
over the central arterial, which is at ground level there. Downtown 136 remains
underground. Design-rule checks report zero issues. Portals and terrain cutouts
remain deferred.

Interchange curves use local PBF layout/layer context, adapted to the existing
screenshot-traced alignments. They are not exact source-way imports. Deck heights
are authored for gameplay, not surveyed. The task's `interchange-connections.html`
contains compiled top/oblique previews and movement coverage; provenance and
clearance/topology results are in its `interchanges/` folder. These ramps have
not yet had an in-game driving test.

The five additionally requested street selections have been extracted and mapped
from local OSM, but are not yet authored in `map.road`. Their complete linework
now has zero below-water or outside-terrain samples. This verifies consistency
with the shared OSM source, not independent survey accuracy.

## Historical reference data

The CSV files preserve original screenshot picks and registration parameters.
Columns prefixed `legacy_` contain coordinates and heights from before this
rebuild; do not use them as current world coordinates. The active frame is solely
`georeference.json`. Before/after terrain, source rings, placement diagnostics,
road checks and a visual comparison are attached to the new-map Moon task.

## Remaining road alignment differences

The road-corridor-audit.html task attachment compares source linework in the same
unwarped geographic frame. The original screenshot traces still drift: A15 is
about 100 ground metres from its source carriageways on average; the Champlain
water span is about 228 m off, and the Jacques-Cartier span is about 264 m off.
These nearest-carriageway diagnostics are not survey measurements. The northern
bridge endpoint is near its surface approach, but that continuous street has not
been authored. A15’s depressed corridor is also not modeled as a continuous trench.
The terminal connectivity correction leaves these alignment refinements pending.

## Hover hierarchy

The 243 segments use 23 sparse name anchors and five explicit name-change boundaries.
OpenCity infers the intervening road groups from the connection graph. Hovering shows its
route or street, named crossing or corridor where applicable, then its segment ID.
For example: Route 134 > Jacques-Cartier Bridge > bridge_134_s001. These labels
are map data and survive editor rename/copy operations; they do not alter geometry.
Concurrent route numbers use additive road_routes anchors. Ramps retain their own
interchange labels. All existing segment names and geometry were preserved during migration.
