# Signal Run

Repair two signals at OpenCity’s default four-way junction. Lin drives the line
cart to the east relay. Sol takes the survey rover to the north beacon.
Use the arrow keys to drive, E to service a tower, P to pause and Esc to exit.

Run `make run` for the macOS executable or `make serve` for the browser.
Use `make native-engine` or `make web-engine` after changing the Rust engine.
C++ or game content edits only require rebuilding the client or pack.

To author another game, copy this directory, change `GAME` in the Makefile,
and edit `game.cpp` and `game.json`. List your map and elevation files in
`MAP_FILES`. Map paths are relative to the manifest. Each prefab is a list of
colored boxes in metres; named placements belong to a map. The engine reads
the resulting pack on both platforms. The game owns the missions and controls.
