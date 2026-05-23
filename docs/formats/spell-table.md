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
| `0x00..=0x24` | internal enemy-attack tiers; **empty inline name pointers** (see below) |
| `0x25..=0x7f` | **named monster attacks** (`Fire Breath` `0x25`, `Tail Fire` `0x27`, …) + capture-class spells (`'c'` at `+0`) |
| `0x80` | "Flip Frog" — boundary entry below the player block (`mp`/`anim` both 0), not part of the sequential set |
| `0x81..=0x8b` | **player Seru-magic** — 11 named summon spells, `anim` ids `0x25..=0x2f` |

The `0x00..=0x24` records carry MP / element / target but their `name_ptr` is an
empty string. These are **not** the ids a monster's archive spell entries store
(those local `+0x4C` entry ids in `0x0C..=0x1F` only gate the SP cost). The
named monster attacks live at **`0x25..`** in this same table, and an enemy is
named exactly like a party caster: the AI spell picker (`FUN_801E9FD4`,
`overlay_0898`) reads a **global** spell id from the monster record's
magic-attack array at [`+0x21..=+0x23`](../subsystems/battle.md) (values `> 1`
are live), writes it into the live actor at `+0x1DF`, and the battle-action SM
prints `&DAT_800754D0 + id*0xC` (`0x27` → `Tail Fire`). So the enemy spell name
*is* in this shared table after all - just keyed by the record's global id, not
the local entry id. Decoder: [`legaia_asset::spell_names`](../../crates/asset/README.md).

(The `0xC5` MES substitution table at `DAT_80075EC4`, once mistaken for a
spell-name source, is the [Tactical Arts name
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

### Per-spell damage power is not static data

There is **no per-spell magic-power / multiplier field anywhere in this table**,
and it isn't a separate static array either. Verified bytes + trace:

- Record bytes `+5..+8` are zero for every spell; `+0`/`+1` are the
  class/sub-index category selectors, not a power scalar. The whole player Seru
  block `0x81..=0x8b` shares `cat = 0x32`, `sub = 0` — so the SCUS table cannot
  even *distinguish* Gimard (weakest) from Nova; their damage must live
  elsewhere.
- State `0x28` of the action SM (`overlay_0898_801e295c.txt` case `0x28`) reads
  only `+3` (MP, deducted from actor `+0x150`), `+0` (`'c'` capture flag), and
  the name pointer. No power read.
- The per-summon effect/animation is dispatched by **`(spell_id - 0x81)`**:
  state `0x29` sets `_DAT_8007ba2c = (&PTR_s_re_check_801f6734)[spell_id - 0x81]`
  and calls `func_0x8003ec70(spell_id - 0x79, 0)`. Each summon's damage is
  produced inside that effect script (the `efect.dat` battle-effect path), not a
  scalar table.
- The static attack-vs-defense kernel `FUN_801ec3e4` (line 2582:
  `power = stat[+0x164] + (stat[+0x158]*4)/5 + buff`) is **melee/arts only** — it
  returns early unless the action-queue head is in `0xC..=0x1F` (line 2552,
  `0x13 < *param_2 - 0xc`), which magic (`ActionConstant::Magic = 0x02`) never
  enters.

So the per-spell power the engine wants requires decoding the per-summon effect
scripts at `PTR_801f6734[id - 0x81]`, an open thread (see
[`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md)). Until then
the `base_power` figures in `legaia_engine_core::retail_magic` are explicitly
MP-scaled placeholders.

The mirror lives at `legaia_engine_core::retail_magic` (`SERU_MAGIC` +
`retail_seru_magic_catalog`); the Seru that teach these ids are wired in
`legaia_engine_core::seru_learning::SeruRegistry::retail`.

## See also

- [Item-name table](item-table.md) - the sibling static `SCUS_942.54` name table.
- [`subsystems/battle-formulas.md`](../subsystems/battle-formulas.md) - the MP-cost and damage kernels that read these stats.
- [`reference/gamedata.md`](../reference/gamedata.md) - the curated ground-truth magic tables.
