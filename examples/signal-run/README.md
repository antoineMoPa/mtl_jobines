# Signal Run

Repair two signals in a maintenance yard built around OpenCity’s default four-way junction.
Lin drives the line cart to the east relay. Sol takes the survey rover to the north beacon.
Use the arrow keys to drive, E to service a tower, P to pause, and Esc to exit.

Run `make run` for macOS or `make serve` for the browser. Use `make native-engine` or
`make web-engine` after changing the Rust engine. Game edits rebuild only the client or pack.

To author another game, copy this directory and set `GAME` and `OWE_ROOT` in its Makefile.
Edit `game.cpp` for rules and `game.json` for content. Set `MAP_FILES` to your map, optional
overlay, and elevation paths so Make can track edits. The packer resolves paths relative to
the manifest. Each prefab is a list of colored boxes in metres. Each map supplies its named
placements. The same pack runs on both platforms.
