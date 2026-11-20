# Switchback Circuit

Race three rivals for three laps through the mill district. The home straight
leads into a right-side hairpin, an S-bend and a sweeping west turn. Hold Up to accelerate,
Down to brake and Left or Right to turn the nose into a bend.
The orange car travels in the direction it faces. Scraping the verge slows you down.
Change lanes to pass. P pauses, Esc quits, and E restarts after the finish.

Run `make run` on macOS or `make serve` for the browser. Open the URL printed by Make.
The start signal appears over the grid. Racing begins on GO! Your lap and position then appear in
the corner.

Edit `circuit.road` for the course and buildings. The client samples its ordered raceway
curves from the loaded map at runtime so every racer follows the rendered course. Edit `game.cpp`
for
race rules and `content.hpp` for the cars and gantry. Both compile into the client independently of
the engine.
