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

`FUN_801DA51C` (the world-map / field entity tick, see [`subsystems/world-map.md`](../subsystems/world-map.md#fun_801da51c---world-map-entity-tick-260-bytes)) at offsets `0x801DA620..0x801DA678`:

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

The `s1` register is the actor record (caller's `a0` in `FUN_801DA51C`); `+0x94` is the encounter-record pointer slot. The clear-then-copy ordering means a `monster_count < 4` record correctly leaves trailing slots zeroed. After the copy the reader clears `entity[+0x94]` and advances the entity's 5-state SM (`entity[+0x8A]++`), so the formation copy fires exactly once per arm.

Just before the copy (`0x801DA5F8..0x801DA61C`) the reader also reads `record[+0]` (the **opcode byte** the record overlays — see the writer below) and, when it is non-zero, ORs bit `0x80` into a battle-setup flag byte. Because the install opcodes are themselves non-zero, this bit is effectively always raised for a scripted arm; the byte is the first of the record's three "reserved" bytes (`+0x00..+0x02` = the install opcode + its two operand bytes).

**Discriminator (relevant to wiring this in an engine).** There is no dedicated "encounter" opcode: the install opcodes below are the field VM's generic **halt-acquire** family (`0x37/0x41/0x38/0x43/0x47/0x4C`), the same ones used for ordinary script yields. What turns a halt into an encounter is the *consumer*: only world-map / field **entities** ticked by `FUN_801DA51C` (those carrying the 5-state `entity[+0x8A]` SM) ever read their `+0x94` as a formation record, and only once the SM reaches the encounter-confirm state. The random-encounter path enters that state via the `FUN_801D9E1C` roll (state 0); a *scripted* arm relies on the scene bytecode having authored `[count @ +3][ids @ +4..]` after the halt opcode on such an entity's context. Which specific opcode a given scripted fight uses, and how that scene advances the entity SM to the confirm state, are therefore per-scene bytecode facts (not resolvable from the static dispatcher alone).

**Engine port.** The clean-room field VM mirrors this discriminator split: the bare arm-encounter op (`0x37`/`0x41`) calls `FieldHost::is_scripted_encounter_armed()` and, only when armed, hands `FieldHost::install_scripted_encounter()` the bounded record window overlaying the opcode (`[opcode][op1][op2][count][≤4 ids]`). The engine consumer (`World`) parses that window as an `EncounterRecord`, registers the formation, and forces the next `on_field_step` roll (`World::install_scripted_encounter` / `arm_scripted_encounter`); a successful install disarms (fire-once, matching the retail `entity[+0x94]` clear). `scripted_encounter_armed` is the engine stand-in for "the active entity's `FUN_801DA51C` SM reached the confirm state" until the per-scene carrier identity is pinned.

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

### Worked example: the Rim Elm training fight

The game's opening battle — the training fight in Rim Elm (`town01`) — is a
scripted **single-monster** encounter. The opponent is monster archive id
`0x4F` ("Tetsu"); it is the only monster in the formation. Reading the
formation cell across the training-fight capture corpus shows the install
boundary cleanly:

| Capture phase (`town01`) | `0x8007BD0C..0F` | Interpretation |
|---|---|---|
| Pre-battle field (free movement, before the fight) | `00 00 00 00` | No formation installed — the cell is clear. |
| Battle loading (`game_mode 0x15`, graphics not yet drawn) | `4F 00 00 00` | One-monster formation: id `0x4F` in slot 0. |
| Battle running (graphics / command menu / submenu) | `4F 00 00 00` | Same lone-monster formation. |
| Post-battle field (back to `game_mode 0x03`) | `4F 00 00 00` | Cell retains the last formation until the next install (it is cleared only at the next encounter, not on victory). |

So the formation copy happens at battle entry, exactly as the reader above
describes: the cell is empty in the field and carries the lone id `0x4F`
from battle-load onward.

**The id `0x4F` is not an inline script literal — it is a per-scene formation
index.** Two independent surveys of town01's bytecode find no `[count=1][0x4F]`
install operand anywhere:

1. The `scene_event_scripts` prescript at PROT entry 3 — the small structured
   records carry no such pattern, and the `0x4F` bytes in the bulk payload are
   high-entropy asset data, not bytecode.
2. The scene's **MAN partition-1 field-VM scripts** (record 0 = scene-entry system
   script, records 1.. = per-actor interaction scripts) walked **opcode-aware** with
   the field-VM disassembler (`legaia_engine_vm::field_disasm` driving
   `legaia_engine_core::man_field_scripts::walk_partition1_scripts`). The walk lands
   on every `0x37`/`0x41` yield byte and decodes the trailing `[count][ids]` window
   at each: across 53 records and 71 yield sites, **zero** carry the `[1][0x4F]`
   Tetsu signature. Every window that decodes is a `count=0` artifact from the
   walker stepping into embedded MES dialog text (the windows are plain ASCII —
   `1F 64 6F 20` = `"do "`, `1F 56 61` = `"Va"`, …). This is exactly the false
   positive a naive `0x37`/`0x41` byte-scan produces; the opcode-aware walk is what
   distinguishes a real arm boundary from a dialog byte. The system entry script
   (record 0) decodes near-cleanly (a real executable stream), while the
   interaction records desync into dialog — itself evidence the encounter arm is
   not script-borne. See the disc-gated regression test
   `crates/engine-core/tests/town01_p1_arm_sites.rs` and the
   `legaia-engine man-scripts --scene <name>` survey CLI (its
   `--gflag-partition <n>` flag walks any partition's records for
   `GFLAG_SET`/`GFLAG_CLEAR` writes — e.g. partition 2 surfaces the opening
   prologue's cutscene-timeline `GFLAG_SET 26` town01 hand-off arm).

Instead, the lone-`0x4F` formation is **town01 MAN formation index 4**. The per-scene formations load from
the scene's MAN asset into a contiguous **8-byte-stride** table
(`[3 reserved][count: 0..4][≤4 ids]`) resident in the field work area; in the live
"talk to Tetsu / Come at me!" save state that table reads:

```
[0] 00 00 00 01 04            [4] 00 00 00 01 4f   <- Tetsu (count 1, id 0x4F)
[1] 00 00 00 01 07            [5] 00 00 00 02 0a 0a
[2] 00 00 00 01 0a            [6] 00 00 00 02 3d 3d
[3] 00 00 00 04 3f 3e 3e 3e
```

This is **byte-identical** to the engine's MAN parse (`legaia_asset::man_section`
→ `crate::encounter_man::scene_encounter_from_man`, which yields exactly these 7
formations for town01, `formation_id` 4 = `[0x4F]`). The scripted carrier entity
selects this formation **by index** (it points `actor[+0x94]` at the table row, and
`FUN_801DA51C` copies it into the cell on the dialogue-accept), which is why the
cell shows the lone `0x4F` while no inline operand carries it. The pre-confirm
("Come at me!") capture has the cell still clear — the install fires on the
accept press.

The clean-room engine reaches this fight faithfully through the same indexed
table: a cold boot loads town01's MAN formations (with the monster archive's real
stats merged at scene entry), and `World::install_man_formation(RIM_ELM_TRAINING_FORMATION_ID)`
(`= 4`, in [`encounter_record.rs`](../../crates/engine-core/src/encounter_record.rs))
installs the existing row as the forced next encounter — no re-encoded record, the
scene's merged stats stand (Tetsu's HP 999). `EncounterRecord::rim_elm_training()`
remains for the equivalent hand-built `[count=1][0x4F]` window used by the
arm-seam path.

## Scripted-battle id path (`FUN_8005567c`)

The `actor[+0x94]` record path above is one of **two** ways the formation cell is
populated. The other is a global **battle-id** at `DAT_8007b7fc` consumed at
battle-init by `FUN_80055b6c`, which calls `FUN_8005567c` (`SCUS_942.54`) to expand
the id into the cell:

```c
DAT_8007bd0c = DAT_8007bd0d = DAT_8007bd0e = (u8)DAT_8007b7fc;   // lone / paired id
// id ranges 0x07..0x09, 0x49..0x4d, 0x88..0x8b, 0xa2..0xff get bespoke multi-monster
// / boss expansions (DAT_8007bd0e cleared; DAT_8007bd10.. set per id);
if (DAT_8007b7fc == 0) { cell = [4, _, 4, 4]; }                 // default-zero fallback
```

`DAT_8007b7fc` is a **transient** parameter: it is `0` in every captured Tetsu
frame (the id is consumed and cleared by the time the battle is resident). The
distinguishing signature is the cell *shape* — `FUN_8005567c` writes slots 0/1/2
for a plain id (`[0x4F,0x4F,0x4F,0]`), whereas the Tetsu cell is `[0x4F,0,0,0]`
(slot 0 only, slots 1-3 cleared), which is the `FUN_801DA51C` count-1 record path.
So the Rim Elm fight uses the indexed-record path; `FUN_8005567c` is the
formation source for battles cued by a battle-id rather than an entity record
(no writer of `DAT_8007b7fc` is present in `SCUS_942.54`, so the id is set from a
field overlay).

## Random-encounter trigger path

The script-VM install opcodes above describe **scripted** encounter
arms. Random encounters use a separate path:

**Roll function.** `FUN_801D9E1C` (in the world_map overlay; also paged
in by dance / fishing / slot-machine / cutscene_mapview / dialog_typing /
debug_menu overlays — same code each time) runs once per movement
update. It walks the per-scene **region table** at
`*(_DAT_801C6EA4 + 0x28) + 1` and matches the player's `(x, y)` against
each region's AABB at `pbVar9[0..3]`. The matching region descriptor
supplies:

- `pbVar9[4]`: per-step rate increment.
- `pbVar9[6]`, `pbVar9[7]`: base + count of the formation slice the
  region rolls into.

The rate is then scaled by the user-config setting at `_DAT_8007B5F8`
(`0` off, `1` low, `2` normal → `<< 2`, `3` high → `>> 2`; the world-map
debug menu `ENCOUNT` row cycles this byte) and by accessory / status
modifiers (`FUN_800431D0(0x3B)` / `(0x3C)` / `FUN_8003CE64(0x1D)` /
`(0x1E)`), then subtracted from the step counter at `_DAT_8007B5FC`.
When the counter goes ≤ 0, two RNG draws pick a formation id in
`[pbVar9[6], pbVar9[6] + pbVar9[7])` and the roll function installs:

```c
*(short *)(actor + 0x88) = formation_id;
*(short *)(actor + 0x8a) += 1;
*(uint  *)(actor + 0x94) = formation_table_base + 1 + formation_id * stride;
*(uint  *)(_DAT_8007c364 + 0x10) |= 0x80000;
_DAT_8007b5fc = (RNG % 0x1e7) - ((RNG % 0x1e7) - 0x3ce);
```

Note the install at `+0x94` uses the same slot the scripted-encounter
path uses, but raises a **different flag bit** (`0x80000`, not
`0x400`). `FUN_801DA51C` reads `actor[+0x94]` without checking either
flag, so a single reader serves both paths.

**Engine port (region-keyed roll).** The roll above is ported clean-room as
[`region_encounter`](../../crates/engine-core/src/region_encounter.rs)
(`PORT: FUN_801D9E1C`). `RegionEncounterTable` preserves each region's
tile-AABB + rate increment + formation slice (built from the MAN via
`region_encounter_table_from_man`, the position-routed companion to the
aggregated [`encounter_man::encounter_table_from_man`](../../crates/engine-core/src/encounter_man.rs)).
`RegionEncounterTracker::on_step(world_x, world_z, rng)` reduces the position to
a 128-unit tile (`coord >> 7`), selects the first region whose AABB contains it,
subtracts the setting-scaled rate increment from the step counter, and on a
`<= 0` counter rolls a formation uniformly from `[base, base + count)` with the
one-step anti-repeat and the `0x3ce + rng%0x1e7 - rng%0x1e7` counter reset. The
no-trigger path consumes zero RNG, matching retail (so it is replay-safe).

**Encounter control block (`_DAT_801C6EA4`).** A 100-byte block
allocated by `FUN_8003A024` and populated per-scene by `FUN_8003A110`
("Mesworks set encount group table"). After scene load it carries:

| Offset | Field |
|---|---|
| `+0x20` | Formation table base. Records of stride `+0x5d`; record at index `i` lives at `base + 1 + i * stride` (the `+1` skips a leading count byte). |
| `+0x24` | Condition table base. Records of stride `+0x5e`. |
| `+0x28` | Region table base. Records of stride `+0x5f` (AABB + rate + formation range). |
| `+0x5d` | Formation record stride. |
| `+0x5e` | Condition record stride. |
| `+0x5f` | Region record stride. |

**Per-scene MAN file (asset type `0x03`).** The encounter data lives as
the `Man` asset in each scene's
[`scene_asset_table`](scene-bundles.md#scene_asset_table---count-prefixed-asset-bundle)
7-asset bundle, descriptor index 2. The asset dispatcher
(`FUN_8001F05C`) LZS-decompresses the Man payload into a heap buffer
addressed by `_DAT_8007B898`; `FUN_8003AEB0` (the per-scene MAN
walker, called from `FUN_801D6704` / family scene loaders) then walks
the MAN's multi-section header and finally writes the encounter-
section pointer into `ctrl[+0x20]` before calling `FUN_8003A110`.

The MAN multi-section header is byte-exact across all 80 retail
`scene_asset_table` bundles and lives at MAN offset `0`:

```text
+0x00..+0x02   u16 LE  status_flags                 ; return value;
                                                    ; bit 0x400 hints
                                                    ; world-map bulk
                                                    ; terrain (set on
                                                    ; map01/map02/map03)
+0x01          u8      low_bit_DAT_8007B6A8         ; secondary flag
+0x02..+0x22   16 × s16 LE  depth_lut               ; written negated to
                                                    ; the GTE scratchpad
                                                    ; (0x1F800314+0x48)
                                                    ; for per-scene fog
                                                    ; / depth-of-field
+0x22..+0x24   s16 LE  N0                           ; partition-0 record
                                                    ; count (open)
+0x24..+0x26   s16 LE  N1                           ; partition-1 record
                                                    ; count (consumed by
                                                    ; FUN_8003A1E4 as the
                                                    ; per-scene NPC /
                                                    ; actor placement
                                                    ; list)
+0x26..+0x28   s16 LE  N2                           ; partition-2 (open)
+0x28..+0x2B   u24 LE  u24_28                       ; in-table byte
                                                    ; offset of section
                                                    ; 0's length prefix
                                                    ; within the data
                                                    ; region (relative
                                                    ; to records-end)
+0x2B..+0x2B+3*(N0+N1+N2)  3-byte records           ; concatenated
                                                    ; [P0..P1..P2]
                                                    ; partitions; each
                                                    ; record is a u24 LE
                                                    ; byte offset into
                                                    ; the data region
+0x2B + 3*(N0+N1+N2)     data region                ; encounter section,
                                                    ; actor-placement
                                                    ; payloads, etc.
```

Section 0 (the encounter section) lives at
`records_end + u24_28`. Sections 1..=5 chain via a 3-byte length
prefix: each section is `[u24 LE length][length payload bytes]` and
the next section starts at `current + 3 + length`. Section 5 is
universally a zero-length terminator across the retail corpus.

The six sections install into different globals (per FUN_8003AEB0):

| Index | Install target | Role |
|---|---|---|
| 0 | `_DAT_801C6EA4[+0x20]` | Encounter / formation tables (consumed by `FUN_8003A110`; see below). |
| 1 | `_DAT_801C6EA4[+0x00]` | (Open) - referenced by the field-script context dispatcher; the pointer is advanced past its length prefix immediately after the walk. |
| 2 | `_DAT_801C6EA0` | (Open) - same advance-by-3 treatment. |
| 3 | `_DAT_801C6EA4[+0x04]` | (Open) - same advance-by-3 treatment. |
| 4 | `DAT_80073ED8` | (Open) - advances by 4 (skipping length + 1 byte); the byte at `+3` is copied into `DAT_80073EDC`, and a zero terminator there detaches the pointer (`DAT_80073ED8 = NULL`). |
| 5 | `DAT_80073EE0` | Universally a zero-length terminator. Reserved-but-unused sentinel. |

The encounter-section header (consumed by `FUN_8003A110`) is 4 bytes
followed by three count-prefixed record arrays:

```text
+0x00          u8      formation_stride
+0x01          u8      condition_stride
+0x02          u8      region_stride
+0x03          u8      formation_count
+0x04          formation_count × formation_stride bytes   ; encounter records
                   record_i[+0..+2] = reserved (other-path scratch)
                   record_i[+3]     = monster_count
                   record_i[+4..]   = monster_ids
+next          u8 condition_count + condition_count × condition_stride bytes
+next          u8 region_count + region_count × region_stride bytes
                   region_j[+0..+3] = (x_min, y_min, x_max, y_max)
                   region_j[+4]     = rate increment
                   region_j[+6]     = formation-range base index
                   region_j[+7]     = formation-range count
                   region_j[+8..]   = battle-bg variant flags + extras
```

For `0086_map01` (Drake's kingdom field-scene bundle), the MAN
descriptor sits at file offset `0x3B238` (LZS in 6537 → out 11274
bytes). The decoded layout is:

```
status_flags=0x01B2, N0=12, N1=9, N2=42, u24_28=0x21D8,
data_region @ 0xE8, section_0 (encounter) @ 0x22C0 len 0x43E
section_1 @ 0x2701 len 0x15
section_2 @ 0x2719 len 0x0E
section_3 @ 0x272A len 0xC7
section_4 @ 0x27F4 len 0x6F
section_5 @ 0x2866 (terminator)

encounter: formation_stride=8, condition_stride=4, region_stride=12;
37 formations, 4 conditions, 64 regions.
```

Formation 3 = `[00 00 00 02 04 04 00 00]` matches `mc2`'s in-RAM
formation cell `04 04 00 00` byte-for-byte. The
[`legaia_asset::man_section`](../../crates/asset/src/man_section.rs)
crate exposes this parser plus per-record decoders for formation +
region rows.

## What this doesn't tell us

- **Per-opcode encoding of the trailing operand bytes.** Each install
  opcode (0x37/0x41, 0x38, 0x43, 0x47, 0x4C) packs its first 3 bytes
  differently (target selector / sub-op / flag bits); the count + ids
  layout at `+0x3..` is fixed by the reader but the opcode-header
  bytes need a per-case decode in the dispatcher to interpret as
  "encounter trigger from script X at PC Y".
- **Sibling section roles (sections 1..4).** The MAN multi-section
  walker pins exact offsets and lengths for every section across all
  80 retail scene bundles, but the interior layout of sections 1..4
  (the three pointers `_DAT_801C6EA4 + 0/+4`, `_DAT_801C6EA0`, and
  `DAT_80073ED8` install onto) is still open. The lengths cluster
  small (often 1 byte, occasionally a few hundred), suggesting per-
  scene callbacks / inline state rather than record arrays.
- **The pre-encounter live-pointer state.** No save state in the
  current scenario corpus captures an actor with `+0x94` mid-armed —
  the corpus's `mc0` carries a stale value and every other slot is
  zero or `0xFFFFFFFF`. Byte-level verification of "the encounter
  record bytes at the live `actor[+0x94]` match the parsed Man's
  formation record" needs a fresh save-state capture taken in the
  one-frame window between roll and `FUN_801DA51C` consumption.

## Files referencing this format

- [`crates/engine-vm`](../../crates/engine-vm/) — the field VM dispatcher port reads the operand and writes the actor pointer slot.
- [`crates/engine-core::encounter`](../../crates/engine-core/) — the runtime engine's `EncounterRecord` parser exposes `monster_count` / `monster_ids` from a candidate byte slice.
- [`subsystems/world-map.md`](../subsystems/world-map.md) — world-map controller integration.
- [`subsystems/script-vm.md`](../subsystems/script-vm.md) — the dispatcher op-handler family that installs the pointer.
