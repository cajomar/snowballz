# Making models for Snowballz

Everything except the penguins is built in code (`crates/client/src/meshgen.rs`).
Any of those models can be replaced, one at a time, by a model made in
Blender or taken from an asset pack. No code changes are needed.

```console
blender -b --python tools/export_models.py -- prop art/sauna.blend sauna
blender -b --python tools/export_models.py -- prop kit/watchtower.glb tower --size 3 --turn 90
```

This writes `assets/models/<name>.sbm`. When the game finds that file it uses
it in place of the built-in model, in the browser too. Delete the file to go
back to the built-in one. `cargo test -p client --release exported` checks
every exported file.

## What the game expects

- **Style:** the renderer has no textures. Each face corner gets a single
  colour, so flat-coloured low-poly models suit it best. The colour comes
  from the first of these the model has:
  - a texture: sampled at each corner's UV, which suits palette-textured
    packs such as Kenney, KayKit and Synty;
  - vertex colours;
  - the material's base colour.
- **Material names set special looks:** `Team...` is painted in the owner's
  colour, `Glow...` glows at night (fires, windows), and `Gloss...` is shiny
  (ice). A word anywhere in the name says what the surface is made of, and
  the shader paints it to match: `snow`, `wood`/`plank`/`log`,
  `stone`/`rock`, `cloth`/`fabric` (and `team`), `metal`/`iron`/`steel`,
  `ice`. Built-in models get theirs guessed from their colours.
- **Orientation:** z is up and the front faces -y, which is Blender's
  "Front" view looking at it. `--turn DEGREES` fixes models that face
  elsewhere.
- **Size:** one Blender unit is one map tile and a penguin is about 1 tall.
  An igloo is about 4 across and a building 2 to 4. `--size TILES` scales
  the model so its widest side is that many tiles. The exporter centres the
  model and puts its lowest point on the ground.
- **Weight:** at most 6,000 triangles, and far fewer for things there are
  many of: trees, stumps, walls should stay under about 300.
  `--decimate 0.3` keeps 30%.
- **Sources:** `.blend`, `.glb`/`.gltf`, `.fbx` or `.obj`. Every visible mesh
  in the file is used.

## File names

| Name | What |
|---|---|
| `igloo-0`, `igloo-1`, `igloo-2` | igloo at each tier |
| `ruin` | collapsed igloo |
| `tree-0` … `tree-2`, `stump-0` … `stump-2`, `log` | forest |
| `crystal-0` … `crystal-3`, `pinnacle-0` … `pinnacle-2`, `ice` | ice scenery |
| `hole-open`, `hole-frozen` | fishing |
| `snowball`, `seal`, `power-defense`, `power-double` | small things |
| `campfire`, `sauna`, `tower`, `store`, `spa`, `workshop`, `track` | buildings |
| `bunkhouse`, `sling-range`, `packer-yard`, `ski-lodge` | camps |
| `palisade-0` … `palisade-2` | walls |
| `prop-slinger`, `prop-packer`, `prop-sled-lancer`, … | gear worn by each unit, drawn on the penguin (penguin origin, about 1 tall) |

The penguins (`worker.sbm`, `warrior.sbm`) are animated. They are baked from
the Snowballz 2 rig by `export_models.py penguin`. A new penguin needs the
same animation names; `crates/client/src/model.rs` lists them in its test.

## The buildings

Buildings replacing the built-in ones are modelled from code in Blender
by `tools/buildings.py` (the igloos, tower, palisades, sauna, campfire, store, the Bunkhouse, the camps (workshop, sling range, ski lodge, packer yard, spa, track) and the scenery: trees, stumps, crystals, ice pinnacles, fishing holes, and the ruined igloo):

```console
blender -b --python tools/buildings.py                       # all of them
blender -b --python tools/buildings.py -- tower --preview    # one, plus /tmp/snowballz-tower.png
```

It writes `assets/models/<name>.sbm` and an editable `art/buildings/<name>.blend`.
Palisade pieces are exactly one tile long (x from -0.5 to 0.5) so
neighbours join up.

Each also gets construction stages, extra animations named `stage-1` to
`stage-3`: the parts that start low enough on the building (it goes up
course by course), in scaffolding with snow blocks waiting. Parts marked
`finished_only` (pennants, banners) and objects named `core...` or
`inside...` only appear when it is done. A model with no stages rises out
of the snow as it is built instead. `SNOWBALLZ_SHOWCASE=0.5` shows every
building half built.

## The penguins

Every kind is the Snowballz 2 penguin (`art/source/penguin.blend`:
its body, rig and animations), slimmed down and dressed in its own outfit
and gear by `tools/penguins.py`:

```console
blender -b --python tools/penguins.py               # every kind
blender -b --python tools/penguins.py -- skier      # just one
```

This writes `assets/models/unit-<kind>.sbm`, which the game draws, and an
editable copy with the outfit on, `art/units/<kind>.blend`. The source file
is never changed.

- **Gear** is made in the script with simple shapes, written in game units
  (the penguin stands about 1.05 tall, faces -Y) as it stands in the Stand
  pose, and pinned to a bone (`spine.006` is the head, `hand.L/R` the
  flipper tips, `foot.L/R`, `spine.00x` the body) so it follows the rig.
- **Clips** are the source's own animations (Stand, Walk, Throw, Bend...)
  with poses laid over them in code: knocked flat, the Packer's bazooka shot, the
  Warmer's blow, the Skier's and Sled's lean. Props shown only in some
  clips or frames (the Packer's shell and blast, warm puffs, the sling's ball) are listed
  per clip.
- **Slimming:** body, feet and outfits are decimated before the rig bends
  them; each kind stays under 2,500 triangles (the old warrior was 2,658).
- Material names starting `Team` take the player's colour, `Glow` stays
  lit, `Gloss` shines.

Clips the game plays: every fighter has `stand walk throw scoop cheer
flinch knocked`; workers add `carry fish unload hammer chop sit`; the
Warmer has `warm` and no `throw`; the Sled has neither `throw` nor `scoop`.
A kind without a `unit-` model falls back to the old warrior.

## Generated models

Some models are made by a picture-to-3D model running locally: a concept
picture (FLUX.1-schnell) is cut out of its background and turned into a
textured mesh by TRELLIS.2, both in ComfyUI on the graphics card. The
mesh is kept in `art/generated/NAME.glb` (texture shrunk to 1024), and
`tools/generated.py` bakes it into the game's format:

```console
blender -b --python tools/generated.py -- crystal
```

It sorts the faces into the game's materials by the colour of their
texture (crystal glows and shines, white is snow, the rest stone), darkens
the texture's midtones (generated textures are lit softly and read washed
out under the game's sun), stands the model on z = 0 at its footprint's
size, keeps it under its triangle budget, and makes the versions the game
needs (`crystal-0` to `crystal-3`, worn down as it is mined). Adding a
model means a function in `tools/generated.py` and an entry in `MODELS`.
