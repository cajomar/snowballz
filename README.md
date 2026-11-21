# Snowballz

A real-time strategy snowball fight between tribes of penguins. This is a
from-scratch rewrite of the 2007 Python/pygame game: new rules, new maps,
and the penguin art from the Snowballz 2 attempt.

- **Minimal:** Rust, [winit](https://github.com/rust-windowing/winit) for the
  window and [wgpu](https://wgpu.rs) for the GPU (Vulkan, Metal or DX12 on
  desktops; WebGPU or WebGL 2 in browsers), plus the `png` and `lewton` (Ogg)
  decoders. No game engine.
- **3D only:** lit terrain, soft shadows, animated penguins, falling snow,
  glossy ice and fog. (The old 2D sprite
  renderer is gone, and with it Raspberry Pi class hardware.)
- **Multiplayer:** an authoritative server with delta-compressed snapshots
  (around 20–40 KB/s per player). Single player runs the same server on
  a background thread, so AI games and online games behave the same.
  LAN games are discovered automatically.
- **Sound:** synthesized effects (and music, if you add tracks), mixed in
  software. Output is ALSA on Linux (loaded at runtime, so a machine
  without sound just plays silently) and waveOut on Windows. Volumes are
  in Settings.

## Building and running

```console
$ cargo run --release                 # the game
$ cargo run --release -p server --bin snowballz-server   # dedicated server
```

Windows: unzip the package and run `snowballz.exe`.
On Linux you need nothing beyond a GL driver and X11 or XWayland (libGL,
libX11 and libasound are loaded at runtime).

### Playing against the computer

Run the game and press **Play vs Computer** in the main menu. The lobby
fills every empty seat with a computer player (normal strength); pick a
map and press **Start!**. To skip the lobby:

```console
$ cargo run --release -- --quickstart "aurora basin"
```

`--quickstart MAP` picks that map, fills the seats with hard computer
players and starts the game at once.

Every game each computer player is somebody new. It rolls a character:
how bold it is, how long it holds a grudge, whether it keeps its word, how
much it builds, how many towers it puts up, how much it talks, and a
favourite fighter. One or two of those are pushed to an extreme, and its
name carries a hint of the strongest ("Tux the Treacherous", "Waddles the
Builder"). It follows plans that suit it (grow, expand, raid someone,
besiege someone, dig in), changes them every minute or two, and often says
so in chat, in its own voice: a loudmouth brags, a silent one says "Soon.".
The computers also deal with each other, in public chat: one at war offers
another a truce ("We both have bigger fish to fry"), and when somebody
(you, perhaps) runs away with the game, a spiteful or honourable one calls
on the rest to gang up on them. A treacherous one breaks a truce when it
smells a win. When the game ends the scoreboard shows what each one was
really like.

To watch the computer players think, sit out a game: give your own seat to a
computer in the lobby, or join a game in progress. Spectators see the whole
map and, down the left side, a panel for each computer player (a tab per
player; F4 hides it): its advice from the language model, whom it is
attacking, its grudges and truces, and a log of what it did lately and why
(who it warned, what it heard and answered, how the model worded a reply).
Players in a game are never sent this.

Computers don't chat their built-in lines (taunts, warnings, plans, the
deals they strike with each other): those go in the log as "Thinks: ...".
They answer you when you talk to them about a truce or a grudge, and with a
language model on, everything else they say is the model's own words.

**Admin page.** A server serving the web build (`--web`) also has
`/admin?key=KEY`, a live page of every computer player's decisions in the
running game: a tab per computer and one for all of them, the whole log,
a filter, and colours for attacks, plans, deals and unsaid lines. The key
is `SNOWBALLZ_ADMIN_KEY`, or the one the server writes to
`~/.config/snowballz/admin-key` the first time it starts.

With nothing selected, the tribe card under the minimap names the next
sensible step (build a camp, fetch wood, spend spare fish, train soldiers)
until you have it covered. After dark every penguin and igloo stands in a ring
of its tribe's colour so friend and foe can be told apart. The end screen lists
the turning points of the match: igloos taken and igloos that fell in, with
times.

A computer won't march on a person's igloo in the first minutes of a game
(8 on Easy, 5 on Normal, 3 on Hard) unless that person has already trespassed,
so you can set up first; it still scouts, chops and defends. Building or
chopping near its ground (inside a strip around what it claims) gets a
friendly grumble in chat first, then sharper warnings if you move in.

The computer takes note of who works and builds on what it thinks is
its own: the land around its igloos and the groves it chops. Trespass
earns a grudge and, in chat, warnings; a normal or hard computer then
posts guards at the grove and sends fighters against the intruders.
Grudges, more than weakness, decide whom it attacks. An easy computer
forgets quickly, a hard one holds on and will march on a hated rival
past a softer target. You can talk to it in chat:

- `truce` / `peace` / `sorry`: a truce of 3 to 8 minutes (shorter and
  harder to get on higher levels), if you have not wronged it too much.
  It ends if you attack, or camp on its land long enough.
- `I will leave` (while trespassing): a minute's grace, then it checks.
  A broken promise costs you dearly.
- `why are you attacking me?`: it tells you what it holds against you.

With a model on (`snowballz-server --llm`) the host's model puts these
replies into the computer's own words (the game still decides what they
mean), and advises each computer player every half minute. See
[A language model for the computer players](#a-language-model-for-the-computer-players).

Settings (name, UI size, volume...) are saved to
`~/.config/snowballz/settings.txt`.

### A language model for the computer players

A small language model can be the computer players' minds: every 30 s of
game time it reads each one's report and character and picks its plan (and
a target, a favourite unit, a research branch), announcing a new plan in
its own words, and it puts their chat replies into words. The scripted AI
carries the plan out; with no model, or a slow one, the computers pick
their own plans. It is meant for servers and for games you host.

The model runs inside the server, on the graphics card if there is one, and
is downloaded from Hugging Face the first time (Qwen3.5 4B, 2.7 GB). That
needs a server built with the `llm` feature, which pulls in llama.cpp:

```console
$ cargo build --release -p server --features vulkan   # AMD, NVIDIA, Intel (or `metal` on a Mac, or `llm` for the CPU alone)
$ target/release/snowballz-server --llm-setup          # optional: download and try the model now
$ target/release/snowballz-server --llm                # then play: the computer players use it
```

To build it you need cmake, a C++ compiler and clang (for the bindings),
and for `vulkan` the Vulkan and SPIR-V headers and `glslc` (Arch:
`vulkan-headers spirv-headers shaderc`; Debian and Ubuntu:
`libvulkan-dev spirv-headers glslc`). The game client takes the same
features (`cargo build --release -p client --features vulkan`) for games it
hosts. Plain builds leave all of this out.

`--llm` takes `hf:owner/repo/file.gguf` for another model on Hugging Face, a
path to a `.gguf` file, or an Ollama name (`qwen3.5:4b`). The last, and any
server built without the feature, uses a model server that is already
running (Ollama, llama-server or LM Studio on their usual ports) or starts
Ollama and has it download the model. Downloads are kept in
`~/.cache/snowballz` (`SNOWBALLZ_CACHE` moves them) and resume if cut off.
`SNOWBALLZ_LLM_GPU_LAYERS=0` keeps the model off the graphics card.

On a Radeon RX 7900 XT a directive takes 0.8 s and a chat line 0.3 s; on
the CPU two to three seconds.

Whether the advice helps is measured by `snowballz-eval`, which plays
computer-vs-computer games with half the seats advised and half not,
swapping seats between games, and counts which side finishes on top:

```console
$ cargo build --release -p server
$ target/release/snowballz-eval --games 40 --advisor qwen3.5:4b   # the model against no advice
$ target/release/snowballz-eval --games 40 --advisor boom --against rush
```

`--advisor` and `--against` take a model, `none`, `random`, or a fixed
posture (`balanced`, `rush`, `boom`, `turtle`). `--map`, `--players`,
`--minutes`, `--level` and `--seed` set up the games (`--help` lists them).

### Playing over the internet

Hosting (**Host LAN / Internet Game**, or `snowballz --host`) asks your
router to forward TCP port 7777 using UPnP. The lobby then shows the
address friends should type into **Join Game**. If that doesn't work:

1. **Forward the port yourself:** in your router's settings (or its app),
   forward TCP port 7777 to your computer's LAN address, shown in the
   lobby. Friends join at your public IP: search "what is my ip".
2. **Or use [Tailscale](https://tailscale.com):** if everyone is on the
   same tailnet (or you share your machine with them), friends join at the
   Tailscale address shown in the lobby. No router changes needed, and it
   works even behind carrier-grade NAT.
3. **Or run a dedicated server** somewhere public:
   `snowballz-server --name "Friday night"` (add `--upnp` on a home
   machine).

On Linux with a firewall (`ufw`), let the game in:
`sudo ufw allow 7777/tcp` (and `sudo ufw allow 7778/udp` for LAN
discovery). On Windows, allow Snowballz when Windows Firewall asks.

### In a web browser

Friends can play without installing anything. `tools/web.sh` builds the
game for WebAssembly into `dist/web/` (once: `rustup target add
wasm32-unknown-unknown`), and `tools/web.sh serve` also starts a server
that hands out those files and the game on the same port, so the whole
thing is one URL:

```console
$ tools/web.sh serve                  # then friends open http://<your address>:7777/
$ snowballz-server --web dist/web     # the same, on a server you already run
```

The server has no passwords: whoever reaches the port can join, and the
first to join is the host. It limits connections (64), message sizes and
how long a connection may stay silent, and a bug triggered by one player's
message drops that player rather than the server, but treat it as a game
server for friends, not a hardened public service. Put it behind a tunnel
you can switch off, and don't leave it running unattended.

The page downloads about 9 MB of game files, then **Play Online** joins
the server it came from; computer players are added in its lobby like
anywhere else. Browsers can only reach servers over WebSocket, which the
server speaks on its normal port, so native clients and browsers can play
in the same game. Reaching your machine is the same problem as for native
players (see above), plus: a page served over `https://` can only connect
to `wss://`, so put the server behind a TLS proxy (Caddy, nginx or a
Cloudflare tunnel all work) if you want https. **Join Another Server**
takes `host:port` or a `ws://`/`wss://` address, and `?server=host:port`
on the page URL points it elsewhere.

The browser build has no hosting, LAN discovery or local-model map maker
(a browser can't listen on a port), and the game's settings are kept in
the browser's local storage. It needs WebGPU or WebGL 2 (any current browser). Building it needs
`wasm-bindgen-cli` at the version in `Cargo.lock`
(`cargo install wasm-bindgen-cli --version <it> --locked`).

### Downloads for friends

`tools/package.sh` builds `dist/snowballz-windows.zip` (cross-compiled in
Docker, see `tools/windows/Dockerfile`) and `dist/snowballz-linux.tar.gz`.
Each contains the game, the dedicated server, the assets and a short
`PLAY.txt`. The Windows build needs Windows 10 or later.
`tools/windows/Dockerfile.wine` smoke-tests the Windows build under Wine.

### Command line

```
snowballz [--connect HOST[:PORT] | --host] [--name NAME] [--size WxH] [--no-msaa]
          [--quickstart MAP] [--practice] [--screenshot FILE [--shot-after SECS]]
          [--selftest] [--look X,Y[,ZOOM]] [-h]

snowballz-server [--port 7777] [--name NAME] [--maps DIR] [--user-maps DIR]
                 [--web DIR] [--no-announce] [--upnp] [-h]
```

`--quickstart "aurora basin"` jumps straight into a game against computer
players; `--screenshot` renders one frame offscreen to a PNG and quits.
`--selftest` plays a game by injecting mouse and keyboard input (box
select, orders, groups, scrolling, minimap, building, zoom, chat) and
reports what worked. `--look X,Y[,ZOOM]`
aims the camera for screenshots. `-c` is a short form of `--connect`, and
`-h` prints the usage. On the server, `--user-maps DIR` saves maps made in
the lobby so they are listed again on later runs.

`tools/headless.sh [args]` runs the client on an invisible display
(gamescope's headless backend, still on the GPU), so screenshots and
self-tests don't open windows: `tools/headless.sh --selftest`.

### Developer switches

Environment variables for screenshots, soak tests and trying new rules.
For a dedicated server, set the game-affecting ones (`SNOWBALLZ_AUTOPILOT`,
`SNOWBALLZ_SEED`, `SNOWBALLZ_WEATHER`,
`SNOWBALLZ_ASSETS`) on the server and the rest on the client:

| Switch | Effect |
|---|---|
| `SNOWBALLZ_ASSETS=DIR` | load the assets from DIR |
| `SNOWBALLZ_AUTOPILOT` (any value) | a hard computer player plays your seat (screenshots of a game in full swing, soak tests) |
| `SNOWBALLZ_ENDSCREEN=1` | jump straight to the end-of-game screen with made-up numbers |
| `SNOWBALLZ_SHOW=help` / `menu` | open the help or the game menu |
| `SNOWBALLZ_FORCE_TIER=N` | pretend every igloo is at tier N (screenshot helper) |
| `SNOWBALLZ_LLM_URL=URL` | the local model server to use (an OpenAI-style base such as `http://127.0.0.1:11434/v1`). Used by the map maker and, with a model named, the computer players |
| `SNOWBALLZ_LLM_MODEL=NAME` | same as `snowballz-server --llm NAME`: see [A language model for the computer players](#a-language-model-for-the-computer-players) |
| `SNOWBALLZ_LLM_START=CMD` | how to start the model server instead of `ollama serve` |
| `SNOWBALLZ_MAKE_MAP="prompt"` | the model designs a map with that prompt as soon as you host a lobby |
| `SNOWBALLZ_NOFOG` (any value) | turn fog of war off |
| `SNOWBALLZ_ADMIN_KEY=KEY` | the key to the server's admin page (8 characters or more); otherwise one is kept in `~/.config/snowballz/admin-key` |
| `SNOWBALLZ_AI_NOTES=0` | (server) don't show players the panel of what the computer players think; it is shown to everyone for now, while they're being tuned (F4 hides it) |
| `SNOWBALLZ_SELECT_IGLOO` (any value) | keep your own igloo selected (screenshot helper) |
| `SNOWBALLZ_SPECTATE` (any value) | the server treats every player as a spectator: no fog, and the panel of what the computer players think (screenshot helper) |
| `SNOWBALLZ_SEED=N` | every game runs with seed N instead of a random one (screenshots and soak tests) |
| `SNOWBALLZ_WEATHER=thaw\|summer\|night\|blizzard` | start the game in that season, or in a blizzard |

## Playing

You lead the penguins of one igloo. **Workers** fish (and mine crystal) on
their own. **Snowballers** throw at any enemy in range and scoop up new
snowballs whenever they stand on snow (not ice).

- **Saunas.** Penguins warm up by bathing: a lit sauna takes six at a time
  for about eight seconds and sends them out fully warm. A worker has to stay
  beside it to tend the fire (right-click the sauna with workers selected);
  it burns wood, and goes out when the tender leaves. Cold and badly hurt
  penguins head for a lit sauna on their own.
- **Campfires.** **M** in the build menu: a log fire that any worker can light
  for one piece of wood. Penguins warm only by standing right beside it, and
  slowly; it burns out after a minute.
- Hits make penguins cold. Fish and wood no longer warm anyone. A cold penguin
  goes to a lit sauna, else a campfire, else home to the igloo, which only
  warms slowly. A penguin with no warmth left freezes solid for a
  few seconds.
- **Open ground is not territory.** There are no territory zones: you can
  fish and walk anywhere, and build anywhere except within 12 tiles of an
  enemy igloo or building (unless beside your own finished buildings). The
  slow cold is switched off for now, so penguins only lose warmth when hit
  (and in blizzards, away from shelter).
- Throws are more accurate up close and reach farther from high ground.
  Trees soak up snowballs, so fight from the forest.
- **The ground wears down.** Every scoop takes snow off the tile, and busy
  routes get trampled into ice roads: quicker to walk on, but there's
  nothing to make snowballs from. Snowfall slowly covers them again.
- **Buildings take wood carried to the site.** Placing a site costs nothing;
  the builders chop the nearest tree (or take a log from the igloo store when
  no tree is near) and carry it to the site, and it goes up only as far as the
  wood delivered pays for. The igloo's wood is for training some soldiers,
  research and sauna fuel. Until the first block goes on, a site is only
  staked out (drawn see-through): it blocks nobody, snowballs fly over it
  and nobody throws at it, so empty sites are no use as walls or decoys.
  While you place a building a grid shows the tiles round the cursor, with
  the ones nothing can go on shaded red.
- **Chop trees** (right-click one with workers selected) for wood. Workers
  keep clearing the trees around it, opening new paths and taking away
  cover. With armies there is no woodpile: nobody chops on their own and
  no wood is stored. Builders fetch logs for their own site, every sauna
  and campfire calls in a stoker who fetches logs to keep it burning (a
  stoked campfire never goes out), and anything else that cost wood
  (training, founding an igloo) is paid in fish instead; more slots are
  builders' work, never bought. Felled trees
  grow back after three minutes.
- **Armies.** Every igloo has a Chieftain, and its soldiers follow him in
  formation: shields in a wall in front, throwers behind, Skiers on the
  flanks, Slingers at the back in cover. Move the Chieftain to move the army.
- **Slots you set.** Buildings fill themselves with penguins: an igloo's
  2 fishers, a Sushi House's fishers, a Bunkhouse's snowballers, a camp's
  specialists, a watchtower's sentries and a fire's stoker. You choose how
  many: click a slot on the building's card (it wants that many filled; click
  the last one again for one fewer), or roll the mouse wheel over the slots.
  Each wanted slot calls in the nearest idle worker. Building a Sushi House,
  Bunkhouse or camp up (on its card: builders bring snow and logs) adds room
  but never fills it by itself. Little squares over a building show its
  wanted slots: solid is filled, filling up is training, hollow is waiting.
  The help (F1) explains every unit.
- **Newcomers swim in from the sea.** There is no Landing and there are no
  wild colonies: a wanted slot with nobody idle to call pays 4 fish for a
  newcomer, who swims in to the nearest beach and walks over to take the
  place. Fish is the only limit; housing is free and nobody eats a fish a
  minute. Anyone thawed out of the ice comes back a plain worker.
- **Fishing slots and Sushi Houses.** Only workers in a fishing slot fish;
  the rest wait for building work. Each igloo has 2 slots, never more; a
  Sushi House (B, then F, within a few tiles of water) has 2, built up to 4
  then 8. A Sushi House's fishers fish the banks beside it and drop the
  catch there instead of walking home. Sites you place call their own
  builders, from idle workers first, then from the fishers (who keep their
  fishing slot and go back to it).
- **Igloos make nobody and guard nothing.** An igloo raises its Chieftain;
  that's all. To guard one, build a Watchtower beside it: two sentries from
  your armies climb up by themselves (want up to four on its card).
- **A small start.** A tribe begins with its Chieftain and two workers.
- **Build igloos anywhere.** An Igloo is on the build card (B, then P):
  it must stand at least 30 tiles from every other igloo. Once your
  workers finish it, it is your igloo: its builders move in, it raises its
  own Chieftain and has its own two fishing slots.
- **Bunkhouses draft, camps house your elite.** A Bunkhouse (Camps page, B
  then R) has 4 bunks (built up to 8, then 12): each wanted bunk drafts an
  idle worker into a snowballer for 6 fish. Each camp has slots (1, built up
  to 3 and then 5), and each wanted slot calls in an idle worker and pays
  its fish. Crystal gates elites: the worker mines a crystal first, then
  trains in 3 seconds. Mined crystal grows back slowly. When a soldier
  freezes its slot calls in another; a thawed one is a plain worker again.
  Every kind has a job it does by itself:
  - **Snowballer** (Bunkhouse): the backbone, good at everything.
  - **Shieldbearer** (Ice Workshop, N): steps in front of its friends; its
    slab blocks snowballs thrown at anyone behind it. Beats Slingers.
  - **Slinger** (Sling Range, O): throws farthest, but can't hit anyone close
    and backs away from them. Needs a line in front.
  - **Skier** (Ski Lodge, Y): fast on snow, slow on ice. Goes round a fight
    for Slingers, Warmers and Sleds; its charge knocks a thrower flat for
    two seconds. Charging a Shieldbearer knocks both down, opening the wall.
  - **Packer** (Packer Yard, X): siege. Aims at walls, towers and igloos
    before penguins.
  - **Warmer** (Warming Spa, K): warms the coldest friend near it and thaws
    the frozen into your tribe.
  - **Sled** (Sled Track, G): doesn't fight. Fetches frozen penguins,
    anyone's, and drags them home, where they thaw into your tribe.
  - **Chieftain** (one per igloo): leads its army. Nobody near it runs away.
- **Siege enemy buildings** with snowballs. Igloos cannot be taken by
  walking inside; a tribe is out when all its buildings are destroyed.
- **Population is fixed but for the sea.** Nothing hatches. Frozen penguins
  lose their owner, then thaw and can be persuaded by either tribe, or a Sled
  drags them home. No eggs or prisoners.
- **Ending a match.** Open the menu (Esc) and Concede to give up: your
  penguins freeze and your igloos fall in. If nobody has won after 20
  minutes, the Long Night begins: a ring of deadly cold closes in from the
  map's edges for 10 minutes, freezing penguins and wasting igloos and
  buildings outside it. When it has stopped and 4 more minutes have gone by,
  the tribe with the most penguins standing wins.
- **Fair starts.** Every shipped map gives each tribe at least 30 trees and 30
  fishing banks within 16 tiles.
  Penguins keep clear of trees when they walk.
- **Found an expansion igloo.** Right-click a neutral igloo with workers.
  Founding takes 40 fish and 20 pooled wood (60 fish with armies). It becomes another base for
  warmth, storage and training.
- **The HUD** is a wooden console along the bottom: the minimap, a paper
  about whatever is selected, and a card of command stamps for it (hover a
  stamp for what it does and costs). With nothing selected the card is the
  build menu; an igloo's card shows its fishing slots; a building's card
  shows its slots (set how many are filled), builds it up or knocks it down.
- **Build**: pick a building on the card with nothing selected, or press
  **B** (it picks your workers if none are selected), then **M** campfire (select one
  and build it up into a sauna), **J** snow palisade (drag for a line),
  **C** watchtower (it sees far), **F** Sushi House, **P** igloo,
  **R** Bunkhouse, **N** Ice Workshop, **G** Sled Track, **K** Warming Spa,
  **Y** Ski Lodge, **O** Sling Range, **X** Packer Yard.
  There are no gates: a gap in a wall is a weak point, so reinforce it.
  A watchtower holds four throwers: right-click it with throwers selected.
  They stand on the roof, throw farther, and can't be hit until it falls.
  Build a watchtower up and it becomes a Catapult Tower: three workers
  crew it and it lobs big snowballs that wreck enemy buildings and igloos
  up to 14 tiles off (select it and right-click one to aim). Freeze its
  whole crew and your workers can take it over.
  Building digs up the snow around the site.
- **Courage:** watching friends freeze shakes penguins (less in a huddle or
  near a Chieftain). A penguin with no courage left flees and won't obey
  until it calms down.
- **The Chieftain** steadies the tribe around it: friends nearby recover
  their nerve faster and lose it slower. Select it and press **Y** for a
  **War Cry**: friends within earshot run and throw faster, and stop
  fleeing. It needs a minute to catch its breath afterwards. If the
  Chieftain freezes, the tribe loses heart and its igloo raises another
  a minute later.
- **Seasons** turn every four minutes (clock top right): deep winter
  snows heavily, the thaw melts snow buildings and wears paths fast,
  midsummer makes the fish bite twice as fast, and in the polar night everyone throws
  shorter. Blizzards shorten throws and chill penguins out in the open;
  huts and igloos shelter them.
- **Learn tricks.** With an igloo selected, the panel shows a row of
  research buttons: **Snowcraft** (throwing), **Icecraft** (buildings and
  ice) and **Fishery** (fish and warmth), three levels each. The fish comes
  from that igloo, the wood and crystal from the whole tribe; the work
  takes 40–80 s and is lost if the igloo is destroyed first. The second
  level needs a Snow Hall, the third an Ice Palace. What you learn belongs
  to the whole tribe, so every igloo of yours gets it.
- **Water and fishing.** There are no fish holes. Workers fish from the
  bank of any water, without end: bays and lakes, and the small ponds that
  stand around each igloo. One worker to a bank, so a crowd spreads along
  the shore. Right-click water with workers selected to put them on its
  bank. Water can't be walked on.
- **Leopard seals:** a few lurk underwater on open ice
  away from igloos. A penguin caught alone — lone fishers,
  rescuers down to a handful — soon has a seal surface at the edge; run
  for the group or it gets snatched away. Two snowball hits drive the
  seal under for a while, but don't gang up and leave, or it doubles back.
- Powerups: the blue gem gives extra warmth and armour; the golden balls
  give double throws. Freeze the carrier to steal it.
- **Make your own maps:** in the lobby, the host clicks **Make a new
  map...** and describes it ("frozen islands with crystal in the middle").
  A language model running on your computer designs it and the game
  builds it. Without one, the map is made from the words you typed. Your
  maps are saved and listed again next time. To use a model, run any
  OpenAI-style local server (llama-server on port 8080, Ollama or LM
  Studio are found by themselves), or set `llm_url` (and optionally
  `llm_start`, a command that starts it) in the settings file.
- Win by owning every igloo.
- **When it's over** you get a scoreboard: what each player trained,
  threw, hit, froze, captured and took prisoner, plus a graph of army
  sizes over the match. "Watch on" puts it away so you can look around.
  When an igloo changes hands a banner in the new owner's colour says so,
  and threatened igloos ping on the minimap.

| Control | Action |
|---|---|
| Left click / drag | Select (double-click selects all of that kind on screen) |
| Right click | Move/fight, enter an igloo, gather from a resource, or chop a tree |
| Middle click, Ctrl+right | Run there without stopping to fight |
| WASD / arrows / middle drag / screen edge | Scroll |
| Wheel, PgUp/PgDn | Zoom · **Q/E** rotate (3D) |
| Ctrl+1–9, 1–9 | Set / recall groups (tap twice to jump there) |
| H | Cycle your igloos |
| Y | War Cry (with a Chieftain selected) |
| B, then M T C F R N G K J Y O X P | Build (with workers selected) |
| O or Backspace | Disband (penguins go back to their igloo) |
| Space | Jump to the latest alert (it pulses on the minimap) |
| Enter | Chat · `>name msg` whispers · `.` replies · `,` resends |
| F1 | Help · **F3** (hold) FPS · **F11** fullscreen |

### The new core (try it)

Every game plays by the new core from [docs/DESIGN.md](docs/DESIGN.md)
section 0.

- **Penguins are never made, only won.** Your starting few and the
  newcomers who swim in from the sea are every penguin there is; nothing
  hatches. Walk a penguin up to a wild (thawed) one and keep it nearby, and
  the wild one joins your tribe. Every one you win is one an enemy doesn't get.
- **Freezing ends ownership.** A frozen penguin belongs to nobody, and
  where it fell it stands as a block of ice that stops walking and
  snowballs: battles leave the ground cluttered with cover, and a pass can
  clog shut.
- **Frozen penguins thaw.** Left alone, one thaws out wild, for anyone to
  persuade. A **Warmer** (**K**) thaws it much faster, and it joins the
  Warmer's tribe — after a battle both sides race their Warmers onto the
  field.
- **Bases fall.** Walking into an enemy igloo no longer takes it: soldiers
  pelt it until it collapses into a ruin, and a tribe with no igloos and
  no buildings left standing is out of the game.
- **Fog of war:** you only see what your own penguins and igloos can see,
  and igloos see farther than penguins do.

## What's new compared to the 2007 game

- Hundreds of penguins at a steady frame rate; the simulation costs about
  0.05 ms per tick.
- Real pathfinding (A* with smoothing) instead of wandering towards the
  target. No more stuck penguins.
- Penguins lead moving targets and fan out into formations when you move
  a group.
- Fishing banks that never run dry, igloos you build anywhere, teams, and
  three AI difficulty levels. The AI masses at a rally point before an
  assault, defends its igloos and still trash-talks like it did in 2007.
- A lobby with map choice, AI slots, teams, colours and chat; joining and
  rejoining games in progress; LAN discovery.
- The map changes as it's used: snow wears to ice, trees can be chopped.
  This is the first step towards a bigger game, see
  [docs/DESIGN.md](docs/DESIGN.md).

## Project layout

```
crates/sim      game rules, AI, pathfinding, map format, network protocol (no deps)
crates/server   lobby + authoritative game server (library and binary)
crates/client   window, renderers, UI
assets/         maps, baked models and font
tools/          asset pipeline (Python + Blender)
```

### Maps

The shipped maps are built from `assets/mapspecs/*.json` by
`cargo run -p sim --release --example make_maps` (The Pass and Frozen Lake
are hand-made: `--example the_pass`, `--example frozen_lake`). They are
balanced, not mirrored: every map is its own land, and the generator measures
what each start has and fixes the shortfalls. Maps start as big forests
that building thins (felled trees grow back): every start gets at least 80
trees in reach and a crystal deposit of its own a short walk behind its
igloo, and every igloo can walk to every other. Each also gets room for two
fishing outposts
(banks 7 to 22 tiles from the igloo and 16 apart; a small pond is added
behind an igloo that lacks it), no more than 30% ice and about the same as
the typical start (ice is where nobody can scoop a snowball). Check them with:

```console
$ cargo run -p sim --release --example map_balance     # what each start has in reach
$ cargo run -p sim --release --example seat_fairness   # computers play every seat; SB_SEEDS=20
```

`tests/balance.rs` and `tests/mapgen.rs` keep every shipped map inside the
limits, and check that no map is a copy of itself turned round.

### Asset pipeline

The generated assets are committed, so you only need these tools to change
them.

```console
# Map specs -> assets/maps/*.sbmap (and the rule tests' fixtures)
$ cargo run -p sim --release --example make_maps

# Snowballz 2 Blender files -> baked vertex-animation models
$ blender -b ../snowballz2/datasrc/penguin.blend --python tools/export_models.py -- penguin

# Signed-distance-field font atlas
$ tools/bake_font.py assets/fonts/Mali-Regular.ttf assets/fonts/ui
```

Igloos, pine trees, stumps, logs, crystals, ice pinnacles and powerups are generated in
code (`crates/client/src/meshgen.rs`), and so are the sound effects
(`crates/client/src/audio.rs`). There is no music in the repository yet:
drop `menu.ogg`, `battle1.ogg` and `battle2.ogg` (Ogg Vorbis, 44.1 kHz)
into `assets/music` and the game plays them; without them it is silent
apart from the effects.

## Credits

Snowballz began as a remake of the 2007 Snowballz by Joey Marshall and
Matthew Marshall; the rules, maps and code here are a from-scratch rewrite.
Penguin models come from Snowballz 2. The UI font is
[Mali](https://github.com/cadsondemak/Mali) (SIL Open Font License, see
`assets/fonts/Mali.txt`).

## License

MIT, see `LICENSE`. The Mali font keeps its own license (OFL).
