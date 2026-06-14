# New-game starting-party table

When the title screen's NEW GAME row is confirmed, the boot chain
(`FUN_80025B64` → `FUN_801D6704`, see [`boot.md`](../subsystems/boot.md)) launches
the field/town overlay with a fresh game state. Part of that fresh state is the
**starting party**: a small static table in `SCUS_942.54` holds each roster
member's opening stats and display name, which the seed routine expands into the
live per-character records at `0x80084708 + n*0x414`.

The interactive scene a New Game enters is `town01` (Rim Elm) — the executable's
default map-name buffer at `0x8007050C` holds the literal `"town01"`, and the
global reset/init `FUN_8001D424` reads an optional dev `initmap.txt` override
when the debug flag `_DAT_8007B8C2` is clear.

## Table base + record layout

| | |
|---|---|
| Base address | `0x80078C4C` (Vahn's record) |
| Stride | `26` bytes (`8×u16` stats + 10-byte name) |
| Records | `4` — `Vahn`, `Noa`, `Gala`, `Terra`, in roster order |

| Offset | Type | Field |
|---|---|---|
| `+0`  | u16 | `hp_max` (also the starting HP) |
| `+2`  | u16 | `mp_max` (also the starting MP) |
| `+4`  | u16 | `agl` — also seeds the spirit-gauge value + stat cap |
| `+6`  | u16 | `atk` |
| `+8`  | u16 | `udf` (upper / physical defence) |
| `+10` | u16 | `ldf` (lower / magical defence) |
| `+12` | u16 | `spd` (turn-order initiative seed) |
| `+14` | u16 | `intel` |
| `+16` | u8[10] | display name, NUL-padded |

At a true New Game only Vahn has joined; the other rows are the templates the
game uses as each character is introduced. Tetsu's tutorial-fight dialogue
string (`"I will show you how to fight…"`) sits immediately after the table.

### Decoded values

| Slot | Name | HP | MP | AGL | ATK | uDEF | lDEF | SPD | INT |
|---|---|---|---|---|---|---|---|---|---|
| 0 | Vahn  | 180 | 20  | 100 | 24 | 16 | 12 | 19 | 9 |
| 1 | Noa   | 150 | 10  | 120 | 21 | 13 | 11 | 30 | 3 |
| 2 | Gala  | 210 | 40  | 80  | 30 | 43 | 30 | 15 | 20 |
| 3 | Terra | 400 | 200 | 200 | 45 | 20 | 17 | 45 | 25 |

The `+4` stat is one value the seed fans out to several live fields. Cross-validated
against an early `town01` save state, Vahn's `+4` (`100`) lands in the live record
as `agl`, `cap_constant`, and the initial spirit-gauge value all at once; the
per-character archetypes (`Noa = 120`, `Gala = 80`) read as agility.

## Starting inventory (code-built, not a table)

The opening inventory is **not** a static table like the party stats — the
new-game data-init `FUN_80034A6C` (`docs/subsystems/boot.md`) builds it in code,
writing a single slot into the live owned-item array at `0x80085958`
(= `SC` base `0x80084140` + `0x1818`; 2 bytes/slot `[id: u8][count: u8]`,
id-`0` terminator):

```
0x80034b0c  addiu $v0, $zero, 0x77    ; item id  = Healing Leaf
0x80034b10  sb    $v0, 0x1818($s0)    ; inventory[0].id   ($s0 = SC base)
0x80034b14  addiu $v0, $zero, 5       ; count    = 5
0x80034b18  sb    $v0, 0x1819($s0)    ; inventory[0].count
```

So a vanilla New Game starts with **Healing Leaf ×5** and nothing else. A
6-instruction loop right after (`0x80034b04..`, `0x80034b1c..0x80034b2b`) zeroes
the 512 bytes *below* the inventory — but **both** callers of `FUN_80034A6C`
(`FUN_8001DCF8`'s new-game branch, guarded by the new-game flag at
`0x8007b7ac`, and `FUN_8001FFA4`) `memset` the whole `SC[0..0x1a18)` region —
which contains the entire 72-slot inventory — immediately before the call, so
that inline zero-loop is redundant. The reclaimable seed region is therefore
the **10 instructions at [`STARTING_INV_SEED_VA`] (`0x80034b04`, 40 bytes)**.

The array at `0x1818` is a **single ordered `(id, count)` owned-item list shared
by every item category** — the inventory menu only *filters* it into Items /
Goods / Key tabs by item id. (Verified against a real end-game save: consumables,
equipment, and accessories all sit in this one list as plain `(id, count)` pairs,
running past the first 72 slots to a `(0, 0)` terminator.) So a seeded slot can
hold any item id regardless of category — the randomizer's starting-bag toggles
seed the Door of Wind / Incense consumables and the Speed Chain / Chicken Heart /
Good Luck Bell accessories through this same array (see
[`randomizer.md`](../tooling/randomizer.md#starting-bag-convenience-toggles)).

`legaia_asset::new_game::StartingInventory` decodes this region by replaying its
`$v0`-source byte/halfword stores into an `SC`-offset → byte map and reading
`(id, count)` slots from `0x1818`, so it reads back either the vanilla `sb` pair
or the randomizer's packed `sh` halfword store (slot = `id | (count << 8)` in one
instruction). The relevant LE-instruction-word top-16 signatures are:

| Op | Top 16 bits | Meaning |
|---|---|---|
| `addiu $v0, $zero, imm` | `0x2402` | load constant into the source reg |
| `sb $v0, off($s0)` | `0xA202` | byte store at `SC + off` |
| `sh $v0, off($s0)` | `0xA602` | halfword store at `SC + off` |

## Starting level / XP seed

The same routine (`FUN_800560B4`) also seeds each live record's progression
fields. The displayed combat **level is derived from cumulative experience**, not
stored as a level byte, so the relevant live-record cells are:

| Record offset | Field | Vanilla seed |
|---|---|---|
| `+0x0` (u32) | cumulative experience (the "Max Exp" cheat target); the displayed level derives from this | `0` |
| `+0x4` (u32) | next-level XP threshold (the status screen's "next" readout; the level-up applier compares experience against it) | `reach(L2)`: Vahn/Terra `121`, Noa `102`, Gala `140` |
| `+0x130` (u8) | magic-rank counter — `+1` per level-up *event*, **not** the character level (a captured 4-level jump raised it by only `+1`) | `1` |

`+0x100` stays zero and is unrelated to the level. The seed writes `+0x4` per
character (`0x800560F0` Vahn+Terra / `0x80056100` Noa / `0x80056108` Gala) and
leaves `+0x0 = 0`, so a vanilla New Game derives level 1.

The starting-level randomizer seeds the lead character at level `N` by writing an
in-band experience value (the midpoint of `reach(N)..reach(N+1)`) into `+0x0` and
`reach(N+1)` into `+0x4`, recomputing the template stats to the level, and leaving
the magic-rank byte alone. It does this with same-size in-place edits to three
seed instructions: the Vahn `+0x4` literal at `0x800560F0`, and the slot-3 (Terra)
+ slot-1 (Noa) threshold seeds at `0x800560FC` / `0x80056100`, repurposed to an
`addiu $t0` preload + `sw $t0, 0x5c8($s0)` store of the experience value into slot
0's `+0x0`. Noa/Terra re-scale when they join, so their dropped seeds are never
observed. See [`subsystems/level-up.md`](../subsystems/level-up.md) for the XP
thresholds + growth curves and [`tooling/randomizer.md`](../tooling/randomizer.md)
for the feature.

## Provenance + parser

The table base + stride are pinned by byte-search of `SCUS_942.54` for Vahn's
stat run (`18 00 10 00 0c 00 13 00 09 00` = ATK..INT) followed by the `"Vahn"`
name, with every field cross-validated against the live per-character record at
`0x80084708` lifted from an early `town01` save state. `legaia_asset::new_game::StartingParty`
parses the table from a `SCUS_942.54` image at runtime (PSX-EXE `t_addr` →
file-offset map, identical to the [item-name table](item-table.md) resolver), so
the engine can seed a faithful New Game from the user's own disc without
committing any Sony bytes. The disc-gated `new_game_real` test pins the four
rows against the real executable. CLI: `asset new-game <SCUS> [--json]` (dumps
the party template + the code-built starting inventory).

## See also

- [Per-character save record](save-record.md) - the live `0x414`-byte record this template seeds.
- [Item-name table](item-table.md) - the sibling static table using the same offset-map resolver.
- [`subsystems/boot.md`](../subsystems/boot.md) - the New Game boot chain into `town01`.
