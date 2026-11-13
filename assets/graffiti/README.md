# Graffiti paint masks

Two original procedural artworks exported from the task's sprite explorer: an angular glyph
and a flower. Source recipe: seed 4729, mixed styles, chalk palette, weathering 0.08, drips 0.25,
overspray 0.25; sheet indices 5 and 1, respectively. No real names or logos are used.

Both PNGs are 512 by 256 RGBA. R stores grayscale paint shading, G stores the bright highlight
mask, B is unused, and A stores straight paint coverage. These are data textures, uploaded as
RGBA8 unorm with alpha-weighted mipmaps; they are not color textures.

The game supplies linear base colors and independently seeded opacity/wash samples. A compact
uniform can override the color and change opacity/wash ranges without replacing the artwork.
Erosion is fixed in world space. Paint is alpha blended, does not write depth, cast shadows,
or affect collision. Receivers are indexed once, then nearby 30 metre cells generate sparse paint
on demand. At most 30 marks are active for a view, with no citywide allowance. An evicting tile
cache keeps revisited walls deterministic. Opposite wall faces are independent; tall retaining
walls use larger artwork than small concrete barriers.
