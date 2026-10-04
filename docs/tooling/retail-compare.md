# Retail comparison corpus

A corpus-wide oracle: for every retail save state in the library that the
engine can be put into, seed the engine into the same situation, let it
settle, and score what it shows against what retail showed - per channel,
per state, with a ratcheted summary. It is the breadth sibling of the
single-subsystem oracles (`field_camera_zone_oracle`,
`w3a_retail_frame_oracles`, `vram-oracle`, `audio-trace`): those pin one
mechanism exactly, this one asks how far the whole port is from retail at
every captured moment, and ranks the moments by how far.

Nothing here runs an emulator. A save state's RAM holds every retail
observable the corpus compares, and its VRAM holds the frame on the TV - the
display registers say which rectangle - so retail's side is read offline, the
way `mednafen-state vram-dump --display-crop` reads it.

| Piece | Lives in | Role |
|---|---|---|
| corpus + channels + ratchet | [`retail_compare.rs`](../../crates/engine-shell/src/retail_compare.rs) | Enumerate the library, read retail, seed the engine, score |
| battle half | [`retail_compare_battle.rs`](../../crates/engine-shell/src/retail_compare_battle.rs) | Read the encounter out of a battle state, enter it, score the battle channels |
| script phase | [`retail_compare_script.rs`](../../crates/engine-shell/src/retail_compare_script.rs) | Read the running field contexts off the actor lists, run the engine to the same script phase |
| frame channel | [`retail_compare_image.rs`](../../crates/engine-shell/src/retail_compare_image.rs) | Crop retail's frame, render the engine's, the metric |
| `legaia-engine retail-compare` | [`retail_compare_cli.rs`](../../crates/engine-shell/src/retail_compare_cli.rs) | Human report (markdown + JSON + side-by-side PNGs) |
| PCSX-Redux GPU reader | [`legaia_pcsxr::gpu`](../../crates/pcsxr/src/gpu.rs) | VRAM + GP1 control log out of a `.sstate` |
| driver | [`scripts/ci/retail-compare.py`](../../scripts/ci/retail-compare.py) | Resolves the gitignored data, builds, runs, blesses / checks |
| ratchet test | [`retail_compare_corpus.rs`](../../crates/engine-shell/tests/retail_compare_corpus.rs) | Disc-gated; fails on any per-state channel drop |
| baseline | [`retail-compare-baseline.json`](../../scripts/ci/retail-compare-baseline.json) | Scores and classes only - no pixels, no RAM |

## Contents

- [The corpus](#the-corpus)
- [Retail observables](#retail-observables)
- [The seeding model](#the-seeding-model)
- [Mid-script states](#mid-script-states)
- [Battle states](#battle-states)
- [Menu states](#menu-states)
- [Channels](#channels)
- [The image channel](#the-image-channel)
- [The ratchet](#the-ratchet)
- [Running it](#running-it)
- [Reading the report](#reading-the-report)
- [Divergence shapes](#divergence-shapes)
- [See also](#see-also)

## The corpus

Every `[[scenarios]]` block in [`scripts/scenarios.toml`](../../scripts/scenarios.toml)
whose `backup_fingerprint` resolves to a file under `saves/library/mednafen/`
or `saves/library/pcsx-redux/` is one corpus state; a backup several
scenarios name is walked once, under the first label. Each state is classed
by the game-mode word and the scene label:

| Class | Retail condition | Seeded |
|---|---|---|
| `field` | mode `0x03`, a non-overworld scene, a plausible player pointer | yes |
| `world_map` | mode `0x03` on a kingdom overworld (`mapNN`) | yes |
| `field_init` | mode `0x02`, the scene mid-load | no |
| `battle` | mode `0x14` / `0x15` | yes ([below](#battle-states)) |
| `menu` | mode `0x17` (title, save screens, the pause menu) | the pause-menu screens ([below](#menu-states)) |
| `minigame` | mode `0x19` | no |
| `cutscene` | mode `0x1A` / `0x1B` (STR playback) | no |
| `other` | anything else | no |

Unseeded states stay in the report with their reason, so the summary counts
what the instrument cannot reach instead of dropping it. A minigame the
retail state shows *inside* a field-run frame (the casino floor, the dance
hall before the song) is a `field` state and is scored as the field it is.

A field-run state is named by the scene it is **running**, which is not
always the label. A walked door writes the destination label to `0x8007050C`
with the scene-change packet, frames before the field init loads the block
and stores its raw CDNAME define to `0x80084540`. A capture in that window
shows the outgoing scene - its frame, camera, player and track - under the
incoming label, so the corpus scores it as the scene the define names and
records the label as `pending_scene` in the detail
(`RetailObs::settle_on_loaded_scene`;
[the arrival states](#arrival-states-are-captured-before-the-town-runs)).

## Retail observables

| Observable | Address | Notes |
|---|---|---|
| scene label | `0x8007050C` | CDNAME label, 8 bytes |
| loaded scene | `0x80084540` | raw CDNAME define of the loaded block ([above](#the-corpus)) |
| game mode | `0x8007B83C` | the next-mode word the dispatcher reads |
| player `(X, footing, Z)` | `*0x8007C364 + 0x14/0x16/0x18` | `i16`s; the footing is the floor sample under the player |
| camera pitch / yaw | `0x8007B790` / `0x8007B792` | 12-bit angles |
| GTE `H` | `0x8007B6F4` | |
| camera eye | `0x800840B8/BC/C0` | the view builder's translation words |
| camera focus | `0x80089118` / `0x80089120` | the world X / Z the view orbits, stored negated |
| BGM track word | `0x8007BAC8` | written by op `0x35`'s start arms |
| fog-pool gate | `0x8007B854` | written only by op `0x4C` nibble 3 ([field-ambient-fx](../subsystems/field-ambient-fx.md#mechanism-4---the-ambient-particle-emitter)) |
| party / flags / bag / gold | `0x80084140`, `0x1A18` bytes | the live game-state window |
| displayed frame | VRAM + display registers | [below](#the-image-channel) |

The live game-state window is byte for byte the front of a save block - the
save composer copies exactly these bytes over its buffer
([save-screen](../subsystems/save-screen.md#which-buffer-the-sum-runs-over)) -
so the corpus pads it to a block and lifts it through
`legaia_save::SaveFile::from_retail_sc_block`, the same parse a memory-card
load goes through. Party, story flags, the 256-slot bag and gold therefore
come out in the engine's own schema on both sides.

A PCSX-Redux state carries its GPU as one protobuf submessage: the `GPUSTAT`
word, a `0x400`-byte log of the last value written to each GP1 command, and
the 1 MiB VRAM. `legaia_pcsxr::gpu` finds it by shape (a `0x400` and a
`0x100000` member side by side) and reads the display start off `GP1(0x05)`
and the mode off `GP1(0x08)` - the same pair a mednafen state stores as
`DisplayFB_XStart/YStart` and `DisplayMode` - so both emulators crop the
displayed frame by one rule.

## The seeding model

**State channels** are measured headlessly. The engine is opened on the
state's scene and seeded through its own card-load path,
`BootSession::resume_save` over the lifted save (seed the story flags;
enter the scene; hydrate party, flags, bag, gold over whatever the entry
reset). Seeding the flags first is retail's order - the card load copies the save block over
the live game-state window before the field init runs - and it is what lets
an entry script or a bind-time prologue read the saved story: `rikuroa`'s
Genesis-tree objects arm their withered morph only while flag `0x142` is
clear. The player is then seated on retail's
`(X, Z)` with the floor-sampled `Y` (`SceneHost::debug_seat_standing` over
`World::debug_seat_player`, the kernel behind `LEGAIA_SEAT`; on a field scene
it is a warp landing and re-centres the region box and the windowed
static-object list on the seat), the zone camera's arrival snap is re-armed,
and the session ticks a fixed settle window with no input.

The seat is a player **standing** on the tile, not one crossing onto it: the
walk-on dispatcher's last-tile pair (`FUN_801D1EC4`) is stamped with the seat
tile, because a retail capture of a player stood on a trigger tile holds that
tile in the pair already. Seating without the stamp fired the tile's walk-on
record on the first tick - `kor5_post_43a_checkpoint` stands on `P2[4]`, whose
record walked the player out of the temple. The overworld's portals are
entity auto-engages rather than walk-on records, so on a world map the seat
also holds its tile against them until the player steps off
(`WorldMapState::seat_hold_tile`): a pending-door capture stands on the
portal that already fired, and re-engaging it crossed the door a second time
inside the settle window. BGM starts are recorded by a director on the scene
host's event route.

**The image channel** comes from `play-window`, the real renderer, run as a
child process from a scratch directory under the report, and seeded the same
way: the lifted save is written as a scratch LGSF file whose resume point is
the state's scene, and `play-window --resume-save` lands it through the same
`BootSession::resume_save` the headless side calls. `LEGAIA_SEAT=X,Z` then
seats the player on retail's position, and `--screenshot` captures at a
fixed world tick. Live NPCs are on in both the child and the headless
session: a placement's script runs only while its actor carries the engaged
bit (`+0x10 & 0x100`, raised by a touch), so an idle resume holds the seat
instead of letting a talk body walk the player off it.

A `--scene` door entry is **not** an equivalent seed. It stages the scene
for the free-roam picker (story-twin event flags, the entry BGM pause
dropped) and runs its arrival from the picker's seat, so an entry script
moves the player or points a dialogue shot before the seat applies - a
frame whose headless camera channel is exact would score the arrival
instead of the scene.

What the seeding does **not** carry - each is an instrument limit, not an
engine verdict:

- **Script progress outside a running record.** A state inside a running
  field record is run to that record's phase
  ([below](#mid-script-states)). What the phase gate does not reach - a
  record the engine never gets to, or progress held only in actor state
  (a camera aimed by a script that has since returned) - is the entry
  prologue's, not the retail script's.
- **Actor state.** NPC positions, animation phases, open doors and live
  effects are whatever the engine's own entry produces.
- **Timing.** Retail's frame is one instant; the engine's is a fixed tick
  after entry. Ambient animation, fog-pool population and water CLUT phases
  cannot be phase-aligned. The one phase the channel does align is the field
  party HUD's idle countdown (`_DAT_801F348C`, `FUN_801D0D38`): the settle
  window outlasts the `0x28`-frame near idle, while a card-load state is
  typically caught two or three frames into it, so an unaligned stationary
  seat draws a readout retail's frame is still most of a second short of.
  The countdown is read off the state and handed to the child as
  `LEGAIA_HUD_COUNTDOWN`; the window rearms the HUD until the countdown lands
  on that value at the capture tick
  (`world_map_panel_host::hud_phase_hold`). A state whose countdown has
  expired (`0`) scores the readout on both sides.

`--flags-first` is a diagnostic arm for the headless side: hydrate, enter
through `enter_scene_live` directly (no resume landing, no saved seat),
hydrate again. Comparing its report with the default isolates what the
landing itself contributes.

## Mid-script states

A field capture taken while a spawned record runs holds that record's
context on the SCUS actor lists (`_DAT_8007C34C..`, linked through
`+0x00`): the field actor tick `FUN_8003BC08` at `+0x0C`, the engaged bit
`+0x10 & 0x100`, the script base `+0x90` (the record's script start) and
the PC `+0x9E` that the runner `FUN_80039B7C` reads, the flat MAN record
index `+0x50` (`FUN_8003BDE0` stores `N0 + N1 + i`) and the op-`0x4A` wait
accumulator `+0x54`. The cutscene camera mover (tick `FUN_801DC0BC`) on the
same lists gives the shot's progress: `+0x9C` counts up to the duration
`+0x9E`, and `+0x10 & 0x8` marks it landed. The scene system script (tick
`FUN_801DA51C`) and the player are not candidates.

Such a capture is not sampled a fixed window after the entry. The seed runs
the engine until its own context for the record - found by the record's
first 48 bytes, so the partition and the index need no mapping - holds the
retail PC with at least the retail wait and at most the retail glide left,
or has just executed the op retail is parked on (the engine clears some
parks inside one slice that retail holds across frames: a channel flag
already up, a walk already at its tile). A text segment is met once the
engine's box has typed its page and waits for the press - the press that
turns the page, or on the last page the press that closes the box - the
frame every such capture shows. Counting only the page-turn wait missed
every capture on a closing page (`garmel`'s Songi taunt), and the run
sampled the settle window instead. The run has a deadline of 9000 ticks; a gate it never
meets keeps the settle-window sample, and the `script` detail says which
it was.

Two drives get the engine there without changing the retail state it
started from:

- **Resume.** A record the card-load entry does not start - its one-shot
  gate flag is already in the save, or its trigger is a walk-on tile the
  seat does not cross - is installed from its first opcode at the settle
  tick, ungated, as the modal timeline (a concurrent context when another
  timeline holds that slot). The record replays its own staging: its
  `MoveTo`s, camera beats and pokes run from the top.
- **Paging.** From the settle tick on, while the record sits in a dialog box
  short of the gate PC, `Cross` is pressed every other tick - the presses
  the player made to page the conversation to where it was captured.

`play-window` takes the same gate as `LEGAIA_SCRIPT_GATE`, with the same
resume and paging, so the image channel frames the phase the state
channels scored; it is handed only when the headless run met the gate.

What the replay exposes is the engine's own record execution, and two
shapes it has shown are worth knowing:

- **Talk bodies a cutscene never engages.** A placement's own script runs
  only while `+0x10 & 0x100` is up, and only a touch raises it; a poke, a
  walk or a placement from a cutscene record leaves it down
  ([`script-vm.md`](../subsystems/script-vm.md#engagement-and-the-system-script)).
  An engine that stepped every placement a record addressed ran their talk
  bodies: `dolk2_market_noa`'s Noa (`P1[2]`) set `0x2FE`, and the `town01`
  opening states picked up `0x20A` / `0x23D` from `P1[10]` / `P1[11]`.
- **Where a record waits on a walk.** A player compass walk (`B7 F8` /
  `C1 F8`) does not park its record: the record runs on and waits at its
  next op on the player. A gate on that next op is met while the leg still
  plays, and an engine that parked on the walk itself met it only after the
  leg, with every beat after the walk late by a leg
  ([below](#ending-vignettes-are-mid-script)).

## Battle states

A battle capture carries its encounter in RAM, all of it resident while the
fight runs ([battle](../subsystems/battle.md)):

| Observable | Address |
|---|---|
| party / monster count | battle context `*0x8007BD24`, `+0x00` / `+0x01` |
| command-flow byte, action-state cursor | context `+0x06` / `+0x07` |
| monster ids | formation cell `0x8007BD0C[0..4]` |
| scripted-fight bit | `0x8007BD60 & 0x80` |
| present party | `0x8007BD10` (1-based roster ids) |
| combatant HP / max / MP / max | actor table `0x801C9370[slot]`, `+0x14C` / `+0x14E` / `+0x150` / `+0x152` |
| camera | the field's rotation / translation globals; `H` is `256` |

**Seeding.** The scene is entered through the card-load path and the field
settles the same window a field state does - the fight is entered from a
running field, as retail's was, so the scene's track has started. The
formation cell is matched against the scene's registered MAN rows; a cell
no row carries (a fight installed from another table) is registered as a
formation of its own, carrying the scripted bit, with the monster archive's
stats for its ids. The fight's own composition and stage are seeded from the capture: retail's
present list `0x8007BD10` becomes the engine's active party (a guest seat, or a
battle-id fight whose init re-seeds the trio, is not the save window's field
party), and the stage variant `0x8007BD60 & 0x1F` and battle init's
keep-object-1 byte `0x8007B64B` are stamped
(`World::seed_battle_stage_variant` / `seed_battle_backdrop_keep_object_1`;
`play-window` reads both as `LEGAIA_BATTLE_STAGE=variant,keep`), because a
battle capture's player actor is no longer the field walker whose tile names
them. Both come from the region reader, and the second is not decoration:
nilboa's Thunder Ravine region keeps the backdrop shell's object 1, the
horizon mist ribbon, which a replay seeded with the variant alone dropped. `World::force_encounter` then arms the row through the
ordinary transition - the path `play-window --battle` takes, including the
scripted carrier's replayed tutorial arm and the carrier's replayed BGM
words ([below](#the-track-word-in-battle)). When the mode flips, the retail
combatants' live HP / MP are written over the engine's, and the session is
placed at the capture's phase.

**The stream at the entry.** Retail's RNG state at the instant its fight began
is not in the capture, and left alone the engine's would be whatever the
field settle happened to draw - so every channel that rides a draw (a
monster's pick and target, the battle camera, the frame) moved whenever a
field-side port changed how many draws it takes or how they are shaped,
with nothing in the battle changed. The world stream is therefore set to a
fixed seed just before `World::force_encounter`, on the headless seed and in
the image child alike (`LEGAIA_BATTLE_RNG_SEED`). A capture of something a
draw decides is still one realisation of the stream, so the seeds in
`BATTLE_RNG_SEEDS` are tried in order and the first under which the fight is
still on, its opening reached a prompt, and the drive or replayed cast
reached the capture's phase is the one scored; a state no seed satisfies
keeps the first seed's run, `never reached`.

**The image child plays the same fight.** The frame is only evidence about
the state the channels scored if the `play-window` child replays that fight
tick for tick, and three things used to put it on another one:

- the headless seed settles the landed field for `SETTLE_TICKS` before it
  sets the stream and arms the encounter, and the child armed at boot. The
  encounter transition then owned the child's first tick, so the scene's
  entry scripts never ran - on an overworld that left the ambient-particle
  gate clear, and the emitter that draws the stream once a frame never drew.
  The child now settles the same ticks first (`LEGAIA_BATTLE_SETTLE`) and,
  like the seed, installs the fight's roster after the settle, since a
  scripted duel's entry can re-seat the party;
- the fog pool's render step is not presentation-only (it writes the live
  count and depth view the next tick's spawns read, and a spawn draws the
  stream), and the headless seed renders nothing. It now runs the step the
  hosts' draw pass runs after every tick (`BootSession::fog_render_tick`),
  and the child is tick-locked - one tick per redraw - so its draw passes
  land on the same ticks;
- the mid-fight HP / MP were written on the headless side only, so the child
  fought on full bars and its monster AI picked from a different table. The
  bars it seeded reach the child on the same first battle tick
  (`LEGAIA_BATTLE_BARS`).

A settled field also carries its script state into the fight. In `nilboa`
the settle leaves the Nivora duel's dialogue parked on a text page when the
encounter is forced. Retail cannot show that box over a fight: its pager
`FUN_801D84D0` lives in the field overlay (PROT 0897, slot A), which the
battle overlay replaces, so the parked context keeps its park and nothing
draws it. The engine matches - `World::script_dialog_panel`, which both
hosts draw the box from, answers `None` in battle mode. What still parts the two is
render-coupled: a battle clip's end is read off
the window's pose sampling, so an effect-script spawn can land a few ticks
apart (the Delilas Spirit band `0x47`). Comparing a per-tick trace of
`World::rng_state` and the action SM from both sides is how such a split is
found.

A seed whose entry rolled a formation advantage (`ctx+0x290` / its latch
`+0x291`: a back attack or a pre-emptive strike) is passed over too, except
on an opening capture. A capture of a running fight is not its opening round,
and a surprise opening hands one side a round of swings - the monsters' on the
party, or the party's on the monsters - between the HP / MP the seed wrote and
the replayed action, so a replayed cast read the engine's extra damage as an
HP miss (`EngineBattle::surprise_opening`).

**Placing the phase.** The capture's command-flow byte `ctx[+0x06]` picks one
of five plans (`SeedPlan`):

| Retail `ctx[+0x06]` | Plan | What the engine runs |
|---|---|---|
| `0xFD`, `0x00`, `0x0A`, `0x0B`, `0x0C`, `0x14` | opening | nothing past the battle-mode flip; sampled there |
| `0x1E` | prompt | the opening to the first round prompt, then a fixed settle |
| a selection state above `0x1E` | menu | the pad path from the prompt to that surface, on the member cursor `ctx[+0x13]` ([below](#driving-to-the-phase)) |
| `0xFF`, summon band | cast | the capture's cast replayed ([below](#replayed-casts)) |
| `0xFF`, anything else | action | the pad path through rounds until the action SM holds `ctx[+0x07]` on seat `ctx[+0x13]` |

The entry band is everything below the round prompt: `0xFD` is SCUS battle
init's own store (`FUN_80055B6C`, `sb v0,0x6(v1)` at `0x80055FA8`, before the
overlay's init writes `0x00`), `0x0A` / `0x0B` the intro timer, `0x0C` the boss
stage module's baton, `0x14` the one-frame turn setup
([battle](../subsystems/battle.md#the-battle-open-flow---ctx0x06-from-the-intro-timer-to-the-first-swing)).
Every value decodes to the engine's `Idle`, so an opening capture is compared
with the engine before its own opening has run, and its frame is taken there
too (`BattleDrive::Opening`): the first battle frame whose monsters are bound,
and for a capture past the intro timer (`0x0C` / `0x14`) the first one whose
enemy-name labels have cleared, rather than a fixed tick past the round
prompt - a surface retail had not reached. That comparison is only as
good as the engine's opening: the port does not park its command flow on the
intro timer (`battle::intro_names` - the round prompt opens with the names
still up, which the recorded replays pace off), so an ordinary fight holds its
prompt already at the flip and an opening capture of one reads `phase` `0`.
The corpus's opening captures are all the sparring fight, whose opening the
tutorial holds back. Their `camera` reads a battle-entry sweep the port does
not model: `v0_1_battle_loading_tetsu` (`0xFD`) holds pitch `60`,
`TR (0, 1472, 6912)` and `s5_tetsu_battle` (`0x00`) pitch `16`,
`TR (0, 2010, 3552)`, both on the formation centre, where the engine snaps
to the tutorial's dialogue close-up `TR (0, 1280, 1638)` at the flip. Which
routine writes the sweep is not traced; the battle tick's `0x0A` / `0x0B`
arms write no camera word.

A battle state whose RAM does not describe a seedable fight (the context
pointer not yet resident, counts out of range, an empty cell) is kept with a
`battle not seedable:` reason and counted as a classified limit, not as a
seed failure.

**Battle channels.**

| Channel | Score |
|---|---|
| `enemies` | fraction of retail monster seats whose id the engine seated in the same order |
| `enemy_hp` / `battle_party` | fraction of equal HP, max HP, MP and max MP fields over the retail combatants (max MP left out where the engine carries none) |
| `phase` | 1 when the engine's command-flow state equals retail's `ctx[+0x06]` decoded to the engine's band - and, for a replayed or driven action, the same action-SM state on the same seat; for a driven menu, the same member |
| `bgm` | retail's track word against the field track the engine will resume ([below](#the-track-word-in-battle)) |

A state the manifest tags with a `resident_patch` was made on a patched disc
and replays that build's executable; its `enemy_hp` / `battle_party` details
say so, since what the patch writes into a combatant is not retail behaviour.
The three `shiny_refactor_gimard_*` states read `enemy_hp` `0.5` for exactly
that reason: their monster's maxima are the shiny-Seru boost's `x135/100`
(`133` over the disc's `99`, `27` over `20`).

`scene`, `mode` (engine `Battle`), `camera`, `flags`, `inventory` and `image`
keep their field meaning. HP / MP current values are seeded, so their misses
are what the settle window changed; the max values are the real check
(record-derived on the party, archive-derived on the monsters).

### Driving to the phase

A menu or action capture is reached through the engine's own command surfaces
(`BattleDrive`), one press every other tick so each press is an edge. Members
ahead of the capture's seat commit a plain Attack (the ring's Left arm, `Auto`,
the first target); the seat itself takes the arm that leads to the captured
surface - Left then `Command` for the arts entry `0x50`, Up for the item window
`0x3C`, Right for the magic window `0x46`. An action drive commits the same
Attack every round, except that the capture's seat takes Spirit when its
committed category `+0x1DE` is `4`, and a monster seat that was casting
(`+0x1DE = 2`) casts the capture's spell id `+0x1DF` on its next turn
(`BattleState::forced_monster_cast`, with the capture's already-debited MP
credited back). On such a monster-cast drive the whole party commits Spirit
instead of Attack, so the caster is still standing when its turn comes - a
party that kills it first ends the fight with the seeded cast never taken
(Zeto's two mid-cast captures sit in a party that does it in two swings).
Monster seats are translated from retail's fixed pool slots
`3..` onto the engine's seating straight after the party. A message box on
screen takes Cross.

The phase is **held** when the engine's flow state equals the capture's and -
for a per-member surface - the member is the same; an action phase when the
round is executing and the same seat holds the same `ctx[+0x07]`. A menu
surface also waits for the battle camera's glide to land
(`BattleCamera::is_gliding`): retail's capture is a surface the player sat
on, and a frame taken the tick the flow byte changes scores the glide. The drive
gives up after its budget or when the fight ends, and the `phase` detail then
reads `driven by pad, never reached`.

A menu surface, once reached, is **held** with no input for
`MENU_HOLD_TICKS` before it is sampled, on both the headless seed and the image
child. A retail menu capture is a surface the player sat on, so its camera has
finished the transition that opened it - the case-`0` glide onto the member,
or the submenu-exit swing back to the far framing on the commit confirm - and
the drive reaches the surface on the tick it opens; sampled then, `camera`
reads the transition's first step.

A drive plays rounds the retail history did not, so a driven capture's
combatant, bag, flag and track channels are read at the first prompt, before
the drive - where the seed placed them - and only `phase` and `camera` at the
phase itself. The image child runs the same drive (`LEGAIA_BATTLE_DRIVE`) and
captures the first frame that holds the phase; a drive the headless side
never completed is not imaged.

What a drive cannot reach is a real finding, not a seeding limit - each open
case is below. Two the drive has already surfaced and the engine now follows:
a Spirit commit plays the spirit band `0x46..=0x48` (retail's seed sends
category `4` there unconditionally, `li v0,0x46` at `0x801E2F5C`) rather than
ending the action on the spot, and the battle flow byte follows the player
into the item / magic / arts windows ([battle](../subsystems/battle.md#how-the-engine-raises-the-flow-state)).

- **A monster's capture-class special.** The spell catalog carries no
  capture-class record, so a monster turn whose pick named one (Cort's, Zeto's,
  Dohati's, the Delilas duels') found no record and struck instead; retail
  casts it through the capture band `0x6E..=0x71`, routed on the spell
  table's class byte. Every monster pick now builds that record off the disc
  table (`World::monster_cast_def`), the seed's forced cast included, which
  is how the Cort and Delilas captures reach `0x6F` / `0x70`. Opening it up
  surfaced three more defects the seed alone never reached: Dohati's Chaos
  Breath fired on every turn once charged, because the breath's arm 3 -
  which spends the caster's `+0x170` gauge the pick is gated on - had been
  filed as presentation
  ([cast module](../subsystems/cast-module.md#the-twelve-bodies-the-trampoline-map-names));
  Mystic Circle and Doomsday never reported done, so the band held `0x70`
  forever; and Cort's Mystic Shield, which halves the party's damage until
  he is at half HP and keeps his Evil Seru Magic shut until then, was not
  modelled at all
  ([cast module](../subsystems/cast-module.md#the-fourteen-trampoline-arms-that-are-the-bands-other-tick-bodies)).
  Neither boss fight involved is a scripted loss: no story flag 0 latch
  precedes `dohaty` P2[10]'s `3E FF 0A` (Dohati, monster `0x8A`) or
  `chitei2` P2[13]'s `3E FF 0D` (Cort, `0xB4`).
- **Zeto's waves.** The forced cast is armed but never consumed inside the
  budget - the monster pick that would take it is not reached on Zeto's seat -
  so `0x70` is not; why is open.
- **A monster's plain cast clip.** Retail stages a monster cast's clip as the
  tag-`0x23` archive entry its pick walked to (`FUN_801E9FD4`,
  `sb s2,0x1e0(s4)` at `0x801EA540`), not an entry tagged with the spell id.
  The engine had searched for the latter, found none for Gimard's Tail Fire,
  and left `0x29` for the Done band without the `0x2A` / `0x2B` chain; it now
  walks the same entries ([monster animation](../formats/monster-animation.md#action-tags-and-the-0x1ef-reaction-map)).
- **A capture taken on the killing blow.** A party action in flight whose
  target already reads `0` HP is the swing that killed it, and a replay
  seeded with that HP ends the fight at the first `0x5A` wipe gate, before
  the swing starts. When no seed reaches such a capture with the HP as read,
  the search runs again with each such victim at `1` HP
  (`RetailBattle::action_victims`), so the replayed swing makes the kill; the
  victim's HP is then read at the phase, not at the prompt.
  The other members then commit Spirit rather than Attack
  (`BattleDrive::Action`'s `spare`): the victim at `1` HP dies to any swing,
  so a plain Attack ahead of the seat in initiative order made the kill on
  the wrong seat.
- **An absorbed Seru.** A capture on the Done band's multi-cast continuation
  `0x52` carries the Seru the killing blow absorbed in `ctx[+0x269]`, and the
  grant before it already prepended spell `seru + 0x80` to the acting
  character's list - so the lifted save knows the spell, and the replayed
  kill's absorb lookup answered "known" and staged nothing, leaving `0x51`
  for `0x5A`. The drive takes the spell back off that list on its first
  battle tick (`absorbed`), the twin of crediting a cast's MP back.
- **Past the end signal.** Once the `0x5A` gate raises `DAT_8007BD71 = 0xFE`
  the action SM is no longer stepped: `ctx[+0x07]` reads `0x5A` and
  `ctx[+0x13]` the pose actor through the whole results sequence, so the
  state names a span of several hundred vsyncs. Such a capture is placed by
  the sequencer's own words instead (`SpanGate`): the phase word
  `_DAT_8007BD2C`, the phase halfword `ctx[+0x6CE]` and the results hold
  `gp+0xA54`, against `World::battle.victory` on the same pose actor
  ([battle](../subsystems/battle.md#battle-end-retails-way---the-results-sequencer)).
  The Done band's continuation `0x52` is a span of the same kind - it holds
  for its countdown `ctx[+0x6D8]`, `0xB4` frames after an absorb - so a `0x52`
  capture also waits for the engine's countdown to run down to retail's.
- **A monster's capture-class cast mid-load.** The capture band's `0x6E`
  and `0x6F` wait on the disc - `0x6E` on the CD-ready poll
  `FUN_8003DE7C(1)` (`0x801E4F08`), `0x6F` on `FUN_8003F2B8(1)`
  (`0x801E5024`) while the cast module streams in - and the engine's polls
  are always ready, so it crossed both in a tick each and a `0x6F` / `0x70`
  capture was sampled a few ticks into the case-6 glide onto the caster.
  Retail's own words say how long the reads took: every frame of either
  state calls `FUN_801D5854(ctx[+0x13], 6)`, whose prologue adds
  `8 * frame_step` to `ctx[+0x87C]`, and `0x6F` ramps `ctx[+0x6D0]` down by
  `16 * frame_step`, a word `0x70` leaves alone. The drive holds the
  engine's polls busy (`SpanGate::CaptureFade`, `BattleDrive::steer`) until
  its depth has come down to retail's and its accumulator has run as far
  through `0x6E` as retail's, less what the `0x6F` frames still to come add.
  Held so, the engine lands where every such capture is framed - pitch `0`,
  `TR y = 0x500`, focus on the caster's seat. What a `0x70` capture still
  reads is the cast module's own shot (Cort's Ultra Charge pulls out to
  `TR (0, 3072, 7315)`), which the capture-class modules arm and the port
  does not model.
- **How far into the state.** Every other action-SM state spans frames too,
  and the drive reaches each on its first tick while a retail capture sits
  wherever the save was made - a monster's approach `0x19` or a Spirit band
  `0x47` scored the glide onto the actor rather than its framing. The
  close-up accumulator `ctx[+0x87C]` places the capture: the acting actor's
  clip commit zeroes it and every framing call adds `8 * frame_step`. The
  drive takes the first tick in the state whose engine accumulator has run
  as far (`SpanGate::Age`); a state the engine leaves sooner is re-run on
  the same stream and sampled on its last tick (`EngineBattle::age_short`).
  While the engine holds the capture's state, the acting action's framing
  style `ctx[+0xD]` - a draw the action seed rolls, which the post-strike
  cases fork on - is set to retail's, the camera twin of the orbit-yaw
  alignment. Not inside the capture band: `0x70` pins the style to `1`
  without re-arming a framing, so the camera a band capture shows was placed
  under the rolled style. The yaw counter `ctx[+0x6DA]` carries a draw too:
  a party attacker's first swing-clip commit re-seeds it to
  `(rand() % 2) * 0x800 + 0x280` (`FUN_8004E13C`), and cases 6 to 8 film
  from the side that coin picks. From the seat's seed pass to the capture's
  state the drive keeps the engine's counter on retail's half-turn
  (`BattleCamera::align_action_yaw_half`), leaving the drift to the engine.
- **Ahead of the seed pass.** `0x00`, `0x0A` and `0x0B` run before the seed
  pass copies the next actor into `ctx[+0x13]` from `ctx[+0x274]`
  (`0x801E2C50..0x801E2C5C`), so a capture there reads the previous actor -
  at a round's start, the last ring member - in `ctx[+0x13]`. The capture's
  seat is `ctx[+0x274]` instead. `0x0A` waits on the CD
  (`FUN_8003F2B8(1)`) for as long as the actor's data takes, which the
  engine's always-ready poll does not, and a capture whose tween table
  (`ctx[+0x118C]`) reads every stepped component at its endpoint has sat
  there long enough for the far framing to land: the drive holds the
  engine's wait until its own glide lands (`SpanGate::Landed`), and the yaw -
  which case 9 passes through and nothing in `0x0A` writes - is aligned as
  an orbit clock. `evil_medallion_rage_battle` is one.
- **Inside a module's run.** A `0x70` capture of a module whose countdown
  the engine directs carries the module arm `ctx[+0x279]` and the countdown
  word ([`capture_countdown_va`](../subsystems/cast-module.md#a-capture-class-module-owns-the-camera-in-0x70));
  the phase is held until the engine's module sits in that arm with its word
  run down as far, which places the module's shot in flight.
- **The killing blow's body.** On a killing-blow capture whose target is a
  victim, the phase is held only with the acting seat on that target
  (`ActionSteer::target`). Seeded at its read HP of `0`, the victim is no
  target at all, the auto-target picks the next standing monster, and the
  run held the right state against the wrong body; the search moves on to
  the `1`-HP re-run that makes the kill.
- **Seat and timing.** A pick no seed reproduces (a monster's plain strike on
  a given seat) can run out of budget or end the fight first.

### Replayed casts

A capture taken inside the summon band - a party seat
(`ctx[+0x13]`) on action-SM state `0x32..=0x36` with a spell id queued at
`+0x1DF`, or on the Done band it hands on to (`0x37` / `0x38`, `0x50..=0x52`)
with its category `2` still committed - is replayed rather than parked. The seed hands the engine that cast
(`World::battle.inflight_seed`, target byte `+0x1DD`), dispatched the moment
the first command prompt opens, and the capture's MP charge is credited back so
the band's own debit lands on the captured figure. The seed also carries every
combatant's live `+0x34` / `+0x38` pair (`InflightCastSeed::ground`, the
`;x:z,...` tail of `LEGAIA_BATTLE_INFLIGHT`), applied at the dispatch: retail
walks nobody home after an action
([battle-action.md](../subsystems/battle-action.md#where-an-action-leaves-its-combatants)),
so a mid-fight caster stands wherever earlier actions left it, and the cast
close-up frames that ground - a fresh entry's authored seats frame it
elsewhere. Nothing in the summon band itself moves the caster. The session then runs to
the capture's **phase**, not a fixed settle: the same action-SM state and,
while the band's full-screen flash is up, the same flash the same number of
vsyncs in. Retail's flash is a SCUS fade actor (tick word `FUN_80025000` at
`+0x0C`, not done, kind `1`, id `1`), told apart from a creature's own fades by
its per-frame delta; its `+0x7C` block's countdowns give its age
(`delay0 - delay` in the start delay, `delay0 + duration0 - duration - 1`
once landed - the landing frame steps both). `FadeState::age_vsyncs` is the
engine's twin. `phase` then also compares the action-SM state, and `play-window`
captures on the same predicate (`LEGAIA_BATTLE_INFLIGHT`,
`LEGAIA_CAPTURE_GATE`), stopping its tick loop the frame it holds. A
`0x33` capture with no flash yet is gated on how long the caster's invoke
clip has run instead: the clip's commit zeroes the close-up accumulator
`ctx[+0x87C]`, which then gains `8` a vsync, so the engine frame is the
first one where its caster has committed clip `9` and the engine's own
accumulator (`BattleCamera::close_up_accum`) has reached retail's (the
`a<acc>` suffix of `LEGAIA_CAPTURE_GATE`). The framing prologue adds its `8`
on the commit frame itself, so the accumulator reads `8 * (frames since the
commit + 1)`; counting the frames instead is one vsync late, which on a
capture taken on the band's last `0x33` frame is a gate never met. `0x33` itself runs until the clip's first effect
record fires, so the state alone placed the frame at the band's first
vsync, before the commit. A capture inside PROT 0903's walk arm (`11`) is
placed by the yaw base `ctx[+0x6DA]` the arm swings `6 * scalar` a vsync
from `0x200` (the `y<yaw>` suffix), since the phase byte only names the
arm's entry; an engine walk that arrives sooner leaves the arm first, and
its exit is then the frame taken. The headless seed seats no creature, so
its walk arm passes at once.

The frame and the RAM are not the same instant. Retail double-buffers its
packet pools, so while the CPU builds frame `N` the display scans out
`N - 2`: of the flash's two full-screen `POLY_F4` packets, one carries the
block's value and the other the value one step earlier, and the displayed
frame is one step older than that. A step is the adaptive frame step
`*(0x1F800393)` - `2` or `3` vsyncs through the band, rebuilt from the
frame-duration history at `0x80084098` since the scratchpad is not in a
main-RAM image - so the flash on screen is `2 * step` vsyncs younger than
the block says (a block at `178` shows `123`). The RAM channels are sampled
on the block's age; the image gate takes the lag off
(`RetailBattle::display_phase_gate`). Scored the other way, a flash-in
ramping by `12.75` a vsync reads as a white-out the retail frame never
shows.

Before this, a mid-cast frame was scored against the round prompt, which reads
as `image` near `0` on a white-out: an instrument artifact, not an engine
verdict. The like-for-like frame is what exposed the cast close-up: through
`0x33` / `0x34` retail frames the caster from a low camera pitched up at it
(`FUN_801DC0A0` case `0x12`, `battle_cam_script::summon_cast_framing`), so the
upper, darker half of the stage backdrop fills the frame - there is no
separate darkening pass. What still differs is the engine's: the caster's name
plate draws over retail's flash where the engine's flash covers it, the engine
captions the spell name over the caster, and from `0x35` the creature stager's
own camera is not modelled.

That last gap is the per-summon module's, not the band's. In `0x35` / `0x36`
the camera belongs to the slot-B module, which arms its own framings on the
**creature** (actor slot 7) and paces its arms on a countdown of its own
([`cast-module.md`](../subsystems/cast-module.md#the-module-owns-the-camera-and-the-bands-length)).
Where the engine ports a module's pacing director (PROT 0903, 0905, 0908)
the band's length is the module's, so a `0x35` / `0x36` capture of that module is gated on
the module's phase byte `ctx[+0x279]` as well (`PhaseGate::module_phase`, the
`m<phase>` suffix of `LEGAIA_CAPTURE_GATE`). A walk arm is gated on its entry,
not on how far the creature has walked, so a capture mid-walk reads its
`camera` against the framing the walk starts from. A camera-only director
(the summon creatures) does not move the band's length, so its captures stay
gated on the flash alone. A module with no director keeps case 6 on the
caster through `0x35` / `0x36`, and its capture's `camera` reads that gap -
PROT 0913 (Nova), whose opening arms wait on the creature's CD read, is the
one `0x35` capture of that kind left. Where the creature stands is the formation's: a fight whose
engine seats differ from retail's reads the difference in the creature focus
too, since the module places the creature relative to caster and victim.

**The idle orbit.** The orbit's
yaw is a clock (`-4` per camera step from whatever azimuth the field left), so
an unaligned prompt sample reads the capture instant. When retail's command-flow
byte is one the battle tick's orbit runs on (`0x1E` / `0x32` / `0x6E` / `0xFE`,
`FUN_801D0748` at `0x801D0784..0x801D07A4`) both sides align it: the headless
seed sets its orbit to retail's yaw before it samples `camera`, and the image
child gets the yaw as `LEGAIA_BATTLE_ORBIT_YAW` and holds its orbit there, each
only while the orbit owns the yaw (`BattleCamera::align_orbit_yaw`) - the
camera twin of the field HUD countdown hold. Unaligned, an orbit sample
hundreds of units off put the formation on the other side of the frame and
scored the clock rather than the scene. A party whose
present list names a seat the save window's roster does not seat (a guest
combatant) reads as a short engine party in `battle_party`.

**The image** comes from `play-window --resume-save <lifted save> --battle <row>
--party <ids>` with the retail stage variant - the card-load resume the headless
side takes, so the frame's party is retail's (levels, equipment and the battle
meshes assembled from it, the HP / MP the HUD prints) rather than the New Game
template a bare door entry seeds - captured a fixed number of ticks past the
fight's first prompt - the evolved-Cort arrival (PROT 0968) holds the prompt
back about a thousand frames longer than an ordinary opening, and the headless
side's opening window runs long enough to wait it out. A fight with
no MAN row to name is not imaged; its reason is in the report.

### The track word in battle

Retail's track-select word `0x8007BAC8` keeps the **field** track through a
fight: every catalogued battle state holds a field or overworld id there, never
the battle theme, which is started without the op-`0x35` store. The engine
routes its battle swap through the same start event its field scripts use,
so its copy of the word reads the battle theme; the comparand is the track the
engine stashed to resume (`World::audio.field_bgm_resume`), or its word itself
when the fight took no swap (a battle sound set of `-1`).

The word is **script progress**: a scripted boss's event starts its theme
with op `0x35` sub-op `9` just before the fight and selects the battle sound
set with sub-op `7` (`korb3`'s Gaza record starts `2028` and selects `-1`;
`jouine`'s Cort record `P2[5]` starts `2071` and selects `8`), and the retail
word holds that theme. The seed enters the scene fresh and forces the
formation without running the event, so it replays the record's words
instead: the last start before the record's `3E FF <row>`, the control words
after it, and the last sound-set selection
(`World::replay_scripted_battle_score`, read off the MAN by
`man_field_scripts::walk_battle_entry_scores`; `play-window --battle` replays
the same words). Each goes through the field VM's own op-`0x35` handler.

What the replay cannot reach is a word an *earlier* beat chose. `nilboa`'s
entry picks its track by which duel-return marker is up (`0x477` /
`0x478` -> `4096`, `0x479` -> `2028`, `0x47A` -> stop), clears it and spawns
the post-duel record; each duel record raises its own marker immediately
before its `3E FF`. A battle capture therefore holds the marker of the fight
in progress, and the seed's entry consumes it as if returning from that
fight: `nivora_duel_mid_blazing_slash` (Gi duel, row `31`, marker `0x479`)
reads `2028` where retail's word is the `4096` an earlier return parked.
The duel record itself starts no track (it selects sound set `4`, which the
replay does carry), so there is no start to replay. See
[audio](../subsystems/audio.md#the-battle-sound-set-picks-the-fights-track).

## Menu states

A menu-class capture (mode `0x17`) names its screen in the menu overlay's
sub-screen word `DAT_801E46A4` ([save-screen](../subsystems/save-screen.md#sub-screen-function-pointer-table)).
The pause menu's screens are the root rows' routes - Items `0x05`, Magic
`0x0E`, Equip `0x12`, Status `0x15`, Options `0x17`, Load `0x18`, Save
`0x19` - plus the Equip row's two later steps, the slot browse `0x13` and the
candidate list `0x14` (`0x12` itself is Equip's character picker). Those are
seeded: the field seed runs (card-load resume, seat, settle), then the pause
menu is driven through its own pad path - `Start`, `Down` onto the row,
`Cross`; one more `Cross` past the character picker for `0x13`, and for
`0x14` a `Down` past Best Equipment and a `Cross` into the first slot's
candidates, one edge every `MENU_PRESS_GAP` ticks. The headless side
presses them into `BootSession`'s menu; the image side hands the same edges to
`play-window` as a `--pad-script` after the card-load resume.

The `menu` channel is 1 when the engine's menu holds the same sub-screen
(`0x01` on the root list, the open row's id; the engine's Equip screen reads
as `0x12` in its character picker, `0x13` on its slot list and `0x14` on its
candidates). `mode` wants the engine in `Menu`; the field channels keep their
meaning, since the menu opens over the seated field.

A capture with the word clear is the title / boot family (the attract loop,
the title picker, the card-boot save select) and a screen no root row routes
to is script-entered (the casino prize exchange, `0x20`); both are kept with
a `menu not seedable:` reason and counted as classified limits.

What a like-for-like menu frame shows is the engine's. The options screen
carries the port's extra Key Config row. The Status and Equip character lists are the present
party (`DAT_80084594` over `0x80084598`,
`field_menu_dispatch::status_snapshots` and `EquipScreenModel::party_row`), not
every roster record - the New Game template seeds all four records, so a
roster walk listed Noa, Gala and Terra beside a Vahn still travelling alone.
Each row's name is the record's own `+0x2A7` display name
(`field_menu_dispatch::roster_names`), so a save with a renamed hero shows
that name, as retail does.

## Channels

Each channel scores in `[0, 1]`; a state's score is the mean of its
measured channels.

| Channel | Score |
|---|---|
| `scene` | 1 when the engine landed in retail's scene |
| `mode` | 1 when the engine's mode is `Field` (field class) / `WorldMap` (overworld class) / `Battle` (battle class) |
| `position` | player `(X, Z)` after settling: 1 within 4 units, linear to 0 at 256 |
| `footing` | engine floor sample at retail's `(X, Z)` vs retail's footing: 1 within 2, 0 at 128 (field class only; not scored while a script holds retail's player height, [below](#a-script-held-height-is-not-a-footing)) |
| `camera` | mean of eight parts: pitch and yaw (1 within 16, 0 at 256, wrapped), `H` (1 within 4, 0 at 128), each eye word and each focus word (1 within 16, 0 at 1024) |
| `bgm` | 1 when the engine's track-select word (`SceneHost::bgm_track_word`, the park sentinel `0x1000` included) equals retail's; the detail marks a held track on either side ([below](#a-held-track)) |
| `fog_gate` | 1 when the engine's fog-pool gate equals retail's |
| `party` | fraction of equal fields over retail's roster: HP / MP current and max, level, the eight equipment bytes |
| `flags` | 1 - differing bits / bits set on either side, over the whole story-flag bitmap |
| `inventory` | fraction of non-empty bag slots equal, slot for slot, plus gold |
| `enemies` / `enemy_hp` / `battle_party` / `phase` | battle states only ([above](#battle-states)) |
| `menu` | menu states only: 1 when the engine's pause menu holds retail's sub-screen ([above](#menu-states)) |
| `image` | fraction of `8 x 8` blocks within tolerance ([below](#the-image-channel)) |

`party` and `inventory` are seeded straight from the retail window, so on
their own they check the seeding and whatever the settle window changes (an
entry script that grants or takes an item, a heal). `flags` does the same
for the flag bank: its misses are bits the engine's entry scripts wrote
during the settle window, or bits the save round trip does not carry.

## The image channel

Retail's drawing area is `320 x 224` at draw offset `(0, 4)` / `(0, 244)`,
and the display scans out from the same origin
([renderer](../subsystems/renderer.md#the-screen-the-gte-projects-onto-is-320x224-not-320x240)),
so the compared frame is the top `224` rows of the display crop - the rows
below are never drawn. The port keeps a `320 x 240` logical screen whose row
`y` is retail's draw row `y` (the `OFY = 114` bias puts the GTE origin on the
same row in both), rendered at an integer scale, so engine row `s * y` is
retail row `y`: the capture's top `224 * s` rows are box-filtered down by
`s`, and the port's bottom `16` logical rows are left out.

Two numbers per state:

- `mae` - mean absolute error per channel per pixel, 8-bit units;
- `within` - the fraction of `8 x 8` blocks whose mean colour is within
  `24` of retail's on every channel. This is the score.

Block means rather than pixels: the engine rasterises at its own resolution
with its own filtering, so a per-pixel test would mostly score dither,
filtering and sub-pixel edges. A block test moves on wrong camera, missing
or extra geometry, wrong CLUTs and wrong overlays, and not on rasteriser
noise. A retail frame whose mean luma is below `8` - a fade - is not scored:
a dark engine frame would match it and say nothing about the scene.

The engine frame comes from `play-window` run as a child in a scratch
directory whose options file pins `camera_distance = "retail"`. The window's
interactive default frames the field further out than retail, and a frame
compared at that distance scores the zoom rather than the scene.

A `--screenshot` child is tick-locked - exactly one world tick per redraw,
whatever the wall clock did - because the draw pass feeds the simulation (the
fog step above). A wall-paced capture ran fogged scenes on a stream that
moved with machine load, which is what made image runs load-sensitive.

The report writes `retail | engine | |diff|` side by side for every scored
state. **Those PNGs are retail pixels**; the report directory must stay
gitignored (`captures/` is).

## The ratchet

`scripts/ci/retail-compare-baseline.json` holds, per state label, each
measured channel's score, plus every state's class. The test
`retail_compare_corpus` re-runs the corpus and fails when any state's
channel falls more than `0.0005` (the JSON round-trip slack) below its
baselined score, when a baselined channel of a state the run did seed goes
unmeasured, when a seedable state fails to seed, or when no state is seeded
at all. A rise is allowed and is folded in
by a reviewed `--bless`. A bless merges into the existing file: a state
outside a `--filter`, or one the run could not seed, keeps its baselined
channels, and a run without a display keeps each state's baselined image
score. A state the run did seed otherwise takes the run's channel set, so a
channel that no longer applies to it (a field state re-classed `world_map`
has no `footing`) leaves the file rather than failing every later check as
`not measured`. A baselined state missing from the local library is
skipped (backups are per-machine), and the image channel is skipped unless
the run renders frames (`LEGAIA_RETAIL_COMPARE_IMAGES=1`, which needs a
display).

The test is disc-gated (`LEGAIA_DISC_BIN`) and finds the library and the
extracted disc through `LEGAIA_SAVES_LIBRARY` / `LEGAIA_EXTRACTED_DIR`
before the repo-relative defaults - in a git worktree the data lives in the
main checkout.

## Running it

```bash
scripts/ci/retail-compare.py                  # state channels, report under captures/retail-compare/
scripts/ci/retail-compare.py --images         # + the image channel (needs a display)
scripts/ci/retail-compare.py --images --check # assert the baseline
scripts/ci/retail-compare.py --images --bless # fold a reviewed rise into the baseline
scripts/ci/retail-compare.py --filter town01  # only matching labels (a,b = either)

LEGAIA_SAVES_LIBRARY=... LEGAIA_EXTRACTED_DIR=... \
  cargo test -p legaia-engine-shell --profile release-test --test retail_compare_corpus -- --nocapture
```

The driver builds `legaia-engine` under the `release-test` profile first
(`--no-build` skips that), writes the report to `--out` (default
`captures/retail-compare/`), and exits 0 with a `[skip]` line when the
library or the extracted disc is missing; it looks for both in the worktree
and then in the main checkout.

The subcommand is `legaia-engine retail-compare`, which takes `--library`,
`--extracted-root`, `--manifest`, `--out`, `--images`, `--filter`,
`--flags-first`, `--write-baseline` (the driver's `--bless`) and
`--check-baseline` (the driver's `--check`).

## Reading the report

`report.md` opens with the channel means over the seeded states and the
class counts, then lists every seeded state **worst first**, each channel
with both sides' values and, when the frame ran, the side-by-side image.
The unseeded states close it, each with its reason. `report.json` carries
the same rows for scripts.

The worst states are the product. For each, decide first whether the
divergence is the instrument's - a [seeding gap](#the-seeding-model) - or
the engine's: a camera channel at 1 with a bad image is a rendering
question; a camera channel far off on a state whose label names a cutscene
is script progress; a `bgm` miss on an arrival state is capture timing
([below](#arrival-states-are-captured-before-the-town-runs)).

## Divergence shapes

Shapes the corpus separates, each with what it indicates:

| Shape | Indicates |
|---|---|
| camera exact, frame smeared by stretched texture planes or one flat colour | a mesh retail does not draw there: a sky dome's back faces ([NCLIP](../subsystems/renderer.md#the-field-pass-culls-back-faces)), a never-spawned actor slot at the origin, a prop at the wrong [render scale](../subsystems/renderer.md#field-static-object-placement-town01), or a window-owned prop outside the region box |
| retail frame black beyond a rectangle, the engine's filled | the visible-tile window (op `0x46`); the port draws the whole scene |
| an idle status panel in the engine frame only | the engine's panel placement, or its suppress / rearm gates, against retail's - the countdown's phase is aligned |
| effect missing in the engine frame (save-point crystals, spell glows) | an actor or effect the fresh entry does not spawn, or one the port does not draw |
| camera and dialogue off together, `script` detail says the gate was not met | a record the phase gate could not bring the engine to ([above](#mid-script-states)) |
| flags `+sys` bits only the engine has, on a gated state | a placement the record poked ran its talk body in the engine ([above](#mid-script-states)) |
| flags `+sys` / `-sys` one bit apart inside `0x19B..0x1AA` | the entry script's one-hot region selector, re-evaluated at the seat ([below](#the-region-selector-band-and-the-entry-order)) |
| `fog_gate` and flag `0x01F` up in the engine only (`rikuroa_post_genesis_tree`) | script progress a card load undoes ([below](#a-flag-the-entry-raises-on-every-load)) |
| player seated exactly, camera focus thousands of units away (`kor5_post_43a_checkpoint`: player Z `5312`, focus Z `11840`) | script progress: a scene script aimed the retail camera at another part of the map; the engine's follow camera frames the player |
| a town label over the overworld's `H`, word `2000` and fog gate | a door caught before the town's field init ran; scored as the overworld `0x80084540` names ([below](#arrival-states-are-captured-before-the-town-runs)) |
| retail word held by a flag the entry script already consumed (`garmel`'s `0x196`, `rikuroa`'s `0x289`) | script progress: the track was started by a beat that has since cleared its trigger flag, so a card load would not restart it |
| camera depth and position off on an ending vignette (`ending_vignette_rimelm_walkaway`) | a residue of about a dozen frames of the credits walk against the camera glide ([below](#ending-vignettes-are-mid-script)) |
| camera exact, frame aimed at another part of the room; retail focus `0x80089118/20` is not `-player` | a probe-poked capture ([below](#a-poked-player-keeps-the-arrival-focus)) |

### A script-held height is not a footing

Op `0x4C` nibble-4 sub-2 ramps an actor's `+0x8E` and, while the actor's
`+0x10 & 0x20000000` is up, writes `world_y = -value` over whatever the floor
says. `cort_evolved_approach_cutscene` is caught inside `jouind P2[4]` after
`CC F8 42 5D 02 0A 00` lifted Vahn to `Y = -605`, on tiles whose floor tier is
`0` - the floor under him is `0` in both games. The corpus reads the bit and
`+0x8E` off the state and leaves `footing` unscored for such a state, with
the held height in the detail, because the channel measures the floor model
and retail's `Y` there is not a floor reading.

### The region selector band and the entry order

A field entry script typically carries a one-hot **region selector** in
`0x19B..0x1AA`: its init clears the band, and each pass of its per-frame body
walks op `0x42` mode 0 over the region-type mask `_DAT_8007B8F4` and, on a
region whose bit is not yet the selected one, clears the band, sets that
region's bit and re-applies its view window (op `0x46`) and camera region.
The bit therefore records where the player stood the last time the system
loop ran a pass - and the system SM `FUN_801DA51C` runs no pass while a
record holds the player ([script-vm](../subsystems/script-vm.md#engagement-and-the-system-script)).

`cort_evolved_approach_cutscene` holds `0x19F` while `jouind P2[4]` has Vahn
on region type 3, where the body would select `0x19E`: retail chose `0x19F` at
the arrival tile and has sat out every pass since. The engine's seed enters,
hydrates and seats before its first tick, and in a scene the engine does not
pre-run (every scene but the three opening legs) the entry script's install
slice runs on that first tick - after the seat - so the band is cleared and
re-selected at the seat. Retail runs the install slice in the load frame
(`FUN_8003AB2C`, to the first executed `0x21`). Moving every scene's install
slice into the load frame closes this bit but runs the entry's BGM cue and fog
op before the seed's flags exist, and the corpus `bgm` channel fell by a
fifth (by a tenth even with `--flags-first`), so the order stands and the
bit is a seeding limit. The opening legs are pre-run already, and there the run
stops where retail's does (`World::pre_run_entry_script`), so the two
`opdeene` states carry retail's selector through the seed.

### A flag the entry raises on every load

`rikuroa`'s entry script raises the fog gate and sets flag `0x01F` on every
load (`4C 30`, `50 1F` at `P1[0]` `+0x47`); the Genesis Tree beat `P2[54]`
lowers both (`4C 31`, `60 1F` at `+0x1FC`). `rikuroa_post_genesis_tree` was
taken after that beat in the same visit, so retail holds both down, and no
record is running. A card load of that save runs the entry script again and
raises both, which is what the engine's seed does: the divergence is history
the save does not carry.

### A poked player keeps the arrival focus

`retock_innkeeper_talk_open` and `retock_inn_stay_prompt` were captured by
warping into the inn at `(15168, 1280)` and then **poking** the player to
`(14816, 1728)` (`LEGAIA_POKE_POS`). The states hold the player's position
and its previous-position pair `+0x1C` / `+0x20` equal, so the ease's
stationary test (`0x801DB578..0x801DB5A4` in `FUN_801DB510`) sees no move and
the focus-writing legs never run: the focus stays at the arrival tile
(`-15168`, `-1280` stored; the X is a tile centre, the Z the edge clamp) while
the player stands elsewhere. Every retail writer of the focus pair takes the
player as its anchor, so no script or region record accounts for the offset.
The engine seats the player and frames on the seat, so its angles, `H` and eye
trio match while the frame looks at a different part of the room; only the
`camera` channel's focus part misses. Seated at the
arrival point instead, the engine frames the counter, the walkway and the void
below it the way retail's frame does. The `image` miss on these two states is
the capture method, not the camera.

### Arrival states are captured before the town runs

`s2_rimelm_town01`, `doman_arrival_from_korb2` and `son_arrival_from_doman`
carry the town's scene label and field mode, but every other observable is
still the overworld's: GTE `H = 368` (the value every `world_map`-class state
holds, and no other `field`-class state), BGM word `2000`, the fog gate raised,
and an overworld footing. `son`'s scripts write no fog gate at all, and
`FUN_8003AEB0` clears the gate on every scene load that is not a warp return
(`sw zero,-0x47ac(v0)` at `0x8003B690`, behind the `_DAT_8007B8B8 != 2` test
at `0x8003B510`), so a `son` whose loader had run could not hold it raised.
The loaded-scene define settles it: `0x80084540` still reads `map01` (`85`)
or `map03` (`391`) in all three. The corpus therefore scores them as the
overworld ([above](#the-corpus)), where the word, the gate and the camera
agree; scored under the label, every one of those channels had compared the
overworld against the town.

One bit still differs on `s2_rimelm_town01`: the engine holds flag `0x528`,
retail none of `0x526..0x531`. That band is the entry script's own: each
pass of `map01`'s per-frame body clears `0x527..0x52E` and sets `0x528` while
the player stands in the map, so the engine's freshly entered `map01` raises
it, while retail's state is a door already under way with the band down. It
is the same capture timing as the word and the gate, on the flag bank.

### A held track

The word says which track a script selected, not whether it sounds. Retail's
field slot `0x8007052C` is attached and audible only while the playing word
`0x8007B708` is raised (the replay primitive `FUN_80026478` sets it; the stop,
pause and timed-release arms clear it) and the slot's `SsSeqSetVol` word
`+0x6` is non-zero (`FUN_8002657C`, zeroed by the same arms). The engine's
side is its director's last control op after its last start. The `bgm`
detail marks either side `(held)`; the score stays on the word, because
every held-versus-sounding split in the corpus is script progress - a
timeline pause (`rim_elm_zoom_intro`, `rikuroa_pre_caruban`), a minigame's
own stop (`minigame_dance_pcsx`), a swap in flight - which the seed cannot
resume.

A swap in flight is the shape the word misreads most. Op `0x35` sub-op `9`
stores the new id at once, but the old track keeps the slot until the script's
sub-op `0xA` commit ([audio](../subsystems/audio.md#the-track-swap-handshake-fun_800243f0--op-0x35-sub-op-0xa)):
`chapter2_garmel_pre_songi` and `_pre_zeto` hold `_DAT_8007B750 = 0x9`
(start pending, load settled), the word naming `2033` / `2052` while the
loaded barrier `0x8007BA9C` still names `2043` and the slot is silent. The
same limit covers a flag the running script wrote after the entry read it:
`koin1`'s slot-machine record (`P1[56]`) raises `0x4B8` before its warp, and
the entry parks the track (`0x1000`) when that flag is up, so a fresh entry
over `minigame_slot_machine_pcsx`'s flags parks where retail's earlier entry
started `2018`.

### Ending vignettes are mid-script

`ending_vignette_rimelm_walkaway` is `map01` inside the credits: retail reads
pitch / yaw / eye depth `416 / 75 / 10389`, the engine `370 / 0 / 6400` after
the settle. The engine's camera is not wrong, it is early. The seed enters
`map01` and seats the player, and the credits script starts over: one tick in
it moves Vahn to the start of the path and snaps the eye depth to `6400`
(op `0x45`, slot 5 alone); about sixty ticks later a glide beat pulls the
camera back and round (slots 0, 1, 3..8). Traced per tick, the engine's
globals reach `416 / 77 / 10477` near tick 190 of that walk - within two
angle units and a hundred depth units of the retail state.

Retail is parked on the record's `B8 F8 82 08` rotate at `+0x97` with the
player still walking the compass leg `C1 F8 03 C4` at `+0x93` (the player's
`+0x94`), and the camera mover 123 of 780 frames into its glide. That is
the compass-walk arm's own shape: it seats the leg on the player and returns
past the op, and the `B8 F8` waits in the dispatcher prologue until the leg
lands. The `45 0B` glide after `+0x6E` likewise starts with the `+0x6E` leg,
not after it. The engine plays the walk the same way, and the gated state
lands within a few dozen units of retail's player; what remains is about a
dozen frames between the walk and the glide (retail's glide is 123 frames
in after 111 walked units, the engine's after 123).

### The dance-hall state is inside the contest-entry cutscene

`minigame_dance_pcsx` is `koin3` with the qualifier request (story flag
`0x134`) raised, captured on the frame the hall starts loading the dance. Its
NPCs stand where `koin3`'s partition-2 record 6 walks them: a run of
cross-context `4C 51` NPC-run ops (from MAN offset `0x3ACF`) moves the player
to tile `(46, 101)`, Gala to `(49, 101)`, the Disco King to `(6144, 12672)` and
Mary to `(5760, 13120)` - every one of those positions is the retail
actor's own - and four of the floor dancers the headers place are gone from
the state's actor list. The engine seeds a fresh
scene entry, so it draws the placement headers (the Disco King front and
centre at `(6144, 13248)`, the four dancers present) and the frame reads as
"dancers placed wrong". It is script progress, not placement; the retail
frame is also partway into the load fade.

The settled engine frame also holds the record's dialog box over a scene the
screen push has darkened, where retail's frame is lit and boxless. The engine
draws that push under the box, the order retail's ordering table gives a
kind-2/8 push (`FUN_80024EE4` links at bucket `a0`, the MES glyphs at bucket
1 - [cutscene](../subsystems/cutscene.md)); an engine that washed every push
over its text drew a near-black frame here, which the block metric scored
above the box. The gap is the script's timing, not the wash.

The phase gate resumes record 6 and replays that staging, which puts the
camera exactly on retail's shot and the player on retail's `(5952, 12992)`.
An engine that stepped the placements the record pokes left the player at
`(6208, 13120)`; no poke engages a placement, so none runs.

### An attack-face capture frames the shot the state before it armed

The three `super_queue_replace_*` states are one base capture with a
RAM-injected action queue, so they carry the same camera words: `ctx[7]` is
`0x14`, `ctx[+0xD] = 3`, `DAT_8007BD71 = 0xFF`, eye `(0, 972, 2867)` at pitch
`0x80`. State `0x14` lasts one frame on both sides: its body (`0x801E305C`)
calls `FUN_801D5854(seat, 6)` and leaves for `0x19` or `0x1E` before it
returns. So the shot on screen is the one the `0x0C` seed armed, and both
sides take case 6's in-fight arm. The engine's framing is not the gap. The gap
is where the glide starts from:

- Retail's depth `2867` is `prescale(0x700)`, exactly one sixth of the way
  from the arts-entry close-up `prescale(0x600)` toward case 6's
  `prescale(0xC00)`. The acting member came straight out of its own arts
  entry. In the engine's round the monster acts first, and the pad drive takes
  Auto at the attack-mode prompt, so the engine's glide starts from the
  monster's end-of-action shot.
- The captured yaw `3205` is mid-glide, not a framing. The capture's tween
  table (`ctx[+0x118C]`) holds the endpoint `4062` = `0x800 + 0x800 -
  actor[+0x46]`: the seed pass's `ctx[+0x6DA] = 0x800` with style `3`'s
  half-turn, armed two frames before the save, which the counter's `0x200`
  (the Attack branch's store) has not yet re-armed.

Pitch `0x80` already reached and `TR.y = 972` do not fit a single glide step,
so the capture's own glide history is not fully explained either.

## See also

- [recomp-differential](recomp-differential.md) - the frame-tagged
  differential against the static recomp.
- [host-drift](host-drift.md) - the three-host parity gates.
- [mednafen-automation](mednafen-automation.md) and
  [pcsx-redux-automation](pcsx-redux-automation.md) - how the library states
  are captured and catalogued.
