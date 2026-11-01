# WIP terrain, independent highways and major streets

Open `map.road` in the editor, or run the Moon OpenCity UI target. The relative
`elevation.png` retains the approved 153.6-square bounds, heights -0.18 to 1.2,
and water at 0. The painted mountain has been relocated as described below;
the shoreline and water samples are unchanged.

The map contains eight separate corridors, traced from the supplied screenshots:

| Corridor | Profile | Placement |
|---|---|---|
| Route 15 and southern crossing | Divided, three lanes each way | Existing approved section |
| Bridge 134 | Four unseparated car lanes with shoulders; no median | Raised highway deck at 0.35 |
| Route 40 | Divided, three lanes each way | Main highlighted route only |
| Route 20 | Divided, three lanes each way | Separate western corridor |
| Route 136 | Divided expressway | Surface western end, descending underground downtown |
| Western major street | Urban arterial, sidewalks and four car lanes | Major-street reference 1 |
| Central major street | Same urban profile | Reference 2; longer northwestern corridor |
| Eastern major street | Same urban profile | Reference 3; northeast of the central street |

Only same-corridor segments share endpoint references. All eight corridors remain
unjoined, including routes 20 and 136; complex interchange connections and ramps
are deliberately deferred. Bridge towers, rail tracks, tunnel portals and terrain
cutouts are not modeled. Negative grades place the downtown road below terrain;
the preview uses dashed purple to expose this otherwise buried geometry.

## Reference and island alignment

The trajectory is traced from the highlighted route in the user-provided
1424 x 1226 screenshot supplied on 2026-10-31. Colored-stroke jitter is smoothed
before constructing the editable cubic curves.

One global affine mapping converts reference trajectories into the native terrain
coordinates; bridge ends are not pulled independently to arbitrary riverbank
positions. This mapping is not applied to the terrain preview: the new preview
keeps the original square pixels without shearing or stretching the island.
Approximate coastal controls:

| Control | Reference X | Reference Y | Terrain image X | Terrain image Y |
|---|---:|---:|---:|---:|
| Main island southwest tip | 155 | 922 | 90 | 735 |
| Northern island southwest tip | 475 | 548 | 255 | 480 |
| Main island southern tip | 918 | 875 | 744 | 735 |
| Small island center beside crossing | 1021 | 781 | 878 | 642 |
| Southwestern small island center | 230 | 1020 | 160 | 843 |

Least-squares fit from reference pixels `(u, v)` to elevation pixels and world:

```text
image_x = 0.9407354748766414*u + 0.28411515970627677*v - 336.8120092643886
image_y = 0.009210386025627739*u + 0.7591157995550288*v + 52.38520491182365
world_x = -76.8 + image_x * 153.6 / 1023
world_y = -76.8 + image_y * 153.6 / 1023
```

Control residuals reach about 35 terrain pixels: the hand-painted shoreline is
an approximate regional fit, not survey-accurate. PNG row zero remains minimum
world Y, so image north points toward negative world Y. `alignment.csv` records
reference pixels, authored world coordinates and deck heights for each endpoint.
All construction remains editable in `map.road`.

## Additional screenshot registration

Each of the four newer screenshots is registered to the original reference using
the same two persistent map markers. `reference-registration.csv` records the
similarity transform coefficients. The partially covered blue marker in the
route-40 screenshot is located from its visible circular arc, avoiding a biased
centroid from the occluding route popup. For screenshot pixel `(u, v)`:

```text
original_u = a*u - b*v + translation_x
original_v = b*u + a*v + translation_y
```

The original affine mapping above then yields terrain pixels. A shared northern
coastline correction extends that southern fit toward the northeast island tip:

```text
image_x += 0.5 * max(0, 500 - image_y)
```

It places the northeast route endpoint near terrain pixel (944, 50). This
correction applies consistently to all four new corridors. The existing route-15
section and relocated summit lie south of this correction and remain unchanged.
The terrain render itself is never transformed. `additional-alignment.csv`
records screenshot endpoints, final world coordinates and grades.

The selected dark-purple route is traced; light-blue route-40 alternatives are
excluded. Destination-marker street turnaround artifacts are not treated as an
expressway interchange. Shorelines remain hand-painted, so the reference-fitted
river crossing and landing positions are approximate; no riverbanks were edited
to force a fit.

## Comparison and checks

The task attachment `comparison/route-comparison.png` shows the reference at its
original proportions beside the native 1024-square terrain view. Their individual
view bounds differ; neither panel is sheared or stretched. Amber is the centerline
evaluated from parsed road curves, and green dots identify the summit in both
panels. These are top-view geometry comparisons, not game screenshots.

`comparison/terrain-native.png` provides the full upright terrain preview, while
`comparison/mountain-before-after.png` compares identical native coordinates.

The highway-only pass has 147 segments: 25 in the original corridor, 5 on bridge
134, 66 on route 40, 39 on route 20 and 12 on route 136. All parse, resolve,
compile without unresolved subdivisions and pass full road design-rule checks.
Graph checks on that highway-only pass confirm five independent chains, ten
intentional terminal ends, no shared inter-route references and exact seam heights.
Full-width samples of
surface roads clear terrain by at least 0.008 map units. The bridge profile has
no `middle` band. The original route-15 reference deviation remains 2.92 pixels.

Route 136 starts at the local surface, descends between roughly 28% and 70% of
its length, and ends at height -0.12, with approximately 0.23 units of terrain
above the downtown endpoint. No editor, game or GPU job was launched for this pass.

The new `comparison/terrain-network.png` attachment is the native square overview
with corridor colors and dashed underground geometry. The attachment
`comparison/additional-routes-comparison.png` overlays each new road on its own
original reference, preserving the screenshot's proportions.

## Major streets

The three additional street references use identifiers `arterial_west`,
`arterial_central` and `arterial_east`. Their common `urban_arterial` profile has
sidewalks and four unseparated car lanes, with ordinary street role. It is a WIP
cross section; route-specific parking, bike lanes and one-way operations are not
authored in this pass. Walking-route dots establish the street alignment rather
than a pedestrian-only path. Alternative walking paths and destination artifacts
are excluded.

`street-reference-registration.csv` records screenshot-to-original similarity
transforms, and `street-alignment.csv` records source pixels, world positions and
grades. The clipped western reference marker and partly occluded central marker
are recovered from visible circle edges. The same northern coastline correction
applies. Street curves follow the painted terrain, allowing up to an 8% approach
grade envelope where the mountain relief requires it.

The two parallel streets cross route 40. Its local deck and approach grades are
raised to provide at least 0.15 map units of vertical separation at the crossings.
This creates overpasses without ramps or junction references. The mountain,
coastlines, other highway trajectories and underground route are unchanged.

The updated map has 187 segments: the original 147 highway segments plus 9 western,
19 central and 12 eastern street segments. Parse, projection, full-width geometry,
terrain-clearance and design-rule checks pass. Eight independent chains have
sixteen intentional terminal ends and matching seam heights.

`comparison/terrain-major-streets.png` is the upright network view. Pink is the
western street, white the central street, and blue the eastern parallel street.
`comparison/major-streets-reference.png` overlays those roads on the three supplied
references without stretching their images.

## Mountain relocation

The summit reference is latitude 45.5085881, longitude -73.5897111 from
[OSM node 2517010971](https://www.openstreetmap.org/node/2517010971).
Fitting that location into the supplied screenshot places it near reference
pixel (962, 590), corresponding to native elevation pixel (736, 509).
This remains approximate because the reference-to-painted-shore fit is manual.

The original painted peak was at (744, 585). Its existing numeric relief was
translated 8 pixels west and 76 pixels north, restoring the flat-land base under
the old footprint. The full relief was retained, with no clipped samples, and
the peak grayscale remains 61283. Every water sample and every land/water
classification is unchanged. The PNG remains 1024-square, 16-bit grayscale,
without alpha. A pre-edit image is backed up in the task comparison folder.

Road XY coordinates are unchanged. Grades were recomputed against the moved
terrain, and the full geometry, design-rule and clearance checks pass again.
The original southern highway's maximum deck height is 0.4 map units; it no longer
climbs the misplaced summit. The added urban streets follow the hillside relief.
Desktop/XCF working originals have not been overwritten.
