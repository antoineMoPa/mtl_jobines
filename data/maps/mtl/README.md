# Authored geographic map

## Current installed street network

The installed map has 2,822 segments and 200 native surface junctions, covering
57 selected corridors from an inventory of 6,254 exact source street names.
The final candidate has zero design-rule issues, unresolved roads or naming
conflicts. The terrain-aware audit also finds zero missing junction seams or blocked
lane entrances. Both active road and terrain files match the validated research artifacts.
The sections below describe the earlier geographic migration and interchange work;
their historical segment counts and pending street-authoring notes are superseded
by this installed network.

New streets use simplified two-way profiles, explicit shared junctions and 51
authored clearance crossings. Nine corridor offsets (up to 1.81 map units) and
local endpoint adjustments fit the old screenshot traces. Source selections are
preserved separately; the research page compares source and authored placement.
This is a gameplay map, not a surveyed road network. Exact lane counts, one-way
restrictions and real-world over/under arrangements are not reproduced.

New surface grades are limited to approximately 11.5%. Two pre-existing highway
transitions remain at approximately 25.5% and 24.9%. The terrain uses a broad,
smoothed painted hill and gradual river/canal banks, independent of road alignments.
The hill peak is approximately 78.92 game metres instead of 111.05. Shore tapering
uses water connected to the map frame; isolated hill ponds do not flatten the summit.
Every water cell is preserved. These are authored game heights, not a surveyed DEM.

Street endpoints are lower by a median of 10.03 game metres, with a maximum
reduction of 55.90 m. Existing interchange attachments retain their reviewed heights.
Elevated crossings remain decks rather than seeds for spreading raised terrain;
unrelated nearby streets no longer inherit their heights. Road-bed fitting changes
194,210 pixels relative to the independent natural-relief raster and interpolates
through 165,224 block pixels. Its 1,729,791 full-width samples give a 0.50 m minimum,
0.80 m median, 1.21 m 95th percentile and 2.92 m maximum sampled road-center gap,
excluding intentional elevated/protected and water-crossing areas. No new water
pixels are created. Source road XY, names, junctions and routing are preserved.

Research, exact validation receipts, geometry/terrain audits and QA evidence are
in `research/montreal-street-decimation/`, attached to the map task. That directory
remains ignored. Authoring uses frozen road and elevation inputs, so installing
the result does not feed the edited terrain back into future generation.

Junction seam discovery follows short, fully clipped approach pieces to the
visible pavement; runtime height interpolation and safety barriers use those same
seams. Curved entrances stay open while closed junction sides keep their barriers.
Turn-room limits follow subdivided connecting roads. Two adjacent source T ends
share one junction, and one wholly hidden dead-end stub is removed.

Verification: 175 game tests pass (one profiling test ignored). There are 110 focused
core checks across the recorded batches, including external models, junctions,
terrain support and indexed building collisions, plus five research-helper checks.
Native and browser builds succeed. The final downtown driving check travels 75.3 m
on asphalt at 106.6 km/h without collisions. Its inspected screenshot shows
continuous junction pavement. Neighborhood and both tower views are inspected. Research
page controls/layout pass desktop and mobile checks. This samples driving rather
than exhaustively checking every possible route. The task includes screenshots
and `completion.json` with the full evidence and limits.

## Neighborhoods, shops and landmarks

The live map includes 51,898 residential buildings, 1,612 mixed-use buildings,
and 22 researched landmark models on fitted plinths: 53,554 building features
including the 22 foundations. Existing detailed street frontages are joined by
51,085 packed background and infill buildings. Background occupancy and orientation
come from local source-density cells; new infill follows nearby surface streets.
Lots reserve street strips, rendered junction pavement, existing buildings and
8,289 anonymous protected green-space polygons. Water and unsupported sites are
excluded. They represent developed areas rather than
individual source parcels. Neighborhood form varies by height, density, lot width, setback and
depth: garden houses in the west, central plex blocks, southwest row/plex blocks,
eastern low/mid-rise housing, and glass/concrete towers in the financial core.
These are representative authored patterns, not parcel-by-parcel reconstruction.

The density pass enlarges 48,851 background buildings, increasing the retained
background footprint area by approximately 114%. It adds 966 street-facing and
1,235 block-interior buildings and removes 8,880 generated buildings from protected
green space. Foundations fit the authored slopes with steps up to six game metres;
the terrain, roads, starts and landmark models are preserved. Parks, woods, wetlands,
gardens, cemeteries and other protected land come from the anonymous authoring layer
`green-spaces.road`. Relation outer rings are conservatively reserved, including
their interiors. The reproducible Rust authoring tool is in `tools/city-density`.

Landmark locations and footprints come from the local PBF and the shared geographic
calibration. Study 01 is installed at its actual mapped footprint. Model shapes are
uniformly sized to those footprints in the compressed map; two use smaller scales
to clear simplified roads. Studies 20, 22, 23, 26, 28 and 30 remain reserved because
roads occupy their geographic sites. No model is silently moved to an invented site.
Full source IDs, coordinates and fitting decisions are in research `population.json`.

`data/landmarks` contains compact models baked in Rust from the textured research
GLBs, with transformed geometry/normals and sampled texture colors. The renderer
replaces their visual proxy boxes and retains conservative collision footprints.
Parcel generation is baked into `.road` buildings; the editable zoning source is
retained in research `population-source.road`. Both platforms load the baked map.
Developed-area source cells and the Rust baker are retained beside that zoning file.
Interior area shops use compact glass/door panels; authored street frontages retain
their detailed shops, stairs and balconies. Interior blocks use existing procedural-
window materials; detailed masonry palettes stay on the authored curb facades.
Flat terrain runs share quads while
sloped cells retain the original support triangles. The combined scene has
17,875,167 vertices, below the existing 20-million cache cap.
Native City preparation currently takes approximately 40–50 seconds on the working
machine, including cached-mesh loads. Dense-map startup remains a performance limit.

The canonical live files are `data/maps/mtl/map.road` and `elevation.png`. Native
loads them at startup; browser builds embed them and require a rebuild/reload.
The research helper's `--apply-terrain` checks the candidate against the active
road source and verifies terrain acceptance before installing it. After changing
terrain, regenerate fitted population foundations before rebuilding the browser.
The base population recipe is `--settle-city`, `--fill-development`, `--validate`,
then `--audit`, using frozen inputs. `--settle-city` also refits existing foundations
and preserves the explicit study_02/study_14 height corrections. Do not run the
historical `--ground` pass against this expanded map.
After a population rebuild, reapply the density authoring pass using the frozen
map, terrain and `green-spaces.road` inputs before installing and rebuilding the browser.

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
The initial interchange map had 243 segments, eleven one-way ramp movements and a divided
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

## Surface-road ground correction and game start

The preceding ground-correction map had 578 segments. Its 41 ordinary surface curves were split into
376 exact cubic pieces so grades follow the existing painted terrain rather than
remaining tens of game metres above it. XY alignment is preserved. Lane attachments
were remapped to the corresponding child curves; bridges and ramps retain their geometry.

Local road-bed earthworks affect 3,381 of the 1,048,576 elevation pixels (about 0.32%).
Water pixels and terrain around highway/ramp footprints are preserved. Away from
intentional overpass areas, sampled road-centre clearance is 0.80 game metres at the
median, 0.92 m at the 95th percentile, and 2.14 m maximum. These are authored game
heights, not surveyed elevations. The corrected road network has zero design-rule issues.

`player_start(47.771893, 9.284870, -143.036250);` starts the game on the central
surface corridor near downtown, facing toward the north of the street grid.
The generic game metadata uses world XY and heading degrees. Explicit `--start`
coordinates, including editor launches, override this default.

The subsequent street-decimation network is now installed; see the current
network section above for counts, validation and remaining artistic limits.
