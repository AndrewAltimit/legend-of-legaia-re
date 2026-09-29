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
- [Battle states](#battle-states)
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
| `menu` | mode `0x17` (title, save screens, the pause menu) | no |
| `minigame` | mode `0x19` | no |
| `cutscene` | mode `0x1A` / `0x1B` (STR playback) | no |
| `other` | anything else | no |

Unseeded states stay in the report with their reason, so the summary counts
what the instrument cannot reach instead of dropping it. A minigame the
retail state shows *inside* a field-run frame (the casino floor, the dance
hall before the song) is a `field` state and is scored as the field it is.

## Retail observables

| Observable | Address | Notes |
|---|---|---|
| scene label | `0x8007050C` | CDNAME label, 8 bytes |
| game mode | `0x8007B83C` | the next-mode word the dispatcher reads |
| player `(X, footing, Z)` | `*0x8007C364 + 0x14/0x16/0x18` | `i16`s; the footing is the floor sample under the player |
| camera pitch / yaw | `0x8007B790` / `0x8007B792` | 12-bit angles |
| GTE `H` | `0x8007B6F4` | |
| camera eye | `0x800840B8/BC/C0` | the view builder's translation words |
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
`BootSession::resume_save` over the lifted save (enter the scene, then
hydrate party, flags, bag, gold). The player is then seated on retail's
`(X, Z)` with the floor-sampled `Y` (`World::debug_seat_player`, the kernel
behind `LEGAIA_SEAT`; on a field scene it is a warp landing and re-centres the
region box and the windowed static-object list on the seat), the zone camera's arrival snap is re-armed, and the
session ticks a fixed settle window with no input. BGM starts are recorded by
a director on the scene host's event route.

**The image channel** comes from `play-window`, the real renderer, run as a
child process from a scratch directory under the report, and seeded the same
way: the lifted save is written as a scratch LGSF file whose resume point is
the state's scene, and `play-window --resume-save` lands it through the same
`BootSession::resume_save` the headless side calls. `LEGAIA_SEAT=X,Z` then
seats the player on retail's position, and `--screenshot` captures at a
fixed world tick. The child also runs `--no-live-npcs`, because the headless
session leaves the free-roam liveliness approximation
(`FieldNpcState::animate`) off: with it on, every placement script
free-steps from the first tick, and a talk body among them can walk the
player off the seat (`vell`) or pull it to another placement and open its
dialogue (`koin1`).

A `--scene` door entry is **not** an equivalent seed. It stages the scene
for the free-roam picker (story-twin event flags, the entry BGM pause
dropped) and runs its arrival from the picker's seat, so an entry script
moves the player or points a dialogue shot before the seat applies - a
frame whose headless camera channel is exact would score the arrival
instead of the scene.

What the seeding does **not** carry - each is an instrument limit, not an
engine verdict:

- **Script progress.** A state captured mid-cutscene, mid-dialogue or on an
  arrival is a point inside a running script. The engine enters the scene
  fresh, so it runs the entry prologue instead of resuming the retail
  script: a scripted camera, an open dialogue box, a banner or an event
  track is not reproduced.
- **Actor state.** NPC positions, animation phases, open doors and live
  effects are whatever the engine's own entry produces.
- **Timing.** Retail's frame is one instant; the engine's is a fixed tick
  after entry. Ambient animation, fog-pool population and water CLUT phases
  cannot be phase-aligned.

`--flags-first` is a diagnostic arm for the headless side: hydrate, enter,
hydrate again, so the entry scripts see retail's flags. Comparing its report
with the default shows how much of a channel's divergence the card-load
ordering explains.

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
stats for its ids. `World::force_encounter` then arms the row through the
ordinary transition - the path `play-window --battle` takes, including the
scripted carrier's replayed tutorial arm. When the mode flips, the retail
combatants' live HP / MP are written over the engine's, the opening runs to
the first round prompt, and the session settles with no input.

A battle state whose RAM does not describe a seedable fight (the context
pointer not yet resident, counts out of range, an empty cell) is kept with a
`battle not seedable:` reason and counted as a classified limit, not as a
seed failure.

**Battle channels.**

| Channel | Score |
|---|---|
| `enemies` | fraction of retail monster seats whose id the engine seated in the same order |
| `enemy_hp` / `battle_party` | fraction of equal HP, max HP, MP and max MP fields over the retail combatants (max MP left out where the engine carries none) |
| `phase` | 1 when the engine's command-flow state equals retail's `ctx[+0x06]` decoded to the engine's band |
| `bgm` | retail's track word against the field track the engine will resume ([below](#the-track-word-in-battle)) |

`scene`, `mode` (engine `Battle`), `camera`, `flags`, `inventory` and `image`
keep their field meaning. HP / MP current values are seeded, so their misses
are what the settle window changed; the max values are the real check
(record-derived on the party, archive-derived on the monsters).

**What the seed cannot carry.** An action in flight: most battle captures are
mid-strike or mid-cast (flow `0xFF`), and the engine is compared parked on its
round prompt, so their `phase` channel reads capture timing. The idle orbit's
yaw is a clock (`-4` per camera step), so on a prompt state the yaw part of
`camera` - and most of the frame - reads the capture instant. A party whose
present list names a seat the save window's roster does not seat (a guest
combatant) reads as a short engine party in `battle_party`.

**The image** comes from `play-window --battle <row> --party <ids>` with the
retail system flags, captured at a fixed tick past the opening. A fight with
no MAN row to name is not imaged; its reason is in the report.

### The track word in battle

Retail's track-select word `0x8007BAC8` keeps the **field** track through a
fight: every catalogued battle state holds a field or overworld id there, never
the battle theme, which is started without the op-`0x35` store. The engine
routes its battle swap through the same start event its field scripts use,
so its copy of the word reads the battle theme; the comparand is the track the
engine stashed to resume (`World::audio.field_bgm_resume`), or its word itself
when the fight took no swap (a battle sound set of `-1`).

The word is **script progress**, so a forced fight often cannot match it. A
scripted boss's event starts its theme with op `0x35` sub-op `9` just before
the fight (`korb3`'s Gaza event starts `2028`, then selects sound set `-1`;
`jouine`'s Cort event starts `2071`), and the retail word holds that theme.
The seed enters the scene fresh and forces the formation without running the
event, so the engine's word is the scene *entry*'s choice - `korb3` parks at
`0x1000`, `nilboa` picks `4096` or `2028` by which duel flags are up. Those
`bgm` misses are this limit, not a stash defect; see
[audio](../subsystems/audio.md#the-battle-sound-set-picks-the-fights-track).

## Channels

Each channel scores in `[0, 1]`; a state's score is the mean of its
measured channels.

| Channel | Score |
|---|---|
| `scene` | 1 when the engine landed in retail's scene |
| `mode` | 1 when the engine's mode is `Field` (field class) / `WorldMap` (overworld class) / `Battle` (battle class) |
| `position` | player `(X, Z)` after settling: 1 within 4 units, linear to 0 at 256 |
| `footing` | engine floor sample at retail's `(X, Z)` vs retail's footing: 1 within 2, 0 at 128 (field class only) |
| `camera` | mean of six parts: pitch and yaw (1 within 16, 0 at 256, wrapped), `H` (1 within 4, 0 at 128), each eye word (1 within 16, 0 at 1024) |
| `bgm` | 1 when the engine's track-select word (`SceneHost::bgm_track_word`, the park sentinel `0x1000` included) equals retail's |
| `fog_gate` | 1 when the engine's fog-pool gate equals retail's |
| `party` | fraction of equal fields over retail's roster: HP / MP current and max, level, the eight equipment bytes |
| `flags` | 1 - differing bits / bits set on either side, over the whole story-flag bitmap |
| `inventory` | fraction of non-empty bag slots equal, slot for slot, plus gold |
| `enemies` / `enemy_hp` / `battle_party` / `phase` | battle states only ([above](#battle-states)) |
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

The image channel is sensitive to machine load: fog and ambient animation
are wall-clock paced in the window, so a run that overlaps a heavy build can
read a few hundredths low. Check the image ratchet on an idle machine.

The report writes `retail | engine | |diff|` side by side for every scored
state. **Those PNGs are retail pixels**; the report directory must stay
gitignored (`captures/` is).

## The ratchet

`scripts/ci/retail-compare-baseline.json` holds, per state label, each
measured channel's score, plus every state's class. The test
`retail_compare_corpus` re-runs the corpus and fails when any state's
channel falls below its baselined score. A rise is allowed and is folded in
by a reviewed `--bless`. A bless merges into the existing file: a state
outside a `--filter`, or the image channel on a run without a display, keeps
its baselined value rather than being dropped. A baselined state missing from the local library is
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
scripts/ci/retail-compare.py --filter town01  # only matching labels

LEGAIA_SAVES_LIBRARY=... LEGAIA_EXTRACTED_DIR=... \
  cargo test -p legaia-engine-shell --profile release-test --test retail_compare_corpus -- --nocapture
```

The subcommand is `legaia-engine retail-compare` with the same flags
(`--library`, `--extracted-root`, `--manifest`, `--out`, `--images`,
`--filter`, `--flags-first`, `--write-baseline`, `--check-baseline`).

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
| an idle status panel in the engine frame only | the engine's panel timing / placement against retail's |
| effect missing in the engine frame (save-point crystals, spell glows) | an actor or effect the fresh entry does not spawn, or one the port does not draw |
| camera and dialogue off together | script progress - the seeding cannot resume a script |
| retail BGM word `2000` on a town arrival | the state was captured before the town's field init ran ([below](#arrival-states-are-captured-before-the-town-runs)) |
| retail word held by a flag the entry script already consumed (`garmel`'s `0x196`, `rikuroa`'s `0x289`) | script progress: the track was started by a beat that has since cleared its trigger flag, so a card load would not restart it |
| an engine walk-on record on a seated arrival (`kor5`'s `P2[4]`) | the seat lands on a trigger tile the retail player reached with the last-tile pair already set; a real arrival fires it too |

### Arrival states are captured before the town runs

`s2_rimelm_town01` and `doman_arrival_from_korb2` carry the town's scene
label and field mode, but every other observable is still the overworld's:
GTE `H = 368` (the value every `world_map`-class state holds, and no other
`field`-class state), BGM word `2000`, the fog gate raised, and a player `Y`
of `-96` - an overworld footing. `son_arrival_from_doman` agrees on the word,
the gate and the footing. `son`'s scripts write no fog gate at all, and
`FUN_8003AEB0` clears the gate on every scene load that is not a warp return
(`sw zero,-0x47ac(v0)` at `0x8003B690`, behind the `_DAT_8007B8B8 != 2` test
at `0x8003B510`; the states hold `0`), so a `son` whose loader had run could
not hold it raised. The `bgm`, `fog_gate` and `footing` misses on these
three states are capture timing, not engine verdicts.

## See also

- [recomp-differential](recomp-differential.md) - the frame-tagged
  differential against the static recomp.
- [host-drift](host-drift.md) - the three-host parity gates.
- [mednafen-automation](mednafen-automation.md) and
  [pcsx-redux-automation](pcsx-redux-automation.md) - how the library states
  are captured and catalogued.
