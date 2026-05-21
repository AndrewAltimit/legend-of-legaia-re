# Spell table

The battle-action state machine resolves a cast's MP cost, name, and target
shape from a static table inside `SCUS_942.54`. The table is referenced
through two interleaved base pointers that view the same 12-byte records:

| Base | Doc | Field read |
|---|---|---|
| `DAT_800754C8` | [battle-action states `0x28` / `0x3C`](../subsystems/battle-action.md) | stats (`+3` = MP cost) |
| `DAT_800754D0` | same | `name_ptr` (the stats base shifted `+8`) |

Both step by `0xC` per spell id, so record `id` lives at `DAT_800754C8 +
id*0xC`.

## Record layout (12 bytes, stride `0xC`)

| Offset | Type | Field |
|---|---|---|
| `+0` | u8 | class byte — `'c'` (`0x63`) marks a capture-class spell |
| `+1` | u8 | sub-index within the class |
| `+2` | u8 | target shape (see below) |
| `+3` | u8 | **MP cost** |
| `+4` | u8 | animation id |
| `+5..+8` | — | padding (zero) |
| `+8` | u32 | `name_ptr` — pointer to the display-name C-string |

The display name string carries a leading MES colour-control prefix (`0xCE`,
an element-colour byte, a space) before the ASCII name.

### Target shape (`+2`)

A 2-bit shape: bit `0x40` targets enemies (else allies), bit `0x20` is "all"
(else single).

| Value | Shape |
|---|---|
| `0x44` | one enemy |
| `0x64` | all enemies |
| `0x06` | one ally |
| `0x26` | all allies |

## Id ranges

| Ids | Contents |
|---|---|
| `0x00..=0x24` | elemental enemy-attack tiers; **empty inline name pointers** (see below) |
| `0x25..=0x7f` | monster / capture-class spells (`'c'` at `+0`) |
| `0x80` | "Flip Frog" — boundary entry below the player block (`mp`/`anim` both 0), not part of the sequential set |
| `0x81..=0x8b` | **player Seru-magic** — 11 named summon spells, `anim` ids `0x25..=0x2f` |

There is **no global elemental-spell name table**. The `0x00..=0x24` records
carry MP / element / target but their `name_ptr` is an empty string - these are
internal enemy-attack ids, named per-monster in the [monster archive's spell
records](../subsystems/battle-formulas.md) (name at the spell entry `+0`), not
from a shared table. The only spells with inline names in this table are the
player Seru-magic block. (The `0xC5` MES substitution table at `DAT_80075EC4`,
once mistaken for a spell-name source, is the [Tactical Arts name
table](art-data.md#arts-name-table-dat_80075ec4) - per-character art names, no
spells.)

## Player Seru-magic block (`0x81..=0x8b`)

MP cost + target shape are byte-exact from `SCUS_942.54`; the element column
is the cross-reference against the curated [`gamedata`](../reference/gamedata.md)
magic table (every MP value matches). Spell id `0x81` = Gimard also matches
the save-state pin recorded in `legaia_engine_core::capture_observations::seru_capture`.

| Id | Name | Element | MP | Target |
|---|---|---|---|---|
| `0x81` | Gimard | fire | 10 | one enemy |
| `0x82` | Theeder | thunder | 24 | one enemy |
| `0x83` | Vera | light | 6 | one ally |
| `0x84` | Gizam | water | 28 | all enemies |
| `0x85` | Nighto | dark | 13 | one enemy |
| `0x86` | Zenoir | fire | 36 | one enemy |
| `0x87` | Viguro | thunder | 64 | all enemies |
| `0x88` | Swordie | wind | 32 | one enemy |
| `0x89` | Orb | light | 18 | all allies |
| `0x8a` | Freed | water | 40 | all enemies |
| `0x8b` | Nova | wind | 48 | one enemy |

Per-spell **damage power** is not in this table — retail derives it from the
caster's magic stat × a separate per-spell multiplier (not yet located). The
`base_power` figures in `legaia_engine_core::retail_magic` are MP-scaled
placeholders.

The mirror lives at `legaia_engine_core::retail_magic` (`SERU_MAGIC` +
`retail_seru_magic_catalog`); the Seru that teach these ids are wired in
`legaia_engine_core::seru_learning::SeruRegistry::retail`.
