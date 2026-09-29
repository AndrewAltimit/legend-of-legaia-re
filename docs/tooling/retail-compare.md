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
| `battle` | mode `0x14` / `0x15` | no |
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
behind `LEGAIA_SEAT`), the zone camera's arrival snap is re-armed, and the
session ticks a fixed settle window with no input. BGM starts are recorded by
a director on the scene host's event route.

**The image channel** comes from `play-window`, the real renderer, run as a
child process from a scratch directory under the report: `--scene`,
`LEGAIA_SEAT=X,Z`, one `--set-flag` per raised bit of the retail system-flag
bank (`0x80085758`, raised before the scene entry, so the entry scripts
branch on retail's flags), and `--screenshot` at a fixed world tick.

What the seeding does **not** carry - each is an instrument limit, not an
engine verdict:

- **Script progress.** A state captured mid-cutscene, mid-dialogue or on an
  arrival is a point inside a running script. The engine enters the scene
  fresh, so it runs the entry prologue instead of resuming the retail
  script: a scripted camera, an open dialogue box, a banner or an event
  track is not reproduced.
- **Party and bag in the frame.** `play-window` shows its default party, so
  the idle status panel draws the default roster rather than retail's.
- **Actor state.** NPC positions, animation phases, open doors and live
  effects are whatever the engine's own entry produces.
- **Timing.** Retail's frame is one instant; the engine's is a fixed tick
  after entry. Ambient animation, fog-pool population and water CLUT phases
  cannot be phase-aligned.

`--flags-first` is a diagnostic arm for the headless side: hydrate, enter,
hydrate again, so the entry scripts see retail's flags. Comparing its report
with the default shows how much of a channel's divergence the card-load
ordering explains.

## Channels

Each channel scores in `[0, 1]`; a state's score is the mean of its
measured channels.

| Channel | Score |
|---|---|
| `scene` | 1 when the engine landed in retail's scene |
| `mode` | 1 when the engine's mode is `Field` (field class) / `WorldMap` (overworld class) |
| `position` | player `(X, Z)` after settling: 1 within 4 units, linear to 0 at 256 |
| `footing` | engine floor sample at retail's `(X, Z)` vs retail's footing: 1 within 2, 0 at 128 (field class only) |
| `camera` | mean of six parts: pitch and yaw (1 within 16, 0 at 256, wrapped), `H` (1 within 4, 0 at 128), each eye word (1 within 16, 0 at 1024) |
| `bgm` | 1 when the last track the engine started equals retail's track word |
| `fog_gate` | 1 when the engine's fog-pool gate equals retail's |
| `party` | fraction of equal fields over retail's roster: HP / MP current and max, level, the eight equipment bytes |
| `flags` | 1 - differing bits / bits set on either side, over the whole story-flag bitmap |
| `inventory` | fraction of non-empty bag slots equal, slot for slot, plus gold |
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

The report writes `retail | engine | |diff|` side by side for every scored
state. **Those PNGs are retail pixels**; the report directory must stay
gitignored (`captures/` is).

## The ratchet

`scripts/ci/retail-compare-baseline.json` holds, per state label, each
measured channel's score, plus every state's class. The test
`retail_compare_corpus` re-runs the corpus and fails when any state's
channel falls below its baselined score. A rise is allowed and is folded in
by a reviewed `--bless`. A baselined state missing from the local library is
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
scripts/ci/retail-compare.py --images --bless # rewrite the baseline after a reviewed rise
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
is script progress; a `bgm` miss on an arrival state is a timing question
for a live probe, not a verdict.

## Divergence shapes

Shapes the corpus separates, each with what it indicates:

| Shape | Indicates |
|---|---|
| camera exact, frame smeared by stretched texture planes | primitives near or behind the eye rasterised by the port; retail's field cell emitter near-clips on `OTZ < 0x40` ([renderer](../subsystems/renderer.md#the-field-ground-pass-two-emitters-one-gate)) |
| retail frame black beyond a rectangle, the engine's filled | the visible-tile window (op `0x46`); the port draws the whole scene |
| an idle status panel in the engine frame only | the engine's panel timing / placement against retail's |
| effect missing in the engine frame (save-point crystals, spell glows) | an actor or effect the fresh entry does not spawn, or one the port does not draw |
| camera and dialogue off together | script progress - the seeding cannot resume a script |
| retail BGM word `2000` on a town arrival | retail still holding the overworld track at capture; check with a live probe |

## See also

- [recomp-differential](recomp-differential.md) - the frame-tagged
  differential against the static recomp.
- [host-drift](host-drift.md) - the three-host parity gates.
- [mednafen-automation](mednafen-automation.md) and
  [pcsx-redux-automation](pcsx-redux-automation.md) - how the library states
  are captured and catalogued.
