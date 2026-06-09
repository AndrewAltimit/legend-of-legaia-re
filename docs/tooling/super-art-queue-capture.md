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
LEGAIA_FRAMES=1800 \
timeout --kill-after=20s 600s \
bash scripts/pcsx-redux/run_probe.sh \
    --scenario battle_noa_miracle_art_combo \
    --lua scripts/pcsx-redux/autorun_super_art_action_queue.lua
```

(The `timeout` wrapper is required — the probe does not always self-quit; see
[`pcsx-redux-automation.md`](pcsx-redux-automation.md). `--scenario` resolves to
the immutable library backup; `--sstate <path>` works for an ad-hoc state. Run
under `xvfb-run -a` to keep the emulator window off the desktop.)

## Reading the result

Output lands under `captures/super_art_action_queue/<run-ts>/`:

- `super_art_action_queue.csv` — `tick=0` rows are the per-party-actor
  `+0x1D8..+0x200` snapshots taken at arm time (the resident queue); `tick>0`
  rows are `+0x1DF` range-watch writes (PC + post-write bytes).
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

**Still open (wants a Super capture):** the *Super* path tail-replaces with
combo-specific connectors (e.g. Vahn's `0x27` → `0F` vs `0E`). The Miracle
capture validates the shared mechanism (location + encoding + replacement
form); a Vahn Tri-Somersault / Power Slash save run through the same probe would
validate the Super tail connectors the same way.
