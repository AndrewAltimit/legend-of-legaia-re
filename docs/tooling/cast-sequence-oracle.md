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
caster `3500` units from the ground retail recorded. It drops the still's pin
to the default step too (`LEGAIA_BATTLE_PARTY_STEP=0`, for a replay whose gate
counts passes): a film plays the cast through on the party's step, which
`chitei2_cort_pre_mystic_circle`'s recording runs from its first frame, and on the
pin Mystic Circle read `.717` against `.852` without it. Over the strike and enemy
cast films the drop lifts most (`ropeway_curry_devil` `.63` to `.90`,
`geremi_ogre_strike` `.72` to `.93`); the two Death Game films and
`jou_cort_181_strike` read lower off the pin, which had held them on the step their
bodies were tuned to ([the frame step](../subsystems/battle-stage-camera.md#the-capture-band-against-the-films)). The scroller rects
(`LEGAIA_SEAT_VRAM_RECTS`) are the exception: the child holds them only until the
cast's first action pass (`LEGAIA_SEAT_VRAM_RECTS_UNTIL_ACTION`), then lets them
roll. A scrolled page's phase is time since its scroller started, which a pre-cast
state carries and the engine's fresh battle does not: the Cort states whose shield
was raised a turn earlier start the sphere's bands where retail's stand, and
`cort_guilty_cross` reads `.801` against `.611` without the seed. The hold lasts through
the fresh fight's intro timer `0x0A` / `0x0B` (the plates and the banner), which the
engine runs ahead of the cast where the state stood past it: released on `0x0B`, the
shield's page rolled through the whole intro and stood `72` rows on at the cast's
`0x28` (a `LEGAIA_DIAG_CAST_VRAM_AT` image against the seed), and the sphere drew
another band's colour through every arm. Released on `0x0C` it stands `3` rows on,
and `chitei2_cort_pre_mystic_circle` reads `.676` to `.893`,
`chitei2_cort_pre_guilty_cross` `.686` to `.826`. A harness seat.

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
| `--hit-trace` | Retail: each melee hit's `rand()` seed word to `retail/hits.csv` (breakpoint core), read at the head of `FUN_801EC3E4`'s hit body (`0x801EC5A8`); `--strike-deal` deals these words in place of the spark-burst reading ([below](#a-monsters-physical-attack)) |
| `--hit-trace` block columns | The same recording's `hits.csv` also carries each hit's block-roll inputs (both sides' SPD `+0x164` and ATK `+0x158`, `ctx[+0x6D2]` / `+0x6D4`, the defender's category, clip, block entry and juggle window), and `blocks.csv` the two rolled sides and the verdict at `0x801EC878`; `LEGAIA_DIAG_BLOCKS=1` (through `--engine-env`) prints the engine's |
| `--ribbon-trace` | Retail: each streak-ribbon call `FUN_801E1D98` to `retail/ribbons.csv` (breakpoint core), read at `0x801E1E4C` past its billboard projection: the seed word, the caller, the anchor trio, the four projected corners and the GTE view (`R`, `TR`, `OFX`, `OFY`, `H`), the inputs a ribbon's segment count and so its `rand()` draws read |
| `--popup-trace` | Retail: each damage numeral `FUN_801DF6B8` projects to `retail/popups.csv` (breakpoint core), read at `0x801DF820`: the ring timer, the projected corners, the ordering-table depth and the GTE view |
| `--ghost-trace` | Retail: each after-image ghost the draw tick's walk `FUN_80049348` draws to `retail/ghosts.csv` (breakpoint core), a row per `jal 0x80048A08` at `0x8004950C` with the frame counter, the ring depth and the seat: the ghosts a frame's `cost` was measured with |
| `--range-trace` | Retail: each call of the range law `FUN_8004E2F0` to `retail/range.csv` (breakpoint core): its caller, the attacker's live pair and the target's live pair, body pair, heading, node cursor and clip, the inputs a walk-in's reach poll reads |
| `--wrapper-trace` | Retail: each capture-class damage roll to `retail/wrappers.csv` (breakpoint core): at the entry of `FUN_801DD4B0` / `FUN_801DD6B4` the power, slots, seed word and the stat words the rolls read, then the two roll words `sp+0x18` / `sp+0x1C` before and after the finisher ([a roll against retail's](#a-capture-class-roll-against-retails)) |
| `--sfx-callers` | Retail: breakpoints on the cue producers `FUN_80035B50` / `FUN_80035BD0`, the battle router `FUN_8004FE5C` and the dispatcher `FUN_8004FCC8`, each call's `a0` / `a1` / `ra` to `retail/sfxcall.csv` (breakpoint core): who fired a cue |
| `--gpu-cost` | Retail: `gpu.csv`, per vsync the frame-cost words `gp+0x3A8` / `gp+0x3AC`, the ring's newest entry, the GPU draw-time model of the table the GPU was handed last (`lib/probe/gpu_cost.lua`, draw and DMA in hblanks, polygons) and the PC and `ra` at the vsync; [`frame-cost-agreement.py`](../../scripts/ci/frame-cost-agreement.py) scores it ([a real console's frame cost](../subsystems/battle-stage-camera.md#what-a-real-consoles-frame-costs)) |
| `--cd-trace` | Retail: `cd.csv` (breakpoint core), each libcd command call (`FUN_8005CF80` / `FUN_8005D0B8` / `FUN_8005CCB4`, the command, its first parameter byte, `ra`), CdSync's entry and return, the response dispatcher `FUN_8005DAB0`'s entry, interrupt causes, callbacks and exit, every exception at `0x80000080`, and the frame's `VSync(n)` return and CPU-done marks, each with the CPU cycle. [`cd-poll-steps.py`](../../scripts/ci/cd-poll-steps.py) prices each frame's CD interrupt time ([the CD stop watch](../subsystems/battle-stage-camera.md#the-cd-stop-watch)) |
| `--console-cd` | Engine: price a voice clip's CD stop watch at a console's response time (`LEGAIA_BATTLE_CD_TIMING=console`, opt-in). Without it the film runs the engine's default PCSX-Redux timing, the machine the recordings ran on ([the CD stop watch](../subsystems/battle-stage-camera.md#the-cd-stop-watch)) |
| `--gpu-cost-step` | Engine: a battle pass's ring entry is `max(cpu, gpu)`, the GPU half priced over the frame the window drew last (`LEGAIA_BATTLE_GPU_COST=1`, `WorldToggles::battle_gpu_cost_step`, opt-in). Not a harness seat: it changes the engine's own clock ([the engine's GPU source](../subsystems/battle-stage-camera.md#the-engines-gpu-frame-step-source)) |
| `--retail-parts` | Retail: `parts.csv`, every live move-VM part per vsync (`+0x10`, colour word, level, draw kind, `+0x5A`, `+0x9E`, packed colours, `+0xB4..+0xBA`, the position halfwords `+0x14..+0x18`, scale, `+0x52`, `+0x9C`, UV / tpage / CLUT, H / S / V adds, view depth `+0x34`, `+0x7A`, `+0x6D`, `+0x42`, the rotation banks `+0x24..+0x28`): the retail half of `LEGAIA_DIAG_CAST_PARTS`. Beside it `fxpool.csv`, every live child of the effect pool `_DAT_8007BD30` (slot, frame count, cursor, position `>> 8`): where the 2D sprites, a burning body's puffs among them, sit |
| `--pose-objects` | Retail: `pose.csv` (breakpoint core), at each burning-body emit (`0x8004A61C`) on the summon creature, its cursor, live pair, facing and decoded object table; `LEGAIA_DIAG_CAST_PARTS` writes the engine's `pose.csv` from `stream_pose` |
| `--prim-calls` | Retail: `primcall.csv` (breakpoint core), each prim dispatcher entry with its word, level, caller and first group words, and each cued leaf: whether a part reached the dispatcher at all, and by which leaf |
| `--learned <level>` | Both sides: the spell sits on the caster's list at this level. Off the list `FUN_801DD864` reads list slot `0x20`, and level `0` wraps into a capped hit ([the summon roll](../subsystems/battle-formulas.md#a-spell-the-caster-has-not-learned)), so a damage measurement wants this |
| `--monster-hp <hp>` | Both sides: every living monster's HP from the cast begin, so a cast's whole damage shows in the HP columns. HP feeds the defender roll (`hp >> 8`) and a monster's own swing, so keep it modest and it goes in after the monster's turn |
| `--guard <seats>` | Engine: party seats in the Spirit stance when the cast starts (`LEGAIA_BATTLE_GUARD`; `-` = none). Defaults to the probe's `guard=` (the seats at `+0x1DE == 4` on cast begin), and for a recording without that field to the state's own stance unless the run is `--monster-pick` |
| `--monster-pick` | Retail: with `--caster 3..7`, the cast is written where the monster AI's pick returns (a breakpoint on `0x801DAF50`, see [below](#a-monster-cast-from-any-battle-state)), so any battle state that reaches the monster's turn records any spell the monster owns a clip for. Runs on the breakpoint core, several times slower |
| `--cast-queue <hex,..>` | Retail, with `--monster-pick`: the bytes behind the spell at `+0x1E0` (clip, shot, `ff`). A pick that was not a cast leaves no terminator there and `0x2A` parks on it; read the bytes off a capture of the same cast (`battle_korb2_aluru_cannon_mid_cast` holds `3F 09 0E FF`) |
| `LEGAIA_BATTLE_MONSTER_CAST=seat,spell,p,t<target>` (engine child) | The driver passes `--target` as `t<seat>`, so a single-target cast lands on retail's victim; a group target stays the cast's own class |
| `--rand-track` | Engine: seat every module-band pass on the `rand()` word retail's frame before it ended on (`LEGAIA_CAST_RAND_TRACK`, [below](#the-rand-stream)). A harness seat |
| `--retail-steps` | Engine: run on the recording's own frame steps (`LEGAIA_CAST_STEP_TRACK`, `RetailStepTrack`), frame by frame under each key. A harness seat: what remains in the film is the law's, with the frame's cost taken out ([below](#the-camera-against-the-step)) |
| `--rand-at <st7>:<phase>` | Engine: hold the world `rand()` stream on retail's seed word under this cast key (`LEGAIA_CAST_RAND_SEED`, [below](#the-rand-stream)). `--rand-at 0x70:3` on Gigaton Press gives arm 3's pass retail's sixteen rock picks |
| `--allow-patched` | Measure a state whose manifest entry carries `resident_patch` (refused by default: retail states only) |
| `--save-inject` | Retail: keep the whole emulator state one vsync past the inject (`retail/state_inject.rawsstate`), a pre-cast state that replays the cast on the fast core with a plain `--caster` |
| `LEGAIA_SEAT_SIDEBAND_SEEK=<vsyncs>` (engine child) | The recording drive's seek into the first `summon.dat` slot; the driver passes `0` ([the side-band read](#the-side-band-reads-seek)), `--engine-env` overrides it |
| `LEGAIA_CAST_SEQ_EXE=<path>` (environment) | Score another `legaia-engine` build (a before / after pair) without rebuilding the tree's |
| `LEGAIA_PRESS_UNTIL=<vsync>` (environment) | Retail: tap CROSS through the party's command input until this vsync (default `600`). A three-member party or a long opening needs more before the monster's turn comes round |
| `LEGAIA_POKE32=addr=value,...` (environment) | Retail only: words the probe writes at the inject. A capture-class module that forks its choreography off RAM rather than the action id needs it: PROT 0946's arm 0 takes Big Wave's branch only while the caster's latch `0x801C8FE0 + (seat + 1) * 4` is set, so a seat-0 cast of `0x56` needs `0x801C8FE4=1` and otherwise records Call Wave |

The engine recorder also writes `rates.csv` (no header), per tick and per retail seat the
animation rate `+0x21D` and the node cursor `+0x68`, the engine half of the probe's file,
and a `guard` column in `meta.csv`, the party seats in the Spirit stance. A reaction's pace
reads off the first: on Water Crystals both sides now walk Vahn's knockdown through the same
cursors (`0, 8, .. 32, 44, 54, 62, 68, 72, 74`), the engine one step-`4` pass ahead, which
is the pairing rule's offset.

`LEGAIA_DIAG_CAST_PARTS=1` beside the engine recorder (`--engine-env`) adds
`parts.csv`: every live part of the cast's scene graph each tick - record
`model_sel`, halted, colour word `+0x74`, level `+0x78`, `+0x9E`, the packed
colours `+0xA0` / `+0xA4`, the radius and height channels `+0xB4..+0xBA`, the
position, then the draw kind `+0x56`, scale `+0x72`, `+0x52`, the count
`+0x9C`, the UV words `+0xA8` / `+0xAC`, tpage `+0xB0`, CLUT `+0xB2`, the
default-arm mesh's vertex count, the spawn record's address, the flag word
`+0x10`, the angle trio `+0x24..+0x28`, `+0x7A`, a mode-3 cycler's H / S / V adds
`+0x90..+0x94` and its gate `+0x9C` - the engine half of a
part-by-part comparison. It writes `fxpool.csv` beside it, the effect pool's live
children (tick, slot, frame count, cursor, position, anim id). It is how Neo
Star Slash's arm-6 excess was traced to one cylinder's back faces
([renderer](../subsystems/renderer.md#the-field-pass-culls-back-faces)).

`LEGAIA_DIAG_CAST_HIDE=<va>,..` (also through `--engine-env`) leaves the cast
scene's parts seated from those record addresses out of the draw lists while
their programs run on: a film whose frame shows something no retail frame
holds is re-recorded with candidate records hidden until the shape goes, or
stays. Aluru's arm-11 dark triangle stayed with every arm-11 record hidden,
which put it on the creature rather than the cast. The record addresses come
off `parts.csv` and read with `asset move-disasm slot-b`
([move VM](../subsystems/move-vm.md#reading-a-program-asset-move-disasm)).
Retail `parts.csv` recordings older than the probe's `s16` position columns
hold `s32(+0x14)`, `s32(+0x18)`, `s32(+0x1C)` under `x`, `y`, `z`: their `y`
is `+0x18`.

Set against a retail recording made with `--retail-parts`, the file reads a mechanism
off directly: Spoon's fans stood at scale `0x400` beside retail's `0x666`, Barra's cone
at `z = 0` beside `456`, the Rogue's ray emitter on the floor beside retail's
`(28, -388, 1007)`, the caster's display trio. The position columns are the halfwords
`+0x14 / +0x16 / +0x18` on both sides. Each retail part appears once per list walk of its vsync, so
compare by value, not by count. The same goes for `cam.csv` against the pairs of
`report.json` (`shown_vsync`, `engine_tick`): a constant difference through an arm names
a store or a drift, one that grows names a rate.

`LEGAIA_DIAG_CAST_VRAM_AT=<tick>[,<tick>..]` writes the battle VRAM at those ticks as
`engine/vram_<tick>.bin`, to hold against a retail recording's
`LEGAIA_RAM_AT` state through `sstate_vram.py`. A byte comparison of the side-band pages
and CLUT rows is how a palette or texture reading is settled before a frame's colours are
argued over: Iota's cycler rows match the kernel entry for entry.

### A capture-class roll against retail's

A `--wrapper-trace` recording reads a cast's damage off its inputs rather than off the HP columns. V-Windhash from `battle_kor_ghost_knight_v_windhash_mid_cast` enters `FUN_801DD4B0` with power `0x7B`, the Ghost Knight's INT `+0x168 = 302` and Vahn guarding (`+0x1DE = 4`), leaves the roll at `750` against `500` and the finisher at `625`, so `125` lands. The engine on the same draws reads the same three pairs. A film whose child command was seeded before the seat pin (`LEGAIA_BATTLE_SEATS`) seats the fight in the formation's order, so the cast came from the other monster (INT `140`) and landed `63`: re-seed (`--reseed`) a recording copied from an older run before reading its numbers.

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
token a module's stream-wait arm polls) and the words the stream poll
`FUN_8003DE7C(1)` reads (`xaspan` `gp+0x91C`, the countdown only the
development path steps, `cdbusy` `0x8007BA70`, that path's cell,
`cdmode` `0x8007B83C`, `cdread` `0x8007B8C2`), the BIOS `rand()` seed word
`0x80009010` (`rand`), the frame-cost ring's newest entry (`cost`: `0x80084098`
at the cursor `gp+0x440` less one, the hblanks the last finished frame cost) and which ordering table links a screen fade's quad
(`fade_back`: `2` the current record, `1` only the other, `0` none; see
[the pairing rule](#the-pairing-rule)). A second file, `body.csv`, holds
per vsync the acting seat's clip `+0x1D9`, node cursor `+0x68`, live pair,
body pair `+0x3C` / `+0x40` and the camera focus with its tween endpoints,
then the heading `+0x46` and the live and body pairs of the seat's target
`+0x1DD`: the series a summon close-up's framing and a strike's walk are read off. A third, `cam.csv`, holds
per vsync the live pitch, yaw and TR trio with the yaw and TR z records of the
tween table `ctx[+0x118C]` (increment, endpoint): the walk of a module's shot.
At cast begin it also logs each pool slot's ground pair
`+0x34` / `+0x38` and facing `+0x46` (`done.txt`, `x:z:facing`). With `LEGAIA_PRE_TRACE=1` the probe also
writes `pre.csv`, one row per in-battle vsync **before** the inject: the
action state, the acting seat, the frame step, the yaw counter `ctx[+0x6DA]`,
the strike cursor `ctx[+0x15]`, and for seats `0..3` the committed and staged
clip, the rate `+0x21D`, the node cursor, the live pair and the per-action
total `+0x00` (seat 3's live HP), then the battle camera: the live pitch, yaw,
TR trio and focus trio, the depth word `ctx[+0x6D0]` and the tween table's nine
`step:end` records. `LEGAIA_PRE_SHOTS=1` adds a frame (`pre_NNNNN.raw`, named in
the row) on each vsync of a strike, return or Done state (`0x1E..0x20`,
`0x50..0x5A`). A monster-pick run from a `*_first_menu`
state therefore records every party turn on the way to the cast - the retail
timeline of a combo the capture only holds the end of, and the camera through
its Done tail (`koru_strip_forced` with `--monster-pick --retail-only` films
Gala's Lightning Storm into the Done arm the `koru_pre_*` states hold). The run uses the recompiler (`--fast`) unless a
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

Each seat's HP travels from the recording's cast begin (`retail_cast_hp`), over the state's HUD bars. A `--monster-pick` recording plays the party's turns to the pick, so the caster and its victims stand at other HP when the cast begins, and the capture-class wrappers fold the attacker's `hp >> 8` into the roll (`0x801DD728`). Bloody Horns from `battle_tunnelc_xain_strike` lands `595` on Gala in retail; on the state's bars (Xain at `9511` where retail's turns left `7145`) the same draws read `605`, and seated it lands `595` on `--rand-track`. A harness seat.

A `--physical` film keeps the state's drive, which commits Spirit for the whole party while a monster casts (`BattleDrive::Action`), so on its own its victims stand guarded whatever retail's party held.
The film therefore stands the party in the recording's stance exactly on its gate tick (`LEGAIA_FILM_GUARD`: the recording's `guard=`, else the state's own `+0x1DE`), a member out of the stance included, and sets the world `rand()` stream to the state's own seed word `0x80009010` there (`LEGAIA_FILM_RAND`; `--no-state-rand` leaves it, `--rand-at` / `--rand-track` replace it, and a `--strike-deal` film, which seats every hit on its own word, does without it: on top of the deal it moved `battle_map02_mad_ogre_strike`'s landed hit onto the other swing).
The gate is the state's instant, so both are the state's, and the replay's draws and commits before it stop mattering.
`battle_chitei2_gilium_thunder_storm_mid_cast` had landed `147` / `157` / `130` on a guarded party against retail's unguarded `583` / `745` / `544`; seated it lands `583` / `745` / `544`, and its film reads `.893` to `.963`. `battle_tunnelc_xain_terio_punch_mid_cast` lands retail's `366` / `362` / `347` and reads `.857` to `.958`.
`battle_korb2_ghost_knight_neo_windhash_mid_cast` reads `.893` to `.967` and lands `807` against retail's `771`: the two streams agree draw for draw through module phase 3, and the hit pass draws five `rand()` in retail and four in the engine, the rest of the film one draw apart. Harness seats.
Seated, the stream also names an arm's pass count. `battle_retona_lip_king_kiss_of_death_mid_cast`'s two streams agree draw for draw until its hit roll, but the engine's arm 4 runs `31` passes where retail's runs `33`, so the roll lands two draws early and misses where retail's hits (its cues `3 / 3` to `0 / 3`); on the replay's own stream it had rolled a hit by chance.

A monster's AI counter `0x801C8FE4` travels with the state
(`LEGAIA_SEAT_AI_COUNTER`): PROT 0964's strike forks on it, and a replay that starts
from a cleared word plays another element's arms.

The child's mode-3 cycler and mode-4 scroller seats (`LEGAIA_SEAT_CLUT_FX`,
`LEGAIA_SEAT_VRAM_RECTS`) are dropped with the drive: each holds one capture
instant, and a whole run under them leaves the evolved-Cort arena's flesh on
one palette and one scroll where retail's keeps cycling. Their phase is then
the engine's own: nothing aligns the hue clock with retail's across the
fight's history.

The backdrop's slot-1 angle (`LEGAIA_BATTLE_BACKDROP_YAW`,
[battle-stage-camera](../subsystems/battle-stage-camera.md)) is kept, since
its phase is the capture's, but a gate film lets go of it on the gate's frame:
retail's frame driver winds it half a frame step a frame from there on, and
held through the film it stood the horizon mist or cloud band still behind
every strike on a stage that keeps object 1. A film from a pre-cast state lets go on
the cast's begin `0x28`, the recorder's first frame: held to the end,
`rikuroa_caruban_pre_fire_breath`'s Fire Breath stood `rikuroa`'s horizon mist arc
still while retail's swept across the HP-bar settle `0x51`, and the film read `.868`
where it reads `.955` let go (`0x51` `.689` to `.943`). Every pre-cast film on a stage
that keeps object 1 gains with it: over the enemy set's pre-cast films the mean goes
from `.882` to `.912` (`rikuroa_pump_bat_pre_windcutter` `.918` to `.989`,
`balden_orb_v2_pre_jewel_beam` `.860` to `.934`).

The child is a `--screenshot` run, so it is tick-locked, and the recording is
deterministic. The recorder starts on cast begin `0x28` and writes the frame
already cropped and box-filtered to the compared `320 x 224`
(`Frame::from_engine_capture`, the corpus's alignment). Its `meta.csv` uses
retail seat numbers: the engine packs the party ahead of the monsters, and the
recorder maps them back. Beside it, `cam.csv` holds the live camera pose per
tick (pitch, yaw, TR trio), the engine half of the retail side's file, and
`body.csv` the acting seat's clip `+0x1D9`, staged byte `+0x1DA`, whether the
clip loops, its cursor in sixteenths of a keyframe, the live pair, then the
body pair, heading and the target's two pairs: the engine half of the retail
`body.csv`, which a clip that ends early or late, or a struck body that does
not slide where retail's does, is read off.

The child drops the capture's drive and runs no earlier actor, so nothing in it left
the camera where retail's stood. On cast begin it takes retail's pose from the
recording's first row of that state (`LEGAIA_DIAG_CAST_SEQ_SEAT_POSE`, `retail_opening_pose`
in the driver): `cam.csv`'s pitch, yaw and eye with `body.csv`'s focus. This is a
seat of the harness and moves no game state. Without it `0x28` and `0x29` score
wherever the child's camera happened to stand; with it their framing tracks retail's to
a couple of units, and the opening's error is the frame's own. A `--physical` film opens
on the state's own instant, mid-glide wherever the state sat, so it seats on the gate's tick
the camera the state's own RAM holds, as the retail-compare seed read it (`rc/report.json`'s
camera detail, `state_camera_pose`): its drive plays the round through the cast's own `0x28`
first, which would take the seat there. The recording's first row is not that instant: the
probe writes it once the first vsync has run, and on a capture-band state that vsync can
hold a module's pass. `battle_map02_lip_kid_kiss_of_death_mid_cast` sits in `0x70` ahead of
PROT 0955's arm 0 at TR z `4729`; the first row stands at `3943`, one step-`3` pass into arm
0's twelve-frame shot (increment `262`). Seated on that row, the engine's arm 0 built its
shot again from `3943`, on an increment of `197`. Seated on the state, it builds retail's
`262`. The state seat lifts the `battle_map01_lippian_kiss_of_death_mid_cast` film from
`.885` to `.905` (arm 1's pull-in `.689` to `.762`) and the Lip Kid film's arm 0 from `.677`
to `.716`; a recording older than the report falls back on the first row.

A state saved past a module arm's shot carries two more seats on the gate. The replay's arm
built its shot from wherever its own camera stood, and the pose seat moves the live words but
not the table: `battle_map01_lippian_kiss_of_death_mid_cast`'s arm-0 shot walked TR z by `265`
a vsync and never moved the yaw (the replay's caster had faced elsewhere when arm 0 ran),
where retail's table walks `301` and `14`. The still's child names retail's endpoints and the
vsyncs its table has left (`LEGAIA_SEAT_MODULE_SHOT`, `RetailBattle::module_shot_seat`), and the
film re-arms the shot onto them from the seated pose; the film's camera then tracks retail's to
a unit or two through arm 2. The caster's clip cursor is the other: a module that gates on its
countdown leaves the clip free-running, and the replay reached the gate `128` cursor units
ahead of retail's, so the lips stood shut where retail's frame has them open. A `--physical`
film seats the state's own RAM cursor on the gate (`state_opening_cursor`), falling back on the
recording's first row (`retail_opening_cursor`'s `first_row`) when the state cannot be read. The
first row is a pass late: the probe writes it after the first vsync has run, so seated on it the
caster stands a pass ahead. Over the films both seats were measured on, the RAM's cursor reads
higher or equal on every one: Kiss of Death `.915` against `.907` (arm 2 `.901` against `.857`),
Lip King `.981` / `.959`, White Shield `.967` / `.960`, Bull Charge `.968` / `.950`,
`battle_geremi_ogre_strike` `.985` / `.973`. Both seats are harness seats.

**The ground carries the facing.** An earlier actor of retail's round leaves its seat
turned, and the range law measures a target by its body pair, the pose centroid turned by
that facing. The engine child stands each slot on the recorded facing beside its pair
(`BattleState::forced_ground_facing`, read from the `x:z:facing` tokens of
`LEGAIA_BATTLE_MONSTER_CAST` / `LEGAIA_BATTLE_INFLIGHT`). Recorded from
`battle_tunnelc_xain_strike`, Gala had struck Xain earlier in the round and faced `839`;
her body pair stood `425` from Xain against a reach of `416`, so Bloody Horns' arm 0 walks
one pass in. Unturned she stood `398` off, the engine skipped the walk and Xain cast from
`67` units further back: the film read `.799` that way and `.873` seated. A recording made
before the probe wrote the facing carries bare `x:z` pairs and replays the unturned stand,
so re-record it (`--recapture`) rather than read a skipped walk as a port defect: re-recorded,
`--range-trace` reads arm 0's poll (`ra = 0x801F7280`) on Gala's body `(85, 374)`, the
engine's on `(84, 376)`, and both walk Xain one step to `(388, 575)`.

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

The party Tactical Arts chains in `jou` are the same shape. Retail's frame cost (`cost`,
the ring's newest entry) climbs over the strike loop from `551` to `699` hblanks and
reaches the clamp a frame ahead of the arts shout's commit, so the commit pass already
runs at step `4`; the engine's costs sit at `630..656` and reach the clamp on the pass
after the cue. Mid-stream, retail has one frame at `652` (step `3`) between clamped ones.
A frame within a few hblanks of the `0x2D0` boundary is the emulator's cost, not a game
rule, so the port does not fit it. Seated on the recording's steps (`--retail-steps`),
`battle_jou_party_vahn_chain_open` reads `.915` against `.890` on the engine's own, and
`battle_jou_party_gala_chain_open` `.935` against `.860`.

The track starts where the recording does: on the cast's begin, or on the capture's gate
for a film that opens on a state's own action (`LEGAIA_DIAG_CAST_SEQ_BEGIN=gate`). The
engine runs the whole fight up to that pass, the rounds a drive replays and a mid-cast
state's opening passes among them, and those run on the fight's own steps and spend none
of the recording's frames; the pass the film opens on is the key's first frame
(`RetailStepTrack::arm`). Counted from the fight's first pass instead, a monster cast's
replay ran its earlier rounds' Done bands on the cast's own frames and skipped the arm
table, and `rugi_rogue_pre_element_change` read `.804` against its plain `.954` (`.970`
started on the cast). The seat still does not fit every capture-band film: on
`battle_dohaty_dohati_chaos_breath_mid_cast` retail's arm 1 runs `65` frames at step `4`
over `390` vsyncs, the engine on the track runs the same frames in `259` ticks, and the arm
reads `.47` against the plain run's `.93`. Read a film the track takes down as the seat's
miss, not as the law's.

<a id="a-frames-step-belongs-to-its-own-pass"></a>

**A frame's step belongs to its own pass.** A track line is one value of the frame
counter (`gp+0x480`, bumped at the head of `FUN_80016B6C` at `0x80016BA8`), keyed on
the state on the frame's first row and carrying the step on its last. The pass's logic
runs after the counter's first row: across the recordings no change of state or phase
lands on a frame's first row, so that row holds the state the pass opens on, the state the
pass before it left. The step column is the pass's own drain. Water Crystals' arm
countdowns (`capture_countdown::WATER_CRYSTALS`, re-armed by `Add`, so a residue carries)
end every arm on the frame the recording shows when each pass drains by its own frame's
step, and a frame late when it drains by the frame before's: arm 1 opens behind a
step-`3` frame and its eight step-`4` frames drain `256` exactly. A retail pass also
spans its own step: the next pass's logic lands that many vsyncs later. So the track
hands a pass's step on as the pending step the next frame claims
(`World::take_pending_frame_step`), the way an arm table's goes in, and the next pass
both drains by it and spans it; the arming pass hands on the recording's first. Keyed on
a frame's last row with its own step and re-timed onto the frame the pass had just
opened, every frame spanned retail's length while the pass after it drained by the
frame before's: Water Crystals' first arm-1 pass drained `24` where retail's drained
`32`, and the one step-`3` frame of arm 3 fell outside it, so arm 3 ran `128` game frames.
A recording that opens on a state's own instant can open on a frame whose pass ran before
the first row; no `rand()` draw lands in it (every battle pass makes the frame driver's
discarded draw), and the track leaves it out. Handed on, its step ran Lip King's first
arm-1 pass on the leftover step `3`, and Kiss of Death read `.899` where it reads `.993`.

`body.csv` and `seats.csv` are read at another point in the vsync than `rates.csv`, so on a
commit vsync the clip column can still name the outgoing clip while the cursor is already
`0` and the rate the new clip's. `battle_jou_party_vahn_chain_open` shows clip `0x10` at
cursor `0` on the vsync before `0x11` appears. This is the commit of `0x11` read early,
not a re-commit of `0x10`.

A `--physical` film has three traps of its own, each closed in the driver or the
recorder. It keeps the capture's re-seat (`PHYSICAL_KEEP_ENV`), which stands the round
on the capture's ground; it lets go of the drive at its gate
(`ScreenshotConfig::battle_drive_released`), because the drive's steer pins the yaw
counter for as long as the state lasts and leaves the camera short by what the state
would have drifted, and the release also answers the two capture-band polls the drive
held (`prev_action_cleared` on `0x6F`, `sound_bank_ready` on `0x6E`), which nothing
else answers once the drive stops: a state saved inside `0x6F`
(`battle_map01_gomboo_steal_mid_cast`) otherwise parks the band there for the whole
run, and the release also stands every seat on the capture's ground, its live pair
and heading (`LEGAIA_BATTLE_GROUND_SNAP`, `ground_snap::snap_battle_ground` with no
body pair), because the action framing's yaw is the counter less the framed seat's
heading and the replay's rounds need not have turned or walked a seat where retail's
did (`battle_nilboa_amethyst_heal_mid_cast`'s two monsters stood at `0x800`, the film
`.795` to `.866`); and the score keeps the recording's first action only
(`Side::first_action_only`), because a recording's tail can hold the next actors'
whole turns, and the alignment prefers a later actor's complete run of states to a
capture that starts inside the strike loop. The first action is the one the first
frame shows: an inject ahead of an earlier actor's turn
(`party_basic_attack_vs_gobu_gobu`, where Gobu Gobu acts before Vahn's cast) logs
that turn's Done band before any frame, and the cut does not count it. A strike's
second clip and every melee roll are
a draw, so a strike film wants `--strike-deal`, or it scores another hand: Thermo's
strike lands clip `7` twice on the engine's stream where retail's second is `8`.

Two more seats stand the gate on the state's own instant rather than on the
still's. The capture's clip seeds (`LEGAIA_SEAT_BATTLE_CLIPS`) put each body on
the displayed frame, two passes older than the RAM, which is right for the still
and wrong for a film that runs on from the gate: the driver seats the acting body's
clip and node cursor read off the state (`state_opening_cursor`,
`LEGAIA_DIAG_CAST_SEQ_SEAT_CURSOR`, taken on the gate's tick). The seat lands a
one-shot clip too (`World::seat_acting_clip_cursor`), where a still's clip seed
re-poses only a loop: the cast clip the gate stands in was committed on the replay's
clock. Refused, Caruban's Fire Breath clip `9` opened its films a step-`2` pass ahead
of retail's (cursor `48` against `40`), case `0xA`'s yaw, which reads the cursor, ran
`15` units ahead, and the full-party films read `.930` / `.927` where they read
`.964` / `.959` seated. It moves a one-shot only within a pass of the replay's
cursor (`16` units): a state saved mid draw tick can hold an after-image's older
cursor in the node word, and `cave01_theeder_thunderbolt`'s `8` stood its
Thunderbolt `32` units behind the body retail's recording shows. The other bodies stand on the state's RAM too
(`state_body_cursors`, `LEGAIA_DIAG_CAST_SEQ_SEAT_CLIPS`, loops only): the still's
clip seeds land on a capture frame a film never reaches, so the party idled on the
replay's phases (`132`, `72`, `108` where the `_2_` state holds `96`, `112`, `48`),
and seated the two films read `.967` / `.963`. And a monster
striker's committed queue `+0x1DF..` goes on the drive (`state_strike_queue`, the
`q` field) unless the still already steered it: the picker drew those clips before
the state was saved, and a replay that reached the gate on another pick swings
another clip. `battle_geremi_ogre_strike`'s Ogre walked from cursor `72` where
retail's stood on `96`, then swung `09` where retail's queue holds `07 07`; seated
on both, every segment runs retail's length and the film reads `.983` for `.920`.
Neither is a deal: both are bytes the state holds, and `--strike-deal` still owns
the later draws.

A cast whose hits are the melee kernel's takes the deal too. PROT 0952's Astral
Slash stages Gaza's four-beat clip, each beat a block roll: retail's first roll
lands and the three behind it cannot block a victim mid-reaction, while the
replay's own stream blocked the first and so all four. The capture's drive
holds no strike hand there, so `--strike-deal` deals the hit words alone
(`strike_deal::deal_hit_words`), from a recording made with `--hit-trace`.

<a id="the-headings-stand-with-their-pairs"></a>

The headings stand with their pairs. A strike's attacker re-faces every pass onto its
target's body pair, which the target's heading turns about its live pair, so a heading
seated on the replay's pair moved the attacker's bearing off retail's: in
`battle_dolk_drake_ghost_strike` the drake stood `15` units off its captured pair, and
Vahn's captured heading `145` swung the framing's yaw a further `30` units round
(`.876` with no heading seated, `.814` with headings alone, `.964` with pairs and
headings). `battle_balden_devil_pump_strike` (`.898`, `.842`, `.920`) had its whole row
up to `168` units off. `battle_vell_nighto_49_strike` (`.741`, `.697`, `.688`) is the
case where the heading was right and the frame step was not: its films read low until
the frame's draw load drove the step
([the frame step](../subsystems/battle-stage-camera.md#a-frames-draw-load)).

<a id="the-frames-after-the-gate-run-on-the-captures-ring"></a>

The frames after the gate run on the capture's frame-cost ring. A replay whose
history left the party's step inside its sixteen passes, or one that replays a cast,
keeps the default step up to its gate (`LEGAIA_BATTLE_PARTY_STEP=0`,
`RetailBattle::replay_keeps_default_step`), which is what the stills measure best on.
The pin held for the whole film, so a full party's strike ran at `2` against retail's
`3` from its first frame and the yaw drifted two units a pass. The seed now also hands
the child the capture's ring `0x80084098` and cursor (`LEGAIA_BATTLE_COST_RING`,
`frame_cost_ring`). At the gate release the child stands the engine's `FrameCostRing`
there and lifts the pin (`World::seed_frame_cost_ring`), so the frames after it take
the ring's step and then the measured costs
([the frame step](../subsystems/battle-stage-camera.md#what-the-ring-holds)). It is a
harness seat: the walk up to the gate is unchanged and no play path reads it. Over the
81 strike films it lifts step agreement with retail from `.80` to `.90` and the mean
`within` from `.838` to `.842`, Cort 181's strike from `.763` to `.860` (yaw error `86`
to `5`). Only a capture in the strike loop `0x1E..=0x20` takes the ring; a cast keeps its
pin past the gate: its module arms run on their own step tables, and the Death Game films lost `.05` of `image` off the pin while
their yaw closed.

An injected cast seats the caster's clip and cursor on its begin
(`LEGAIA_DIAG_CAST_SEQ_SEAT_CURSOR`) beside the pose: a heal's `0x3D` waits out the idle
the caster stands on.

## The camera channel

`legaia-engine cast-sequence-score` writes `camera.csv` beside the report: **every**
retail vsync row paired with its engine tick, RAM against RAM, on the segment
alignment and the game clock the image pairs use, so no display lag. Each row holds
the pass step, pitch, yaw, the TR trio and the focus pair on both sides. A summon module's
seek arm holds the engine's whole frame, so its still ticks are left out of the pairing's
offset, as the image pairs leave them: paired one for one they read as a pan the engine
stopped. `--camera-only`
writes the file and reads no frame, so a recording whose frames have been deleted
(or one made with `LEGAIA_DIAG_CAST_SEQ_EVERY` raised to skip them) still scores.
The driver forwards the flag.

[`cast-camera-rank.py`](../../scripts/ci/cast-camera-rank.py) reads a directory of films
and ranks them by the camera error instead of the image score: per film the first
row past `--px` pixels, the error held for `--sustain` rows per channel, and two pixel
columns, the mean displacement of six world probes around retail's focus projected
through each side's pose. `raw px` is the pair's own error. `law px` takes the best
engine tick within one frame step of the pair, so a pass boundary that fell a vsync
apart reads as pacing, not as a framing error. A shot that dollies through the eye (TR z
below `800` on a module's fly-by) counts zero rather than an unbounded number. Two
directories compare, by film name.

Read the top of the list against the mechanisms it has held: a capture body with no
camera director holds the pose `0x6F` left (a `law px` in the tens across whole arms);
a finishing arm that stores the wrong yaw counter leaves a Done-band yaw error that
grows along the ease; a film that opens inside a module arm or in `0x71` carries the
state's seated pose, not the arm's tween, and its first segment is the harness's, not
the game's. An engine seat at another `acting` seat than the retail recording is a
pairing artefact as well. These are read off the film, not the score.

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
3. **Display lag.** A retail grab shows the image two passes old
   ([Replayed casts](retail-compare.md#replayed-casts)), counted in the
   flip routine's wait ends rather than in vsyncs or counter bumps
   ([below](#the-flip-on-a-wait-end)): the image up between two wait ends is
   the one built by the pass whose state stood up to the wait end before
   them. On an on-time step-`1` or step-`2` frame this is the row `2 * step`
   vsyncs earlier; a slow frame keeps its image up for every vsync it
   overran, and the grab on a late frame's bump still shows the image
   before. `horn_summon` enters module phase 9 on a seven-vsync step-`4`
   frame, and its close-up reaches the screen fourteen vsyncs later, not
   eight: the vsync rule scored the phase-8 shots still on screen against
   the engine's phase-9 close-up. A
   capture without the frame counter falls back to `2 * step` read off the
   grab's own row. `--retail-lag <n>` fixes it instead. The engine renders the tick it
   just ran and takes no lag. On a cast whose close-up the engine already
   matches, `auto` scores those segments higher than a fixed lag of `0`, `2`,
   `6` or `10`; a re-score with `--score-only --retail-lag` repeats that check.
   A screen fade reads its own table
   ([retail-compare](retail-compare.md#the-double-buffers-phase-is-a-draw-tick-phase)): where
   the grab's `fade_back` is `1` and the paired engine frame shows a fade
   (the recorder's `fade_level` above `0`), the pair is keyed one game frame
   back instead of two (`FADE_FRAMES_BACK`, the report's `fade_pairs`). A
   recording without the column keeps the two-pass law. On the
   `battle_kor_spoon_strike` Zenoir film the summon's white rise scored `.44`
   under the law and `.80` at a fixed lag of `0`, which read like a display
   rule; the state's RAM says otherwise. Every vsync of the rise links the
   fade in the current table (`fade_back` `2`), and the flash-in's block on
   the first `0x34` row already holds delay `17` of its `20`: the actor ticks
   on its spawn pass, and the engine's fade stood one pass behind retail's
   until it did too (`spawn_screen_fade`).
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
5. **CD-seek waits.** A summon module's seek and poll arms are paired inside a
   tolerance window instead of strictly ([CD-seek waits](#cd-seek-waits)).
   Everywhere else the rule above holds as written.

Each pair is scored with the corpus metric (`within`, the fraction of `8 x 8`
blocks within `24` per channel; [The image channel](retail-compare.md#the-image-channel)).
A pair whose retail frame has mean luma above `230` (a white flash) or below
`8` (a fade) goes into its own count and stays out of the mean. The cast's
number is the **mean `within` over the remaining scene pairs**.

<a id="the-flip-on-a-slow-step"></a>

### The flip on a wait end

The flip routine `FUN_80016B6C` runs once a frame's pass has built its
ordering table. It bumps the frame counter (`0x80016BA8`), waits `VSync` on
the step that pass drained by (`0x800171C0`), flips, and draws the table. The
next pass starts when the wait ends. Three things follow, and the pairing
reads all three off the `frame` and `step` columns (`Side::wait_ends`):

- **A wait ends** on the bump's own row when the pass ran past its step, and
  otherwise that step after the wait end before it. The `rand` column says
  which: the battle frame driver draws at the head of the pass the wait lets
  go, so the first row past a bump whose seed word moved is the vsync after
  the wait end. The counter alone cannot tell a step-`2` frame that keeps
  its step from one that runs a little past one vsync, and the second is the
  common case: a step of `2` is what a frame costing over a vsync sets. A
  recording without the column falls back on the counter.
- **The recording's first run** is cut by its start: its bump fell before the
  first row, so its wait is counted back a step from the next bump, and a wait
  that falls before the first row is not in the recording. On the on-time
  step-`3` frames of `battle_jouind_gilium_lv3_strike` the head draw sits on
  the row after every bump, so each wait ends on its bump's row. Counted
  forward from the first row as if it were a bump, the pass ahead of the first
  wait end took a whole step in a row or two, and every pair of a `--physical`
  film's opening segment, which starts on the state's own first vsync, sat a
  pass late: that film read `.875` against `.975`, and Tri-Somersault's cues
  `.607` against `1.000`. A cast film starts at its inject, ahead of every
  segment it scores, and reads the same.
- **A pass's state** stands on the rows from the vsync after the wait before
  it to its own wait end. A pass that takes longer than a vsync therefore
  shows its state a row or more ahead of its bump, and a row's counter does
  not say which pass its RAM belongs to.
- **The image up** between wait ends `k` and `k + 1` is the table of the pass
  whose state stands up to wait end `k - 1`.

A RAM image on every vsync of PROT 0904's arm 4 (`LEGAIA_RAM_AT`) shows all
three. The arm's held rise is a ten-frame fade whose block (the actor parked
at `ctx[+0x102C]`, block at `+0x7C`) steps by the frame step, and the frames
go from one vsync to three as the rise lands:

| vsync | counter | step byte | block, tenths | grab shows |
|---|---|---|---|---|
| 554 | 1596 | 1 | 1 | |
| 555 | 1597 | 1 | 2 | |
| 556 | 1598 | 1 | 3 | |
| 557 | 1598 | 1 | 4 | 2 |
| 558 | 1599 | 2 | 4 | |
| 559 | 1599 | 2 | 6 | 3 |
| 561 | 1600 | 3 | 6 | 3 |
| 562 | 1600 | 3 | 9 | |
| 563 | 1600 | 3 | 9 | 4 |
| 565 | 1601 | 3 | 9 | 6 |

The grabs on 567 and 569 still show `6` and the one on 571 shows `9`; a
grab's tenths are its mean luma read against the engine's frame at each
tenth.

The block reads `4` on vsync 557 while the counter still reads `1598`: the
pass has run and its bump has not. Counted in counter runs, the grab on 559
keyed on the row two bumps back (555, the block at `2`) where the screen
holds `3`, and the game clock gave the two rows of counter `1598` one pass
between them where they hold two. Arm 4 scored `.054` that way against an
engine whose block matches retail's row for row from the spawn pass; on wait
ends it scores `.907`. Aluru's arm 4, on the same ramp of frame cost, goes
from `.277` to `.775`, and Theeder's arm-11 flash, which read a step ahead,
from `.850` to `.899`. The band's `0x33` state, step-`2` frames a little
over a vsync long, reads `.997` on every film of the set where it read
`.880`.

The pair's engine tick is the last one of the pass, as the keyed row is the
last of retail's. Where the engine's two ticks of one pass draw different
frames the score says so: on the lone-caster Vera recording the monster near
the camera drew as a ghost on the first tick of the `0x28` pass and solid on
the second, where retail holds one look a pass, and `0x28` read `.910`. The
engine ran the near-camera ghost pass `FUN_8004DC68` every tick; it is the
controller node's, once a pass (`World::tick_battle_camera_ghost`), and run so
`0x28` reads `.999`. Palma's arm 5 is the other arm the count of counter
runs had flattered: its five-vsync step-`4` frames read `.789` pass for
pass, and `.899` against the engine tick four earlier. The camera channel
named the cause: the arm-4 exit's pan sets out from TR x `-0x2000` and TR z
`0x2000`, which the module stores straight, and the engine's stored-value drift
was repeated for the rest of a pass that handed over on its first tick
(`ArmDirection::rest_drift`); with the arm's first pass on retail's step `2`
the arm reads `.943`.

On frames that keep one length the two counts agree, which is why the
earlier measurements hold. On the lone-caster Odd Dimension recording
(`0x93`), of the `596` pairs of grabs two vsyncs apart on step-`4` rows,
`579` change exactly when a bump lies between the rows before them: a frame
there runs past its step, its wait ends on its bump's row, and the flip
shows on the next vsync. A RAM image taken on the last row of a step-`4`
frame `F`, its current ordering table rasterised with `ot-raster.py`,
matches the grabs one and three vsyncs past frame `F + 2`'s bump and not the
grab on it.

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

Seeded at the cast's begin (`--rand-at 0x28:0`), the stream is also a probe of
the engine's frame pacing: the battle frame driver draws once a pass
(`FUN_80046A20`), so the first word the engine reaches early or late names the
pass where its frames part from retail's. On the lone-caster
`party_basic_attack_vs_gobu_gobu` films that is how the summon close-up's
step drop and the ring's one-pass lag were found
([the frame step](../subsystems/battle-stage-camera.md#what-the-ring-holds));
with both in, the draws hold word for word through `0x34` and part at the
module's CD-seek hold.

Unseeded, the logged words still count draws: the LCG steps between one vsync's
word and the next are the draws made in between, on either side. Counted per key
on the lone-caster films, Odd Dimension (`0x93`, PROT 0921) and Ozma (`0xA0`, PROT
0934) part in two ways once the module's CD-seek hold is past:

| Cast | Key | Retail draws / vsyncs | Engine draws / ticks |
|---|---|---|---|
| `0x93` | arm 1 (the seek) | `1` / `1` | `6` / `31` |
| `0x93` | arms 5..8 | `724` / `773` | `724` / `724` |
| `0x93` | arm 9 | `81` / `166` | `36` / `132` |
| `0xA0` | arm 16 | `5820` / `240` | `60` / `240` |
| `0xA0` | arm 18 | `14095` / `508` | `1918` / `512` |

Odd Dimension's arm 9 runs about one pass every two vsyncs in retail and one
every four in the engine (its frame-step row). The two Ozma rows are the
jittered rings' draw words, which the engine now deals
([effect VM](../subsystems/effect-vm.md)): arms 16 and 18 read `5724` and
`14095` on the engine's side.

Counted per frame instead of per arm, the same subtraction reads one pass's
draws, which is how Ozma's arms 5..9 were closed: a retail frame there reads
`111`, `123`, `167` then `187` words as the camera pulls in, the frame driver's
one, the eight streak ribbons' and the rumble's two, with a volley's `16..64`
on top every eighth frame. The engine's frames read the same numbers once the
ribbon draws on the world stream, and under `--rand-track` its bolts are
retail's bolt for bolt. So are its volleys: the part that calls the stager
(`0x801F9F10`, a `0x5F` wait then one call every `0x1F`) drains its wait by
`8 * step` a frame and re-enters its program below zero
(`0x80021E40..0x80021E4C`, `0x80022B9C`). Retail's seat frame runs step `2` (the
part's `+0xB6` channel gains `40` there and `80` a frame after), the ring's
step before arm 5's clamped frames, and the engine ran it on the arm's `4`, so
every volley landed a frame early, on another frame's words. With an arm's step
opening on its second pass ([the frame step](../subsystems/battle-stage-camera.md#a-pass-runs-on-the-step-the-frames-before-it-left))
the volleys fall on frames `24`, `32`, `40`, `48`, `56` and `64` of arm 5 on both
sides, and the per-frame draw counts agree for its first seventy frames.

A cast whose outcome is a roll parts on the deal the same way, and the sound
channel shows it first. `battle_korb2_mage_void_accessories_mid_cast` scores a
cue score of `0.000` on the engine's own stream: retail's arm 3 rolled "Nothing
happened.", the engine's stripped the accessory and played the victim's knockdown
and its three cues. With `--rand-at 0x70:3` both sides miss, the cues score
`1.000` and the frames `.773` against `.756`.

A `--rand-at` word deals one pass. An arm that draws on every pass outruns it: PROT
0935's Earthquake bursts a rock pair a frame off `rand()`, and retail's other passes
draw tens of times a frame where the engine draws a few, so from the second pair on the
rocks fall on the engine's own stream. `--rand-track` holds every pass instead. The driver
writes `retail-rand.txt`, one `state phase word` line per retail frame inside a module
band, the word the frame's last row holds, keyed as the step track keys a frame. The
engine child (`RetailRandTrack`) seats the word for the pass that opens under the same
key, ahead of the frame driver's discarded draw, counting each key's passes from the
band's entry. The key the recording opens on is left out, because the capture's instant
falls inside it. Measured with it:

| Film | Own stream | `--rand-track` |
|---|---|---|
| `battle_jagaroom_van_saryu_earthquake_mid_cast` | `.808` | `.943` |
| `battle_map03_scale_lizard_earthquake_mid_cast` | `.748` | `.910` |
| `battle_taiku_puera_death_game_mid_cast` | `.806` (cues `.800`) | `.839` (cues `.970`) |
| `battle_korb2_mage_void_accessories_mid_cast` | `.823` (cues `0`) | `.841` (cues `1.000`) |
| `battle_retockin_evil_shadow_fatal_decision_mid_cast` | `.841` | `.830` |

Death Game deals its four cards in arm 0 (`0x801FA288`), after the library's states
were saved, so neither `--rand-at` nor the track reaches the deal: both states' tables
still read zero. `LEGAIA_SEAT_DEATH_GAME_DEAL=a,b,c,d` (through `--engine-env`) lands the
engine's deal on a table read off a retail RAM image (`LEGAIA_RAM_AT` in arm 2); the
retail-compare seed derives the same seat for a state past arm 0. A harness seat.

A draw whose count is a projection couples the camera to the stream. The streak ribbon
`FUN_801E1D98` draws seven words for its bottom segment and four for each segment it
chains up the screen, so its count is where its anchor projects under the GTE view the
pass inherits. A `--ribbon-trace` recording of `battle_jouind_gilium_lv3_thunder_storm_mid_cast`
held each call's seed, anchor and corners against the engine's: the anchors stood three
ring words short, the heights were another modulus's, and the hit rolled ahead of the
scatter ([Thunder Storm](../subsystems/cast-module.md#prot-0957-thunder-storm)). With
those three ported the seeds agree call for call, and `--retail-steps --rand-track`
deals retail's damage on both recordings. On `--retail-steps` alone the stream enters
the cast on the engine's own words, so the numbers there are a deal, not a defect.

It does not help every film. The words are retail's frame ends, so an engine pass that
draws in another order than retail's inside the frame still parts: Fatal Decision gains
five extra cues. Read it as a harness seat that takes the stream out, not as a score.

Nor does it seat a roll on a pass the engine does not run. Water Crystals' hit
(`FUN_801DD4B0(0xC0)` at `0x801F7318`) on `battle_jou_freed_v3_water_crystals_mid_cast`
lands on the pass arm 3's `scalar << 7` countdown opens. On its own steps the engine's
wait runs a pass short of retail's: one of retail's frames there runs on step `3` (`131`
game frames against `128`), so retail's hit takes that pass's discarded draw first and the
wrapper's four draws after it, and the bonus draw `rand() % 49` that sets a
Spirit-guarded victim's damage reads `35` (`113`) where the engine, a pass earlier, reads
the frame's first four (`103` on its own stream). The wrapper's draws and arithmetic
reproduce retail's `113` from the recorded word. Seated on the recording's steps
([keyed on the state a pass opens on](#a-frames-step-belongs-to-its-own-pass)) the engine
runs the step-`3` pass where retail's does, arm 3 runs `131` game frames, and with
`--rand-track` the hit reads `113`. Grade `disassembly` + `capture`.

### The Seru set on three seats

Run three ways, plain, `--retail-steps` and `--retail-steps --rand-track`, the 29 Seru
films of [`film-sets/seru.txt`](../../scripts/ci/film-sets/seru.txt) split their residue.
An arm the step track lifts was the frame's cost; one the rand track lifts was the
deal (Gizam's bubbles, Aluru's and Iota's ray fans, Ozma's arm 21); an arm that reads
low on all three seats is the model's or the draw's. Ranked by the pairs it loses on
its best seat, the held arms were the place to read the disassembly, and the camera
classes found there were each a few lines of the director:

- a shot that frames the victim's or the creature's display pair `+0x3C` / `+0x40`
  where the port read the seat ([cast module](../subsystems/cast-module.md#the-victims-display-pair)):
  Barra's arm `0xF` `.630` to `.941`, Gizam's arm-9 cut `.845` to `.997`;
- Kemaro's victim under the tint pass's cursor-dim arm, and its shots on the arm-9
  part's height (arm `0x12` `.638` to `.820`).
- an arm step that is a fixed amount a **pass**, not a frame product: Nova's arm-16
  victim turn and camera yaw ([cast module](../subsystems/cast-module.md#body-paced-directors)),
  which on retail's steps now add turn for turn with the recording (arm 16 `.690` to
  `.724`, film `.865` to `.881`);
- the ground shadow's pitch / roll gate, which reads the battle body's `+0x44` /
  `+0x48` ([renderer](../subsystems/renderer.md#the-ground-shadow)): Kemaro's frozen
  victim casts none (arm `0x12` `.820` to `.825`).

The step track had a seat's own defect: the pass applied the recording's step after
the SM step that ran the module, so on every step change the module pass and the
director read the step the last frame ran. The track now hands each step on as the
step the next frame claims ([a frame's step belongs to its own pass](#a-frames-step-belongs-to-its-own-pass)),
so the module reads the frame's own step (`World::module_frame_step`). Nova's arm 16 drifted a tick a change before it, and missed one of its 83
passes. Over the 29 films the step seat's mean moved `.901` to `.909` (`0x99`
`.857` to `.912`, Gola Gola's `0x8C` `.872` to `.909`). `0x9F` fell `.895` to `.863` on
arms 9..11, and the lookahead was not the defect there: it moved arm 8's handing-over
tick one tick earlier, and what the port did on that pass was wrong either way. The
port re-applied the rest of the pass's camera drift after the passing arm's cut,
which retail builds from the drifted globals ([cast module](../subsystems/cast-module.md#the-rest-of-a-handing-over-pass)),
and Terra walked its whirl parts and its row's orbit a vsync at a time where retail
steps them once a pass. With both carried, the arm-7 whirl heights match a
`--retail-parts` recording part for part, the victim's live pair matches the
recording's `body.csv` through arms `0xA` and `0xB`, and `0x9F` reads `.960` on
retail's steps.

The Mushura film's arm 12 was held by its victim's stand, not by the chase or the
reaction clip: arm 6's pass steps the victim back a sixth of its body radius, and
arm 7's pre-hit turns it down the lane before the reaction slides it, and with both
the victim stands where a retail RAM image holds it, frame for frame. Over the set:

| Seat | Before | After | Moved |
|---|---|---|---|
| plain | `.9065` | `.9079` | `0x8D` `.907` to `.922`, `0x9E` `.906` to `.926` |
| `--retail-steps` | `.9091` | `.9133` | `0x9F` `.863` to `.960`, `0x9E` `.918` to `.937` |
| `--retail-steps --rand-track` | `.9314` | `.9356` | `0x9F` `.863` to `.960`, `0x9E` `.920` to `.940` |

Mushura's arms 8..11 lift on every seat (arm 10 `.890` to `.987`). Its arm 12 chase
reads `.792` / `.750` / `.736` on the three seats, against `.789` / `.796` / `.778`:
it now starts from retail's stand, and what it does from there (the targets it
trades, the hits that knock the row about) is the chase's own to read. Gilium's `0x95` loses `.002` plain, a few hundredths on arms `0xD`,
`0x10` and `0x11` whose camera now stands nearer the recording's.

Kemaro's arms `0x12` and `0x13` were held by the draw: the spear arm 9 seats and arms
13..17 walk by handle never moved, and arm 17's two discs, once seated on retail's trio,
drew over the band wash that covers them in retail
([cast module](../subsystems/cast-module.md#summon-creature-directors)). With both, arm
`0x12` reads `.982` and arm `0x13` `.973` on every seat, against `.825` and `.741`, and no
other film of the set moves:

| Seat | Before | After | Moved |
|---|---|---|---|
| plain | `.9079` | `.9084` | `0x90` `.935` to `.950` |
| `--retail-steps` | `.9133` | `.9138` | `0x90` `.936` to `.950` |
| `--retail-steps --rand-track` | `.9356` | `.9361` | `0x90` `.936` to `.950` |

Barra's arms 6..0xB and Gizam's arm 5 were the module's own stores. Barra's sky is a
scanline wash whose sample origin two module words slide every pass, and its arm 6
takes the ground away only from the arc's second half
([cast module](../subsystems/cast-module.md#barras-sky)); Gizam's arm 4 hides its
creature by its colour word as well as its flag, so it does not stand at the caster
through the phase-5 hole ([cast module](../subsystems/cast-module.md#prot-0906-gizam));
and Theeder prints its own captions. No other film of the set moves:

| Seat | Before | After | Moved |
|---|---|---|---|
| plain | `.9080` | `.9100` | `0x8F` `.884` to `.921`, `0x84` `.874` to `.893`, `0x82` `.954` to `.957` |
| `--retail-steps` | `.9183` | `.9207` | `0x8F` `.924` to `.973`, `0x84` `.880` to `.899`, `0x82` `.962` to `.964` |

Barra's arm 7 reads `.995` on retail's steps and `.75` on the engine's own: its white
pulses are a four-record cloud cycle stepped once a pass, so where they fall is the
frame cadence, not the module.

What still holds on every seat is mostly not a rule a director misses. Horn's arm 4
and Ozma's arms 6..9 overrun their frames by half again (`+196` and `+274` vsyncs), so
the passes the rand track seats fall on other frames; Juggernaut's arm 26 reads `.44`
against one retail recording and `.80` against another of the same cast, so its ring
phase is the recording's; and Viguro's lightning, Spoon's rings and Terra's vortex part
on phase rather than shape. The rest is listed in the
[open threads](../reference/open-rev-eng-threads.md#battle--rendering).

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

The probe records the frame counter on every vsync. A pass's state stands on
the rows between two of the flip routine's wait ends
([the flip on a wait end](#the-flip-on-a-wait-end)), and it drains by the step
byte the counter run before it ends on. The **game clock** adds that step per
pass, spread evenly over the pass's rows, so an overrunning frame advances it
by its step, not by its length. The pairing
measures retail's offset into a segment on that clock, and the engine's on its
own ticks less the ones its frames overran. The engine overruns a frame the XA stream
loads by one vsync, or every frame by retail's `VSync` law under the opt-in
([a frame's span](../subsystems/battle-stage-camera.md#a-frames-span)), and the recorder marks each
such tick in `meta.csv`'s `over` column (`Side::live_clock`). Counted as game
time, those ticks left a voiced cast's magic band a pass behind retail's for
every four it ran: Gaza's Astral Wave read its `0x2B` burst some `45` ticks
late, `.809` with cues `.500`, and on the clock `.887` with cues `.750`. A capture without the `frame` column
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

## CD-seek waits

A summon module (PROT 0903..0934) seeks the drive to its voice clip on one
arm and polls the seek on a later one (`module_drive_arms` reads the pair off
the image): the seek arm's `CdlSetloc` blocks the game loop while the drive
finishes the battle selector's seek before it, and the poll arm returns busy
until the module's own seek completes, with the rest of the frame running
([the drive holds a summon's voice](../subsystems/audio.md#the-drive-holds-a-summons-voice)).
How long either takes is the drive's seek time, and that is not one number:
it varies across drives, and across emulators. The engine keeps one model
(`battle_cd_drive`, fitted to mednafen: `0.3` s plus a full-disc traverse a
second, plus two vsyncs), which holds the seek arm `23..36` frames whole and
the poll arm up to `29` ticks; the PCSX-Redux films this oracle pairs against
seek in `1..4` vsyncs. Paired strictly, the difference is scored as engine
error: on Gimard (`0x81`) the held frames put the rest of arm 2 twenty-three
ticks out of step (`.033` over the arm), and on Viguro (`0x87`) the content
that ran on through the poll leaves arm 3 sixteen ticks ahead (`.233`).

The oracle accepts any wait inside a window instead. The engine recorder
writes the module's two arms and the drive's remaining vsyncs on every tick
(`drive_seek_arm`, `drive_poll_arm`, `drive_busy`), and the pairing reads:

- **The engine's wait**: the seek-arm ticks its display-frame clock stood
  still on (the blocking seek) plus the poll-arm ticks the drive was busy.
  This is the model's prediction, realised.
- **Retail's wait**: the vsyncs its two arms ran past the engine's own passes
  there (wall vsyncs), and the same on the game clock, where a blocked frame
  counts only its step.
- **The window**: `0` (a seek the drive answers inside the arm's own pass) up
  to the engine's wait plus `max(8, wait / 2)` vsyncs
  (`cast_sequence::drive_window_margin`). A retail wait outside it is paired
  strictly.

Inside the window the wait is absorbed in three places:

1. **The seek arm** pairs on the engine's live ticks: a held frame does not
   advance the offset, as retail's blocked frame advances its game clock by
   one step only.
2. **The wait arms' extra retail frames.** Where retail's drive held longer
   than the engine's, its frames past the engine's live frames in that arm are
   counted apart (`drive hold`) and left out of the mean, not scored against
   the engine's next arm.
3. **The arms after it.** The poll's difference (engine poll hold less
   retail's game-clock wait, the report's `live shift`) is how far whatever
   ran through the wait (a fade or a rise spawned before it) stands ahead in
   the engine afterwards, while what the next arms spawn runs on their own
   clock. Each module segment past the wait is paired on whichever of the two
   clocks its scene pairs score higher on: the arm clock (shift `0`) or the
   wait-through clock (shift `-live shift`). Nothing between: a free search
   over the span fits one-tick offsets the wait has nothing to do with.

A poll that waits inside its arm's own longer countdown (Gilium's family,
`0x91..=0x95`) ends on the countdown on both sides: the measure reads retail's
wait as the engine's and absorbs nothing. Outside drive waits nothing changes,
and `--strict-drive` turns the window off. The report states the wait, the
window, whether it was absorbed, the live shift, the drive-hold count and the
strict mean, and the segment table gives each segment's shift; the driver
prints the same line. Measured on the `party_basic_attack_vs_gobu_gobu` and
harvested-strike Seru films (44 with drive arms), the mean goes from `.792`
strict to `.808`, Gimard from `.847` to `.897`, Viguro from `.757` to `.786`;
the five monster casts in the same run, with no drive arms, are unchanged.

A monster cast reached through the state's pad drive stages the clip the drive's
replay picked, and the picker stores its clip bytes under draws the replay does not
share. The seed therefore carries the capture's queue for a monster cast (`q` on
`LEGAIA_BATTLE_DRIVE`), and the drive holds `+0x1E0` and the Spirit band's `+0x1E7` on
it until the clip stages: `battle_balden_orb_v2_hyper_heal_mid_cast`'s Orb plays clip
`9` as retail's does, where the walk's last tag-`0x23` entry `0x0A` held the Spirit band
twice as long (`.818` to `.943`). A harness seat.

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
  an earlier one reads as a strike that "lands nothing". A retail recording whose
  first row has already left the state (`cam.csv` differs from the seated pose) is a
  pass ahead of the gate tick, so the score drops the engine's leading rows on that
  pose (`--engine-skip`, counted by the driver). A strike's camera sweeps fastest in
  that opening: one step of skew costs its `0x1E` segment about `.08` of `within`.

A heal's state stands in the Spirit / Item flow (`0x3C..=0x40`, the harvester saves it at
`0x3E`), which the probe also takes as the recording's start: before it did, a
`*_heal_mid_cast` recording waited for the next actor's strike and filmed that.

On the retail side the same switch starts on a state held on `0x2B` or `0x70`, with
`--retail-only`: `LEGAIA_RAM_AT` then films a monster cast from the state on the
fast core, where the `--monster-pick` run is an interpreter's.

Once the capture's own instant is passed, nothing holds the two rounds together:
the block roll, the monster's next queue byte and any later pick draw on each
side's own stream. So the channel's number mixes rule differences with deals.
`scripts/asset-investigation/cue_by_clip.py <dir>` takes the deal out: it files each
cue under the seat and clip it landed in, with its offset from the clip's commit,
on both sides, so a clip both sides played can be read cue for cue.

`--strike-deal` takes the deal out of the score as well. When the capture's gate opens, the engine child stands on the hand retail held, a monster's strike or a party member's:

- a monster striker's queue `+0x1DF..` off the state (a party striker's is its own command input, which the drive already replays). A monster's swings were rolled at its pick, so the
  drive's gate holds only the bytes up to the cursor. A byte the replay's loop has staged and not
  yet committed is re-staged from the queue too (`restage_dealt_byte`): a walking monster opens the
  loop with its first byte over the idle, and `battle_rugi_caruban_3_strike`'s replay had staged its
  own `07` where retail held `08`, so it played Bite alone and skipped Flap (`.690` to `.772`);
- the approach terms `ctx[+0x6D2]` / `ctx[+0x6D4]` off the state. They feed the first hit's block roll,
  and the replay walked in from elsewhere to a target facing elsewhere: `battle_rugi_barra_strike`
  holds `393` / `64`, the replay `86` / `116`;
- the striker's working ATK `+0x158` off the state: `battle_map02_mad_ogre_strike` stands on a Power
  Charge's `257` where the replay, which never cast it, holds `206`;
- the party seats in the Spirit stance at the strike's start (`+0x1DE == 4`, the probe's `guard=`,
  passed as `LEGAIA_BATTLE_GUARD`). The block roll scales the defender's side by `3 / 2` for the
  stance (`0x801EC6C0..0x801EC6D4`), and the drive commits Spirit only on a round the whole party
  sat out on it, so a round that mixed the two replayed an unguarded member.
  `battle_korb2_warhalf_strike` holds Vahn and Gala on Spirit and Noa on Attack: Warhalf's second
  swing rolls `683` against Vahn's `978` in retail (his `435` times `3 / 2` for the stance and again
  for his ability bit `0x100000`, `0x801EC80C..0x801EC828`) and blocks, where the replay's unguarded
  Vahn stood on `652` and took the hit;
- the seed word `0x80009010` going into each hit. A `--hit-trace` recording logs it at the head of the
  strike kernel's hit body (`0x801EC5A8`, past the head guards and ahead of the block roll's two draws),
  for every hit the kernel resolves, blocked, missed or landed, as `retail/hits.csv`; the deal hands those
  words out as `word@vsync`. Without the trace the words are read off the recording's `rand` column. A hit's spark burst
  makes its frame draw tens of times where any other frame draws once, so the frames read straight
  off the generator's distance between two rows. The frame driver's discard at `0x80046D2C` runs ahead of the
  hit, so the word steps on once. A blocked or missed swing draws no burst and leaves no word,
  so each word goes out as `word@vsync` and the engine hands it to the hit within twelve display
  frames of that vsync (`World::dealt_hit_word`); a hit with none in reach draws on its own stream.
  Handed out in order, `battle_rugi_caruban_3_strike`'s two words (Bite's hit and the last swing's)
  fell on Flap's two events, whose first retail blocks and whose second draws no spark.

- the attack camera's track coin `ctx[+0x26D]`, which `FUN_8004E13C` draws `rand() % 2` at
  every commit of a clip whose `+0x87` byte is set and which picks the per-art camera's
  column. The probe logs it per vsync (`meta.csv`'s `coin`), the driver hands its change
  points over as `LEGAIA_STRIKE_DEAL_COIN` (`coin@vsync`), and each engine draw at such a
  commit takes the value retail stood on eight display frames on (`World::dealt_track_coin`).
  `battle_jou_party_vahn_tri_somersault_super_open` holds `1` throughout; the replay's own
  stream drew `0` at the Super Art's `0x1A` commit and filmed the other shot: `.728`
  without the deal, `.798` with it, camera `67` px to `27`. A recording made before the
  column deals no coin.

```bash
scripts/ci/cast-sequence.py --physical --state battle_rugi_barra_strike --engine-only --strike-deal
scripts/ci/cast-sequence.py --physical --state battle_jou_nova_v3_strike --strike-deal --hit-trace
```

A strike film scored without the deal is also a statement about every round the engine played up to
the gate: a change to battle timing moves how many passes those rounds take, each pass draws, and the
strike is dealt another hand. Moving the cast tick behind the node ticks
([the order inside the pass](../subsystems/battle-stage-camera.md#the-frame-step)) took six caster fights'
strike films down on the engine's own stream (`battle_jou_orb_v3_strike` `.896` to `.830`, cues `1.000`
to `.333`: its swings blocked where retail's landed). Dealt, the same six read alike on either order,
within `.008`, and every cue pairs on both. Compare an engine change across strike films with
`--strike-deal` on both sides.

Most strike recordings dealt this way match retail cue for cue: block verdict, damage rolls, the flinch's
impact pick and the times. The burst reading is not complete: a blocked swing leaves no word, and an
emitter's draw can fall between the frame's first word and the hit. `battle_jou_nova_v3_strike`'s burst words
named the frame before the hit for its third, fourth and fifth swings (`0xEC2A0320` where the roll drew from
`0x68CC220A`), so its fourth swing blocked on the engine and landed on retail. On the traced words the engine's
block verdicts are retail's on all five swings (land, block, block, land, land: the third and fourth with the
defender still in its block clip, `+0x1D9 == +0x1F3`, and `+0x1F7` down, so the roll's verdict stands), and the
film's cues match `15` of `15` (`.867` before). `battle_ropeway_party_vahn_slash_kick_mid` is a party strike, which
the deal did not seat before: Vahn's `0x0E` swing on the Ropeway monster is blocked on retail (`s0 213`,
`s1 283`, cue `0x0C`) and landed on the engine's own stream (cue `0x07`); dealt, it blocks, and its Slash Kick hit
lands on both sides (`10` of `10` cues, film `.611` to `.804`). A walk-clip capture's gate
can also open past the first swing (`battle_retona_king_frog_strike`), and a defender's own stats stay the
replay's. Read a residual by hand, as `battle_map02_mad_ogre_strike`'s Power Charge was read.

**A recording that opens past `0x1E` is the state's own action.** The probe starts a
`--physical` film on `0x1E`, `0x1F` or `0x20`. A recording made before it took the last two
waited through the state's action for the next seat's strike and filmed that one:
`battle_doman_killer_wolf_strike` then paired a Freed striking Vahn with the engine's Killer
Wolf striking Gala (`.367`), and a state whose fight held no later strike kept no frame at
all. Recorded from the state's first vsync, the same film reads `.947`. A strike film whose
retail side shows another attacker than the engine's is re-recorded (`--recapture`) before
it is read.

**A walk's phase at arrival is the frame steps it ran on.** A monster whose walk entry is
locked plays the walk on into the strike loop, and the first byte commits on the walk's
natural end. `battle_map02_mad_ogre_strike` stands there: clip `1` at cursor `96` with `7`
staged, the walk ending thirty vsyncs on with two `0x12` step cues. The engine's replay
arrives on the pass its own walk ends (cursor `312` to `336`), so the arrival's idle commits
and no step sounds: cue score `.571`. The law is the same on both sides. The cursor at
arrival is the walk's passes times `8 * step`, and retail's yaw counter at the capture reads
`638`, off the `512 + 4k` lattice a walk at step `3` keeps, so retail ran some of that walk at
step `2` where the replay ran it at `3` throughout (the table's `0x19` row). Read such a film
as the frame cost's, not as a missing walk.
`--engine-env LEGAIA_DIAG_CAST_SEQ_PRE=1` prints the acting seat's attack band up to the
gate into `engine.log`, a line a tick (state, clip, staged byte, `+0x1DC`, cursor, live
pair, heading, the target's pair, the yaw counter, the step, the strike cursor): the engine
half of the probe's `pre.csv`, and how the arrival above was read.

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

`--calc-clock` (retail only) writes `calc.csv`: every entry of the libsnd calc `FUN_80062F98` and every
KON write `FUN_8006B854(1, mask)` with the CPU cycle, the action state and the module phase, so a calc
skipped or doubled across a cast's blocking arms reads as a cycle gap of `0` or `2` vsyncs.

`rates.csv` (retail only) holds each seat's rate `+0x21D` and node cursor `+0x68`
per vsync: a same-id timing miss on a reaction is usually the rate, read straight off it.

**An edge sampler misses a same-id re-push.** The ring is sampled, not watched, so a cue
pushed into the slot that held the same id one frame earlier reads as one cue: the ager clears
the slot and the frame body refills it between two samples, and no sample sees the gap. Neither
the frame counter nor the countdown separates the two, because samples land both inside a
frame's body and in its vsync wait. `battle_chitei2_kemaro_lv2_lava_breath_mid_cast` is the
case: three homing landings ring `0x49` (three `FUN_8004FCC8` calls from `0x801E185C` on a
`--sfx-callers` recording, the third in the frame after the first two), the sampler counts two,
and the engine's third reads as an extra. A lone extra whose id sat in a slot the frame before
is checked against `sfxcall.csv` before it is read as an engine defect.

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
- **Zenoir's splash on a seat out of reach.** The kill phase's three extras on
  `battle_kor_spoon_strike` and `battle_tower_swordie_strike` were the second
  monster's reaction: the port read the module's victim off the caster's
  `+0x1DD`, which arm 8 turns to `9`, where retail reads the word arm 0 latched
  ([PROT 0908](../subsystems/cast-module.md#prot-0908-zenoir)). The film's HP
  columns showed it first: retail's seat `4` holds its HP through arm 12.
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
- **A block is a roll as well.** A cast whose hits run the melee kernel rolls the victim's
  block per hit, and the block clip and the flinch ring different cues: on Lu's Plasma
  Strike (`nivora_duel_pre_plasma_strike`) retail's Noa blocks all seven beats (`0x0C` and
  `0x249` each) where the engine's stream flinched on three (`0x60` / `0x61` and `0x4A`);
  on Gaza's Astral Slash (`battle_kor_gaza_165_astral_slash_mid_cast`) the reverse, Vahn
  flinching in retail (`0x54`, `0x27F`) and blocking in the engine (`0x51`, `0x0C`). The
  draw counts per frame match retail's up to the first roll, and `--rand-track` scores both
  `1.000`.
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

## The cue ratchet

A film's image score moves a little with every engine change, so the films stay
report-only. The cue channel does not: a cast plays the cue retail plays on the same
clock or it does not, and a cue that stops playing is a defect even when the picture
holds. [`film-cues.py`](../../scripts/ci/film-cues.py) holds that channel of the
committed film sets to [`film-cue-baseline.json`](../../scripts/ci/film-cue-baseline.json):

```bash
scripts/ci/cast-sequence-batch.py scripts/ci/film-sets/seru.txt --out captures/films/seru
scripts/ci/film-cues.py --check captures/films/seru
scripts/ci/film-cues.py --check captures/films/seru captures/films/enemy --propose /tmp/cues.json
```

The sets are batch files under [`film-sets/`](../../scripts/ci/film-sets/): the Seru
casts from the Gobu Gobu opener, the enemy casts and the dealt monster strikes. A film's
`retail/` may be a symlink to an existing recording, which the batch driver reuses. A
baselined film fails when its cue score falls or it matches fewer cues; one whose
retail side holds another number of cues was re-recorded and is listed, not scored; one
with no report is counted as a skip, not a pass. `--propose` writes a candidate
baseline, each film at the better of baseline and measurement. A lower value needs a
[`baseline-loosenings.toml`](../../scripts/ci/baseline-loosenings.toml) entry, which
[`check-baseline-loosening.py`](ratchet-integrity.md) enforces on the file alone.

Each strike and enemy film is pinned to the battle stream it was baselined on
(`--pin-seed`, retail-compare's `LEGAIA_RC_PIN_SEED`): the seed tries that stream
ahead of the search list, and a change to the draws before the phase no longer hands
the film whichever stream now reaches first. A pin that stops reaching falls through
to the list, and `cast-sequence.py` says which stream it dealt. Re-pin from the
`stream` detail of the film's `rc/report.json`.

Two cue drops show why. A summon kill's fade stand-in once faded the body before its
knockdown committed, so the death arm and its defeat cue `0x0D` never ran: five Seru
films read `.889` with the image near unchanged. Chaos Flare's victim lost its
per-pass rate store when reactions began to commit in the seat's own node tick: the
strike's `2` met the commit's decay to `4`, and both of the knockdown's `0x17` cues
rang 48 and 68 ticks early. Four of the five Seru films' images moved by `.001` and Chaos
Flare's by `.003`: only the cue channel said anything was wrong.

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
  seat against retail's 432 before the ground was carried). The handed turn
  spends the seat's initiative key, as the pick it stands for does
  (`World::dispatch_forced_monster_turn`). Left unspent, the Done band's
  re-pick at `0x50` (`FUN_801DABA4`) landed on the same seat, its AI stored
  Attack into the `+0x1DE` the band's framing fork was reading, and the
  `0x51` camera left the caster for case `8` on its target: Megaton Press,
  Scythe Wind, Fissure, Spore Gas and the others whose `0x51` segment read
  about `.5`.
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
- **A cast that wipes the party has no Done band.** Koru's Dead End Crisis
  (`0xA1`) from `nilboa_koru_pre_dead_end_crisis`, whose party is Vahn alone,
  runs its module phases `0..6` and then leaves for `0x22` and the game-over
  flow `0x3C`, so the probe reports a park and there is nothing to pair. A film
  of it needs a state whose party outlives the cast.
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
victim shot follow it. The slot is the engine's, so a monster target (a self-cast:
Koru's Ultra Charge, whose pick stores `+0x1DD = 3`) is re-keyed as the caster is,
`party_count + seat - 3`. Left on the retail number it named no engine actor in
a one-member party, the cast fell back to the spell's own class, and the near-camera
ghost pass and the HUD read a party target: Vahn drew solid in front of Koru, with
his plate up, where retail ghosts him and shows Koru's alone.

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
  at `+0x1DE == 4`. Both per-move wrappers double the defender roll on it and the
  finisher halves the hit, so an engine child standing the party out of the stance
  takes about twice retail's damage: the Rogue casts read `1591` against `751` on Vahn.
  The driver stands the probe's `guard=` seats (the state's stance for an older
  recording, read with `pcsxr-state`, which the driver's build step builds; it warns when
  the state cannot be read rather than standing no seat); `--guard` overrides it. A `--monster-pick` run several rounds on from a
  `*_strike` state has tapped Attack since: its `guard=` reads the stance at the cast,
  and a recording without the field stands no seat.
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
| the side-band arm (below) | `ctx[+0x276]`: the applier `FUN_801F12D0` requests and consumes one `0x10800`-byte slot per stage pair, from the `1` state `0x32` stores | `28` to `30` vsyncs a request, `1` or `2` a consume; four slots for Horn and Jedo (the stage drops at `8`), three for Freed (at `6`) | the byte is up for a modelled read time ([below](#the-side-band-reads-seek)) |
| a stream-poll arm | `FUN_8003F2B8(1)`, the creature's own read | a few vsyncs | one pass, except a summon module's drive poll, which the engine holds ([CD-seek waits](#cd-seek-waits)) |

The read-span words themselves (`xaspan`, `cdbusy`, `cdmode`, `cdread`) hold
still through the whole band in these casts, so the `0x32` wait is the
pending read, not a voice clip's span.

Every summon module `0903..=0934` polls `ctx[+0x276]` once, on one arm
([cast-module.md](../subsystems/cast-module.md#the-side-band-poll-arm)), so
in retail that arm holds while the applier runs on past the band head.
Horn's arm 2 ends on the pass the byte drops. Jedo's band flash-out reads
about `25` vsyncs younger in the engine at the same arm, which is most of
that cast's low score in arms 2 to 4.

<a id="the-side-band-reads-seek"></a>

The byte's length is the drive's. The engine holds it `30` vsyncs a slot
(`SIDEBAND_SLOT_READ_VSYNCS`) plus seven for the seek into the first
`summon.dat` slot, fitted on mednafen's drive against where the stage drops
in the module's arm 1 (`SIDEBAND_FIRST_SLOT_SEEK_VSYNCS`). PCSX-Redux's drive reads no such seek:
on every film of the lone-caster Seru set the stage drops `90` vsyncs after
`0x32`'s store for a three-slot base and `120` for a four-slot one. The
driver therefore seats the recording drive's seek on the engine child
(`LEGAIA_SEAT_SIDEBAND_SEEK=0`, `CastFxState::sideband_first_seek_seat`), a
harness seat that no play path sets. It matters where the byte outlasts the
band head, the four-slot bases `0x9A..=0xA0`: Ozma's arm 2 let go late by
the modelled seek, so the band's fall to black stood that much further on through
arm 3, which reads `.969` without the seat and `.999` with it.

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
