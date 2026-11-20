# Breakwater

Run `make run` here on macOS. The C++20 client links against the shared OWE engine. Editing game.cpp
rebuilds the client independently.

WASD moves, the mouse looks, left click fires and holding right click raises the sights. The arrow
keys also look around. Use 1 or 2 to equip the rifle or handgun, R to reload, E to reset the range
and Escape to quit.

Run `make app` for a self-contained macOS app bundle. To capture the first rendered frame, set
`OWE_CAPTURE_PNG` to an output PNG path before launching.
