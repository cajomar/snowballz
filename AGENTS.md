# Working on Snowballz

Instructions for coding agents (Claude Code, opencode, ...). Read this
first, then `docs/PLAN.md` for what to work on and `docs/DESIGN.md` for
what the finished game should be.

## The project

A real-time strategy snowball fight between penguin tribes, in Rust with no
game engine. It is being grown, milestone by milestone, from a small
remake of a 2007 game into the full game described in `docs/DESIGN.md`.

```
crates/sim      rules, AI, pathfinding, map format, network protocol (no deps)
  src/game/mod.rs      Game, penguins, igloos, orders, combat, movement
  src/game/units.rs    unit kinds and their stats table
  src/game/build.rs    buildings
  src/game/morale.rs   courage, fleeing, prisoners
  src/game/terrain.rs  snow depth, trampling, snowfall, chopping trees
  src/game/season.rs   seasons and blizzards
  src/ai.rs            computer players (they only send Commands)
  src/proto.rs         wire format, snapshots, client-side WorldView
crates/server   lobby and authoritative game server
crates/client   window, 3D renderer, UI, sound
  src/game.rs     in-game input and HUD
  src/view.rs     turns snapshots into a Scene, spawns effects and sounds
  src/render3d.rs the world renderer (WGSL shaders, sun shadows, models)
  src/post.rs     post-processing: AO, bloom, tone mapping, colour grade
  src/gpu.rs      wgpu device, window surface, textures, frames
  src/platform.rs winit window, keys and clock
  src/meshgen.rs  procedural models (buildings, props, trees...)
  src/selftest.rs input-driven UI test
  src/web.rs      browser platform (wasm only); web/ has the page and JS glue
assets/         maps, models, font (generated, committed)
docs/           DESIGN.md (the goal), PLAN.md (the work)
```

## Commands

```console
cargo build --release
cargo test --release                       # all tests; must pass
cargo test -p sim --release --test headless -- --nocapture   # AI-vs-AI games, prints stats
tools/headless.sh --selftest               # UI self-test, 3D
tools/headless.sh --selftest --quickstart "pinewood hollow"   # map with trees
tools/web.sh                               # browser build into dist/web (must keep compiling)
SNOWBALLZ_AUTOPILOT=1 tools/headless.sh --quickstart "pinewood hollow" \
    --screenshot /tmp/shot.png --shot-after 300               # look at a real game
SNOWBALLZ_WEATHER=night ...                                   # start in a season/blizzard
```

## Rules

- **Never open game windows or play sound on the user's desktop.** Run the
  client only through `tools/headless.sh` (invisible gamescope display).
- **Keep `sim` dependency-free** and deterministic: use `self.rng`, never
  time or threads. No new crates anywhere without asking.
- **The renderer must draw every new thing.** New models go in
  `meshgen.rs` and are registered in `render3d.rs` with `Model::baked_or`,
  so an exported model can replace them (see `docs/ART.md`).
- **Every rule change gets a test** in `crates/sim/tests/`. Look at
  `buildings.rs` and `morale.rs` for the style: build a `Game`, set up the
  situation directly, tick, assert. Also keep the client in sync: snapshot
  through `SnapshotEncoder` into a `WorldView` and compare.
- **Changing the wire format:** bump `PROTOCOL_VERSION` in `proto.rs`.
- **The AI plays by the same rules:** it may only push `Command`s.
- Match the surrounding code: short doc comments saying why, plain names,
  no clutter. Player-facing text is plain English.
- **Always commit** each finished, tested step or milestone, with a clear
  message. Don't leave work uncommitted. Work happens on branch `master`.
- When a milestone is done, update the status in `docs/PLAN.md`,
  `docs/DESIGN.md` ("Prototype status") and the README's "Playing" section.
