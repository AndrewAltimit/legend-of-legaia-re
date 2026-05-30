# Summon overlay record table

The per-summon scene-graph **part list** embedded in each Seru-summon code
overlay. Parser: [`legaia_asset::summon_overlay`](../../crates/asset/src/summon_overlay.rs).

## What it is

A Seru-magic summon visual (e.g. Gimard *Tail Fire*) is a **per-summon MIPS
code overlay**, not an opcode or a `befect_data` asset. The battle action SM
[`FUN_801E295C`](../subsystems/battle-action.md) state `0x29` resolves spell id
`0x81..=0x8b` via `PTR_801f6734[id-0x81]` and calls `FUN_8003EC70(id-0x79, 0)`,
which loads PROT entry `(id-0x79)+0x381`. So the summon overlays are PROT
`905..=915`; **PROT `905` = Gimard Tail Fire** (`id 0x81`: `0x81-0x79 = 8`,
`8 + 0x381 = 0x389 = 905`).

The overlay is raw MIPS code, but a **record table is embedded as inline data**
between two functions — the summon scene-graph's part list. The overlay's
staging loop (link base `0x801F69D8`) walks the table and stages one part-actor
per record via `FUN_80021B04`, then animates each by running the record's
move-VM bytecode through the move-table VM [`FUN_80023070`](../subsystems/move-vm.md).
The motion is **geometric** (move-VM part transforms), not palette cycling —
falsified by the `battle_gimard_tail_fire_a/_b` capture pair, whose CLUT band is
byte-identical across two animation-distinct frames.

## Record table — PROT 905 (Gimard), byte-pinned

**Confirmed** (byte-verified against the disc; disc-gated
`summon_overlay_real::gimard_summon_table_is_pinned`):

- The leading function ends with a `jr ra` epilogue at file offset `0x1804`
  (`+0x1808` delay slot); the record table begins at **`0x180C`**.
- **19 records** of **`0x58`** bytes each, ending exactly at `0x1E94`
  (`0x180C + 19*0x58`) — where raw MIPS code resumes (the next function prologue
  is at `0x201C`). The table has **no count field and no terminator**; its length
  is the staging loop's bound, recovered here from the clean MIPS-code boundary.
- The first three records are transform nodes (`model_sel = -1`).

Per-record layout (`0x58` stride):

```text
+0x00  i16  model_sel     ; -1 = transform node (mesh bound by the record's
                          ;      move-VM anim-bank ops 0x00/0x04);
                          ; >= 0 = DAT_8007C018[model_sel + base] (global TMD pool)
+0x02  u16  flags         ; per-record control flags
+0x04  u8   bytecode[0x54]; u16-aligned move-VM stream, self-terminating
                          ; (the VM stops on an opcode >= 0x47). PC starts at +0x04.
```

The record's move-VM bytecode is a **fixed `0x54`-byte slot**: the VM reads u16
opcodes from offset `+0x04` and halts on an out-of-range opcode (`>= 0x47`,
matching the `sltiu v1, 0x47` bound in `FUN_80023070`).

**Inferred** (consistent with the move VM, not independently pinned): the
bytecode drives each part's per-frame transform through the already-ported move
VM ([`docs/subsystems/move-vm.md`](../subsystems/move-vm.md)); `model_sel >= 0`
binds a mesh from the global TMD pool while `-1` binds via the move-VM anim-bank
ops. The static Gimard flame mesh is `DAT_8007C018[26]` (PROT 871 `etmd.dat`);
the flame texture atlas is PROT 870.

## Scope of the parser

The parser recovers the **record table** the summon driver consumes:

- `parse_at(overlay, offset, count)` — explicit parse.
- `parse_gimard(overlay)` — PROT 905 at its pinned `(0x180C, 19)`.
- `locate_table_offset(overlay, scan_limit)` — recovers the table offset from
  the disc by scanning for the `jr ra` epilogue whose data is a validated record
  run (first record a transform node). The naive "first `jr ra`" misses (several
  functions precede the table), so the candidate is validated against the record
  shape.

The offset and count are **pinned for PROT 905 only**. The sibling summon
overlays (`906..=915`) place their tables at their own offsets/counts and are
not yet individually verified.

## What is NOT yet pinned (open)

A faithful animated summon also needs the overlay's **staging logic** — how each
part-actor's initial world position / render-slot transform is computed (the
`FUN_80021B04` arguments) and the spawn/teardown lifecycle wired to the cast
band (`FUN_801E295C` states `0x29`/`0x32`/`0x33..`). That code lives in the PROT
905 overlay itself, which is **not in the dumped corpus**: the three
`0x801F69D8`-base Ghidra dumps are *other* overlays (`0896`, the Muscle Dome and
world-map dispatchers) that alias the same load address, not the summon. Pinning
the staging loop (and thus driving the animation in the engine) needs a Ghidra
dump of PROT 905 loaded as an overlay program. Until then this parser is the
byte-verified foundation; the engine driver is deferred. See
[`docs/subsystems/effect-vm.md`](../subsystems/effect-vm.md) and the
[open-RE-threads summon row](../reference/open-rev-eng-threads.md#battle--arts--level-up).
