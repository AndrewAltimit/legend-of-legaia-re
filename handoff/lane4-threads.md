# Proposed replacement for the `town01` south-gate thread

`docs/reference/open-rev-eng-threads.md` is coordinator-owned. Below is the
replacement text. **The thread closes.** It should move out of the "Field /
locomotion" open table and into
[`re-settled-threads.md`](../docs/reference/re-settled-threads.md), with the
falsified reading recorded in
[`re-do-not-re-walk.md`](../docs/reference/re-do-not-re-walk.md).

---

## 1. Delete the open-table row

```
| `town01` south gate: the reachable trigger band is inert | open - cause pinned, fix owed | [details ↓](#town01-south-gate-the-reachable-trigger-band-is-inert) |
```

…and the whole `### town01 south gate: the reachable trigger band is inert`
section it points at.

## 2. Add to `re-settled-threads.md` (evidence grade: **disassembly**, corroborated by an engine measurement off the real disc)

### Rim Elm's south gate

*What it looked like:* the first scene exit of the game fires when the spine
oracle seats the player on `(25, 46)` and never fires when a player walks
there, so the walk-on dispatch looked broken.

*What it is:* neither of the gate's two `.MAP` kind-1 gate-1 bands is the
mechanism the symptom suggested.

| Record | Tiles | Script |
|---|---|---|
| `P2[10]` | `(24..26, 45)`, `(25, 44)` | `21 21 26 FE FF` - `Nop; Nop; JmpRel`-to-self. Five bytes, no scene change. |
| `P2[0]` | `(24..26, 46)` | `CFlag.Set`, an `Effect` fade, `0x3F` naming `map01` at entry `(0x60, 0x19)`. `C1=[] C2=[]`. |

The exit record is **ungated**; the other record is **inert**. What holds a
player inside Rim Elm is the collision grid - `.MAP` grid row 47 walls
`z ∈ [5888, 5951]` across the doorway - and that row *is* the gate. It is cut
by `town01` `P0[20]`, the gate object's own record, bound by the gate-0 kind-1
trigger at tile `(23, 43)` and executed by the scene-init bind prologue
(`FUN_8003A55C`). The record clears the approach with three `4C 70` paints and
then branches on system flags `327` / `321`:

| `327` | `321` | paints | gate |
|---|---|---|---|
| clear | - | none; the base row-47 wall stands | shut |
| set | clear | re-blocks rows 44..46, seats the gate at `(24, 44)` | shut |
| set | set | `4C 70 18 2D 19 2E` - cols `24..25`, rows `46..47` | **open** |

So a cold boot cannot leave Rim Elm in the port *or* in retail, and the disc
says so rather than the engine. The port already executes the whole chain:
measured on its loaded grid, the three flag states give exactly the three
collision states above, with col 26 correctly re-blocked in the open one.
Pinned by `crates/engine-core/tests/south_gate_disc.rs`; the pad-driven exit is
rung 2 of `crates/engine-shell/tests/critical_path_replay.rs`.

Carrier note: `town0c` holds the same sequence **twice** (its entry script
`P1[0]` and `P0[20]`); `town01` holds it only in `P0[20]`. An engine that
applies nibble-7 deltas from entry scripts alone leaves `town01`'s gate sealed
in every story state.

## 3. Add to `re-do-not-re-walk.md`

### "The reachable band's record force-walks the player through the wall"

*The reading:* record 10 covers the only band a player can stand in, its
timeline ran for two frames and moved the player `+8` in `z` before ending, and
`+8` is one locomotion step - so the record must force-walk the player through
the sealed band and then run the `0x3F`, which would also explain how retail
crosses a wall that blocks ordinary locomotion.

*Why it is wrong:* record 10's entire body is `21 21 26 FE FF`. There is no
walk op and no `0x3F` in it. The two-frame termination is the choreography-wrap
rule doing its job on a `Nop`+`JmpRel`-to-self park, and the `+8` is one frame
of the player's own pad locomotion, observed because the modal-timeline install
is what stopped it. The `0x3F` lives in record 0, on the *other* band, and that
band is opened by a collision paint rather than crossed by a scripted walk.

*The general lesson:* a timeline that "ends without doing the thing" is only
evidence about the runner if the record contains the thing. Disassemble the
record before theorising about the interpreter - five bytes settled a thread
that had already cost several diagnosis cycles on collision, grids, standoffs
and dispatch.
