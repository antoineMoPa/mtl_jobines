<!-- please don't touch this file unless asked -->

# mtl_jobines

![MTL Jobines splash screen showing a snowy Montréal street](web/boot.png)

MTL_Jobine is a Montreal-inspired open world game where the player can work different small jobs
around town.

It features:
* working metro (subway) system
* winter / summer / night / day modes
* npc traffic (bike, pedestrian, vehicles)
* a complex grid network defined in .road language (from my opencity repo)

<p align="center">
  <img src="docs/gameplay-highlights.gif" alt="Gameplay highlights">
</p>

# playing

Play here: [mtl-jobines.pages.dev/](https://mtl-jobines.pages.dev/)

# building from source

```
git clone git@github.com:antoineMopa/mtl_jobines.git
git submodule  --update -recursive
cargo xtask assets
cargo run --locked
```
