# Steal-item table

What the player steals from an enemy with the **Evil God Icon** equipped - once
per battle, from the first monster a party member fells, see
[the steal attack](#the-steal-attack) - is
looked up in a static per-monster table inside `SCUS_942.54` - **not** in the
PROT 867 `battle_data` monster record. An exhaustive offset scan of the decoded
record (correlated against ground-truth steal data) finds no steal field there:
the reward block at `+0x44..+0x49` holds only gold / exp / drop. The steal item
lives in this separate executable table, which is exactly why every record-only
search came up empty (the long-open thread in
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md)).

## Table base + record layout

| | |
|---|---|
| Base address | `DAT_80077828` (file offset `0x68028` in `SCUS_942.54`) |
| Index form | `DAT_80077828 + monster_id*2` (1-based monster id) |
| Stride | `0x2` bytes |
| Id range | monster ids `1..` (entry `0` is a reserved sentinel) |

| Offset | Type | Field |
|---|---|---|
| `+0` | u8 | `steal_chance_pct` - steal success chance, percent |
| `+1` | u8 | `steal_item_id` - stolen item id (the [item-table](item-table.md) id space; `0` = none) |

The id space is the same one [`item-table.md`](item-table.md) names and a
monster record's `drop_item` indexes, so a raw steal id becomes a readable name
the same way (`0x8a` → `Incense`). A `steal_chance_pct` of `0` (or a `0` item)
means the enemy can't be stolen from.

**Field order is chance-then-item** - the reverse of the drop fields in the
monster record (`+0x48 item / +0x49 chance`). Reading the table with the
record's item-first order shifts every chance by one monster, so the order
matters.

## Provenance

Pinned from a live player-steal RAM capture (Evil God Icon equipped; the
`player_steal_skeleton_pre` / `_banner` save pair): the Skeleton (monster id
`13`) entry at `0x80077842` is `1e 8a` = **30% Incense**, exactly the steal the
banner shows. The base, stride, and `[chance, item]` order are confirmed
byte-exact against the complete published steal table (item **and** chance
columns) across every resolvable monster id - zero mismatches. The table is
static rodata in the executable's data segment, resident in RAM and identical to
the file bytes, so it resolves the same way as the
[item](item-table.md)/[spell](spell-table.md) name tables.

## Who reads it

Two routines on the disc read the table, and nothing else does: the
`lui`/`addiu` pair forming `0x80077828` occurs exactly twice across
`SCUS_942.54` and every PROT entry (`find-gp-relative-refs.py --va 0x80077828
--prot`), at `0x8004B53C` in `SCUS_942.54` and at `0x801F79F0` in PROT 0941.

### The steal attack

The player's steal is **not** a command and does not happen on the hit. It
lives in one arm of the battle anim commit `FUN_8004AD80`
(`0x8004B29C..0x8004B65C`, see `ghidra/scripts/funcs/8004ad80.txt`; the
routine's whole row is in [`functions/battle.md`](../reference/functions/battle.md)), which
runs when a monster seat (`>= 3`) commits the end of its **knockdown** clip -
the installed entry's tag byte is `4` (`0x8004B0A4`) - with live HP
`+0x14C == 0`. So the steal lands as the slain monster finishes falling, and
the table is read only for a monster that died.

The arm first checks the monster's cell of the stolen band `0x801C8FE0`
(below). With the cell empty it runs the steal attack:

| Order | Test | Site |
|---|---|---|
| 1 | acting seat `ctx[+0x13] < 3` (a party member holds the action) | `0x8004B3C8` |
| 2 | latch `ctx[+0x27] == 0`, then **set it to 1** | `0x8004B3D4..0x8004B3E4` |
| 3 | `mult = 2` if any member whose `+0x14C != 0` carries record `+0xF8` bit `0x20000` (passive `0x31`, Items Up), else `1` | `0x8004B3E8..0x8004B484` |
| 4 | special-battle word `_DAT_8007BAC0 == 0` | `0x8004B488` |
| 5 | acting seat's action category `+0x1DE == 3` (Attack) | `0x8004B4BC` |
| 6 | acting member's record `+0xF4` bit `0x10000` (passive `0x10`, Steal Attack - the Evil God Icon) | `0x8004B500` |
| 7 | `rand() % 100 < chance * mult` (unsigned `sltu`) | `0x8004B514..0x8004B584` |
| 8 | the bag holds fewer than `99` of the item (`FUN_80042F4C`) | `0x8004B5BC..0x8004B5D0` |

Three consequences follow from the order. The latch is set at step 2,
**before** the killer is checked at all, and its only writer on the disc is
that one `sb` (a byte scan of `SCUS_942.54`, the battle overlay and every
cast / summon module finds no other store to `+0x27`, zero or otherwise) - so
a battle gets **one** steal attempt, and the first monster any party member
fells spends it whether or not the killer could steal. Items Up doubles the
chance for the whole party, not only for its wearer. And a stack already at
`99` drops the steal silently: no caption, no item.

On success the caption is composed into `0x80077A08`: template
`0x80077A64` copied by `FUN_8003CA78`, the item-name token `{0xC2, id}`
appended by `FUN_8003CB54`, the two-byte tail `0x8007B698` appended by
`FUN_8003CAC4`. The arm stores the buffer into `0x800774AC` - the `+0x14`
payload pointer of screen-element record 91 (`0x80077498`) - raises HUD
element `0x5B` (`FUN_801D8DE8(0x5B, 0)`), writes `ctx[+0x18] = 0x5B`, and adds
the item with `FUN_800421D4(id, 1)`. Record 91 is the full-width bar that
slides from `(16, 236)` up to the active-actor bar's seat `(16, 194)`, width
288 - the bottom bar the `player_steal_skeleton_banner` capture shows the line
in.

### The stolen band - PROT 0941 and the thief's return

PROT 0941's enemy Steal (`0x51`, `FUN_801F730C`) is the band's writer: the
item a thief took lands at `0x801C8FE0 + (seat - 3) * 4` (`0x801F7960` for a
bag steal, `0x801F7A84` for a steal off a monster seat, which also bumps
`0x801C8FE0 + (seat + 5) * 4` at `0x801F7AF0`). When that thief dies, the same
`FUN_8004AD80` arm hands the item over instead of rolling a steal attack:
`FUN_800421D4(item, 1)`, the cell cleared, and a caption built the same way
from `0x80077A4C` + tail `0x8007B694` (loot taken from the party) or
`0x80077A38` + tail `0x8007B690` (loot taken from a monster). When
`ctx[+0x18]` already reads `0x5B`, the pointer is swapped for the whole-line
`0x80077A70` caption instead (`0x8004B35C..0x8004B378`).

The port runs both arms in `legaia_engine_core::battle_steal`, reached from
the knockdown-end commit both hosts tick; the templates are read off the
user's executable through `legaia_asset::battle_ui_strings`. The disc-gated
`steal_attack_round_disc` test drives a real Skeleton fight through the live
round and asserts the item lands and the caption is raised, with a contrast
pass that spends the attempt without the passive and grants nothing.

## Parser

`legaia_asset::steal_table::StealTable::from_scus` resolves the table from a
`SCUS_942.54` image (PSX-EXE `t_addr` → file-offset map, identical to the
[item-name table](item-table.md) resolver). `entry(monster_id)` returns the
`[chance, item]` pair; `steal_item(monster_id)` returns the item only when the
entry is stealable. The disc-gated `steal_table_real` test pins the
Skeleton→Incense anchor plus a span of ids against the real executable.
CLI: `asset steal-table <SCUS> [--all] [--json]` (the stolen item id is joined
to its name).

The randomizer (`legaia_patcher::steal`) edits this table on a user-supplied disc
to reassign steal items; see [`randomizer.md`](../tooling/randomizer.md).

## Retail steal capture

The player's steal is not a battle command and does not happen on the hit.
It is a side branch of the SCUS anim commit `FUN_8004AD80`
(`0x8004B29C..0x8004B660`, see `ghidra/scripts/funcs/8004ad80.txt`), taken
when an enemy seat's death clip commits with live HP `+0x14C == 0`. A
PCSX-Redux probe
([`autorun_w7c_steal_oracle.lua`](../../scripts/pcsx-redux/autorun_w7c_steal_oracle.lua))
exec-breaks every step of that branch in the retail
`party_basic_attack_vs_gobu_gobu` battle (Vahn alone against one Gobu Gobu,
monster id `4`, steal row `[30, 0x78]`), with read-watches on the table and
write-watches on the bag and on the latch.

Every run below writes two synthetic gates, and every result rests on them:
the enemy's HP is set to `1` so Vahn's queued Attack kills it, and Vahn's
record word `+0xF4` gets bit `0x10000` (passive `0x10`, Steal Attack - the
bit an equipped Evil God Icon provides) because the state's party carries no
steal accessory. Where a row says *forced*, the probe overwrites the
`rand() % 100` value in `v0` at the compare (`0x8004B580`) to probe the
boundary; *natural* rows leave the BIOS `rand()` alone and vary only the
vsync the Begin press lands on.

| Capture (`captures/w7c-0921/`) | Gates | `rand()` | `% 100` | threshold | Outcome |
|---|---|---|---|---|---|
| `steal_a` | steal bit | 7694 | 94 | 30 | no steal, no caption, bag untouched |
| `steal_nat25` | steal bit, Begin 15 vsyncs later | 22937 | 37 | 30 | no steal |
| `steal_nat40` | steal bit, Begin 30 vsyncs later | 23717 | 17 | 30 | **stole `0x78`**: caption, bag slot 1 := `[0x78, 1]` |
| `steal_b` | steal bit, forced | 7694 | 29 | 30 | stole (caption + grant) |
| `steal_c` | steal bit, forced | 7694 | 30 | 30 | no steal |
| `steal_d` | steal + Items Up bits, forced | 7694 | 59 | 60 | stole |
| `steal_e` | steal + Items Up bits, forced | 7694 | 60 | 60 | no steal |
| `steal_nobit` | none | - | - | - | branch exits at the `+0xF4` test; `rand()` is never called |

What the rows pin, field by field:

- **Where.** The branch is entered at `0x8004B2AC` on the vsync the enemy's
  HP reads `0`, first from `ra 0x8004AE70` and then from `ra 0x80047B5C`
  every other vsync for the rest of the death fall (33 entries in `steal_a`).
  Only the first entry rolls; the others stop at the latch.
- **Who.** The acting seat `ctx[+0x13]` is `0` (Vahn), `ctx` being
  `*(gp + 0xA0C)` = `*(0x8007BD24)`; the killer's `+0x1DE` reads `3`.
- **The roll.** One BIOS `rand()` (`A0:2F`), reduced `% 100` through the
  `0x51EB851F` reciprocal, compared **strictly** (`sltu`) against
  `chance * mult`: `29 < 30` steals, `30 < 30` does not. `mult` is `2` with
  record `+0xF8` bit `0x20000` (passive `0x31`, Items Up) on a living member,
  and the threshold doubles to `60` exactly (`steal_d` / `steal_e`). The
  table is read twice per roll - chance at `0x8004B54C`, item at
  `0x8004B5B8` - and a third time at `0x8004B608` for the caption's item
  token; nothing else reads it during the battle.
- **The bag.** `FUN_80042F4C` receives the item id and answers `0` for an
  item the bag does not hold (not `99`, so the steal proceeds).
  `FUN_800421D4(0x78, 1)` then writes the first empty slot's id byte and
  count byte (`0x800422BC`, `0x80042300`) - slot 1 here, since slot 0 was the
  only occupied one. The battle's ordinary drop lands in the next slot
  later, from `ra 0x8004F610`.
- **The caption.** HUD element `0x5B` is raised (`FUN_801D8DE8(0x5B, 0)`)
  with its payload pointer `0x800774AC` aimed at the composed buffer
  `0x80077A08`.
- **The latch re-arms per strike chain, not per battle.** `ctx[+0x27]` is
  set to `1` at `0x8004B3E4` before any other gate - so a kill by a killer
  without the passive also spends it (`steal_nobit`) - but it is also
  **cleared**: a write-watch on the byte catches `sb zero,0x16(s5)` at
  `0x801E3A84` in the battle-action state machine `FUN_801E295C` (PROT 0898,
  `s5 = ctx + 0x11` from `0x801E2994`), the delay slot of the arm at
  `0x801E3A70` that moves an actor's strike loop (`0x1E`) on to its recovery
  wait (`0x1F`). It fires at the end of **every** strike chain, the
  monster's included (twice in each run above before Vahn's kill). In a
  second retail state (`arts_input_start_gala_nail`, two Gobu Gobus at HP
  `1`, capture `steal_multi`) Gala's kill sets the latch at vsync 1037 and
  the same store clears it - old value `1`, new `0` - at vsync 2363, when
  the next strike chain ends. So each
  attacking action gets one roll - on the first monster it fells - not each
  battle. A byte scan for a store to `+0x27` cannot see this writer: the
  displacement in the instruction is `0x16`, off a base register that
  already carries `+0x11`.

## See also

- [Item-name table](item-table.md) - the id space this table's `steal_item_id` indexes.
- [Spell table](spell-table.md) - the sibling static `SCUS_942.54` table.
- [`reference/gamedata.md`](../reference/gamedata.md) - the curated ground-truth enemy drop/steal tables.
