# Chalet ski set

Authored source:
`../model-harness/models/a-set-of-skis-and-sticks-a-ski-boots-low-poly-4/assembly.glb`.
Re-import from the repository root with `python3 scripts/import-ski-set.py [source.glb]`.
The importer converts millimetres and +Z-forward geometry into metres and +X forward,
updates accessor bounds and preserves the original 164 named parts and materials.
The set contains two 1.7 m skis, two boots and two 1.2 m poles (27,424 triangles total).

`ski_set(x, y, heading_degrees)` places one set on supported ground in a map. It appears
only in winter, outside the chalet entrance in the default map. On foot within 2 m,
F (or the interaction prompt) puts the skis and boots on the player's feet and the poles
in their hands. F removes the set at the player's current feet; it can be picked up again.
The worn pieces follow the player's foot and hand joints. Pole lean keeps tips at snow
height. The standing pickup disappears while worn. Summer and respawn reset it at the
original location; vehicle entry puts it down. Maps without a ski_set marker have no set.
