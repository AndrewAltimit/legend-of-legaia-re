# Move-power / parameter table

The battle-action overlay's per-move **power and behaviour** table. Each battle
move (a physical attack, an art component, an enemy special, …) has a 26-byte
record that the action state machine reads to drive the move's damage roll,
homing motion, hit reaction, sound cue and spawned effects.

Parser: `legaia_asset::move_power`. CLI: `asset move-power <raw PROT 0898 entry>`.
Engine consumer: `engine-core::move_power::MovePowerCatalog` pairs the table with
the id→index map and loads it onto `World::move_power` from PROT 0898; the
monster special-attack damage path rolls each move's `+0` power through the
arts/physical kernel (see [battle-formulas.md](../subsystems/battle-formulas.md#arts--physical-branch-attacker_slot--7)).
The catalog also resolves a move id to a full presentation/timing descriptor
(`MovePowerCatalog::fx_for_move_id` → `MoveFx`): every behavioural field past
`+0` plus the cross-table joins (the `+0x0a` impact selector → its config word,
each `+0x12`/`+0x16` effect-id-list byte → an `EffectListEntry` with its
spawn-prototype param + SFX cue). It is a descriptor surface only — see Open.
Provenance is the battle-action overlay (PROT entry 0898, CDNAME
`overlay_battle_action` / `overlay_0898`); dumps under `ghidra/scripts/funcs/`
are labelled `overlay_battle_action_*` and the byte-identical aliases
`overlay_0897_*` / `overlay_magic_*` / `overlay_muscle_dome_*`.

## Location — static overlay data

| Thing | Value |
|---|---|
| PROT entry | 0898 (battle-action overlay) |
| Table runtime VA | `0x801F4F5C` |
| Table raw-entry file offset | `0x26744` |
| Record stride | 26 bytes (the `26 * index` math in `FUN_801dd0ac`) |
| id → index map runtime VA | `0x801F4E63` (`0x80` bytes before the table) |
| id → index map file offset | `0x2664B` (= table − `0xF9`) |
| id → index map length | `0x80` (move ids `0x00..=0x7F`) |

The whole `0x801F4F5C..0x801F69D8` window is **static** — loaded with the
battle-action overlay image, not built per battle (byte-identical across two
unrelated battle save states). Confidence: **Confirmed** — the raw PROT 0898
bytes byte-match the in-RAM table, and `FUN_801dd0ac`'s code body maps with the
same overlay base.

## Indexing — `power_table[map[move_id]]`

The kernel's record index is **not** the battle move id directly. The setup site
reads the actor's move id at `actor[+0x1df]`, looks it up in the 128-byte
id → index map, and indexes the table with the result:

```
record = &table[ map[ actor[0x1df] ] ]      (FUN_801dd0ac(*(byte*)(actor+0x1df) + 0x801F4E63, ...))
```

A map byte of `0x00` or `0xFF` means "this move id has no power record". The map
resolves move ids `0x04..=0x74` to power indices `0x01..=0x2b`. The move id is
the **same id space as the SCUS spell-name table** (`DAT_800754C8`,
[spell-table.md](spell-table.md), also indexed by `actor[+0x1df]`), so joining
the two labels every record:

- records `0x10..=0x2b` (move ids `0x25..=0x74`) are the **named monster
  special attacks** (Fire Breath `0x25`, Tail Fire `0x27`, … the late-game
  attacks at `0x61..=0x74`) — this is their special-attack *power*, separate from
  the *name* the spell table carries.
- records `0x01..=0x0f` (move ids `0x04..=0x1f`) are the spell table's unnamed
  **internal enemy-attack tiers** (escalating-power triplets).

Record 0 is an all-zero unused slot.

**Cross-checked against the monster archive** (PROT 867 `+0x21..=+0x23` global
magic-attack ids): **28 of the 29** mapped named ids (`≥0x25`) are exactly the
special attacks enemies cast — so the named-attack records line up with the
enemy roster's attack lists. But the table is a **subset** of all enemy
named attacks: across 186 monsters, 46 distinct attack ids appear, of which only
28 are in this table; the other 18 (`0x2E`/`0x2F`/`0x3C`/`0x4A..=0x6E`, and
`0xA7`/`0xB8` — the last two beyond the `0x00..=0x7F` map entirely) have **no
move-power record**. Those are the magic / elemental casts, whose damage is
caster-state-derived (the magic path — see [spell-table.md](spell-table.md) and
the per-spell-power thread in [open-rev-eng-threads.md](../reference/open-rev-eng-threads.md)),
consistent with this being the **physical/arts** power table, not the magic one.
Every enemy attack id is `≥0x25` — none land in the basic-attack band — and
95 of 186 monsters carry no magic attack at all (they fight with the basic
physical, the unmapped path).

The **one** mapped named record with no caster (move id `0x2C`, record idx 22) is
the **unused "Freeze Thunder" enemy spell** — a dummied-out attack
([TCRF](https://tcrf.net/Talk:Legend_of_Legaia): forcing it via GameShark
`30084845 002C` crashes with an "Opcode 14 UNK" / missing-asset error). Its
move-power record survived (power 37, `sfx 0x4A`) but its on-contact/launch
effect lists are empty and no production formation casts it — so it shows up as
the single mapped record the roster never uses.

### This table is special-attack-only — a party member's basic attacks / arts do *not* use it

The map covers exactly 44 special-attack ids (the internal tiers `0x04..=0x07` /
`0x12..=0x1F` and the named monster attacks `0x25..=0x74`). The **basic-attack and
Tactical-Art move-id bands `0x08..=0x11` and `0x16..=0x18` are entirely unmapped**
(`map[id] == 0`). Pinned from a live battle capture: a party member's queued
Tactical Art (Vahn's Somersault) carries move id `0x0F` in `actor[+0x1df]`, and a
basic enemy's attack (Gobu Gobu) carries `0x09` — both resolve to record 0 (no
power). Since `FUN_801dd0ac`'s damage is `roll(record[map[actor+0x1df]].power)`,
an unmapped id would roll against the zero-power record, i.e. deal nothing — so
neither a party member's art nor an enemy *basic* attack draws its damage from
this table.

Damage sources therefore split cleanly: **enemy special attacks** roll through
this move-power table (`FUN_801dd0ac`); **a party member's Tactical Art** takes its
power from the per-strike *art-record* power byte ([art-data.md](art-data.md));
enemy basic attacks use the generic physical path. The engine mirrors this — the
move-power table is wired for enemy specials only, and a character's art damage
uses the art power byte. (`move_power_map_is_special_attack_only` pins the
coverage on disc.)

## Record layout (26 bytes)

The record is consumed by three battle-action functions. `FUN_801dd0ac` /
`801f3990` (the damage kernels) read **only `+0x00`**. `FUN_801dea50` (action
setup) computes the record address once and stashes the pointer in the
per-battle context at `ctx+0x1014` (`overlay_battle_action_801dea50.txt:528`,
`sw v0,0x1014(a0)`). `FUN_801e09f8` (the per-frame action tick) dereferences
that held pointer ~25× and reads the residual fields off it — the byte offsets
it loads are exactly `+0x02,+0x06,+0x08,+0x09,+0x0a,+0x0b,+0x0d,+0x0e,+0x12,
+0x16`, and **never `+0x0c`**.

| Off | Type | Field | Meaning | Confidence |
|---|---|---|---|---|
| `+0x00` | `i16` | power | Damage roll modulus. The kernel uses it at full / half / quarter scale (`>>0`, `>>1`, `>>2`). | Confirmed |
| `+0x02` | `i16` | strike Y offset | Subtracted from the per-arm Y lane (`ctx + arm*8 + 0x1146`) when the move's hit point is seeded from the target's position. | Inferred (read confirmed) |
| `+0x04` | `u16` | move counter | The whole-move timing counter, seeded into `ctx+0x6c6` and decremented each frame. | Confirmed |
| `+0x06` | `u16` | phase duration | Per-arm phase duration written to `ctx + arm*2 + 0x6c6` at the strike / re-arm transitions (distinct from `+0x04`). | Inferred (read confirmed) |
| `+0x08` | `u8` | homing speed | Scales the per-frame XY step toward the target (`* DAT_1f800393 * 8`); `0x40 - speed` reseeds the approach counter. | Inferred (read confirmed) |
| `+0x09` | `u8` | effect-tracks-strike flag | When non-zero, the move's live XY is copied into the spawned effect actor each frame (the effect follows the strike). | Confirmed (read); semantic Inferred |
| `+0x0a` | `u8` | impact-effect selector | Enum (typically 1..5): stored at `actor+0x21f`, indexes the 5-entry packed-config table at `0x801f53d4` (`(value-1)*4`) into `actor+0x04`, and values 3/4/5 branch to extra status-proc rolls. `0` = none. The table holds packed `u32` config words (`0x3FF`-masked lanes), not pointers. | Confirmed (read); enum naming Inferred |
| `+0x0b` | `u8` | trail texture page | Trail / afterimage sprite-page id; the streak draw helper turns it into the GP0 texpage word `0x7700 + id` (`overlay_battle_action_801e1ab0.txt:250`). | Confirmed |
| `+0x0c` | `u8` | designer tag | A `'C'/'E'/'G'/0` annotation baked into the data on the internal-tier records (ids 1,2,3,9,12,15) only. **No runtime reader exists** in any battle-action function — unused at runtime. | Unknown (no reader) |
| `+0x0d` | `u8` | sound cue id | Handed to the UI/voice cue dispatcher `FUN_8004fcc8`. | Confirmed |
| `+0x0e` | `u8` | list mode | `0xFF` broadcasts the move's trail/effect to all four party arms (a sweeping / multi-target move); otherwise it is the head of a small effect-id list the setup loop spawns. | Confirmed (read); semantic Inferred |
| `+0x12` | `[u8;4]` | on-contact effects | Effect-id list dispatched on the hit branch (`0x00`/`0xFF`-terminated). | Confirmed |
| `+0x16` | `[u8;4]` | launch effects | Effect-id list dispatched at the initial-strike transition; same dispatch as `+0x12`. | Confirmed |

### Effect-id list semantics (`+0x12` / `+0x16`)

Both lists are up to 4 ids, walked until a terminator. `0x00` ends the list scan;
each remaining byte is dispatched per `FUN_801e09f8` (`overlay_battle_action_801e09f8.txt:1182..1225`
for `+0x16`, `:1285..1312` for `+0x12` — identical dispatch, the only difference
is *when* they fire):

| entry | meaning |
|---|---|
| `0x00` | terminator (ends the scan) |
| `0x01..=0x63` | spawn effect prototype `0x801f6324[id]` + play SFX `0x801f6418[id]` (when non-zero) |
| `0x64` (`100`) | fixed screen-flash effect (no table lookup) |
| `0x80`-bit set, `!= 0xFF` | route to `FUN_801dfdf0(id & 0x7F)` |
| `0xFF` (and unused `0x65..=0x7F`) | no effect, scan continues |

Both effect-id lists index the **same** two tables (the doc's earlier "+0x12 →
`0x801f6324` / +0x16 → `0x801f6418`" pairing was imprecise — each list uses
both). The tables live in the same PROT 0898 overlay after the power table:

- `0x801f6324` (file `0x27B0C`) — effect-**prototype pointer** table: a `u32`
  per id, an **overlay VA** pointing at a **variable-length move-VM scene-graph
  record** (e.g. ids `0x27`/`0x28` → `0x801F5BBC`/`0x801F5BDC`). It is passed as
  arg 3 to `FUN_80050ed4`, which forwards it to the shared spawn stager
  `FUN_80021B04` — the same record format and stager the player Seru-magic
  **summons** use (`legaia_asset::summon_overlay`, `SPAWN_HELPER`). The "~`0x20`-
  byte struct" reading was a coincidence (record `0x27` is 0x20 bytes; `0x28`
  begins where it ends — packed variable-length records, not a fixed stride). See
  Open for the decoded layout.
- `0x801f6418` (file `0x27C00`) — per-effect **SFX id** (`u8`, `0` = silent).

The prototype table is exactly `(0x6418 - 0x6324) / 4 = 61` entries; the same
61-entry index space bounds both (the runtime's `< 100` spawn guard is a loose
safety check). Parsed by `legaia_asset::move_power::EffectAuxTables`; the
per-entry dispatch is `EffectListEntry::classify`.

### `+0x00` at full / half / quarter

`FUN_801dd0ac` / `801f3990` read `+0x00` three ways for the same move: `>>0x10`
(full), `>>0x11` (half), `>>0x12` (quarter) after the `lhu << 0x10` sign-extend.
The roll the kernel performs is `rand % ((power >> 2) + 1)` at the quarter scale.

## Worked example (real disc bytes)

Record 3 (move id `0x06`, the third internal-tier attack), from PROT 0898:

```
power 1500  ctr 0  phase 480  homing 0x20  yoff 250  impact 1  trail 0
sfx 0x4d  list 0x00  tag C  contact[0x27,0x8e,0x8d]  launch[0x28,0x64,0x9d]
```

A homing physical strike: it approaches at speed `0x20`, runs its strike phase
for 480 frames, plays impact effect 1 + cue `0x4d`, spawns one effect list on
launch and a different one on contact, and carries the unused designer tag `C`.

## Effect-prototype records — the spawn path

A `0x01..=0x63` effect-list byte spawns the move-VM record `0x801f6324[id]`
points at. The dispatch (`FUN_801e09f8`) calls
`FUN_80050ed4(world_pos, src_pos, 0x801f6324[id], 0x1000)`; `FUN_80050ed4` is a
0x60-slot allocator that tail-calls the shared stager `FUN_80021B04` with the
args intact (the Ghidra C decomp drops them; the disassembly preserves
`a0..a3`). `FUN_80021B04` (`SPAWN_HELPER`):

- reads the record's `+0x00` `model_sel` (`lh`/`lhu` at `80021b2c`/`b30`); `< 0`
  / `0x4000` / `0x4001` are transform-node / render-mode sentinels, else the mesh
  is `DAT_8007C018[model_sel + gp[0x754]]` (decomp `210..216`; in battle the base
  `gp[0x754] = 3`, live-captured — see Open),
- allocates an actor (`jal 0x80020de0`), stores the record pointer as the actor's
  move-VM buffer base (`*(actor+0x48) = record`, `80021c80`), forces the move-VM
  PC to u16-index 2 (`*(actor+0x70) = 2`, `80021c78` → bytecode at `record+4`),
- and drives it through the move VM (`jal 0x80023070`, `80021dc0`).

So each `0x801f6324` record is **byte-identical to a summon part record**
(`+0x00 i16 model_sel`, `+0x02 u16 flags`, `+0x04` move-VM bytecode) and reuses
the same stager, move VM, and `DAT_8007C018` TMD-pool bridge — see
[`legaia_asset::summon_overlay`](../../crates/asset/src/summon_overlay.rs). The
`0x80`-bit list bytes route to the *separate* 2D-billboard path
(`FUN_801dfdf0(id & 0x7F)` → the `efect.dat` `EffectCatalog`), already ported as
`spawn_by_ui_id`.

## Open

The residual record fields are all decoded or accounted for. The `+0x0c` designer
tag has no runtime consumer (reported as Unknown rather than guessed).

The engine exposes the whole power record as a resolved `MoveFx` descriptor
(behavioural fields + the impact-config and effect-list cross-table joins,
including each spawn entry's `0x801f6324` prototype VA), and **renders the move-FX
scene-graph**: `World::spawn_move_fx(move_id, origin)` parses a move's
`0x01..=0x63` spawn-entry records with the summon-record reader, stages them as a
`SummonScene` with model base 3 (so `model_sel` resolves into the resident PROT
0871 effect-model library `global_tmd_pool[3..=32]`), and drives each part's
`+0x04` bytecode through the ported move VM (`World::tick_move_fx` /
`active_move_fx_part_draws`; `play-window` `H` debug-spawns it in battle). This
reuses the summon machinery wholesale, so it inherits the same faithful-tick /
interpreted-transform boundary (the exact per-part transform composition is the
shared open `FUN_801F811C`/PROT-0900 piece). The base 3 is the captured
`gp[0x754]`:

A spawn also surfaces the move's two presentation fields for the render / audio
layers: the **trail texpage** (`+0x0b` → `0x7700 + id`) on
`World::active_move_fx_trail_texpage()` (for the streak pass — the 2D afterimage
draw `FUN_801e1ab0` itself is not yet emitted), and the **sound cue** (`+0x0d`) as
a pending id `World::take_pending_move_fx_cue()` the host routes through the ported
`FUN_8004fcc8` dispatch decode (`legaia_engine_audio::classify_cue` →
`CueDispatch::Ring`/`Voice`; the SFX ring is `SfxScheduler`/`FUN_80035B50`, and the
SPU note-on stays with a battle SFX bank that is not yet wired).

**`gp[0x754] = 3` in battle (live-captured).** A PCSX-Redux exec-bp on
`FUN_80021B04` during a battle move-FX spawn (probe `autorun_summon_model_base`)
hit it once: `ra = 0x80050F08` (the `FUN_80050ed4` call), `a3 = 0x1000`, with the
prototype-table base `0x801F6324` and the effect-list id `0x22` live in registers
— the whole `FUN_801e09f8 → FUN_80050ed4 → FUN_80021B04` chain confirmed — and
`gp` (`0x8007B318`) `+0x754` (global `0x8007BA6C`) read **3**. So a battle move-FX
mesh is `DAT_8007C018[model_sel + 3]`, which lands `model_sel` exactly in the
inferred `DAT_8007C018[3..=32]` effect-model window. (Single capture, battle
move-FX context. The summon-part stage reads the *same* global but during its own
library load, so the per-summon base is a separate capture — re-point the probe at
a summon-part hit to pin it.)

The **summon** branch of `FUN_801dd0ac` (attacker slot `param_2 == 7`) does
*not* use this table — a summon's magnitude is derived from caster/summon battle
state (see [spell-table.md](spell-table.md) and the summon-render thread in
[open-rev-eng-threads.md](../reference/open-rev-eng-threads.md)).

## See also

The roll this table seeds is then scaled by the **element-affinity matrix**
(`FUN_801dd864`), a sibling static table in the same overlay (PROT 0898) under
the same link base — see
[battle-formulas.md § Element-affinity matrix](../subsystems/battle-formulas.md#element-affinity-matrix-fun_801dd864-0x801f53e8)
(parser `legaia_asset::element_affinity`).
