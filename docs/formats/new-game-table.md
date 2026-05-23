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

## Provenance + parser

The table base + stride are pinned by byte-search of `SCUS_942.54` for Vahn's
stat run (`18 00 10 00 0c 00 13 00 09 00` = ATK..INT) followed by the `"Vahn"`
name, with every field cross-validated against the live per-character record at
`0x80084708` lifted from an early `town01` save state. `legaia_asset::new_game::StartingParty`
parses the table from a `SCUS_942.54` image at runtime (PSX-EXE `t_addr` →
file-offset map, identical to the [item-name table](item-table.md) resolver), so
the engine can seed a faithful New Game from the user's own disc without
committing any Sony bytes. The disc-gated `new_game_real` test pins the four
rows against the real executable.

## See also

- [Per-character save record](save-record.md) - the live `0x414`-byte record this template seeds.
- [Item-name table](item-table.md) - the sibling static table using the same offset-map resolver.
- [`subsystems/boot.md`](../subsystems/boot.md) - the New Game boot chain into `town01`.
