# Build plan

The road from the current game to the full game in `DESIGN.md`. Work top
to bottom; each milestone must leave the game playable with all tests
passing. Tick boxes as things land. Read `AGENTS.md` first.

## Done

- [x] **M1 Terrain that changes** — snow depth per tile, scooping and
      trampling wear it to ice, snowfall restores it; workers chop trees
      into stumps for wood; wood warms igloos. (`game/terrain.rs`)
- [x] **M2 Buildings** — hut, wall, pile, lookout, store; snow quarried
      around sites; cover; repair/demolish; AI builds. (`game/build.rs`)
      The Snow Wall and the Lookout are gone since (removed 2026-11-21): the
      Palisade is the only wall and the Watchtower sees far.
- [x] **M3 Courage and prisoners** — courage, fleeing, huddles; carrying
      frozen enemies home to convert them; release; rescue. (`game/morale.rs`)
- [x] **M4 Units** — worker, snowballer, slinger, packer, sled lancer,
      shieldbearer with props; the AI wishes for a mixed army.
      (`game/units.rs`) The Elder is cut (removed 2026-11-21).
- [x] **M5 Seasons and weather** — four 4-minute seasons (deep winter,
      thaw, midsummer, polar night) change snowfall, trampling, snow
      buildings, fish regrowth and throw range; random blizzards shorten
      throws and chill penguins in the open (the chill wears off after).
      HUD season clock; night tint, storm haze and snowfall in both
      renderers. (`game/season.rs`)
      Left for later: frozen sea routes (maps need open-water tiles first),
      the AI timing attacks to the seasons.

- [x] **M6 Eggs and wild colonies** — removed 2026-11-21 (the code is
      gone: no eggs, nests, `Command::Steal`, wild colonies or
      `Command::Recruit`). Newcomers swim in from the sea now (C8).

- [x] **Web build** — the client compiles to wasm (`tools/web.sh`); the
      server speaks WebSocket and serves the page on its game port
      (`--web DIR`), so friends play in a browser with no install.
      Join/lobby/play only: no hosting from a tab. (`server/src/ws.rs`,
      `client/src/web.rs`, `web/`)
      Left for later: touch controls, a Rust-side `wss` story without a
      proxy.

- [x] **Look** — a wgpu renderer with sun shadows (4096 map), AO, bloom,
      tone mapping and tilt-shift; firelight (up to 24 point lights from
      igloo doors, fires, saunas and lanterns), a deep-blue polar night with
      aurora, each season's own sun colour, cloud shadows, drifting cloud
      over unexplored land, spindrift, swaying trees, fishing holes with
      thin-ice rims and floes, a glittering sea, embers, snowball trails and
      breath in the cold. (`render3d.rs`, `post.rs`, `effects.rs`)
      Soft 16-tap shadows, meltwater in the thaw. Models generated locally
      (FLUX concept, TRELLIS.2 mesh; `tools/generated.py`, docs/ART.md):
      crystals, the leopard seal, both power-ups, stumps. Generated firs
      were too slim and were dropped.
      Next: the penguins (they are the Snowballz 2 design: ask before
      replacing them), igloo and buildings stay code-made.

## Next: the new core (see DESIGN.md section 0)

The prototype's core loop is the 2007 one. These milestones rebuild it
into the agreed design, one playable step at a time. Earlier M7+ items
below wait until this is done.

### C1 Fixed population and frozen penguins
All behind `CoreRules` switches (`game/core.rs`); they are on by default
(since 2026-11-20 there is no classic ruleset: the switches are gone, and `CoreRules` only keeps `tech`, `cold` and `small_start`). `tests/core.rs`.
- [x] No eggs or hatching; the population is fixed.
- [x] Freezing ends ownership: a frozen penguin is nobody's and blocks
      its tile for walking and snowballs. (`game/frozen.rs`)
- [x] Natural thaw (120 s; faster in warm seasons, never in polar night)
      wakes it wild; standing with a wild penguin for 2 s recruits it.
- [x] Warmers warm friends and thaw the frozen into their tribe; the AI
      makes, uses and recruits with them. (`game/warmer.rs`)
- [x] Enemy igloos can't be walked into and taken.
- [x] Turn the switches on by default (T62) once camps (C3) exist.
- [x] Core games use no eggs or prisoners: igloos retrain workers, and
      frozen enemies become wild.

### C2 A real base
- [x] Great Igloo: starting one per player; workers found expansion bases
      at map igloo sites for warmth, storage and training. (40 fish, 20 wood)
- [x] Buildings have strength and can be destroyed by snowballs; a tribe
      with no buildings is out (`siege` switch: igloos fall into ruins).
- [x] Snow palisades: taller durable walls (the built-in gate was removed)
      block walking and snowballs while closed.
- [x] Roosts (formerly Fish Racks) set how many penguins a tribe can house;
      fish pays upkeep. (Removed in C8; the code is gone, 2026-11-21.)

### C3 Training camps
- [x] The worker is the only basic penguin; camps convert workers into
      Snowballer (Bunkhouse), Slinger, Packer, Shieldbearer, Sledder, Skier
      and Warmer, with rally points.
      Done (`game/camp.rs`): Ice Workshop (Shieldbearer), Sled Track (Sled
      Lancer), Warming Spa (Warmer), Ski Lodge (Skier; fast on snow, slow on
      ice), Sling Range (Slinger), Packer Yard (Packer). The Throwing Range,
      Lookout School (Scout) and Elder Lodge (Elder) were built and removed
      (2026-11-21), and so was the Kite Loft (Kite Flier). Select a camp and
      right-click the ground to set its rally point (right-click the camp to
      clear it). Under armies (C6/C7) each camp slot calls in its own worker,
      who mines a crystal and trains, and camps are built up to more slots.
- [x] Every kind has its own job, done on its own (2026-11-03,
      `game/roles.rs`, `tests/roles.rs`): Shieldbearers step in front and
      block for those behind, Slingers can't hit close and back off,
      Skiers hunt soft targets and knock throwers flat (not shields),
      Packers aim at buildings first, Sleds haul the frozen home to join
      the tribe. Elders, Scouts and Kite Fliers are cut (and their code removed
      2026-11-21); the Chieftain steadies the tribe. Each kind has its own animated penguin, made in
      Blender by `tools/penguins.py` (sources in `art/units/`, see
      docs/ART.md).
- [x] Snowball catapult (2026-11-17, `game/catapult.rs`,
      `tests/catapult.rs`): a Watchtower built up, crewed by three workers,
      lobs at enemy buildings and igloos 14 tiles off (nearest, or aimed);
      uncrewed ones are taken over by enemy workers. No Siege Yard.

### C4 Fog of war and controls
- [x] Fog of war (explored / visible): enemies, wild penguins and seals
      hide outside your team's sight; ground and minimap darken.
      (`sim/src/fog.rs`, `client/src/view.rs`) Still to do: scouting,
      line of sight. (Watchtowers see far: 2026-11-11.)
- [x] Idle-worker button (I).
- [x] Shift-queued moves (shift+right click).
- [x] Formations for mixed selections (done for armies in C6). No stances:
      soldiers fight smart on their own instead (2026-11-10,
      `game/tactics.rs`, `tests/tactics.rs`): they finish off the nearly
      frozen and don't pile onto someone already as good as frozen or throw
      at someone behind a slab; they sometimes sidestep a snowball thrown
      at them; a soldier in the ranks steps up to three paces out to reach
      an enemy just out of range; a thrower under two thirds warm swaps to
      the back beside the Warmers. In AI games a throw freezes someone half
      as often again as before.

### C5 Scale and the AI
- [x] A few hundred penguins at a steady frame rate; snapshot budget.
  Measured 2026-11-09: four computers from the small start reach 440
  penguins in 20 minutes on The Long White at 1 ms a tick (worst 16 ms of
  the 50 ms budget) and about 1 KB a snapshot (19 KB/s) per fogged
  player; 2040 snowballers fighting at once take 2.6 ms a tick and 213
  KB/s unfogged; the client draws 2000 penguins, 250 on screen, far above
  60 fps. (`tests/bench.rs` bench_long_game, `tests/scale.rs`;
  SNOWBALLZ_CROWD=N and SNOWBALLZ_NOVSYNC=1 for the client.)
- [x] Rebuild the AI around the new loop: land grab, camps, timing, thaw
      races after battles. Done through C6 and C7: computers build igloos
      and Sushi Houses outward, fill and build up camps, wait for an army
      before attacking, and race for the frozen (Warmers thaw them near
      their army and igloos, Sleds drag them home, and a penguin walks out
      to persuade each wild one near home). Measured 2026-11-09, three
      20-minute four-computer games: of 788 frozen, 298 were won back
      while frozen and 282 of the 307 that thawed wild were persuaded,
      about evenly by their old tribe and by others.

### C6 Armies, elites and outposts (DESIGN.md 0.1)
- [x] One Chieftain per igloo; armies follow their Chieftain and form
      spread-out ranks facing the enemy; two sentries post themselves on
      each watchtower; Sledders rejoin; start with Chieftain + 2.
      (`game/armies.rs`, `tests/armies.rs`)
- [x] Formations: shield line or ring, spread-out throwers, Slingers in
      cover, Skiers on the flanks breaking shield walls; a Chieftain
      answering an alarm stops short of it.
- [x] Drafting at the igloo, Bunkhouses, camps as elite housing with
      upgradable slots that refill from snowballers; the AI drafts, builds
      the camps it wants and Bunkhouses, and upgrades full camps.
      (`game/camp.rs`, `tests/elites.rs`)
- [x] Store crews (fishing outposts): a store by the water calls four
      fishers who fish beside it and unload there; computers build them on
      shores a walk from home. (`game/outpost.rs`, `tests/outposts.rs`)
- [x] Igloo guard slots: 3 per igloo, upgradable to 5, filled from its
      army and holding the door; pips over every building with slots; the
      help lists every unit. (Dropped in C8: igloos guard nothing.)

### C7 Work, crystal and a small start (DESIGN.md 0.2; agreed and built)
Stages, each tested and committed on its own:
- [x] 1. **Sushi Houses and slots, no percentage bar.** The Store becomes
      the Sushi House with fishing slots 2/4/8 (pips, card, upgrades); an
      igloo is a sushi house too; fishing only there; the jobs bar goes.
- [x] 2. **No automatic chopping or stored wood.** Builders fetch wood per
      site; no wood in igloos; saunas and campfires get an assigned stoker;
      trees regrow.
- [x] 3. **Crystal gates elites.** Workers mine a crystal, then train fast;
      upgrades cost snow and wood and need crystal after; every map has
      crystal by each igloo and scattered; snowballers cheaper and faster.
- [x] 4. **A small start.** One Chieftain and two workers; frozen
      penguins revert to plain workers.
- [x] 5. **Build igloos anywhere** (about 30 tiles from any other, never on
      an enemy's doorstep) with a Chieftain, guard slots and a sushi house.
- [x] 6. **Maps rebuilt** to start as big forests, with crystal and outpost
      room for every start; computers taught all of it.
- [x] 7. **No upgrade is bought from a stockpile** (2026-11-09): an igloo's
      extra guard and fishing slots are built on by builders with snow and
      logs, like camps and Sushi Houses. (Igloo build-ups since dropped: C8.)

### C8 Slots you set, and the sea (DESIGN.md 0.3; agreed and built 2026-11-10)
- [x] A cold penguin with no fire in reach tells its owner (`Event::NoFire`).
- [x] Igloos guard nothing and are never built up; they keep two fishing
      slots. Towers post as many sentries as wanted.
- [x] Every slotted building fills only as many slots as wanted (mouse
      wheel or click on its card); building up adds empty room.
- [x] Bunkhouses draft snowballers by themselves; the igloo card has no
      Recruit, Draft or upgrade buttons.
- [x] No Landing and no wild colonies: newcomers swim in from the sea for
      4 fish to fill a slot nobody idle can (`game/sea.rs`, `tests/slots.rs`).
- [x] No Roosts and no fish upkeep. (The Landing, colonies, eggs, Roosts,
      the Supply and Recruit orders and the worker-split foreman: all the
      code is gone, 2026-11-21.)
- [x] The Lookout is folded into the Watchtower: a tower sees 16 tiles and
      costs 6 snow, 4 logs; no Lookout to build (2026-11-11).
- [x] One wall (the Palisade) and the Sauna is a campfire built up
      (`Game::sauna_site`, 2026-11-12).

## Later (from the prototype plan)

### M7 Territory and tech tiers  (parked: off unless `CoreRules::tech`)
- [x] Igloo tiers: Igloo → Snow Hall → Ice Palace (upgrade with fish,
      wood, crystal); higher tiers unlock units and buildings.
      (`game/tiers.rs`) A tier holds more penguins, warms them faster,
      is harder to take over. The Ice Palace needs
      crystal, which every map has in contested ground; upgrades draw wood
      and crystal from the whole tribe. The crystal race victory is gone.
- [x] Research at igloos: a few upgrades per branch (Snowcraft, Icecraft,
      Fishery) as simple stat multipliers in a table. (`game/research.rs`)
      Three branches of three levels; what is learnt belongs to the whole
      tribe, but the work is done at an igloo, which researches one thing at
      a time and loses it if the igloo is taken. Fish is paid from that
      igloo, wood and crystal from the whole tribe. Level two needs a Snow
      Hall, level three an Ice Palace. The AI learns once its economy is
      steady.

### M8 Second tribe (Adélies) and tribe choice in the lobby
- [ ] `Tribe` per player; unit table per tribe; lobby picker.
- [ ] Adélie twists: pebbles resource, stealing, spies, bribes.

### M9 Heroes and wildlife
- [x] Heroes: a **Chieftain** per igloo, raised by the igloo itself. Its
      courage aura steadies the tribe nearby, its War Cry makes friends run
      and throw faster and shake off their fear, and the tribe loses heart
      when it freezes. A lost Chieftain is replaced by its igloo a minute
      later. The AI shouts with its army. (`game/heroes.rs`,
      `game/armies.rs`)
- [x] Leopard seals: a few per map lurk at fishing banks and open ice away
      from igloos; lone penguins (under 3 friends nearby) draw them up,
      they snatch after a warning, snowball hits drive them off and the
      AI keeps away from recently struck holes. (`game/wildlife.rs`,
      `tests/wildlife.rs`)

### M10 Diplomacy and the local language model (host-only, optional)
- [x] Computers truce with each other and gang up on a runaway leader
      (`ai/diplomacy.rs`, `tests/ai_diplomacy.rs`); a treacherous one
      breaks its truces. People truce with computers by chat.
### M11 Campaign, co-op "Long Night", map editor
### M12 Ranked play, replays, balance

## How to add things

**A unit kind:** add a variant to `Kind` and a row to `STATS` in
`game/units.rs` (keep `Kind::ALL` in order); add a prop in
`meshgen::prop`; add a camp that trains it (`BuildKind::trains` in
`game/build.rs`) and its key in `BUILD_KEYS` (`client/src/game.rs`);
teach `ai.rs::wish_soldiers` when to want it.

**A building:** add a `BuildKind` variant and fill in every `match` in
`game/build.rs`; put its effect in `update_buildings` or where it applies;
add a mesh in `meshgen.rs`, register it in `render3d.rs` (`building_model`
and the model list); add a key in
`BUILD_KEYS`; teach `ai.rs::construct`.

**A command:** add to `Command` (`game/mod.rs`), handle it in
`Game::command`, encode/decode it in `proto.rs` (new tag number), bump
`PROTOCOL_VERSION`, send it from `client/src/game.rs`.

**An event:** add to `Event`, encode/decode in `proto.rs`, react in
`client/src/view.rs` (effects, sounds) or `game.rs` (notices).

## Known issues

- Maps were rebuilt for the new rules (2026-11-07): balanced, not mirrored
  (the user doesn't like exact copies): 45+ trees and two fishing-outpost
  sites per start, ice evened out and capped at 30% (`mapgen::balance_ice`),
  and a pond added behind igloos that lack outpost room. `examples/seat_fairness`
  plays every seat with computers: the spread between seats is 1.2-1.9x on
  14 seeds (the rest is the computers' random temperaments).
- The shipped maps are generated from `assets/mapspecs/*.json`
  (`cargo run -p sim --release --example make_maps`; a test fails if a
  map and its spec drift apart). The rule tests play on
  `assets/maps/fixtures/` (a hand-built flat stage, `trio`, and three
  generated ones), built by the same `make_maps` example.
- `tools/headless.sh --selftest` is now steady (3D seeds 1-7 pass; the 2D renderer was later removed; `SNOWBALLZ_SEED=n` replays a game). It played
  against a Hard AI that rushed the undefended base, froze the workers and
  knocked down the range; it now plays Easy. The tree step skips when no
  tree is within 14 tiles of your igloo, which happens on some seats.
- AI games used to stalemate until the Long Night settled them (43 min on
  average, glacier-crossroads, 4 Hard computers). The computers were
  starving their own armies: the whole catch went to a far-off
  delivery point while every penguin ate a fish a minute, chilled fighters never warmed up,
  raids went out 4 strong, and a tribe that hadn't found anyone waited on
  one scout. Fixed 2026-11-03: those games now end by fighting, 37 min on
  average (96 games, `snowballz-eval --minutes 50 --tempers`).
- All numbers (costs, courage loss, conversion time, unit stats, season
  length) are first guesses; nobody has play-tested a match by hand yet.
- Dev switches for screenshots: `SNOWBALLZ_AUTOPILOT=1` (a computer plays
  your side), `SNOWBALLZ_WEATHER=thaw|summer|night|blizzard`,
  `SNOWBALLZ_NOFOG=1` (no fog of war),
  `SNOWBALLZ_ENDSCREEN=1` (show the end-of-game screen with made-up stats and reveals),
  `SNOWBALLZ_SHOW=help|menu` (open the help or the game menu),
  `SNOWBALLZ_MAKE_MAP="..."` (a hosting client asks for that map at once;
  `SNOWBALLZ_LLM_URL` picks the model server; `SNOWBALLZ_LLM_MODEL` /
  `snowballz-server --llm NAME` also lets it advise the computer players,
  running inside the server (`--features llm`, `server/src/engine.rs`) or
  through Ollama: see `server/src/strategist.rs`),
  `SNOWBALLZ_SEED=n` (play the same game again).
- `SB_SEED=n cargo test -p sim --release --test headless -- --nocapture`
  replays the AI games with another seed; check a few seeds before
  deciding a balance change helped or hurt.
