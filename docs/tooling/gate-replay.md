# Gate replay

A pad schedule recorded on the engine cannot run blind on retail for long: the two
sides reach a door or close a dialogue page a few vsyncs apart, every later press
lands somewhere else, and the runs split. The gate replay plays the engine's pad
on retail beat by beat instead, and resynchronises at each state gate, so a
disagreement stays in the beat that caused it and can be measured there.

```bash
LEGAIA_FGL_ONLY=tetsu_spar LEGAIA_FGL_TRACE=1 LEGAIA_FGL_PIN_RETAIL=1 \
LEGAIA_FGL_RNG_SEED=0x1a1193df LEGAIA_DIAG_TICK_DIGEST=/path/digest.txt \
XMODIFIERS=@im=none cargo test -p legaia-engine-shell --profile release-test \
  --test integration full_game_ladder:: -- --nocapture --test-threads 1

scripts/ci/gate-replay.py s3_rimelm_freeroam --digest /path/digest.txt --until 3700
```

| Side | Piece | Reads or writes |
|---|---|---|
| Engine | `LEGAIA_DIAG_TICK_DIGEST` ([`host_trace.rs`](../../crates/engine-session/src/host_trace.rs)) | One line a session tick; the gates come from its `eng=`, `sc=`, `pl=`, `pg=`, `flow=` and `st=` words and the mode. The last session in the file is the pad pass |
| Retail | [`autorun_gate_replay.lua`](../../scripts/pcsx-redux/autorun_gate_replay.lua) under PCSX-Redux | Reads `plan.txt`, writes `gate_beats.csv`, `gate_replay.csv` and `gate_events.csv` |
| Report | [`gate-replay.py`](../../scripts/ci/gate-replay.py) | Cuts the beats, runs the probe, prints the table |

Output goes under `captures/gate-replay/`: it reads retail RAM and stays out of the
repository. The replay is report-only, like the [walk film](walk-film.md).

Start the engine on the state's own stream, or the RNG column compares two different
hands: read the state's `0x80009010` and pass it as `LEGAIA_FGL_RNG_SEED`.

## Beats and gates

A beat runs from one gate to the next. The engine's session is cut wherever one of
these changes between two ticks:

| Gate | Engine | Retail |
|---|---|---|
| `engaged=0/1` | `World::script_context_engages_player` | bit `0x80000` of `player+0x10`, player at `0x8007C364` |
| `battle=0/1` | `World::mode` is `Battle` | mode byte `0x8007B83C` is `0x15` (the context `0x8007BD24` outlives the fight, so it says nothing) |
| `scene=NAME` | `active_scene_label` | the name at `0x8007050C` |
| `warp` | player moved over 300 units (Manhattan) in one tick | the same, between two vsyncs |
| `page=N` | the open box's window reaching state `0x19` (the digest's `pg=`) | `_DAT_801F2734` reaching `0x19`; the Nth since the last gate that was neither a page nor a waypoint |
| `pos=X/Z` | a walk's turn: a d-pad change to another held direction, settled for 8 ticks, at least 96 units from the last turn | the player within 24 units of the spot (status `gate`), or past it (`passed`), or engaged first (`cut`) |
| `flow=XX:N`, `act=XX:N` | the Nth entry into a command surface (`World::battle.flow`: `0x1E`, `0x28`, `0x32`, `0x3C`, `0x46`, `0x50`, `0x5A`, `0x64`, `0x6E`, `0x78`) or the action start `0x0A` (`battle_ctx.action_state`) | the same bytes, `ctx[+0x06]` and `ctx[+0x07]`. Closes `cut` if the battle ends first |

The engaged, scene, warp, page and waypoint gates are not cut inside a battle, where
the field player record says nothing, and an engaged run shorter than three ticks
(`--min-dwell`) is a script pulse that does not pair with retail's pass grid.

Retail matches levels rather than edges where a level exists, so two gates the sides
reach in the other order read as a zero-length beat rather than a stall. A warp is
latched and lapses after 30 vsyncs. Pages, prompts and action starts are ordinals, so
a page one side reaches and the other does not costs one beat rather than the rest of
the run.

Retail holds the engaged bit across a door's whole run, from the trigger to the
landing, where the engine's word stays clear. A retail engaged interval that contains
a warp is therefore the door's hold and does not satisfy an `engaged=1` gate; the hold
ends where the bit next falls.

The engine's flow byte changes in the row of the press that caused it, so a flow gate
whose row carries a new press ends one tick later and keeps that press: the press that
moves a surface closes its own beat, and is what that beat repeats if it does not land.

Why waypoints: a retail walk and an engine walk start a pass of position apart, and a
door or a ledge fires on where the player stands, so pad timing alone cannot keep a walk
on the engine's path. Each turn is a gate on position instead, and the next leg's pad
starts where retail really is.

## Replaying a beat

Each beat's pad rows are relative to its opening gate. A press is held at least two ticks
(`--min-press`) and a widened press keeps one released tick after it, so a run of pulses
stays separate edges. The engine's one-on, one-off mash has no room for both: every second
pulse lands on a released tick and is absorbed, leaving two on, two off. At the closing gate:

- **Retail reaches it first.** The beat ends and its remaining rows are dropped; the
  `drop` column counts the Cross presses among them.
- **Retail is late.** The rows have run out, so the beat waits. A walking player keeps
  the beat's last d-pad held and nothing else; an engaged player, and a battle, hold
  nothing.
- **A walk closed by a door or a talk comes up short.** Those gates fire on the spot, so
  the walk ends by steering to where the engine stood when the gate fired
  (`SEEK` in the plan, d-pad masks read off the engine's own walking). This is
  input the engine's log did not supply: each is a `synthetic_seek` row in
  `gate_events.csv` and counted in the `synth` column.
- **Retail is still waiting** after `--synth-after` vsyncs (90 by default, 30 when a
  page is already shown waiting) on an engaged beat, or on a battle byte that waits for
  input. An unclosed page or a prompt the replay's presses did not reach is the usual
  cause, so the probe gives a two-vsync Cross every 30 vsyncs until the gate comes. Each is
  a `synthetic_cross` row and counted in `synth`: a finding, not a repair. Nothing is
  pressed while the action SM owns the frame (`ctx[+0x06] = 0xFF`) or on the flow's other
  bytes, where no surface reads the pad.
- **A battle press did not take.** A beat closing on a `flow` or `act` gate, whose rows
  have run out while the command flow sits on a byte that waits for input (`0x1E`, `0x28`,
  `0x32`, `0x3C`, `0x46`, `0x50`, `0x5A`, `0x64`, `0x6E`, `0x78`), repeats its last logged
  press until the byte moves: a two-vsync pulse every 8 vsyncs, none within 18 vsyncs of the
  byte's last move (a surface ignores the pad for that long after it opens). The arts entry
  `0x50` takes a run of presses, so there the beat's timed rows are skipped and the pulses
  replay its logged presses in order (a run of one press counting once), then keep the last. Each pulse is one `synthetic_repulse` row, with the press and
  the byte it found, and one count in `synth`. A beat adds at most `--repulse-max` pulses
  (16 by default, 0 for none); once they are spent the synthetic Cross above applies, then
  the timeout below.
- **The gate never comes.** After `--max-wait` vsyncs (1800 by default) the run stops with
  status `timeout`, and everything after it is unmeasured.

## Reading the table

| Column | Meaning |
|---|---|
| `E`, `R`, `gap` | The beat's length in engine ticks, in retail vsyncs, and `R - E` |
| `wait`, `drop`, `synth` | Vsyncs retail waited past the pad, Cross presses dropped, synthetic inputs |
| `pos diff` | Retail minus engine player `x,z` at the closing gate; `-` in a battle |
| `E rng`, `R rng` | Draws of the `rand()` LCG over the beat; a `+Nj` suffix counts values the LCG cannot reach, which is a reseed or a stomp |

A beat with a small gap and a matching position says the two agree there whatever came
before it; that is what the resynchronisation buys. A gap in one beat is that beat's own.

A second table, `page waits`, has one row per page gate: the tick the engine's `pg=` reached
`0x19`, the vsync retail's pager state `_DAT_801F2734` did, their difference (`delta`, which
carries the drift of the beats before it) and the part this page added (`step`, the beat's
own `gap`).

## What the town01 run shows

- A door's trigger-to-landing run holds the player's engaged bit on retail
  (`s3_rimelm_freeroam`, the door at vsync 169 onwards).
- The talk to Tetsu holds retail's engaged bit from its first page to its last.
- A battle's opening on retail walks the command flow `0xFD -> 0x00 -> 0x0A -> 0x0B ->
  0x14 -> 0x1E` before the first round prompt.
- The action SM passes through state `0x5A` after every action on retail, between two
  actors of one round as well as at a round's end, so the replay gates the prompt on
  the command flow's `0x1E` rather than on `0x5A`.
- A press held for one vsync can fall between two of retail's game passes and be lost,
  which is why the replay holds every press at least two ticks by default
  (`--min-press`; `1` replays the log as written). With one-tick presses two of the
  talk's page turns wait on a synthetic Cross, and the first round prompt ignores all
  six logged Cross pulses until one arrives; with two-tick presses those page turns
  close on the logged presses and the prompt moves on a logged Cross 20 vsyncs in.

- Up in the ring opens the item window on retail as on the engine: in `0x28` Up stores
  `0x3C` (`0x801D1364`), and only Left or confirm reach the attack-mode prompt `0x78`
  (`0x801D152C..0x801D1608`). From `0x78`, cancel goes back to `0x28`, Right to the arts
  entry `0x50`, and Left or confirm to the target cursor `0x5A`.
- Both sides ignore the ring for about 16 ticks after entering it. A mash merged into one
  solid hold gives retail a single edge, which lands before `0x28` shows and is lost; that
  is why a widened press keeps its released tick.

With every surface gated the fight replays to its end. Every surface holds the pad for its
glide on both sides ([the stepper's return](../subsystems/battle-hud.md#the-steppers-return-holds-the-pad)):
the engine dispatches on the eighteenth tick after the flow byte moves and retail about two
vsyncs later, on its two-vsync pass.

| Surface entered | Engine ticks to the next surface | Retail vsyncs to the next surface |
|---|---|---|
| `0x1E` round prompt, `0x28` ring | 17, 18 | 20, 20 |
| `0x78`, `0x5A`, `0x6E` | 18, 18, 18 | 20, 20, 20 |
| `0x3C` window, `0x64` target confirm | 18, 18 | 20 to 22, 20 |
| `0x50` arts entry | 18 | 20 |
| `0xFE` armed, to the action start | 18 | 20 |

The arts entry takes its typed presses after that hold, and the replay spaces them by the
repeat rule: replayed on the engine's clock the arrows did not all land on retail, so
their timing is the replay's own.

A round's actions run the same states on both sides with the same dwell, except at the
close of each: `0x51` holds 62 ticks on the engine against 76 vsyncs on retail, and retail
then walks `0x5A` where the engine walks `0x00`. The item band (`0x3E..0x40`) and the
Spirit band (`0x47..0x48`) are shorter on the engine, and a strike's `0x20` varies with
the swing, which comes from different draws: the two `rand()` streams part in the first
round and the HP changes follow them.

Retail's first-round box waits for a press and otherwise closes on its own hold, as does
the sign-off box at the end of the fight. With no Cross offered, retail spends 364 vsyncs
in `0x14` and 427 in `0xC8` before the fight closes (the sign-off's 360 and the teardown's
67); a press skips the 360 on both sides. The completion tail stores `0xC8` over the round
start, and the engine's flow rests on `Idle` there, with no prompt up. The replay's last
prompt gate closes `cut`, which is that end and not a stall.

The hits themselves are one kernel on both sides: the thirteen melee hits read off
`FUN_801EC3E4` in this fight, fed through `physical_rolls` with retail's stats and the
same `rand()` draws, give retail's `raw` and `guard` on every hit
(`physical_rolls_match_retail_hits_with_retail_rolls`). The two sides' damage differs only
where the rolls and the swing counts differ, and those follow the stream.

See also [`full-game-ladder.md`](full-game-ladder.md), [`walk-film.md`](walk-film.md),
[`pcsx-redux-automation.md`](pcsx-redux-automation.md).
