# Snowballz: Tribes of the Ice — design

A full-scale RTS in the Snowballz setting: penguin tribes, snowball fights,
igloos, fish and ice. It keeps the rule that makes Snowballz itself:
**nobody dies**. Penguins freeze, and frozen penguins can be captured.

The current game is the testbed. Features are proven there first (see
[Prototype status](#prototype-status)) and carried into the new game.

## 0. The core (agreed 2026-10-21; overrides anything below)

The prototype still plays like the 2007 game: fixed igloos are both base
and factory, and you win by walking into them. The real game is a base-
building RTS with its own twist: **a fixed population that changes hands.**

**Penguins are never made, only won.** The population is fixed: a tribe
starts with a few penguins, newcomers swim in from the sea for fish (0.3),
and every other penguin changes hands by freezing and thawing, so each one
you win is one an enemy doesn't have. There are no eggs and no hatching.

**A penguin is yours until it freezes.** A frozen penguin
belongs to nobody. It stays where it fell as a block of ice that stops
walking *and* snowballs, so battles leave the ground cluttered with cover
and can clog a pass shut. Pick where you fight: you may not get through a
narrow entrance you froze full.

**Frozen penguins thaw.** Left alone, a frozen penguin slowly thaws by
itself and wakes up wild, for anyone to persuade to join. A Warmer thaws it much
faster, and it joins the Warmer's tribe. After a battle both sides race
their Warmers onto the field. *(Open: how long the natural thaw takes,
and whether seasons speed or stop it. Default: about two minutes, faster
in thaw and midsummer, never in polar night.)*

**Basic penguins and training camps.** Workers are the basic penguins: they
fish, chop, carry, build, and throw weakly in a pinch. Send them to a
training camp, pay fish and time, and they come out specialists. Every
specialist is a worker you no longer have. *(Open: is training one-way?
Default: yes, with a cheap "retire to worker" at a Great Igloo.)*

| Camp | Specialist | Role |
|---|---|---|
| Bunkhouse | Snowballer | The ranged line |
| Sling Range | Slinger | Throws farther, but less accurately |
| Packer Yard | Packer | Heavy throws that smash buildings |
| Ice Workshop | Shieldbearer | Ice slab that blocks snowballs; the front |
| Sled Track | Sledder | Fast belly-slide charge into throwers |
| Ski Lodge | Skier | Fast on snow, slow on ice; flanks and raids |
| Warming Spa | Warmer | The medic: warms and thaws friends (the blowdryer is just the look) |
| Watchtower, built up | Snowball catapult | A machine crewed by three workers: long range, and breaks buildings fast. Freeze its crew and anyone can crew it |

**A real base.** You start with one **Great Igloo** (drop-off, warmth,
training hub). Workers place buildings: training camps, saunas,
watchtowers and snow palisades. Expand by founding another Great
Igloo at a distant fishing ground. Buildings have strength; destroy all of
a tribe's buildings to knock it out.

**Hundreds of penguins at most.** At that scale players command armies,
not penguins. Specialists act on their own (Warmers go to the coldest
friend, Sledders charge throwers, Shieldbearers step in front), mixed
selections form up (shields front, throwers behind, Warmers at the back),
soldiers fight smart by themselves rather than by stances set by hand
(they finish off the nearly frozen, dodge, step up into reach and send
the hurt to the back), camps train in batches
to a rally point, and courage decides when a side breaks. The limit on
army size is fish, not a supply count.

**The usual RTS basics:** fog of war and scouting, rally points,
shift-queued orders, an idle-worker button, production from the selected
building, control groups.

**Kept from the prototype:** snow depth and ammo from the ground, warmth
as health and as reach, seasons, courage and routs, the Chieftain, leopard
seals, research, terrain that wears down.

**Goes away:** eggs and hatching, capture-by-entering, fixed igloos as
factories (map igloos become expansion sites), prisoners dragged home
(replaced by thawing on the spot). All of this is built.

### 0.1 Armies, elites and outposts (agreed 2026-11-05)

- **One Chieftain per Great Igloo.** You start with one igloo and its
  Chieftain; founding another igloo brings another Chieftain and so a second
  army. A frozen Chieftain comes back at his
  igloo after a while; meanwhile his army falls back and regroups there.
- **Armies follow their Chieftain.** Soldiers belong to their igloo's
  Chieftain and follow him on their own; you order the Chieftain and the army
  forms its front around him. Units ordered on their own rejoin when done.
  Sledders that bring a frozen penguin home go back to their army.
- **Sentry slots.** Towers have a few sentry slots that fill themselves
  from the army; everyone else follows the Chieftain. (Igloo guards were
  dropped in 0.3.)
- **Formations and tactics on their own.** Shields form a line between the
  Chieftain and the enemy, with gaps (a ring when surrounded); throwers
  spread out behind (splash punishes clumps); Slingers keep back and take
  cover behind trees, ice and walls; Skiers wait on the flanks and break
  shield walls (a charge knocks a Shieldbearer down for a few seconds);
  Warmers keep to the back.
- **Hordes and elites.** Worker -> snowballer is a quick, cheap draft at an
  igloo, but each snowballer needs a bunk (a cheap Bunkhouse holds ~10).
  Snowballer -> specialist happens in camps, which are elite housing: a camp
  has slots (1, upgradable to 3 then 5); an empty slot calls in a spare
  snowballer, trains it with a visible progress ring, and refills when its
  specialist freezes. Thawed enemies join as snowballers.
- **Small start.** The Chieftain and two snowballers; computers don't attack
  early.
- **Stores are fishing outposts** with a crew of fishers that fill them.

Stages: (1) Chieftain per igloo, armies follow, sentry slots, smaller
start; (2) formations and tactics; (3) drafting, bunks and camp slots;
(4) store crews, and the computers taught all of it.

### 0.2 Work, crystal and a small start (agreed 2026-11-07)

The user's calls, in their words where it matters. Built: see PLAN.md C7.

- **Fishing happens at Sushi Houses.** The Store becomes the Sushi House; a
  worker's job is *a place* (a slot in a building), not a percentage bar.
  The bar goes away. A Sushi House has fishing slots: it starts with 2,
  upgradable to 4 and then 8. Each igloo also works as a sushi house (and
  maybe a sauna) so a tribe can fish from the start. Slots fill themselves
  from idle workers, shown by pips like every other slot.
- **Nothing chops by itself, and wood is never stored up.** No Lumber Camp.
  Builders fetch wood when a site needs it, from the nearest tree, and that
  is the only chopping (plus a direct chop order). Resources are not
  stockpiled in general. The exceptions: saunas and campfires burn wood, but
  a penguin is assigned to each to stoke the fire now and then; and Sushi
  Houses store fish.
- **Trees regrow**, so a map can't run dry. Maps start as massive forest that
  building depletes.
- **Crystal gates elites.** A worker has to mine a crystal before it can
  assign itself to a camp slot and train. Upgrading a building costs more
  snow and wood; once an upgrade is finished, penguins need crystal to fill
  its new slots. Every map has crystal near every igloo and scattered
  across it, so nobody can monopolise it and stop a tribe making warriors.
- **Training is short.** At an igloo or camp, once the crystal is in hand a
  worker transforms quickly. A Snowballer is cheaper, easier and faster than
  a specialist ("they're boring").
- **Igloos can be built anywhere**: an Igloo goes on the build card (built by
  workers), a good way from any other igloo (about 30 tiles), never on an
  enemy's doorstep. It gets its own Chieftain, guard slots and sushi house.
- **A small start**: one Chieftain and two workers.
- **Frozen penguins always come back as plain workers.**

Stages are in PLAN.md C7.

### 0.3 Slots you set, and the sea (agreed 2026-11-10)

These replace parts of 0.0 to 0.2 where they disagree.

- **You choose how many slots are filled.** Building a building up adds
  room; it never fills it by itself. Set the number with the mouse wheel
  over its slots (or click a slot).
- **Igloos are not upgraded and guard nothing.** An igloo has two fishing
  slots and its Chieftain; for more fishing build Sushi Houses, and to
  guard it build a Watchtower beside it.
- **Buildings fill themselves; the igloo card has no buttons for it.**
  Sushi Houses take on workers, Bunkhouses draft snowballers, camps train
  specialists, all by themselves. No Recruit or Draft button.
- **Newcomers swim over from the Great Sea.** The Landing and the wild
  colonies are gone: a slot with nobody idle to fill it pays fish for a
  newcomer, who swims in and walks to it.
- **Housing is free.** No Roosts and no fish upkeep.
- **A cold penguin with no fire near tells you**, so you can build one.
- **The Watchtower is the Lookout too** (2026-11-11): a finished tower
  sees 16 tiles over the fog, and it costs less (6 snow, 4 logs). No
  Lookout to build, and no throwing bonus for standing beside a tower:
  only the sentries on its roof throw far.
- **One wall, and the Sauna grows from a campfire** (2026-11-12): the only
  wall is the tall Palisade (no low Snow Wall). There is no Sauna to build
  from scratch: build a campfire up and it becomes one on the spot (5 more
  snow, 3 more logs), where it has room for a 2 by 2 hut; the fire is out
  while it goes up.
- **The catapult is a Watchtower built up** (2026-11-17): no Siege Yard.
  Building a tower up (4 snow, 6 logs) puts a catapult on its roof in
  place of the sentries. Three workers crew it from the foot of the tower,
  out in the open; it lobs a big snowball (6 damage to a building, 10 to
  an igloo) every 6 seconds (slower with fewer crew) at the nearest enemy
  building or igloo within 14 tiles, or where its owner aims it. With its
  whole crew frozen, any tribe's workers can walk up and take it over.

## 1. Pillars

*Sections 1 to 11 are the long-term vision: ideas, not promises. Section 0
and the Prototype status at the end say what is built. Where they disagree
(eggs, nurseries, wild colonies to recruit, Roosts, Elders, Scouts, the
Lookout), section 0 wins and those ideas are dropped.*

1. **Freeze, don't kill; frozen penguins can be captured.** Drag a frozen
   enemy home to thaw and convert it, ransom it, or release it for goodwill.
   Losing a battle feeds the winner's army, so every fight asks: rescue the
   frozen, or retreat?
2. **The snow is the battlefield and the building material.** Every tile has
   a snow depth. Scooping and building use it up, digging exposes ice and
   storms bring it back. Armies reshape the map as they fight.
3. **Warmth is fire.** Penguins only warm by fire: a tended sauna or a
   campfire. (The slow cold in the open is parked, kept as a switch for the
   Long Night.)
4. **Seasons change the map.** Deep winter freezes the sea and opens routes;
   the thaw breaks the ice, softens snow buildings and opens the sea.
5. **Tribes are social.** Free-for-all games involve alliances, ransoms,
   bribes and betrayal, and the computer opponents take part.

## 2. Gathering changes the map

Every way of collecting a resource leaves a permanent mark. Collecting and
reshaping the battlefield are the same action.

| Gathering | Leaves behind | Effect on play |
|---|---|---|
| Chopping pines (wood) | Stumps, then open snow | Opens routes and removes cover. Forests block wind; clearing one lets drifts build behind it. |
| Quarrying snow (snow blocks) | Lower ground, then ice | A pit beside a wall is a free moat. Bare ice gives no ammo, so a cleared area starves attackers. |
| Cutting ice | Thin ice, then open water | Channels, moats, broken ice bridges. Thin ice cracks under heavy units. |
| Mining ice pinnacles | Passes through ridges | Opens passes; over-mining collapses the pinnacle into rubble that blocks the pass. |
| Mining crystal | Tunnels | Hidden routes under ridges that can be collapsed on purpose. |
| Fishing | Emptied holes freeze over; new holes weaken the ice | Diving spots for Gentoos, weak points in sea ice. |
| Collecting pebbles | Loose slopes | Raises avalanche and landslide risk, possibly onto your own path. |
| Burning wood | Slush, then holes in thin ice | A fire on sea ice warms an army and slowly melts the ground under it. |
| Heavy foot traffic | Packed paths, then ice roads | Roads form where armies walk: faster, but no ammo on them. |

Rules that keep this fun rather than just destructive:

1. **Every change cuts both ways.** Your new path is the enemy's too;
   clearing a forest removes your cover; a moat traps your own workers.
2. **The map partly recovers on the season clock.** Blizzards bring snow
   back, winter refreezes channels, summer regrows saplings. Collapsed passes
   and tunnels never come back, so each match has a history.
3. **Maps are built around locked routes.** A forest between two bases, an
   ice wall across a valley, a strait that thaws in spring. Choosing which
   route to open, and when, is core strategy.
4. **Workers can reshape the map on purpose:** clear, trench, cut a channel,
   trigger an avalanche.
5. **Changes are easy to read.** Trees fall, ice cracks, the minimap updates,
   and hovering terrain previews the result ("opens a route to Blue").

## 3. Resources

| Resource | Source | Used for |
|---|---|---|
| Fish | Fish holes, sea fishing, krill | Warmth, newcomers, training |
| Wood | Pine forests | Fire pits, sleds, catapults, rafts |
| Snow | Everywhere, but each tile runs out | Snowballs, walls, igloos, snowmen |
| Ice | Lakes and sea edges | Strong buildings, ice weapons, armour |
| Pebbles | Rare beaches and scree | Advanced tech, diplomacy (currency between tribes) |
| Ancient crystal | A few deep deposits | Top-tier tech, heroes, Ice Palaces |

Heat is not stored: buildings and fish give off warmth and penguins draw on
it. The population is fixed (section 0): it does not grow from eggs.

## 4. Population

- **Fixed population** (built): nobody is hatched. Newcomers swim in from
  the sea for fish; frozen penguins thaw wild and join whoever stands
  beside them.
- *(Dropped ideas: rookeries, eggs and chicks, recruiting from wild colonies.)*
- **Prisoners** (not in the current rules): convert (low loyalty, may
  desert), ransom, or release.
- **Loyalty** rises with warmth, food, wins and heroes; falls with cold,
  losses and enemy propaganda.

## 5. Combat

Each penguin has **warmth** (health; at zero it freezes and can be captured),
**wetness** (loses warmth faster until dried by heat), **courage** (morale;
low courage scatters a group) and **ammo** (a penguin holds the one snowball it has scooped
from the snow underfoot, and scoops again after each throw).

Modifiers: high ground (range, accuracy), cover (trees, drifts, walls,
icebergs), facing (flank and rear hits hurt more and shake courage),
formations (line, wedge, turtle of ice shields, skirmish). No stances to
set: penguins pick their fights sensibly on their own (`game/tactics.rs`).

Terrain as a weapon: heavy snowballs set off avalanches on loaded slopes,
thin ice cracks under heavy units, sledders ram through fresh snow walls.

## 6. Tribes

All four share the same roles (worker, basic thrower, scout, siege) so each
is easy to learn.

- **Emperors — the Huddle.** Slow, tough, hold ground. Stopped groups share
  warmth. Big citadels, heavy catapults, a Chieftain who raises courage. Builders
  and miners:
  tunnels, quarries, huge walls.
- **Adélies — the Pebble Barons.** Thieves and dealers. Steal pebbles and fish;
  spies in enemy colours; bribes; better prisoner conversion.
  Saboteurs: cut ice bridges, loosen slopes above enemy bases.
- **Rockhoppers — the Crest Raiders.** Fast and aggressive. Hop over cliffs,
  walls and crevasses; mobile camps; war cries that drain courage. They
  benefit from blocked maps and trigger rockfalls to close routes.
- **Gentoos — the Tidecallers.** Rule the sea. Dive under the ice, chop or
  freeze floes, build rafts, fish the open sea. Strongest in the thaw,
  weakest in deep winter. Turn sea ice into their own waterways.

| Role | Emperor | Adélie | Rockhopper | Gentoo |
|---|---|---|---|---|
| Worker | Waddler | Pebble-picker | Forager | Netter |
| Basic | Huddleguard | Scrapper | Hopper | Splasher |
| Long range | Slinger | Pebble Slinger | Crag Thrower | Harpooner |
| Heavy | Packer | Saboteur | Avalanche Caller | Iceberg Hurler |
| Fast | Belly-sled Lancer | Pickpocket | Crest Rider | Diver |
| Support | Chieftain | Bribe-master | War Drummer | Tide Priest |
| Defence | Shieldbearer | Decoy Snowmen | Ambusher | Floe Warden |
| Siege | Snow Trebuchet | Tunnel Sapper | Rockfall Team | Ice-Ram Raft |
| Special | Fish Guard | Spy | Scout Hawk | Orca Whisperer |
| Hero | Old Frostbeard | Countess Shiny | Crest the Mad | Tidemother |

Heroes level up during a match (three abilities and an ultimate) and keep
their progress through the campaign.

## 7. Buildings

Snow buildings are cheap but soften in the thaw and wear under fire; ice
buildings are stronger and crack in warm spells. Workers repair both.

Igloo (three tiers: Igloo, Snow Hall, Ice Palace; parked), fish hut,
snow quarry, ice cutter, pebble cache, warming hut, training camps, sled
workshop, watchtower and catapult, palisades, snow forts (garrisonable),
snowman sentry, prison pen, crystal shrine.

**Territory:** removed (2026-10-28). Building is only barred close to enemy
buildings.

## 8. Technology

Three base tiers plus four branches per tribe — Snowcraft (projectiles),
Icecraft (buildings and armour), Fishery (economy and warmth), Lore
(tribe-specific) — each ending in a tribe wonder.

## 9. The living map

- Per-tile height, snow depth, ice thickness, water, tree health and packing.
- **Seasons:** deep winter, thaw, polar night (short vision, auroras reveal
  hidden units), midsummer (plenty of fish, little snow).
- **Weather:** blizzards, whiteouts, storm surges.
- **Landmarks:** hot springs (control points), crevasses, ice caves,
  drifting icebergs, shipwrecks, ruins.
- **Wildlife:** leopard seals at the water's edge, and no more than that.

## 10. Modes

Campaign (four tribe stories, a home village between missions), skirmish
(up to 8 players), diplomacy free-for-all, alternative victories (conquest,
King of the Spring, regicide, Golden Egg), co-op survival
"The Long Night", ranked 1v1/2v2, challenge scenarios, replays, spectating.

## 11. Computer opponents

Scripted AI (economy, build order, army, tactics, scouting) that uses
seasons and terrain. An optional local language model runs on the host only,
never in the simulation loop, with the scripted AI as fallback: diplomacy
and betrayal, ransom talks, campaign dialogue, commentary, a post-game
chronicle, and an advisor for new players. Built so far: the model
advises each computer player every half minute with a `Directive` (posture,
target, favoured unit, research branch) that the scripted AI weighs in its
own rules.
The computer also watches its own ground: trespass in its groves and base
builds a grudge (easy forgets fast, hard holds on), which warns, sends
raiders, posts guards at the grove and, more than weakness, picks whom it
attacks. In chat a player can ask for a truce, promise to leave, or ask
why; the game decides, and the model, if there is one, only finds the
words. Spectators see a panel of what each computer is thinking (its advice, grudges, truces and a log of what it did and why). Not built yet: paying tribute (there is no way to give fish or wood
to another tribe), and the computer talking to other computers.

## 12. Technical approach

- Keep: dependency-free simulation, authoritative server, both renderers,
  software mixer, headless self-tests.
- Unit, building and tech definitions in data files.
- Terrain grid sent as changed tiles (chunked when large); pathfinding with
  flow fields for big groups, updated in place as terrain changes.
- Authoritative snapshots; move to deterministic lockstep if bandwidth
  demands it (also gives cheap replays).
- Map editor in the client, replays, observer mode, mod folders.

## 13. Roadmap

1. Vertical slice: Emperors vs Emperors on one map — snow depth, warmth,
   courage and ammo, capture and prisoners, five buildings, six units.
2. Economy and tech: all resources, territory, base tiers.
3. Adélies and seasons.
4. Multiplayer at scale: chunked terrain, bandwidth budget, replays.
5. Remaining tribes, heroes, wildlife.
6. Diplomacy free-for-all and the language-model layer.
7. Campaign, co-op, map editor.
8. Ranked play, balance, polish.

## Prototype status

Built in the current game to test pillar 2 (the map changes as it's used):

- **Runs in a browser.** The same client builds to WebAssembly and joins
  a server over WebSocket (see the README); the browser is a full player,
  but can't host.

- **Snow depth.** Each snow tile holds `SNOW_DEPTH` layers. Scooping a
  snowball takes one; foot traffic packs one every `TRAMPLE_TICKS` of
  walking. At zero the tile is bare ice: faster to cross and preferred by
  pathfinding, but no ammo there. Snowballers out of ammo on ice look for
  snow nearby.
- **Snowfall.** Worn tiles regain a layer with a small chance each second.
- **Trees.** Right-click a tree with workers selected: they chop it (three
  loads of wood) and keep clearing trees nearby, carrying the wood home.
  Felled trees leave walkable stumps and stop blocking snowballs. The AI
  keeps one worker per igloo chopping while its wood is low.
- **Armies.** Every Great Igloo has a Chieftain (founding one raises one
  from its own penguins; a frozen Chieftain's crown goes home and the igloo
  raises another after a minute). Soldiers with no order of their own
  follow their igloo's Chieftain, facing the middle of the enemies near
  him: Shieldbearers in a wall of pairs just ahead (gaps between pairs),
  throwers in staggered ranks 1.7 tiles apart behind it, Skiers on the
  flanks, Slingers behind the throwers and in cover behind a tree or wall
  if one is near, Warmers and Sleds at the back. With enemies all round
  it forms a ring, shields outermost. A Skier's charge knocks a
  Shieldbearer down along with itself, opening the wall; a Chieftain
  answering an alarm stops five tiles short; soldiers picked together with their Chieftain
  fall in. Each finished watchtower posts two sentries by itself. A tribe
  starts with its Chieftain and two snowballers; the map's other starting
  soldiers start as workers. (`game/armies.rs`, rule `armies`)
- **No igloo guards** (2026-11-10). An igloo guards nothing by itself:
  a Watchtower beside it does, with as many sentries as the tribe wants
  (two to start, up to four). (`game/armies.rs`, `tests/slots.rs`)
- **Hordes and elites.** With armies an igloo makes nobody. A Bunkhouse
  (which replaces the Throwing Range) has 4 bunks, built up to 8 and 12;
  each wanted bunk drafts an idle worker into a snowballer for 6 fish.
  Camps are elite housing with 1 slot, built up to 3 and 5. Crystal gates
  elites (C7 stage 3): a wanted slot calls in the nearest idle worker,
  paying the training fish there and then, and only while there is
  crystal for it. The worker mines a crystal, walks to the camp and trains
  for 3 s, the crystal used up. Building up costs snow steps and logs
  (4 and 2, then 8 and 4). Mined crystal grows back one every 30 s. A
  frozen soldier frees its slot and thaws a plain worker.
  (`game/camp.rs`, `tests/elites.rs`)
- **Slots you set** (2026-11-10). Every building with slots (Sushi House,
  Bunkhouse, camp, tower; an igloo's two fishing slots) fills only as many
  as the tribe wants: click a slot on its card, or roll the mouse wheel
  over the slots. Building up adds room, never fills it by itself.
  (`Building::want`, `Command::SetWant`, `tests/slots.rs`)
- **Newcomers from the sea** (2026-11-10; the Landing and wild colonies
  are gone). A wanted slot with nobody idle to call pays 4 fish
  for a newcomer, who swims in to the beach nearest the slot (by walking)
  and takes it. No housing limit, no Roosts, no fish upkeep: fish pays for
  newcomers and training and nothing else. (`game/sea.rs`, `tests/slots.rs`)
- **Fishing slots** (C7 stage 1). With armies only a worker in a fishing
  slot fishes. Every igloo has 2 slots, never more; every finished Sushi
  House with a bank within 8 tiles has 2, built up to 4 then 8 (3 steps
  and 2 logs, then 6 and 4). A house's fishers fish the banks beside it and
  set the catch down there. A fisher called to build keeps its slot and
  goes back to it. Computers want every slot they build, and put Sushi
  Houses on shores 7-22 tiles from home while short of fish.
  (`game/outpost.rs`, `tests/outposts.rs`)
- **No woodpile** (C7 stage 2). With armies nobody chops on their own and
  nothing stores wood: a log nobody needs is left where it fell. Builders
  fetch logs for their own site from trees within 20 tiles. Every finished
  sauna and campfire has one stoker's slot that calls in an idle worker (a
  fisher if nobody is idle); the stoker keeps up to 60 s of fuel in the
  fire, fetching a log (20 s) at a time, and a sauna stays lit while its
  stoker is off fetching. Other prices pay their wood share in fish
  (`CoreRules::price`). Felled trees grow back after 3 minutes, once
  nothing stands on the stump and it is not on a bank or by a fishing or
  crystal spot. Computers no longer chop or tend fires by hand, and place
  buildings without a log in store. (`game/sauna.rs`, `game/terrain.rs`,
  `tests/stokers.rs`)
- **A small start** (C7 stage 4). With armies (and `small_start`, on by
  default) a tribe starts with its Chieftain and 2 workers. Anyone thawed
  out of the ice is a plain worker again, whatever it was. Fishers on banks fish twice as fast as
  before (only the slotted few fish). The fishing slots choose from the
  idle first, then the fires, then the camps; camps never take fishers, and
  a site takes one fisher only when nobody at all is building it.
  Computers with fewer than 8 workers build only Sushi Houses, keep one
  site at a time until 20, and wait for 8 or 10 fighters before training
  Warmers or Sled pullers. In tests, `CoreRules::map_start()` gives the
  map's whole start; the client's self-test plays that way too
  (`SNOWBALLZ_START=map`). (`tests/small_start.rs`)
- **Igloos anywhere** (C7 stage 5). With armies the build card has an
  Igloo (P): a 4x4 site, 16 snow steps and 6 logs, at least 30 tiles
  (centre to centre) from every igloo and igloo site, so never on an
  enemy's doorstep. Finished, the site becomes a real igloo of the tribe:
  its builders live there, it raises its own Chieftain from them and has
  its own guard and fishing slots and bunks. Snapshots carry the place of
  any igloo built since the start (also to clients who join later).
  Computers with sixteen workers per igloo build one 34-46 tiles from home,
  by water, 40 from enemies and where the builders can walk.
  (`tests/igloos.rs`)
- **Maps for the new rules** (C7 stage 6). Forests are about twice as
  thick (every start has 80+ trees in reach), and every start has its own
  crystal deposit 8-14 steps from its door on the side away from the
  middle, in open ground (`add_home_crystal`), besides the contested
  crystal in the middle. Thicker forest can seal a start in between cliffs
  and water, so `connect_starts` clears the cheapest way over land (as the
  game sees it, with the fish-hole ponds), the trees are rebalanced, and
  `check` now fails any map where a door can't walk to the others.
  Computers playing every seat, 16 seeds of 12 minutes: seat spreads of
  1.1-2.2x. (`mapgen.rs`, `tests/balance.rs`)
- **Site crews** (`game/jobs.rs`). Sites placed with nobody picked call a
  crew sized to the building (1 to 4), not every idle worker; a finished
  crew helps the next site short of hands. There is no work split, no
  housing limit and no food upkeep.
- **Sync.** Changed tiles go out in snapshots; new clients get every tile
  that changed since the start. The protocol version lives in `proto.rs` (`PROTOCOL_VERSION`).
- **Look.** Trodden snow is greyer and flatter in both renderers, and the
  minimap follows the terrain.

- **Buildings** (`crates/sim/src/game/build.rs`): campfire (built up into a
  sauna), snow palisade, watchtower (built up into a catapult), Sushi House
  (the store), Bunkhouse, igloo, and the training camps.
  There are no gates (removed 2026-10-28): a gap in a wall is a weak point.
  Builders fetch the wood; every
  construction step quarries a layer of snow from around the site. Walls and
  trees just in front of a target stop throws; Packers smash buildings.
  Palisades crumble back into snow. Build anywhere there is room. The AI
  builds campfires and saunas where its army masses, watchtowers, and
  (Hard) palisades against attacks.
- **Courage** (`game/morale.rs`): seeing friends freeze costs courage
  (halved in a huddle or near a Chieftain); at zero a penguin flees and
  ignores orders until it calms down. Frozen penguins belong to nobody
  (see the core below), so there are no prisoners: they thaw out wild, or
  a Sled drags them home.
- **The core** (`game/core.rs`): the rules of section 0 are the game;
  `CoreRules` keeps only three switches (`tech`, `cold`, `small_start`).
  Penguins are never hatched. Freezing strips ownership: a frozen penguin
  belongs to nobody and stands where it fell, blocking walking and
  stopping snowballs, so battles leave the ground cluttered with cover.
  Left alone it thaws in 120 s (twice as fast in thaw and midsummer, never
  in polar night) and wakes up wild. A wild penguin joins whichever tribe
  stands with it for 2 s. A Warmer thaws a frozen penguin in 5 s and it wakes up in the
  Warmer's tribe. An enemy igloo can no longer be walked into and taken.
- **Siege and ruins** (`game/siege.rs`): soldiers pelt enemy igloos and
  buildings instead of taking them. An igloo has 120, 200 or 300 hp by
  tier; a Packer does 3 damage per throw, a soldier 1. Idle workers beside
  a battered igloo each add 1 hp a second, up to 4 at a time. Pelted down
  to nothing, an igloo falls in: everyone tumbles out, its stores
  are lost, and the empty ruin may be claimed by anyone. A tribe
  with no igloos and no buildings is out of the game.
- **Fog of war** (`crates/sim/src/fog.rs`): enemy penguins, wild penguins
  and surfaced seals are hidden outside your team's sight — 9 for a
  penguin, 12 for an igloo, 16 for a finished watchtower — and unseen ground
  darkens on the map and the minimap. Not yet: scouting and line of
  sight; nothing blocks the view.
- **Units** (`game/units.rs`, one data table): worker, snowballer, slinger,
  packer, sled lancer, shieldbearer, chieftain, warmer and skier, each with
  its own prop. Skiers move faster on snow and slower on ice.

- **Training camps** (`game/camp.rs`): Sling Range, Packer Yard, Ice
  Workshop, Sled Track, Warming Spa and Ski Lodge (the Bunkhouse makes
  Snowballers). Their slots call in workers, as described under Hordes and
  elites above. A selected camp takes a right-click on the ground as its
  rally point.
- **Seasons and weather** (`game/season.rs`): deep winter, thaw,
  midsummer and polar night, four minutes each; blizzards. See PLAN.md M5.

- **Warmth as supply** (`game/warmth.rs`, off by default): penguins outside
  an igloo door, a friendly sauna, an igloo door or a siege they are running
  lose a warmth every 10 s (faster in polar night and blizzards, slower in
  midsummer or at work), down to just above cold, and get it back on home
  ground. Long marches arrive worn down; forward huts fix that.

- **Parked (2026-10-28):** igloo tiers and research are off by default
  (`CoreRules::tech`); the next two items describe them, and they still
  run with that switch on.
- **Igloo tiers** (`game/tiers.rs`): an igloo can be built up into a Snow
  Hall (20 fish, 10 wood, 30 s) and then an Ice Palace (40 fish, 12 wood,
  8 crystal, 45 s). The materials are paid up front (wood and crystal
  from anywhere in the tribe); the igloo keeps
  warming penguins while the work is done, but it is lost if the igloo
  falls first. A grander igloo holds more penguins (40, 70, 110), warms
  penguins faster and takes longer to take over (25 s, 37 s, 50 s). A tier
  also decides which kinds can be trained (slinger, packer and
  shieldbearer need a Snow Hall, sled lancer an Ice Palace).
  The AI builds its home igloo up when it has an army to spare, and
  saves fish and wood for it while it does.

- **Research** (`game/research.rs`): three branches of three levels —
  Snowcraft (throws), Icecraft (buildings and ice) and Fishery (fish and
  warmth). Every level is a few percent multipliers from one table. What is
  learnt belongs to the whole tribe, but the work is started at an igloo
  (10–35 fish from that igloo, wood and crystal from the tribe) and takes
  40–80 s; an igloo researches one thing at a time, and the work is lost
  if the igloo is captured first. The second level needs a Snow Hall, the
  third an Ice Palace. The igloo panel shows the three branches with their
  costs, and a progress bar while the work is done. The AI learns the
  cheapest tricks first, once it has an army to spare. Fishery level two,
  Quick Lessons, makes camps train 30% faster.

- **Game feel and scores** (`client/src/juice.rs`): snowball hits pop and
  throw up a puff, a hit flashes its penguin white, and a freeze shatters
  into frost. Big splashes and igloos changing hands shake the camera a
  little when they happen on screen; a capture bursts in the new owner's
  colour under a short banner. Threatened igloos ping on the minimap and
  Space jumps to the latest alert. The end screen tallies what each
  player trained, threw, hit, froze, captured and took prisoner (counted
  on the client from events) and graphs army sizes over the match.

- **Heroes** (`game/heroes.rs`): a **Chieftain** per igloo, a big
  crowned emperor penguin raised by the igloo itself. Penguins near it keep their nerve: they recover courage faster
  and lose it slower, and a freezing friend hurts them less. Its **War
  Cry** (key Y, once every 45 s) makes friends within nine tiles run a
  third faster, throw faster and stop fleeing. If the Chieftain freezes,
  the tribe around it loses heart and the enemy takes heart, and its igloo
  raises another a minute later. The AI keeps it with the attacking group
  and lets it shout in a fight.

- **Leopard seals** (`game/wildlife.rs`): a few seals per map lurk at
  fishing banks and on open ice away from every igloo. Seals sit
  under a penguin's notice: one left alone (fewer than 3 friends within a
  few tiles) draws the seal up out of the water; after a short warning it
  lunges and snatches the penguin, so lone fishers and
  wood haulers are at risk. Snowball hits (two) drive the seal under and
  it keeps away for a minute and a half, so ganging up — or drifting a
  rescue party — clears a hole. The AI avoids spots a seal recently
  struck. Client-side: a spotted grey model in both renderers, a water
  splash when a seal surfaces, strikes or is driven off, and a notice
  when it takes one of your workers (`tests/wildlife.rs`).

- **Great Igloos:** each player starts with one. Workers can found another at
  a neutral map igloo site for 40 fish and 20 pooled wood. Founded igloos
  provide warmth, storage and training.

Not yet: frozen-sea routes, tribes, tribes' own branches (Lore) and the
tribe wonders, diplomacy and the language-model layer, campaign, map
editor, ranked play.

## Glossary

- **Penguin kinds:** Worker, Snowballer, Slinger, Packer, Sled Lancer,
  Shieldbearer, Chieftain, Warmer and Skier — the nine kinds of the stats
  table (`game/units.rs`).
- **Wild:** belongs to no tribe; thawed out of the ice; joins the tribe
  that stands beside it for 2 s.
- **Frozen:** warmth at zero; belongs to no one, stays where it fell,
  blocking walking and snowballs; thaws out wild on its own, and a Warmer
  thaws it into its own tribe faster.
- **Ruin:** an igloo pelted down to nothing; the shell stays, anyone may
  claim it, and whoever moves in patches it up.
- **Igloo tiers:** Igloo, Snow Hall and Ice Palace; each higher tier holds
  more penguins, warms them faster, takes longer to take over, and unlocks
  the higher kinds there (parked: `CoreRules::tech`).
- **Seasons:** Deep Winter, Thaw, Midsummer and Polar Night, four minutes
  each; each changes the ground and the fighting, and blizzards blow in now
  and then (never in midsummer).
- **Courage:** morale from 0 to 10; seeing friends freeze costs it (halved
  in a huddle or near a Chieftain), and at zero a penguin flees
  and ignores orders until it calms back to 6.
