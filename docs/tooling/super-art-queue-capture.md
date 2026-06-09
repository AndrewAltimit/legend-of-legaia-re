# Capturing the Super / Miracle Art queue builder (F-SUPER)

The Super / Miracle Art **matcher** is ported and wired (which combo triggers
which Super), but the **byte-exact action queue** it expands to is not. The
connector byte after each component art is combo-specific — Vahn's `0x27` is
followed by `0F` in Tri-Somersault but `0E` in Power Slash — so it cannot be
derived from each art's own command string. The runtime builder writes the
queue through the battle-action context at `ctx[+0x274]` (the "queued action"
byte the action SM state `0x00` copies into `actor[+0x1A]`; `ctx[+0x276]` is
the "queued from menu" flag). That builder's PC and the literal connector
bytes have never been captured. See
[`battle-action.md`](../subsystems/battle-action.md) and the F-SUPER / D-ARTS
rows in [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

`ctx` is the pointer global `_DAT_8007BD24` (a `byte *`), so the queue field is
`*(0x8007BD24) + 0x274`. The probe
[`autorun_super_art_queue_builder.lua`](../../scripts/pcsx-redux/autorun_super_art_queue_builder.lua)
reads that pointer at arm time (it is live in any battle save) and watches
`+0x274`/`+0x276` for writes, logging the writer PC + GPRs + the value written
and snapshotting the `+0x270..+0x2A0` queue window on every hit.

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

If a Super Art isn't reachable yet, a **Miracle Art** (the find/replace finisher
of any saved chain) exercises the same `ctx[+0x274]` builder and is an equally
good capture.

A mid-combo battle save we already hold (the Gimard fight "mid-combo" library
state) is **not** sufficient — that is an enemy-boss fight captured for the
capture mechanic, not a player Super/Miracle Art commit, so the queue builder
does not run in it.

## Running the probe

```bash
LEGAIA_FRAMES=1200 \
timeout --kill-after=20s 600s \
bash scripts/pcsx-redux/run_probe.sh \
    --sstate <your-super-combo.sstate> \
    --lua scripts/pcsx-redux/autorun_super_art_queue_builder.lua
```

(The `timeout` wrapper is required — the probe does not always self-quit; see
[`pcsx-redux-automation.md`](pcsx-redux-automation.md). Use `--scenario NAME`
instead of `--sstate` once the state is added to `scripts/scenarios.toml`.)

## Reading the result

Output lands under `captures/super_art_queue_builder/<run-ts>/`:

- `super_art_queue_builder.csv` — one row per write: `tick, pc, addr, off,
  width, newval, queue_hex`. The `pc` column is the **builder** (the routine
  that emits the queue); `newval` is each connector / action byte as it lands;
  `queue_hex` is the `+0x270..+0x2A0` window after each write, so the queue's
  growth across the combo is visible end to end.
- `super_art_queue_builder.hits.txt` — the per-hit GPR snapshots.

The builder PC + the ordered `newval` sequence for a known combo (e.g.
Tri-Somersault) give the literal interleaved queue, which is what the
`SuperMatcher` emission logic needs to reproduce byte-exact.

**Fallback if `+0x274` only shows the action *dequeue*:** if the captured PC is
the SM consuming the queue rather than building it (the connectors may be
staged in a separate queue buffer first), re-run with an Exec-bp on the
captured function and read its source register — the snapshot window and GPR
dump in the `.hits.txt` give the source pointer to chase.
