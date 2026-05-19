# Encounter record format

The on-disc encounter record installed onto a field actor when the script VM triggers a battle. The pointer is written at `actor[+0x94]` by field-VM op handlers and consumed by the world-map / field entity tick at `FUN_801DA51C` to populate the global encounter formation cell.

## Confidence

**Confirmed (record shape, reader, install path) — Inferred (encoding within scripts).**
The reader (`FUN_801DA51C` body at `0x801DA620..0x801DA678`) is fully decoded.
The install path is the script-VM dispatcher's set of "arm encounter" opcodes
(0x37/0x41, 0x38, 0x43, 0x47, 0x4C); Ghidra's C decomp of `FUN_801de840`
makes the install value explicit: `pbVar43 = (byte *)(param_1 + param_2)` —
i.e. the **current script-bytecode opcode pointer**. So the encounter-record
bytes (count at `+0x3`, ids at `+0x4..`) are the trailing operand bytes of
the install opcode itself, inlined into the field-VM script for the scene
that installs the record. **There is no separate on-disc
encounter-record array**; the carriers are the per-scene field-VM script
bundles ([`scene-v12-table.md`](scene-v12-table.md) sister pairs +
[`scene-bundles.md`](scene-bundles.md) `scene_event_scripts`). The exact
opcode encoding (how target / sub-op bytes pack into `+0x0..+0x2`) varies
per opcode and is decoded case-by-case in the dispatcher (see
[`subsystems/script-vm.md`](../subsystems/script-vm.md)).

## Layout

```text
+0x00  u8[3]  reserved             ; cleared to zero by the reader before the copy
+0x03  u8     monster_count         ; 0..4 inclusive
+0x04  u8[N]  monster_ids           ; N == monster_count, each id indexes the
                                    ; monster catalog (the per-scene battle_data
                                    ; group)
[possibly more after — fields not consumed by the formation copy]
```

The reader copies `monster_ids[0..count]` into the global formation cell at `0x8007BD0C..0x8007BD0F` (a 4-byte array, one byte per slot). Slots beyond `count` stay zeroed. `monster_count == 0` clears the formation cell entirely (no monsters spawn this round).

## Reader

`FUN_801DA51C` (the world-map / field entity tick, see [`subsystems/world-map.md`](../subsystems/world-map.md#fun_801da51c-world-map-entity-tick-260-bytes)) at offsets `0x801DA620..0x801DA678`:

```mips
801da620  lui v0,0x8008
801da624  addiu s0,v0,-0x42f4   ; s0 = formation_cell_base = 0x8007BD0C
801da628  sb zero,0x3(s0)        ; clear monster slot 3 (0x8007BD0F)
801da62c  sb zero,0x2(s0)        ; clear monster slot 2 (0x8007BD0E)
801da630  sb zero,0x1(s0)        ; clear monster slot 1 (0x8007BD0D)
801da634  jal 0x801de190         ; helper (effect / sound trigger)
801da638  _sb zero,-0x42f4(v0)   ; clear monster slot 0 (0x8007BD0C)
801da63c  lw v0,0x94(s1)         ; v0 = encounter_record_ptr = actor[+0x94]
801da640  nop
801da644  lbu a1,0x3(v0)         ; a1 = monster_count = record[+0x3]
801da648  nop
801da64c  beq a1,zero,0x801da67c  ; nothing to copy: skip loop
801da650  _clear a0
801da654  move a2,s0             ; a2 = formation cell base
801da658  lw v0,0x94(s1)         ; re-read record pointer (volatile)
801da65c  addu v1,a0,a2          ; v1 = &formation[a0]
801da660  addu v0,a0,v0          ; v0 = record + a0
801da664  lbu v0,0x4(v0)         ; v0 = record[+0x4 + a0] = monster_ids[a0]
801da668  addiu a0,a0,0x1
801da66c  sb v0,0x0(v1)          ; formation[a0-1] = monster_ids[a0-1]
801da670  slt v0,a0,a1
801da674  bne v0,zero,0x801da658  ; loop until a0 == monster_count
801da678  _nop
```

The `s1` register is the actor record (caller's `a0` in `FUN_801DA51C`); `+0x94` is the encounter-record pointer slot. The clear-then-copy ordering means a `monster_count < 4` record correctly leaves trailing slots zeroed.

## Writer (record-pointer install)

The script-VM dispatcher (`FUN_801DE840`, see [`subsystems/script-vm.md`](../subsystems/script-vm.md)) installs the encounter record on an actor with the pattern at `0x801DEEDC..0x801DEEEC`:

```mips
801deedc  lw v0,0x10(s5)
801deee0  sw s0,0x94(s5)         ; actor[+0x94] = s0 (encounter record pointer)
801deee4  sh zero,0x54(s5)       ; reset actor sub-state
801deee8  ori v0,v0,0x400        ; raise "encounter armed" flag (state[0x400])
801deeec  sw v0,0x10(s5)
```

`s0` is set once at the dispatcher prologue (`addu s0, a0, s8` at
`0x801DE858`, i.e. `s0 = param_1 + param_2 = bytecode_buffer + pc_offset`)
and is the **current opcode pointer in the field-VM script bytecode**.
The Ghidra C decomp surfaces this as `pbVar43 = (byte *)(param_1 + param_2)`.
`s5` is the resolved target actor — frequently the player context
(`_DAT_8007C364`); when bit 7 of the opcode byte is set, byte +1 routes
through the system-channel resolver `FUN_8003C83C`.

Multiple opcodes install the same pointer; each pairs the install with
its own pre-install gate (target-actor selector, sub-op switch, etc.) and
each advances the PC by a different amount past the opcode:

| Opcode | Install line | PC advance | Notes |
|---|---|---|---|
| `0x37` / `0x41` (shared case) | `0x801DEEDC` / `0x801DEF08` | `+3` | Bare arm-encounter. Second install on `param_3` if `iVar18 == _DAT_8007C364`. |
| `0x38` | `0x801DEFA0` / `0x801DF038` | `+3` | Falls through to the same install clause; first branch reads a halfword table at `0x80073F04` into `actor[+0x26]` when low-7-bits of byte +1 are zero. |
| `0x43` (sub-op `0/1/A/B`) | `0x801DF3FC` (decomp line 5223) | `+3` | Movement-target setup follows the install (`actor[+0x14..+0x1A]` from operand bytes); the encounter arms when the actor reaches the target. |
| `0x47` | `0x801E1C38` (decomp line 5610) | `+3` | |
| `0x4C` | `0x801E1F44` / `0x801E21C0` / `0x801E... ` (decomp lines 6341 / 6460 / 6556) | `+3` | Three internal install sites in the same case body — one per inner sub-op. |

All install paths share the same pre-install gate:

```
if (actor[+0x94] != 0  ||  actor == _DAT_8007C364) &&
   ((actor[+0x10] & 0x400) == 0  ||  *_DAT_801C6EA4[+8] != 0)
```

— the actor already has a record installed (re-arm), OR it's the player
context (always allowed); AND the armed flag is clear OR the scene
explicitly allows re-arm.

Two non-encounter writes to `actor[+0x94]` also live in the dispatcher
(case `0x34`: `pbVar47 + 0xe` and `pbVar47 + 3`). These do **not** raise
the `0x400` flag and pair with `actor[+0x9c]`/`actor[+0x9e]` zero-writes;
they're a separate "callback" pattern. Only the install sites listed in
the table above are encounter-record arms.

## Formation cell + battle-data variant selector

Adjacent to the formation cell:

| Address | Size | Role |
|---|---|---|
| `0x8007BD0C` | `u8[4]` | Active formation: monster ids per slot, populated by the reader above. |
| `0x8007BD11` | `u8` | Battle-data PROT-id selector. `FUN_800520F0` case-4 path reads this byte and chooses PROT entry **`0x367`** when it equals the case-1 character index, otherwise **`0x36D`**. The selected entry is loaded as a kind-2 streaming asset for the battle scene. |

Snapshots of the formation cell across captures (see [`scripts/scenarios.toml`](../tooling/mednafen-automation.md)):

| Save | `0x8007BD0C..0F` | Interpretation |
|---|---|---|
| `mc1` (pre-encounter, `map01`) | `01 00 00 00` | One-monster record from the previous battle; selector reset to `0x01`. |
| `mc2` (in-battle, `map01`) | `04 04 00 00` | Two-slot encounter, both slots monster id `0x04`. |
| `mc3` (post-battle, `suimon`) | `0A 0D 00 00` | Two-slot encounter, monsters `0x0A` and `0x0D`. |

## What this doesn't tell us

- **Per-opcode encoding of the trailing operand bytes.** Each install
  opcode (0x37/0x41, 0x38, 0x43, 0x47, 0x4C) packs its first 3 bytes
  differently (target selector / sub-op / flag bits); the count + ids
  layout at `+0x3..` is fixed by the reader but the opcode-header
  bytes need a per-case decode in the dispatcher to interpret as
  "encounter trigger from script X at PC Y".
- **The random-encounter trigger path.** The script-VM install opcodes
  catalogued here describe **scripted** encounter arms. Random encounters
  are gated by a per-step rate roll on `_DAT_8007B5F8` (see
  [`subsystems/world-map.md`](../subsystems/world-map.md)). Whether a
  successful roll invokes the script-VM with a "random encounter"
  prologue script that then hits one of the install opcodes — or whether
  the roll function writes the formation cell directly without going
  through `actor[+0x94]` — has not been pinned down. Quick survey: the
  `0085_map01` scene_event_scripts file (Drake's field) carries zero
  install opcodes in its 46 indexed script bodies, despite map01 having
  random encounters in play, which makes the "roll function populates
  the formation cell directly, separately from `actor[+0x94]`"
  hypothesis the more probable shape.
- **The pre-encounter live-pointer state.** No save state in the current
  scenario corpus (`scripts/scenarios.toml`) captures an actor with
  `+0x94` mid-armed — `mc0` has a stale `0x0B33FF70` in that slot and
  `+0x10 & 0x400 == 0`, and every other slot has either zero or a
  `0xFFFFFFFF` sentinel. A byte-level verification of "the install
  opcode bytes match `actor[+0x94]`" needs a fresh capture taken between
  the install opcode dispatch and the `FUN_801DA51C` consumption — a
  one-frame window during scene scripting.

## Files referencing this format

- [`crates/engine-vm`](../../crates/engine-vm/) — the field VM dispatcher port reads the operand and writes the actor pointer slot.
- [`crates/engine-core::encounter`](../../crates/engine-core/) — the runtime engine's `EncounterRecord` parser exposes `monster_count` / `monster_ids` from a candidate byte slice.
- [`subsystems/world-map.md`](../subsystems/world-map.md) — world-map controller integration.
- [`subsystems/script-vm.md`](../subsystems/script-vm.md) — the dispatcher op-handler family that installs the pointer.
