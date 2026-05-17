# Encounter record format

The on-disc encounter record installed onto a field actor when the script VM triggers a battle. The pointer is written at `actor[+0x94]` by field-VM op handlers and consumed by the world-map / field entity tick at `FUN_801DA51C` to populate the global encounter formation cell.

## Confidence

**Inferred — structural reading from a single tracer.** The reader (`FUN_801DA51C` body at `0x801DA620..0x801DA678`) is fully decoded. The on-disc *carrier* of these records (which PROT entry holds the encounter-record array) is not yet pinned — see "What this doesn't tell us" below.

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

`s0` is loaded from the script-bytecode immediate operand earlier in the dispatcher; `s5` is the resolved target actor (from a context-pointer that may indirect through the system-channel resolver `FUN_8003C83C`). Several copies of this clause appear in the dispatcher (`0x801DEF08`, `0x801DEFA0`, `0x801DF038`, `0x801DF3FC`, `0x801E1C38`, `0x801E1F44`, `0x801E21C0` …); each is a different op handler that triggers an encounter on a different actor selector (self / system-channel / actor-by-id / etc.).

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

- **The on-disc carrier of encounter records.** The records come from somewhere on the disc — most likely embedded in `0865_battle_data` (15.99 MB) or in a per-scene field-pack slot — but the writer that reads them off disc and installs the pointer at `actor[+0x94]` is in an overlay slice that hasn't been narrowed yet. Once located, the offset and stride of the on-disc encounter array can be lifted directly.
- **The encounter selector logic.** The script-VM op stores a single `s0` value into `actor[+0x94]`; how the bytecode encodes "which encounter from the per-scene set" (an index? a byte offset? a fixed-record id?) needs the upstream operand decode.
- **Per-scene encounter rate / safe-zone gating.** That's a different mechanism (per-step roll on a counter at `_DAT_8007B5F8` according to [`subsystems/world-map.md`](../subsystems/world-map.md)); the encounter record is the *result* of a successful trigger, not the rate input.

## Files referencing this format

- [`crates/engine-vm`](../../crates/engine-vm/) — the field VM dispatcher port reads the operand and writes the actor pointer slot.
- [`crates/engine-core::encounter`](../../crates/engine-core/) — the runtime engine's `EncounterRecord` parser exposes `monster_count` / `monster_ids` from a candidate byte slice.
- [`subsystems/world-map.md`](../subsystems/world-map.md) — world-map controller integration.
- [`subsystems/script-vm.md`](../subsystems/script-vm.md) — the dispatcher op-handler family that installs the pointer.
