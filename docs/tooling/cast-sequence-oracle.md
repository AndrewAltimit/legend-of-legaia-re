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
| sound channel | [`cast_sequence_sound.rs`](../../crates/parity/src/cast_sequence_sound.rs) | the two sides' sound cues matched on id and on the same clock ([below](#the-sound-channel)) |
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

Several casts go through
[`cast-sequence-batch.py`](../../scripts/ci/cast-sequence-batch.py): a file of
`<name> <arguments>` lines, the retail recordings side by side (`--jobs`), then the
engine runs one at a time. A retail recording is a
[headless](pcsx-redux-automation.md#headless-runs) emulator at 1x (`--window` opens
one) and holds no display lock. Two recordings of one cast are not byte-identical,
in a window or not ([why, and what does hold](pcsx-redux-automation.md#a-recording-is-not-byte-repeatable-at-any-speed)),
so a residual smaller than two retail runs differ from each other is noise. On a
`--monster-pick` run the approach is not pinned either: a one-vsync slip moves where
the blind CROSS taps land in the rounds before the pick, so the inject can come a
round later (a window and a headless run of one state inject hundreds of vsyncs
apart). The cast from its begin keeps its action path and cues, and the engine side
takes its ground from the recording.

Each stage is reused until something it depends on changes. The retail
recording stays until `--recapture`; the seed's child command until `--reseed`;
the engine recording while `engine-key.txt` matches a hash of the binary, the
child's `LEGAIA_*` environment and arguments, the files those name and the
disc, so an `--engine-only` with nothing changed goes straight to the score
(`--rerecord` forces it). The score reads and scores its pairs on several
threads, and the report does not depend on how many.

The output defaults to `captures/cast-sequence/<state>_<spell>/` (gitignored).
It holds `retail/` (the probe's frames and `meta.csv`), `rc/` (the
retail-compare child command the engine run is built from), `engine/` (the
recorder's frames and `meta.csv`), `engine-cmd.txt` (the engine child's
environment and arguments, to re-run by hand), and the products: `report.md`,
`report.json` and `strip.png`. **Every one of them carries retail pixels or
RAM**: keep the directory out of the repo.

The engine child drops the state's own capture seeds as well as its gate and
drive, its re-seat among them: a strike state's re-seat left in stood the
caster `3500` units from the ground retail recorded.

The driver builds `legaia-engine` with `engine-shell`'s `oracles` feature:
`cast-sequence-score` and the recorder's frame shots exist only in that build,
and a binary built without it writes `meta.csv` but no `e_<tick>.png`.

Flags worth knowing:

| Flag | Effect |
|---|---|
| `--caster <seat>` | Seat whose queued action is rewritten (default `0`; `255` = whichever party seat acts first). A seat `>= 3` is a monster: the engine side then seeds `LEGAIA_BATTLE_MONSTER_CAST` |
| `--target <seat>` | Target seat (default `3`) |
| `--every <n>` | Retail vsyncs between frames (default `2`; the engine records every tick) |
| `--recapture` / `--reseed` / `--rerecord` | Re-run the retail capture / the retail-compare seed / the engine recording instead of using the cache |
| `--formation <bytes>` | Retail only: overwrite the formation row `FUN_801DA600` returns. The engine has no seam to seat the same monsters, so a forced formation compares different fights |
| `--engine-env K=V` | Extra environment for the engine child |
| `--sound-tolerance <n>` | Sound channel: ticks a cue still matches at (default `6`) |
| `--no-sound` | Engine: keep the child's `--no-audio`, so no audio director and no `sfx.csv` |
| `--watch <addr,..>` | Retail: each store into these bytes with its pc, `ra` and registers to `retail/watch.csv` (breakpoint core): who wrote a seat's rate |
| `--sfx-callers` | Retail: breakpoints on the cue producers `FUN_80035B50` / `FUN_80035BD0`, the battle router `FUN_8004FE5C` and the dispatcher `FUN_8004FCC8`, each call's `a0` / `a1` / `ra` to `retail/sfxcall.csv` (breakpoint core): who fired a cue |
| `--retail-parts` | Retail: `parts.csv`, every live move-VM part per vsync (`+0x10`, colour word, level, draw kind, `+0x5A`, `+0x9E`, packed colours, `+0xB4..+0xBA`, position, scale, `+0x52`, `+0x9C`, UV / tpage / CLUT, H / S / V adds, view depth `+0x34`, `+0x7A`, `+0x6D`, `+0x42`): the retail half of `LEGAIA_DIAG_CAST_PARTS` |
| `--prim-calls` | Retail: `primcall.csv` (breakpoint core), each prim dispatcher entry with its word, level, caller and first group words, and each cued leaf: whether a part reached the dispatcher at all, and by which leaf |
| `--learned <level>` | Both sides: the spell sits on the caster's list at this level. Off the list `FUN_801DD864` reads list slot `0x20`, and level `0` wraps into a capped hit ([the summon roll](../subsystems/battle-formulas.md#a-spell-the-caster-has-not-learned)), so a damage measurement wants this |
| `--monster-hp <hp>` | Both sides: every living monster's HP from the cast begin, so a cast's whole damage shows in the HP columns. HP feeds the defender roll (`hp >> 8`) and a monster's own swing, so keep it modest and it goes in after the monster's turn |
| `--guard <seats>` | Engine: party seats in the Spirit stance when the cast starts (`LEGAIA_BATTLE_GUARD`), for a capture whose victim guarded (`+0x1DE == 4`): `nivora_duel_pre_megaton_press` needs `--guard 0` for its damage to match |
| `--monster-pick` | Retail: with `--caster 3..7`, the cast is written where the monster AI's pick returns (a breakpoint on `0x801DAF50`, see [below](#a-monster-cast-from-any-battle-state)), so any battle state that reaches the monster's turn records any spell the monster owns a clip for. Runs on the breakpoint core, several times slower |
| `--cast-queue <hex,..>` | Retail, with `--monster-pick`: the bytes behind the spell at `+0x1E0` (clip, shot, `ff`). A pick that was not a cast leaves no terminator there and `0x2A` parks on it; read the bytes off a capture of the same cast (`battle_korb2_aluru_cannon_mid_cast` holds `3F 09 0E FF`) |
| `LEGAIA_BATTLE_MONSTER_CAST=seat,spell,p,t<target>` (engine child) | The driver passes `--target` as `t<seat>`, so a single-target cast lands on retail's victim; a group target stays the cast's own class |
| `--retail-steps` | Engine: run on the recording's own frame steps (`LEGAIA_CAST_STEP_TRACK`, `RetailStepTrack`), frame by frame under each key. A harness seat: what remains in the film is the law's, with the frame's cost taken out ([below](#the-camera-against-the-step)) |
| `--rand-at <st7>:<phase>` | Engine: hold the world `rand()` stream on retail's seed word under this cast key (`LEGAIA_CAST_RAND_SEED`, [below](#the-rand-stream)). `--rand-at 0x70:3` on Gigaton Press gives arm 3's pass retail's sixteen rock picks |
| `--allow-patched` | Measure a state whose manifest entry carries `resident_patch` (refused by default: retail states only) |
| `--save-inject` | Retail: keep the whole emulator state one vsync past the inject (`retail/state_inject.rawsstate`), a pre-cast state that replays the cast on the fast core with a plain `--caster` |
| `LEGAIA_CAST_SEQ_EXE=<path>` (environment) | Score another `legaia-engine` build (a before / after pair) without rebuilding the tree's |
| `LEGAIA_PRESS_UNTIL=<vsync>` (environment) | Retail: tap CROSS through the party's command input until this vsync (default `600`). A three-member party or a long opening needs more before the monster's turn comes round |
| `LEGAIA_POKE32=addr=value,...` (environment) | Retail only: words the probe writes at the inject. A capture-class module that forks its choreography off RAM rather than the action id needs it: PROT 0946's arm 0 takes Big Wave's branch only while the caster's latch `0x801C8FE0 + (seat + 1) * 4` is set, so a seat-0 cast of `0x56` needs `0x801C8FE4=1` and otherwise records Call Wave |

`LEGAIA_DIAG_CAST_PARTS=1` beside the engine recorder (`--engine-env`) adds
`parts.csv`: every live part of the cast's scene graph each tick - record
`model_sel`, halted, colour word `+0x74`, level `+0x78`, `+0x9E`, the packed
colours `+0xA0` / `+0xA4`, the radius and height channels `+0xB4..+0xBA`, the
position, then the draw kind `+0x56`, scale `+0x72`, `+0x52`, the count
`+0x9C`, the UV words `+0xA8` / `+0xAC`, tpage `+0xB0`, CLUT `+0xB2`, the
default-arm mesh's vertex count, the spawn record's address, the flag word
`+0x10`, the angle trio `+0x24..+0x28`, `+0x7A`, a mode-3 cycler's H / S / V adds
`+0x90..+0x94` and its gate `+0x9C` - the engine half of a
part-by-part comparison. It is how Neo
Star Slash's arm-6 excess was traced to one cylinder's back faces
([renderer](../subsystems/renderer.md#the-field-pass-culls-back-faces)).

Set against a retail recording made with `--retail-parts`, the file reads a mechanism
off directly: Spoon's fans stood at scale `0x400` beside retail's `0x666`, Barra's cone
at `z = 0` beside `456`. Each retail part appears once per list walk of its vsync, so
compare by value, not by count. The same goes for `cam.csv` against the pairs of
`report.json` (`shown_vsync`, `engine_tick`): a constant difference through an arm names
a store or a drift, one that grows names a rate.

`LEGAIA_DIAG_CAST_VRAM_AT=<tick>` writes the battle VRAM at that tick as
`engine/vram_<tick>.bin`, to hold against a retail recording's
`LEGAIA_RAM_AT` state through `sstate_vram.py`. A byte comparison of the side-band pages
and CLUT rows is how a palette or texture reading is settled before a frame's colours are
argued over: Iota's cycler rows match the kernel entry for entry.

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
seat's HP, the main loop's frame counter `gp+0x480` (`frame`, read at
`0x8007B798`), the side-band applier stage `ctx[+0x276]` (`c276`, the
token a module's stream-wait arm polls) and the words the read-span poll
`FUN_8003DE7C(1)` reads (`xaspan` `gp+0x91C`, `cdbusy` `0x8007BA70`,
`cdmode` `0x8007B83C`, `cdread` `0x8007B8C2`) and the BIOS `rand()` seed word
`0x80009010` (`rand`). A second file, `body.csv`, holds
per vsync the acting seat's clip `+0x1D9`, node cursor `+0x68`, live pair,
body pair `+0x3C` / `+0x40` and the camera focus with its tween endpoints:
the series a summon close-up's framing is read off. A third, `cam.csv`, holds
per vsync the live pitch, yaw and TR trio with the yaw and TR z records of the
tween table `ctx[+0x118C]` (increment, endpoint): the walk of a module's shot.
At cast begin it also logs each pool slot's ground pair
`+0x34` / `+0x38` (`done.txt`). With `LEGAIA_PRE_TRACE=1` the probe also
writes `pre.csv`, one row per in-battle vsync **before** the inject: the
action state, the acting seat, the frame step, the yaw counter `ctx[+0x6DA]`,
the strike cursor `ctx[+0x15]`, and for seats `0..3` the committed and staged
clip, the rate `+0x21D`, the node cursor, the live pair and the per-action
total `+0x00` (seat 3's live HP). A monster-pick run from a `*_first_menu`
state therefore records every party turn on the way to the cast - the retail
timeline of a combo the capture only holds the end of. The run uses the recompiler (`--fast`) unless a
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
| `LEGAIA_BATTLE_MONSTER_CAST` | `seat,spell,p;ground` instead, for a monster caster |
| `LEGAIA_DIAG_CAST_SEQ` | the recording directory |
| `LEGAIA_DIAG_CAST_SEQ_SEAT` | only a cast by this seat (retail numbering) starts the recording |
| `LEGAIA_DIAG_CAST_SEQ_EVERY` / `_TAIL` / `_MAX` | frame cadence in ticks (`1`), ticks past the Done band before exit (`60`), frame cap (`2000`; the driver passes `4000`, past the retail side's span) |

A monster's AI counter `0x801C8FE4` travels with the state
(`LEGAIA_SEAT_AI_COUNTER`): PROT 0964's strike forks on it, and a replay that starts
from a cleared word plays another element's arms.

The child's mode-3 cycler and mode-4 scroller seats (`LEGAIA_SEAT_CLUT_FX`,
`LEGAIA_SEAT_VRAM_RECTS`) are dropped with the drive: each holds one capture
instant, and a whole run under them leaves the evolved-Cort arena's flesh on
one palette and one scroll where retail's keeps cycling. Their phase is then
the engine's own: nothing aligns the hue clock with retail's across the
fight's history.

The child is a `--screenshot` run, so it is tick-locked, and the recording is
deterministic. The recorder starts on cast begin `0x28` and writes the frame
already cropped and box-filtered to the compared `320 x 224`
(`Frame::from_engine_capture`, the corpus's alignment). Its `meta.csv` uses
retail seat numbers: the engine packs the party ahead of the monsters, and the
recorder maps them back. Beside it, `cam.csv` holds the live camera pose per
tick (pitch, yaw, TR trio), the engine half of the retail side's file, and
`body.csv` the acting seat's clip `+0x1D9`, staged byte `+0x1DA`, whether the
clip loops, its cursor in sixteenths of a keyframe and the live pair: the
engine half of the retail `body.csv`, which a clip that ends early or late
is read off.

The child drops the capture's drive and runs no earlier actor, so nothing in it left
the camera where retail's stood. On cast begin it takes retail's pose from the
recording's first row of that state (`LEGAIA_DIAG_CAST_SEQ_SEAT_POSE`, `retail_opening_pose`
in the driver): `cam.csv`'s pitch, yaw and eye with `body.csv`'s focus. This is a
seat of the harness and moves no game state. Without it `0x28` and `0x29` score
wherever the child's camera happened to stand; with it their framing tracks retail's to
a couple of units, and the opening's error is the frame's own.

Both sides also write `creature.csv`, the summon creature's clip, staged byte,
cursor and stand per row (slot 7's `+0x1D9`, `+0x1DA`, node `+0x68`, `+0x34..+0x38`), so a
walk or a clip the engine does not carry reads directly: Jedo's creature stood at
`z = -0x800` where retail's advanced to `-0x400`. The Done band includes a flute's flee
state `0x65`, so the Spikefish cast ends cleanly. A cast's arm can run longer in retail
than its countdown says: the frame step stops at 4 while a heavy arm's real frame runs
past 4 vsyncs, so the module's word drains slower per vsync. Ozma's arm 6 runs 462
vsyncs to the engine's 191. The pairing keys on arms, so those frames pair by arm.

## The camera against the step

`cam.csv` on both sides read at the score's own pairs (`shown_vsync` on retail's
side, `engine_tick` on the engine's) is the camera's error frame by frame. Set
`--retail-steps` beside the plain run and the two together split it: what the seated
run closes is the frame step's, what it leaves is the framing law's or the seat's. On
`balden_swordie_v2_pre_vacuum_slicer` the seated run leaves `.3` px of the plain run's
`4`, and the whole of that gap is the step ([the frame step](../subsystems/battle-stage-camera.md#the-frame-step)).

A `--physical` film has three traps of its own, each closed in the driver or the
recorder. It keeps the capture's re-seat (`PHYSICAL_KEEP_ENV`), which stands the round
on the capture's ground; it lets go of the drive at its gate
(`ScreenshotConfig::battle_drive_released`), because the drive's steer pins the yaw
counter for as long as the state lasts and leaves the camera short by what the state
would have drifted; and the score keeps the recording's first action only
(`Side::first_action_only`), because a recording's tail can hold the next actors'
whole turns, and the alignment prefers a later actor's complete run of states to a
capture that starts inside the strike loop.

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
3. **Display lag.** A retail grab shows the frame built two game frames
   earlier ([Replayed casts](retail-compare.md#replayed-casts)), so it is
   keyed on the last row of the frame two counter bumps back. A frame that
   follows one which ran past its step flips a vsync after its bump, so a
   grab on that bump's vsync is keyed one frame further back. On an on-time
   frame this is the row `2 * step` vsyncs earlier; counted in frames, a slow
   frame keeps its image up for every vsync it overran. `horn_summon` enters
   module phase 9 on a seven-vsync step-`4` frame, and its close-up reaches
   the screen fourteen vsyncs later, not eight: the vsync rule scored the
   phase-8 shots still on screen against the engine's phase-9 close-up. A
   capture without the frame counter falls back to `2 * step` read off the
   grab's own row. `--retail-lag <n>` fixes it instead. The engine renders the tick it
   just ran and takes no lag. On a cast whose close-up the engine already
   matches, `auto` scores those segments higher than a fixed lag of `0`, `2`,
   `6` or `10`; a re-score with `--score-only --retail-lag` repeats that check.
4. **Offset.** A retail frame `k` vsyncs into its segment, measured on the
   [game clock](#the-game-clock), pairs with the engine frame `k` ticks into
   the aligned segment, clamped to that segment's last tick. An engine arm
   that ends early is compared at its last frame for the rest of retail's
   arm, which is what a viewer would see. The offset runs from the key's first
   row, inside the frame whose pass moved the key, so on a step-`4` arm the pair
   reads a retail image up to one step older than the engine tick it scores:
   Ozma's arm 6 shows a constant `2` in pitch and a flipped shake bit. Counting
   from that frame's last row zeroes those camera words and drops `within` on
   the same casts (Iota `.508` to `.477`, Ozma arm 18 `.567` to `.347`), because a
   part or fade spawned by the pass already took that frame's whole step in
   retail and one tick in the engine. The rule stays; read a small camera residual
   on a slow arm against it.

Each pair is scored with the corpus metric (`within`, the fraction of `8 x 8`
blocks within `24` per channel; [The image channel](retail-compare.md#the-image-channel)).
A pair whose retail frame has mean luma above `230` (a white flash) or below
`8` (a fade) goes into its own count and stays out of the mean. The cast's
number is the **mean `within` over the remaining scene pairs**.

## The rand stream

Retail draws every `rand()` (BIOS `A(2Fh)`, `jal 0x80056798`) from one seed
word, `0x80009010`: `seed = seed * 0x41C64E6D + 0x3039`, the draw
`(seed >> 16) & 0x7FFF`. The engine's world stream is the same generator,
but it reaches a cast's arms after another number of draws, so an arm whose
look is its draws plays another pattern. PROT 0962 `0xA4` arm 3 picks each
of its sixteen falling rocks by `rand() % 4`: on the engine's own stream
Gigaton Press scores its arm-4 segment below the run without rocks.

The probe logs the seed word on every vsync. `--rand-at <st7>:<phase>` takes
the word retail held on its last vsync under that key and has the engine
child set its world stream to it before every tick it opens under the same
key (`engine-capture::cast_rand`), so the pass that leaves the key draws as
retail's did. In a `koru_strip_forced` capture the word on arm 3's last vsync
makes one draw and then the sixteen picks a RAM read of the rocks holds; the
engine, seeded there, makes the same draws. Measured on that capture, the
arm-4 segment scores `.770` with the option and `.649` without.

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
| `0x95` Gilium | 211 | 5 (+71), 21 (+48), 6 (+36) |
| `0x99` | 568 | 24 (+191), 25 (+90), 19 (+74), 23 (+61) |
| `0x37` Guilty Cross (party seat) | 147 | capture phase 4 (+145) |

The game clock is what the engine's lengths match. Gilium's summon band
(`0x35` / `0x36`) is 2423 wall vsyncs and 2240 on the game clock; the engine
runs it in 2244 ticks. Guilty Cross's capture run (`0x70`, phases `1..5` at
`128 / 128 / 128 / 256 / 195`) is 982 wall vsyncs and 835 on the clock; the
engine's Cort casts it in 835 ticks. A soak trace that reads a summon or a
boss cast as "long" is reading the cast's own length.

A short arm can still part the clocks a later segment pairs on. Vera's arm 3
polls the creature stream (`FUN_8003F2B8(1)`) for four vsyncs where the
engine's resident stream passes it in two, and PROT 0905's white rise
(spawned in arm 2, `0x20` vsyncs) runs on through both: a phase-4 pair `into`
frames past the arm's entry holds retail's rise older than the engine's
(`64`, eight vsyncs in, at retail's arm-4 entry in one capture), and
the whole arm-4 segment scores low on the brighter retail frame. The
`vera_summon_mid_cast` still, gated on the rise's own age, matches it.

## A monster's physical attack

`--physical` records the strike a harvested state already holds instead of
injecting a cast (`battle_*_strike`: a monster eight vsyncs into its strike loop
`0x1E`). Neither side is steered onto an action:

```bash
scripts/ci/cast-sequence.py --physical --state battle_korb2_warhalf_strike --every 8
```

- **Retail.** `LEGAIA_PHYSICAL=1`: the probe loads the state and records from
  its first vsync to past the Done band.
- **Engine.** The child keeps the state's own pad drive, so its strike is the
  round `BattleDrive::reached` accepts, and starts recording on that gate
  (`LEGAIA_DIAG_CAST_SEQ_BEGIN=gate`). `LEGAIA_DIAG_CAPTURE_TICK` pushes the
  child's frame past the recording. A first `0x1E` is not that round: the drive
  may replay rounds until its target and clip match, and a recording started on
  an earlier one reads as a strike that "lands nothing".

On the retail side the same switch starts on a state held on `0x2B` or `0x70`, with
`--retail-only`: `LEGAIA_RAM_AT` then films a monster cast from the state on the
fast core, where the `--monster-pick` run is an interpreter's.

Once the capture's own instant is passed, nothing holds the two rounds together:
the block roll, the monster's next queue byte and any later pick draw on each
side's own stream. So the channel's number mixes rule differences with deals.
`scripts/asset-investigation/cue_by_clip.py <dir>` takes the deal out: it files each
cue under the seat and clip it landed in, with its offset from the clip's commit,
on both sides, so a clip both sides played can be read cue for cue.

`--strike-deal` takes the deal out of the score as well. When the capture's gate opens, the engine child stands on the hand retail held:

- the striker's queue `+0x1DF..` off the state. A monster's swings were rolled at its pick, so the
  drive's gate holds only the bytes up to the cursor;
- the approach terms `ctx[+0x6D2]` / `ctx[+0x6D4]` off the state. They feed the first hit's block roll,
  and the replay walked in from elsewhere to a target facing elsewhere: `battle_rugi_barra_strike`
  holds `393` / `64`, the replay `86` / `116`;
- the striker's working ATK `+0x158` off the state: `battle_map02_mad_ogre_strike` stands on a Power
  Charge's `257` where the replay, which never cast it, holds `206`;
- the seed word `0x80009010` going into each hit, off the recording's `rand` column. A hit's spark burst
  makes its frame draw tens of times where any other frame draws once, so the frames read straight
  off the generator's distance between two rows. The frame driver's discard at `0x80046D2C` runs ahead of the
  hit, so the word steps on once.

```bash
scripts/ci/cast-sequence.py --physical --state battle_rugi_barra_strike --engine-only --strike-deal
```

Most strike recordings dealt this way match retail cue for cue: block verdict, damage rolls, the flinch's
impact pick and the times. The ones that still part are not proven to part on a rule, because the deal is
not complete. An emitter's draw can fall ahead of the hit: Nova's second hit follows a frame with an extra draw,
and retail's verdict comes out only one word further on. Stepping every hit by the frame before's draws fixes
Nova and breaks others, so the option keeps the one step the disassembly gives. A walk-clip capture's gate
can also open past the first swing (`battle_retona_king_frog_strike`), and a defender's own stats stay the
replay's. Read a residual by hand, as `battle_map02_mad_ogre_strike`'s Power Charge was read.

What thirteen strike states show (`warhalf`, `warman`, `frog`, `spoon`,
`gilium_lv2`, `hells_rat`, `gizam`, `wolf`, `guardian`, `freed`, `barra`,
`chitei2_gilium`, `mad_ogre`): every clip both sides played carries the same ids
within two vsyncs. Vahn's flinch `0x54` and block `0x51` + `0x0C`, Noa's block
`0x249`, Gala's block `0x58` + `0x0C`, and each monster's swing (`0x2D0` Frog,
`0x2E4` / `0x311` Warhalf, `0x2E3` Mad Ogre, `0x19` Gilium, `0x308` Freed) agree.
`gizam` and `warhalf` play the same ids throughout, and every other difference is
a block roll or queue byte the two streams dealt apart, not a wrong cue. A cue on
a walk clip (`warhalf`'s `0x311`) sits on that seat's walk cycle, which the
capture's instant does not fix. A hit
that lands keeps its hit count: `frog`'s retail swings land, block, land and fold
one HP drop; the engine's land, land, knock down and fold one.

## The sound channel

Both sides also write `sfx.csv`, one row per sound cue played:

- **Retail.** The probe samples the cue ring every vsync: the ids
  `DAT_8007B6D8[4]` (`i16`) and their countdowns `DAT_8007C338[4]` (`u32`).
  The drainer `FUN_80016B6C` plays a slot whose countdown is zero and whose
  id is not negative (`0x80016BF8..0x80016C20`), and the next frame's ager
  `FUN_8001698C` clears it, so each edge into that condition is one cue
  ([the ring](../formats/sfx-table.md#the-ring-is-two-arrays-aged-by-one-function-and-drained-by-another)).
  The row's `drain` column is the drainer's own gate
  (`0x8007BABC == 0x8007BAA0`, `0x80016BC0..0x80016BD4`); a cue that came due
  behind a closed gate was not played and is not scored. The ring carries
  every battle cue the router `FUN_8004FE5C` enqueues and every script cue.
  `FUN_800250D4` releases voices, it keys none, and `FUN_80065034`'s only callers
  outside the drainer are the emitter arm and four minigame overlays, so no battle
  voice is keyed around the ring. A CD-XA voice clip (`FUN_8003D53C`) is not a ring
  cue and is scored apart: a `--sfx-callers` recording's dispatcher rows at `id >= 0x100`
  against the engine's `xa.csv`, under **CD-XA clips** in the report.
- **Engine.** The recorder reads the audio director's command record each
  tick: the cues that came due (`SfxDue` off the delay scheduler, `RingDue`
  off the retail ring, `SfxNow`). The driver drops the child's `--no-audio`
  and sets `LEGAIA_AUDIO_DETACHED=1`, which opens the director over an
  output with no device.

Both ids are descriptor indices (the ring value is the index the drainer
reads the table with). A retail cue is placed into engine time the way a
frame is: its segment, its offset on the game clock, the same offset into
the aligned engine segment. It matches the nearest unmatched engine cue of
the same id within the tolerance. A second pass pairs what is left within
`120` ticks and lists those as **timing** misses with their offset; the rest
are **missing** (retail only) or **extra** (engine only).
Only the span both sides recorded is compared: cues past the last aligned segment on either side are
dropped, so a capped retail recording (Fatal Decision's wheel) does not read as engine extras. A
`--physical` recording's window opens on the state's first vsync and closes at the end of its first
`0x51` band: a state standing past `0x1E` has its first frame rows on a later seat's action, whose
states read the same, and the engine stops after the state's own. The channel's
number is `2 * matched / (retail cues + engine cues)`, once each id's pairs are put back
in time order (repeats play in order, and the nearest-free pick can cross them); `id score` is the same
over the id multisets alone, whatever the time.

`rates.csv` (retail only) holds each seat's rate `+0x21D` and node cursor `+0x68`
per vsync: a same-id timing miss on a reaction is usually the rate, read straight off it.

A missing cue is traced back with `--sfx-callers`, which also write-watches
the ring's id half (a `store` row is any store into `0x8007B6D8..DF`, `a0`
its pc; the ager `FUN_8001698C`'s clears at `0x80016AF4` are among them),
and with `seats.csv`, which both sides write: every seat's clip `+0x1D9`,
staged byte `+0x1DA` and cue cursor `+0x1F6` per vsync / tick (the engine
adds its reaction-channel entry). What the recordings on
`party_basic_attack_vs_gobu_gobu` and the monster-cast states found:

- **The defeat cue.** Static `0x0D`, late in every summon that kills, is
  stored by the clip commit `FUN_8004AD80` itself (`li v1,0xd` / `sh` at
  `0x8004B27C`, called from the natural-end commit `0x80047B54`): its
  monster-death arm writes the cue into ring slot `ctx[+0x09]` with its own
  cursor, once (behind `+0x1DC` bit 3, which the arm raises). The engine's
  death arm now does the same (`World::finish_battle_reaction`).
- **A reacting seat read as idle.** A slot-B module reads `+0x1D9` to skip a
  seat already reacting; the engine's mirror read the action channel, where
  a hit reaction never shows, so PROT 0904's sweep (Theeder) struck the same
  seat every pass and its reaction cue sounded on each.
- **Zenoir's opener.** Retail's arm 8 hit (`0x801F76CC`) takes the victim down
  by a quarter of the roll, clamped to `HP - 1`, and stages its reaction, whose
  clip cue `0x80` (ring `0x21C`, the router from `FUN_800508DC`) sounds on arm
  10's first pass. The engine lands it on the same pass
  (`World::apply_summon_pre_hits`).
- **Theeder's reaction.** The module's sweep stages the victim's reaction when
  the ray reaches the seat, about 33 ticks into arm 12 (`0x801F7D38`). The fold
  lands on that tick's hit, so the reaction and its cues start there.
- **A projectile's impact cue.** Aluru Cannon's landing rings `0x49`: the
  caller log reads `FUN_8004FCC8` from `0x801E185C`, the hit arm's
  `record[+0x0D]` ([move-power](../formats/move-power.md)). The engine's homing
  step now rings it per landed slot.
- **A victim's reaction holding `0x2E`.** The census's `+0x249` counts
  `+0x1D9`, where a hit reaction commits, so the victim's knockdown and get-up
  hold the magic band's exit. The engine counted the action channel and left
  `0x2E` on its second pass, moving the band's closing cues `43` ticks early.
- **A looping walk's second step.** `0x253` on a Nova's Scythe Wind walk rings once
  while `+0x1F6` steps twice: it is a sustained row, so the funnel drops the repeat
  ([the repeat rule](../formats/sfx-table.md#a-held-sustained-cue-drops-its-repeat)).
- **A reaction on the wrong clock.** Earthquake, the three Rogue strikes and Stone
  Circle part by a factor of two to six on every party reaction cue with the ids all
  right: each arm slows the seats it strikes
  ([writers](../subsystems/battle-actor-rendering.md#the-animation-rate-byte-actor0x21d)).
- **A deal is not a cue defect.** Void Accessories' three engine-only cues are its
  strip landing where retail's roll said "Nothing happened": `--rand-at 0x70:3` scores
  it `1.000`.
- **A flinch with its cues skipped.** Palma's three engine-only cues were its pre-hit's
  flinch: arm `0xF`'s pass stores `8` into each row's `+0x1F6` (`0x801F8360`), one past the
  track, and `seats.csv` reads the jump with no cue behind it.
- **A re-commit outside the juggle window.** Swordie's second slash re-staged its flinch at
  frame `8` of a clip whose window closes at `8`, so the cursor restarted and `0x21C` sounded
  twice: the engine's slash arm had left its pass early and the later slashes ran a pass late.
- **The flee's silent walk.** Both flee sites zero the walk record's cue track
  ([the staging](../subsystems/battle-action-helpers.md#the-escape-roll-fun_801e791c)), so a
  Spikefish flee sounds no steps.
- **A mid-cast state's first segment.** A `battle_*_mid_cast` recording opens part-way into its first
  state, and the projection counts from there, so every cue in that segment reads late by the state's
  age: Windhash's `+7..+11` is the state standing about nine vsyncs in. The arm's spacing agrees, and the
  head cue the engine plays before the state's instant reads as an extra.
- **The voice waiting on an idle loop.** A heal's `0x20C` fires on the commit state `0x3D` waits for, which
  the seat's idle clip holds off until its natural end. The recording injects on retail's idle cursor (`256`
  on `deene_garnet_pre_heal`) and the engine's child on its own (`16`), so the same clip of the same length
  reads `+6..+49` late. It is the seat's idle phase, not a late line.
- **A capped recording's last segment.** Fatal Decision's retail run stops inside arm 6 and the engine's
  segment runs on, so its wheel clicks read as extras past the cut. Both sides click every sixteen vsyncs.
- **A roll on the wrong side.** Nighto's kill roll takes arm 14 in the engine and goes round it in
  the retail recording (`13` to `15`, HP unchanged), and its three extras are that hit.
- **The drainer's gate never closed** (`0x8007BABC != 0x8007BAA0`): no
  retail recording has a cue behind it.

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
- **Sound.** The cue score, then the timing misses and the lone cues with
  their segment key.

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
- **Damage and RNG are not aligned** unless `--rand-at` holds the stream
  on one key ([The rand stream](#the-rand-stream)). Popups and HP readouts
  can differ.
- **A forced formation is retail-only.** See the flag table.
- **A monster cast needs `--monster-pick` unless the state queued it.**
  Without it the probe writes the monster's queue on the first `0x0A`, which
  the monster's own AI pick overwrites in the pass that reads it (below). A
  state that already holds the queued cast (`nivora_duel_pre_megaton_press`,
  the `*_pre_*` states the pick probe saved) records it either way. The
  engine half (`LEGAIA_BATTLE_MONSTER_CAST=seat,spell,p;ground`) hands the
  monster the round's first turn from the first prompt, so no pad drive is
  needed, and stands each slot on the ground retail's earlier actions left: a
  walk-in body walks from there (Koru's walked 840 units from its authored
  seat against retail's 432 before the ground was carried).
- **The target has to suit the spell.** A heal aimed at a monster seat parks
  the module (Orb on seat `3` sits in its phase `9`); pass `--target 0` for an
  ally spell. A party caster's group codes are the player picker's
  (`0x801D1CF8`, `0x801D1D68`): `--target 9` for every enemy (spell byte `0x64`),
  `--target 8` for the whole party (`0x24` / `0x26`). A group offensive spell
  injected with `8` frames the band as a party-wide cast and scores low on the
  framing alone. The probe quits on a park (`LEGAIA_PARK`, state and module phase
  unchanged for `900` vsyncs) and the driver reports it.

`--engine-only` on a state with no retail recording runs the engine half
alone. The corpus's `*_summon_mid_cast` states are mednafen captures that
PCSX-Redux cannot load, but their child commands seed the cast. That is
enough to read the engine's own segment lengths for a cast in the fight the
corpus scores it in.
- **An item cast (the flutes) is not injectable.** The probe writes category
  `2` (Magic). An item summon needs the item category and its own id space.
- **A cast longer than `LEGAIA_MAX_SHOTS` (default `1500` grabs) is cut.** At
  `--every 2` that is 3000 vsyncs: the closing arms of Meta and Ozma carry no
  pairs. Set the variable higher to score them.
- **The caster need not know the spell.** An injected cast of an unlearned
  spell rolls a capped hit
  ([battle-formulas](../subsystems/battle-formulas.md#a-spell-the-caster-has-not-learned)),
  and the engine mirrors it, so popups match but read `9999`.

## A monster cast from any battle state

A monster's turn runs its AI pick `FUN_801E9FD4` inside `FUN_801DABA4`
(`jal 0x801E9FD4` at `0x801DAF48`, `a0 = ctx[+0x274] - 3`) and calls
`FUN_801DB124` straight after (`0x801DAF50`), in the same pass as the
category read, so no vsync hook can get between the pick and its use. With
`--monster-pick` the probe arms a breakpoint on `0x801DAF50`: the seat is
`ctx[+0x274]`, and the queue the pick has just written is final and unread
there. It writes `+0x1DE = 2`, `+0x1DF = spell`, `+0x1DD = target` - the same
two stores the AI's own cast arms make (`0x801EB7C0..0x801EB81C`), plus the
target - and tops the seat's MP up. The rest of the turn is retail's.

`--save-inject` then keeps the state one vsync later, the cast queued and
nothing else changed, for the save library. The states saved this way are
labelled `<fight>_pre_<spell>` in `scripts/scenarios.toml` and replay on
the fast core with a plain `--caster`:

| State | Cast |
|---|---|
| `koru_pre_blade_breath` / `koru_pre_thunder_needle` / `koru_pre_gigaton_press` | Koru's `0xA2` / `0xA3` / `0xA4` (PROT 0962), on `koru_strip_forced`'s synthetic formation |
| `gaza2_card_pre_neo_star_slash` | the Sim-Seru Gaza's `0xA6` (PROT 0960), party-wide target |
| `cort_evolved_card_pre_doomsday` | the evolved Cort's `0xB6` (PROT 0965), party-wide target |
| `gaza2_pre_neo_star_slash` / `cort_evolved_pre_doomsday` | the same two casts; **patched resident executable** |

The action in each is the probe's, not the AI's: a capture is a statement
about the cast, not about when the monster would have chosen it. A
`<scene>_<monster>_pre_<spell>` state from the
[harvest route](capture-coverage.md#filling-the-worklist) is the opposite: saved
at the action's `0x0A`, ahead of a pick the AI makes itself. Pass the seat the
fight gave the caster (the formation row's order, which `--filter`'s report names as
`seat=`). The probe writes the target the cast began on to `done.txt`
(`cast_target`), and the engine child lands a single-target cast there
(`,t<slot>` on `LEGAIA_BATTLE_MONSTER_CAST`): the victim's voice cue and every
victim shot follow it.

A cast with a pad-driven hold needs `LEGAIA_PARK` past it: Fatal Decision's arm 6
holds about `1200` vsyncs on an undecided party victim, which the default bound
reads as a park.

A saved state keeps the executable its source state held. The first Gaza 2
and evolved-Cort states were made on patched discs (`battle_gaza2_prompt`,
`cort_evolved_battle_first_menu`), so the two casts saved from them carry
the same `resident_patch`; the driver refuses a `resident_patch` state
unless given `--allow-patched`. The `*_card_*` states are their retail
replacements: a cold boot of a staged image, a memory-card CONTINUE
(`playthrough-ladder-pro00-14` block 11 for Sol Tower, `playthrough-endgame-7saves`
block 6 for Bio Castle), the player set beside each fight's walk-on band
and walked onto it, and the fight's own scripted entry (`korb3` P2[2]'s
`3E FF 0F`, `jouine`'s arrival record). `gaza2_card_first_menu` and
`cort_evolved_card_first_menu` stand at the first command input; the
evolved Cort's arrival holds the flow on `0x0C` for about 3200 vsyncs
([battle-command-flow.md](../subsystems/battle-command-flow.md#flow-0x0c-is-the-boss-stage-modules-baton)),
so a probe run from the fight's opening needs `LEGAIA_PRESS_UNTIL` past it.

### What a harvested state needs passed

A `<scene>_<monster>_pre_<spell>` state replays the AI's pick, but the probe's
`--target` store into `+0x1DD` is the one the cast begins on (`cast_target` in
`done.txt` follows it, whatever seat the pick chose). So:

- Pass the seat the corpus's `*_mid_cast` state of the same cast holds, and for a
  group spell `--target 8`: the [spell table](../formats/spell-table.md#target-shape-2)
  reads `0x64` for all opponents, `0x44` for one. A `--target 0` on Multi Cutter
  records retail hitting Vahn alone against the engine's sweep and reads `.533` for
  `.813`; a `--target 8` on Astral Wave (`0x44`) records retail sweeping the party
  against the engine's single hit.
- The harvester plays Spirit, so a state saved in those rounds holds every party seat
  at `+0x1DE == 4`. Pass `--guard 0,1,2` or the engine's victims take three to four
  times retail's damage. A `--monster-pick` run several rounds on from a `*_strike`
  state has tapped Attack since, and takes no `--guard`.
- A monster-pick run from a `*_strike` state wants `LEGAIA_PRESS_UNTIL=7000` and
  `--frames 12000`: the default window ends at the next round's command input. Some
  still inject nothing in that span (`inject=-1`). `--physical --retail-only` from the
  corpus's `*_mid_cast` state then records the cast from its instant, and
  `--spell .. --engine-only` runs the engine side, which pairs by arm.

A `--physical` recording runs on past the cast into the next action, and its `done.txt`
names that action's caster and target, which the engine half then reads. Patch the
recording's `caster=` and `cast_target=` to the state's own seat and cut its `sfx.csv` and
`meta.csv` at the cast's Done band before scoring, or the engine dispatches another cast.

### The target has to be the engine's

The engine half dispatches the monster's cast with the target class its
spell table gives (`World::pick_monster_action`): a party-wide spell aims
at `+0x1DD = 8`. The battle HUD keys on that byte: a party-wide action
keeps the three party panels up through the cast
([battle-hud.md](../subsystems/battle-hud.md)), a single target shows one
plate. A capture injected with `--target 0` for a party-wide spell records
retail's one-plate HUD against the engine's three panels in every frame:
Neo Star Slash scores `0.566` that way and `0.789` with `--target 8`,
Doomsday `0.567` and `0.770`, with the same engine. Inject the target the
monster's own pick would write.

## Waits the engine does not run

The capture band's two CD polls are the exception: the engine holds them
([below](#the-capture-bands-reads)). Three waits in a summon band are CD reads, which the engine's resident
streams answer at once, so their segments part on length without any engine
error. The pairing keys each segment on its own, so they do not shift the
rest of the cast's pairs; they do move the band's flash against the module's
arms. Measured on the `party_basic_attack_vs_gobu_gobu` casts (Horn, Jedo,
Freed), each a frame-step-`2` run:

| Wait | What it polls | Retail | Engine |
|---|---|---|---|
| `0x32` | `FUN_8003DE7C(1)`: with `0x8007B8C2` set, the pending-read word `gp+0x988` and the CD sync `FUN_8003F2B8(1)` (`0x8003DF70..0x8003DFFC`), the read the cast trigger started in `0x29` | `16` vsyncs, ending about `38` after `0x29` begins | one pass |
| the side-band arm (below) | `ctx[+0x276]`: the applier `FUN_801F12D0` requests and consumes one `0x10800`-byte slot per stage pair, from the `1` state `0x32` stores | `28` to `30` vsyncs a request, `1` or `2` a consume; four slots for Horn and Jedo (the stage drops at `8`), three for Freed (at `6`) | the byte is never raised |
| a stream-poll arm | `FUN_8003F2B8(1)`, the creature's own read | a few vsyncs | one pass |

The read-span words themselves (`xaspan`, `cdbusy`, `cdmode`, `cdread`) hold
still through the whole band in these casts, so the `0x32` wait is the
pending read, not a voice clip's span.

Every summon module `0903..=0934` polls `ctx[+0x276]` once, on one arm
([cast-module.md](../subsystems/cast-module.md#the-side-band-poll-arm)), so
in retail that arm holds while the applier runs on past the band head.
Horn's arm 2 ends on the pass the byte drops. Jedo's band flash-out reads
about `25` vsyncs younger in the engine at the same arm, which is most of
that cast's low score in arms 2 to 4.

### A fight that opens on an ambush

`LEGAIA_BATTLE_MONSTER_CAST`'s `p` stands each listed slot on its ground at the first prompt.
A fight that opens on an ambush reaches the cast in that round's own order, ahead of any
prompt, so the turn arming stands the slots there as well (`World::stand_on_forced_ground`).
Without it Lapis Wave's rings and columns stand on seats the fight never held.

### The capture band's reads

State `0x6E` polls `FUN_8003DE7C(1)` (`0x801E4F08`) for the read `0x28` queued, then
`0x8003EAE4` starts the module's own read, which `0x6F` polls through `FUN_8003F2B8(1)`.
The recordings of the harvest-state casts hold them `14..=17` and `4..=6` vsyncs, whichever
module or fight. Neither poll idles: each `0x6E` pass walks the case-6 framing and each
`0x6F` pass pulls `ctx[+0x6D0]` in by `step * 16`, so an arm-0 tween opened two ticks in
stands on another pose for its whole length (Spore Gas read yaw `1991` where retail held
`1121`) and every shot built on `ctx[+0x6D0]` lands at another depth. The engine holds the
two states `14` and `4` ticks (`World::arm_capture_read_wait`), and the walk then matches
retail to the unit through `0x6E`.

## See also

- [Retail comparison corpus](retail-compare.md) - the per-state breadth sibling and its image metric
- [cast-module.md](../subsystems/cast-module.md) - the slot-B module phase machine the key follows
- [PCSX-Redux automation](pcsx-redux-automation.md) - the probe runner and its traps
