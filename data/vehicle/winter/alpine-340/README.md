# Alpine 340 WT

The larger yellow, enclosed snowmobile from the existing Alpine configurator.
`source.html` preserves that editable source. The game export uses the default skis,
glass doors and standard twin tracks, with no optional accessories or texture dependency.
The original small Motoneige remains a separate vehicle. Both park at the ski chalet,
5 metres apart, facing the same direction.

Re-export from the repository root with Node 18+ and local `three` and `playwright`:

```
THREE_ROOT=/path/to/node_modules/three \
PLAYWRIGHT_MODULE=/path/to/node_modules/playwright/index.mjs \
node scripts/export-alpine-340.mjs
```

The exporter uses local Three modules, removes invisible hit targets and decal textures,
names both door assemblies, and replaces the static belts with 56 evenly pitched shoes
per side. Upper door glazing follows the lower leaf to provide a full-height opening.
The game export scales the original configurator by 1.35 for adult driver headroom,
with matching door/seat/ski and belt-path dimensions. The GLB is Y-up, +X forward,
in metres. Bounds are approximately X −3.084..3.097, Y −0.0169..2.3558, Z ±1.0159.

In-game door controls operate the left/right front-hinged assemblies, including glass
and handles. There are no rear doors. The original open-seat Motoneige has no doors.
Belts on both vehicles follow signed wheel travel around their capsule paths: forward,
reverse, and stopped. The Alpine bogie wheels spin and front skis steer separately.
The existing seasonal traction profile provides snow travel and slow summer-grass travel.
