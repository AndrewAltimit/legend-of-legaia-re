# Cast-sequence oracle

A per-cast frame-sequence oracle: one spell, summon or monster cast recorded
on both sides from cast begin to past its Done band, and scored against retail
frame by frame. It is the time-axis sibling of the
[retail comparison corpus](retail-compare.md). The corpus scores one frame per
save state. A save state holds one instant of a cast that runs for hundreds of
vsyncs, and on most summons that instant is the white flash, which the frame
metric cannot see under.

| Piece | Lives in | Role |
|---|---|---|
| retail recorder | [`autorun_cast_sequence.lua`](../../scripts/pcsx-redux/autorun_cast_sequence.lua) | PCSX-Redux probe: inject the cast, dump the displayed frame and per-vsync metadata |
| engine recorder | `LEGAIA_DIAG_CAST_SEQ` in [`redraw_present.rs`](../../crates/engine-shell/src/window/event_handler/redraw_present.rs) | `play-window` writes the same metadata and the cropped frame per tick |
| pairing + score | [`cast_sequence.rs`](../../crates/parity/src/cast_sequence.rs) | `legaia-engine cast-sequence-score`: pair, score, report, strip |
| driver | [`scripts/ci/cast-sequence.py`](../../scripts/ci/cast-sequence.py) | Runs the three stages with caching |
| pairing test | [`cast_sequence_pairing.rs`](../../crates/parity/tests/cast_sequence_pairing.rs) | Disc-free: synthetic recordings pin the pairing rule |

## Running it

```bash
# first run: retail capture (an emulator run, a few minutes), engine seed, engine run, score
scripts/ci/cast-sequence.py --spell 0x86 --state party_basic_attack_vs_gobu_gobu

# after an engine change: rebuild, re-run only the engine side and the score (seconds)
scripts/ci/cast-sequence.py --spell 0x86 --state party_basic_attack_vs_gobu_gobu --engine-only

# re-score cached recordings with another lag rule
scripts/ci/cast-sequence.py --spell 0x86 --state party_basic_attack_vs_gobu_gobu --score-only --retail-lag 4
```

The output defaults to `captures/cast-sequence/<state>_<spell>/` (gitignored).
It holds `retail/` (the probe's frames and `meta.csv`), `rc/` (the
retail-compare child command the engine run is built from), `engine/` (the
recorder's frames and `meta.csv`), `engine-cmd.txt` (the engine child's
environment and arguments, to re-run by hand), and the products: `report.md`,
`report.json` and `strip.png`. **Every one of them carries retail pixels or
RAM**: keep the directory out of the repo.

Flags worth knowing:

| Flag | Effect |
|---|---|
| `--caster <seat>` | Seat whose queued action is rewritten (default `0`; `255` = whichever party seat acts first). A seat `>= 3` is a monster: the engine side then seeds `LEGAIA_BATTLE_MONSTER_CAST` |
| `--target <seat>` | Target seat (default `3`) |
| `--every <n>` | Retail vsyncs between frames (default `2`; the engine records every tick) |
| `--recapture` / `--reseed` | Re-run the retail capture / the retail-compare seed instead of using the cache |
| `--formation <bytes>` | Retail only: overwrite the formation row `FUN_801DA600` returns. The engine has no seam to seat the same monsters, so a forced formation compares different fights |
| `--engine-env K=V` | Extra environment for the engine child |

## What each side records

**Retail.** The probe loads a library PCSX-Redux battle state, taps CROSS past
any prompt, and on the caster seat's action state `0x0A` writes
`actor[+0x1DE] = 2`, `+0x1DF = spell`, `+0x1DD = target` (the injection
[`autorun_w3a_cast_oracle.lua`](../../scripts/pcsx-redux/autorun_w3a_cast_oracle.lua)
proved out: past the command-flow validator, before `0x0C` reads the
category). Retail pages the module in and runs it on retail bytes. From the
caster's cast begin (`0x28`) it grabs `PCSX.GPU.takeScreenShot()` every
`--every` vsyncs. From the inject it writes one `meta.csv` row per vsync:
`ctx[+0x07]`, `ctx[+0x279]`, the acting seat, the frame step `*(0x1F800393)`,
the `ctx[+0x6D8]` / `+0x6DA` / `+0x87C` words, PROT 0903's countdown, every
seat's HP, and the main loop's frame counter `gp+0x480` (`frame`, read at
`0x8007B798`). At cast begin it also logs each pool slot's ground pair
`+0x34` / `+0x38` (`done.txt`). The run uses the recompiler (`--fast`) unless a
formation is forced, and `run_probe.sh` stages the disc in a private directory,
so no `.ppf` beside the image reaches it.

**Engine.** The driver runs `retail-compare --images --filter <state>` with
`LEGAIA_RC_CHILD_LOG=1` once to get the image child's command: the card-load
resume, `--battle <row>`, the stage, the HUD bars and the RNG seed. It then
re-runs that command with the state's own drive and gate removed, the cast
seeded, and the recorder on:

| Variable | Value |
|---|---|
| `LEGAIA_BATTLE_INFLIGHT` | `caster,spell,target;ground` - dispatched at the first prompt, every combatant seated on retail's ground pair at cast begin; the ground list is re-keyed from pool slots to engine slots (monster `m` at `party_count + m`) |
| `LEGAIA_BATTLE_MONSTER_CAST` | `seat,spell` instead, for a monster caster |
| `LEGAIA_DIAG_CAST_SEQ` | the recording directory |
| `LEGAIA_DIAG_CAST_SEQ_SEAT` | only a cast by this seat (retail numbering) starts the recording |
| `LEGAIA_DIAG_CAST_SEQ_EVERY` / `_TAIL` / `_MAX` | frame cadence in ticks (`1`), ticks past the Done band before exit (`60`), frame cap (`2000`) |

The child is a `--screenshot` run, so it is tick-locked, and the recording is
deterministic. The recorder starts on cast begin `0x28` and writes the frame
already cropped and box-filtered to the compared `320 x 224`
(`Frame::from_engine_capture`, the corpus's alignment). Its `meta.csv` uses
retail seat numbers: the engine packs the party ahead of the monsters, and the
recorder maps them back.

## The pairing rule

Frames pair on the action-SM clock, never on wall time:

1. **Segments.** Each side's rows fold into runs of one key. The key is the
   action state, except inside the slot-B module (`0x35` / `0x36`) and the
   capture module (`0x70`), where it is the module phase `ctx[+0x279]`. The
   module paces its own arms ([cast-module.md](../subsystems/cast-module.md)),
   and which of the two band states an arm runs in is a separate observable:
   the report counts pairs whose states differ under one phase key.
2. **Alignment.** The two segment lists align by longest common subsequence
   on their keys. A state one side visits and the other skips leaves the rest
   aligned and shows up as a one-sided segment row.
3. **Display lag.** A retail grab on vsync `v` shows the frame built
   `2 * step` vsyncs earlier ([Replayed casts](retail-compare.md#replayed-casts)),
   so it is keyed on the row at `v - lag`, with `step` read off the grab's own
   row. `--retail-lag <n>` fixes it instead. The engine renders the tick it
   just ran and takes no lag. On a cast whose close-up the engine already
   matches, `auto` scores those segments higher than a fixed lag of `0`, `2`,
   `6` or `10`; a re-score with `--score-only --retail-lag` repeats that check.
4. **Offset.** A retail frame `k` vsyncs into its segment, measured on the
   [game clock](#the-game-clock), pairs with the engine frame `k` ticks into
   the aligned segment, clamped to that segment's last tick. An engine arm
   that ends early is compared at its last frame for the rest of retail's
   arm, which is what a viewer would see.

Each pair is scored with the corpus metric (`within`, the fraction of `8 x 8`
blocks within `24` per channel; [The image channel](retail-compare.md#the-image-channel)).
A pair whose retail frame has mean luma above `230` (a white flash) or below
`8` (a fade) goes into its own count and stays out of the mean. The cast's
number is the **mean `within` over the remaining scene pairs**.

## The game clock

Retail's frames do not all last their step. The frame driver `FUN_80016B6C`
sets the step from the slowest of the last sixteen frames, capped at `4`
([the frame step](../subsystems/battle-stage-camera.md#the-battle-frame-step-is-the-frames-own-cost)),
and paces the loop with `VSync(step)`. A frame that costs more than `4`
vsyncs, or more than the step the ring has not caught up to yet, takes longer:
its timers still drain one step, so on wall vsyncs retail's arm runs long
while its game time does not. The emulator's GPU and CPU cost model decides
which frames overrun; nothing in the game state the engine carries does, so
the engine does not reproduce it.

The probe records the frame counter on every vsync. A run of one counter value
is one game frame of `L` vsyncs, its step the byte its last row reads. The
**game clock** adds the step per frame, spread evenly over the frame's rows,
so an overrunning frame advances it by its step, not by `L`. The pairing
measures retail's offset into a segment on that clock, and the engine's tick
(one vsync, no overrun) on its own. A capture without the `frame` column
pairs on wall vsyncs, and the report says which clock it used.

The segment table shows both: `retail vsyncs` (wall), `retail game` (the
clock), and `slow frames`, the frames that ran past their step and the vsyncs
they ran over by. The overrun sits in the heavy effect arms, almost all of it
in frames at step `4` that take `5` to `12` vsyncs, plus a few frames at
step `1` that take `3` to `8` on the pass a heavy arm starts, before the ring
has caught up. Measured on the `party_basic_attack_vs_gobu_gobu` captures
(target seat `0` for `0x83` and `0x89`), each run paced on the step the run
before it reads:

| Cast | Overrun in all | Module phases that carry most of it |
|---|---:|---|
| `0x81` Gimard | 143 | 12 (+78), 4 (+37) |
| `0x82` Theeder | 42 | 5 (+16), 14 (+11) |
| `0x83` Vera | 161 | 5 (+89), 8 (+45) |
| `0x84` Gizam | 208 | 13 (+148), 5 (+55) |
| `0x85` Nighto | 18 | 5 (+6) |
| `0x86` Zenoir | 238 | 12 (+122), 5 (+104) |
| `0x87` Viguro | 111 | 12 (+77), 14 (+26) |
| `0x88` Swordie | 168 | 7 (+91), 9 (+71) |
| `0x89` Orb | 73 | 10 (+66) |
| `0x8A` Freed | 350 | 13 (+108), 8 (+64), 9 (+60) |
| `0x8B` Nova | 495 | 4 (+131), 16 (+105), 7 (+60), 15 (+57) |
| `0x90` | 190 | 8 (+116), 12 (+32) |
| `0x94` | 298 | 19 (+64), 18 (+37), 20 (+37) |
| `0x99` | 568 | 24 (+191), 25 (+90), 19 (+74), 23 (+61) |

## Reading the report

`report.md` opens with the mean, the minimum and the excluded counts. Then
come:

- **Segments.** Each key with retail's length in vsyncs, the engine's in
  ticks, its pair count and mean. Lengths that part are a pacing defect. A
  one-tick engine segment against a long retail one is an arm the engine runs
  through without its countdown.
- **Worst pairs.** The lowest-scoring scene pairs.
- **Strip.** The rows of `strip.png`, each `retail | engine | |diff|`: the
  worst pair of each of the worst segments, plus evenly spaced pairs, in time
  order.

`report.json` carries every pair (retail vsync, shown vsync, engine tick, key,
offset, both states, class, `within`, `mae`, both lumas) for scripts.

## Limits

- **The fight is the state's.** The engine fight is the one `retail-compare`
  seeds for the state, so a state it cannot image (a retail frame that is a
  fade) has no engine side.
- **The cast is dispatched from the first prompt, not from retail's turn
  order.** Ground pairs are carried over. Facing, status, HP already lost, and
  the camera history an earlier action left are not, so the frames before the
  module owns the camera (`0x28..=0x34`) can part on framing alone.
- **One tick is one vsync on the engine side.** The engine's battle state
  machine is not on retail's frame step, so a segment's length is compared in
  vsyncs of game time. Retail's arms that count in passes (not in drained
  steps) run `step` times longer than the pass count.
- **Damage and RNG are not aligned.** Popups and HP readouts can differ.
- **A forced formation is retail-only.** See the flag table.
- **A monster cast does not inject yet.** The probe writes the monster's
  queue on the first `0x0A`, but the monster's own turn runs its AI pick in
  the same pass as the `0x0C` category read, which a vsync hook cannot get
  between. The retail side then records the monster's AI action, not the
  cast, and the run fails with no cast begin. A breakpoint on the AI pick's
  return is the missing piece. The engine half (`LEGAIA_BATTLE_MONSTER_CAST`,
  `--caster 3`) is wired.
- **The target has to suit the spell.** A heal aimed at a monster seat parks
  the module (Orb on seat `3` sits in its phase `9`); pass `--target 0` for an
  ally spell. The probe quits on a park (`LEGAIA_PARK`, state and module phase
  unchanged for `900` vsyncs) and the driver reports it.

`--engine-only` on a state with no retail recording runs the engine half
alone. The corpus's `*_summon_mid_cast` states are mednafen captures that
PCSX-Redux cannot load, but their child commands seed the cast. That is
enough to read the engine's own segment lengths for a cast in the fight the
corpus scores it in.
- **An item cast (the flutes) is not injectable.** The probe writes category
  `2` (Magic). An item summon needs the item category and its own id space.

## See also

- [Retail comparison corpus](retail-compare.md) - the per-state breadth sibling and its image metric
- [cast-module.md](../subsystems/cast-module.md) - the slot-B module phase machine the key follows
- [PCSX-Redux automation](pcsx-redux-automation.md) - the probe runner and its traps
