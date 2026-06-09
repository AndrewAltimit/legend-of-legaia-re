# Capturing the Super / Miracle Art queue builder (F-SUPER)

The Super / Miracle Art **matcher** is ported and wired (which combo triggers
which Super), and the **byte-exact action queue** it expands to is now pinned
by capture. The queue is the per-actor **action-parameter byte stream at
`actor[+0x1DF..+0x1F2]`** (see [`battle-action.md`](../subsystems/battle-action.md)):
a Miracle Art clears the queue and writes its replacement string there; a Super
Art tail-replaces the matched bytes. The connector / direction bytes
(`0x0C`=Left, `0x0D`=Right, `0x0E`=Down, `0x0F`=Up, `0x1A`=`SpecialStarter`)
interleave the art action-constants (`0x1B..0x32`) in that stream.

> **Corrected from the original hypothesis.** This was first framed as
> `ctx[+0x274]` (`*(0x8007BD24)+0x274`). A capture
> ([`autorun_super_art_queue_builder.lua`](../../scripts/pcsx-redux/autorun_super_art_queue_builder.lua))
> showed `ctx+0x274` is the **turn-order active-actor index** written by
> `recompute_battle_order` (`FUN_801DABA4`, `lbu v0,0x11(v1); sb v0,0x274`) —
> *not* the connector queue. That probe is kept as a turn-order diagnostic; the
> queue probe is
> [`autorun_super_art_action_queue.lua`](../../scripts/pcsx-redux/autorun_super_art_action_queue.lua),
> which resolves the three party actors from the `0x801C9370` pointer table and
> reads / range-watches each one's `+0x1DF` window.

The battle-actor pointer table is `0x801C9370` (8 × `u32`; slots 0..2 = party).
The queue is resident the moment a combo is committed, so the probe both
**snapshots** `actor[+0x1D8..+0x200]` at arm time (catching an already-built
queue) and **range-watches** `+0x1DF..+0x1F2` (`probe.step.find_writer`) for the
build / dequeue writes.

## Which emulator

**PCSX-Redux**, not mednafen. The capture is a live write-watchpoint, which
needs the interpreter + debugger (`run_probe.sh` sets `-interpreter -debugger`
by default). Mednafen save states are full RAM/SPU/VRAM snapshots but cannot
drive a runtime watchpoint, so a mednafen state cannot answer this.

## Exactly which moment to save

Save a PCSX-Redux state at the instant a **Super Art or Miracle Art combo is
committed in battle and is resolving** — i.e. the chained component arts are on
screen, executing, right after you confirm the arts input. Concretely:

1. Be in any battle with a party member who has learned the component arts of
   a Super Art. The earliest reliable one is **Vahn's Tri-Somersault** (its
   queue is the worked example in the docs: component art `0x27` → connector
   `0F`); any Super or Miracle Art works — they share the builder.
2. Open that character's **Arts** menu and enter the full directional command
   string for the Super combo, then **confirm** it (so the Super banner /
   chained arts begin).
3. **Save the save-state on that frame or one or two frames into the
   execution** — while the chained arts are visibly playing out. Saving exactly
   at the commit is fine; the probe runs ~1200 frames forward through the whole
   resolution and will catch the queue writes either way.

A **Miracle Art** (the find/replace finisher of any saved chain) writes the
same `actor[+0x1DF]` queue and is an equally good — often easier — capture than
a true Super. The catalogued `battle_noa_miracle_art_combo` state (Noa
auto-casting a Miracle on load) is exactly this.

A mid-combo battle save we already hold (the Gimard fight "mid-combo" library
state) is **not** sufficient — that is an enemy-boss fight, not a player
Super/Miracle Art commit, so no player art queue is built.

## Running the probe

```bash
xvfb-run -a timeout --kill-after=15s 240s \
bash scripts/pcsx-redux/run_probe.sh \
    --scenario battle_vahn_tri_somersault_super \
    --lua scripts/pcsx-redux/autorun_super_art_action_queue.lua
# (or --scenario battle_noa_miracle_art_combo for the Miracle path)
```

(`--scenario` resolves to the immutable library backup; `--sstate <path>` works
for an ad-hoc state. `xvfb-run -a` keeps the emulator window off the desktop.
The `timeout` wrapper is required — see the run-time note below and
[`pcsx-redux-automation.md`](pcsx-redux-automation.md).)

## Reading the result

Output lands under `captures/super_art_action_queue/<run-ts>/`:

- `super_art_action_queue.csv` — `tick=0` rows are the per-party-actor
  `+0x1D8..+0x200` snapshots taken at arm time (the resident queue — this is the
  byte-exact answer). `tick>0` rows are `+0x1DF` range-watch writes (PC +
  post-write bytes) and appear only with `LEGAIA_TRACE_WRITES=1`.
- the `pcsx.log` `[aq]` lines mirror the same, plus the active-actor index
  read from `ctx+0x274`.

Decode the `+0x1DF` stream with the `ActionConstant` byte values
(`legaia_art::queue`): `0x0C/0x0D/0x0E/0x0F` = Left/Right/Down/Up,
`0x1A` = `SpecialStarter`, `0x1B..0x32` = art constants. The Miracle/Super
expansion is `[directions][SpecialStarter][art constants…]`.

## Result — captured + validated (Noa Miracle Art)

The `battle_noa_miracle_art_combo` capture pins **`actor[+0x1DF..+0x1F2]`** as
the action queue (active-actor index `1` = Noa). Her resident queue decodes
to `Right, Left, Up, Down, SpecialStarter, Art2A, Art26, Art27, Art2B, Art24,
Art2C, Art2D` — **byte-identical to the modeled replacement string in
`crates/art/src/miracle.rs`** (the Acrobatic Blitz → Dolphin Attack → Mirage
Lancer → Lizard Tail → Swan Driver → Jurassic Blow 1+2 Miracle, previously
sourced from a researcher spreadsheet). So the engine's Miracle queue and the
`ActionConstant` byte encoding are now **runtime-validated** against retail RAM,
and the queue location is pinned.

## Result — Super path validated (Vahn Tri-Somersault)

The `battle_vahn_tri_somersault_super` capture (Vahn auto-casting Tri-Somersault
during a counterattack) closes the Super side. Vahn's resident queue at
`actor[+0x1DF]` is:

```
0F 0E 19 27 0F 19 1F 0E 1A 2B 2B 2B
= [Up Down] [Starter Somersault(0x27) Up] [Starter Cyclone(0x1F) Down]
  [SpecialStarter Tri-Somersault(0x2B) ×3]
```

The matched/replaced tail `19 27 0F 19 1F 0E 1A 2B 2B 2B` is **byte-identical to
`super_art.rs`'s `Tri-Somersault` `replace` field** — runtime-validating the
combo-specific connectors the thread flagged (`Somersault 0x27 → 0F`,
`Cyclone 0x1F → 0E`) and the finisher tail. The leading `0F 0E` is the residual
direction input the tail-replace ran behind. The `find_writer` trace shows the
dequeue at pc `0x801D89D8` (the action SM consuming the queue head-first, one
entry per executed action).

So both an in-the-wild Miracle (Noa) and Super (Vahn) now confirm the engine's
modeled queues + `ActionConstant` encoding byte-exact. Remaining is only the
long tail of the other 13 Supers' replace strings, each a one-capture check
through the same probe if ever needed.

## Note on run time

The queue is **resident at load**, so the per-actor `+0x1DF` snapshot rows are
written to the CSV on the first post-load frame (~a second in). Read the CSV as
soon as the `[aq] party slot …` lines appear in the log — you do not need to
wait for a clean exit. As with every probe here, **wrap the run in `timeout`**:
self-exit under the `-interpreter -debugger` build is slow (the harness waits
`capture_frames + quit_delay` real vsyncs and the interpreter runs at only a few
fps), so the default `LEGAIA_FRAMES` is kept small and a kill after the snapshot
loses nothing (CSV rows are flushed as they are written).

The `+0x1DF` dequeue **write trace** is opt-in (`LEGAIA_TRACE_WRITES=1`): it arms
~30 width-2 write-breakpoints across the three party actors' queue windows,
which slows the interpreter to a crawl, so it is off by default — the snapshot
alone gives the byte-exact queue.
