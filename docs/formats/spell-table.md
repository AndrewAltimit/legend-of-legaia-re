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

Two independent bits over a side/scope pair:

- bit `0x02` = **ally side** (clear = enemy side; equivalently the low nibble
  is `_4` for enemy, `_6` for ally)
- bit `0x20` = **all** targets on that side (clear = single)

| Value | Shape |
|---|---|
| `0x44` | one enemy |
| `0x64` | all enemies |
| `0x06` | one ally |
| `0x26` | all allies |

(The earlier "bit `0x40` = enemies" reading is equivalent for these four
player-block values but misclassifies the internal enemy-attack tiers, whose
byte is `0x04`: that has `0x40` clear yet they are enemy-targeting monster
attacks. The `0x02` / low-nibble reading classifies `0x04` as enemy-side,
matching their role.)

Decoded by `legaia_asset::spell_names::SpellEntry::target_shape`
(`SpellTargetShape`). The engine sources the player Seru-magic catalog's MP +
target from the user's `SCUS_942.54` via
`legaia_engine_core::retail_magic::seru_magic_catalog_from_scus` (falling back
to the pinned `retail_seru_magic_catalog` on a disc-free build); the disc-gated
`spell_catalog_disc` test confirms the decode reproduces all 11 pinned targets
+ MP byte-for-byte.

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

### Per-spell damage power is not static data — it is caster-state-derived

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

**Resolved by static disassembly of the summon overlays (PROT 0900 + 0903..0915).**
The jump table `FUN_801f2d68` reads (`jr *(0x801F69D8 + state*4)`, `state < 7`)
resolve to PROT **0900** file offset 0 — the resident **render** overlay (pinned to
load at `0x801F69D8`). Those five entries are staggered entry points into one
per-frame routine that lerps move-VM anim banks (`FUN_8003ce9c`/`ce64`/`ceb8`) and
emits GPU display-list packets into scratchpad `0x1F800314`. It contains **no
`mult`/`div`, no `actor+0x14c` write, no power read** — so the long-standing "the
magnitude is in this jump table" hypothesis is **falsified**. The JT is animation /
rendering only.

The magnitude is applied by the paired **stager** overlay (PROT 0903..0915 — the file
holding the `jal FUN_80021B04` part-spawn calls), in the *same function* that spawns
the summon body parts. Each stager carries exactly one `actor+0x14c` (HP) writer, and
they split cleanly into damage vs. heal:

- **Damage summons** (PROT 0904 / 0912 / 0914, plus 0915's second arm) compute the
  amount with the shared battle kernel **`FUN_801dd0ac`** (`a0` = a per-summon
  move-type constant `0x10..0x12`, `a1 = 7`, `a2` = target slot), clamp it to the
  target's current HP, add it to the damage-popup accumulator at `actor+0x10`, then
  store `HP = curHP - amount` (`subu`). For the summon path (`param_2 == 7`) the
  attacker roll is
  `rand % (AGL@+0x168 + 1) + HP@+0x14c + DAT_801C9370[ctx+0x13]_AGL@+0x168 * 2`
  minus a defender-mitigation term (`FUN_801dd0ac` returns `roll - mitigation`) — i.e.
  **caster/summon battle-state-derived, not a static per-spell scalar.**
- **Heal summons** (PROT 0903 / 0905 / 0910 / 0911 / 0913, plus 0915's first arm)
  compute the amount inline as `(power_byte << 5) + 0xe0` (= `power*32 + 224`), clamp
  to `maxHP - curHP`, skip dead/flagged actors, then store `HP = curHP + amount`
  (`addu`). `power_byte` is fetched from a table based at `0x80084140` (the
  SC / character-record block) by a 32-slot search that matches the cast spell-id
  (`actor+0x1df`) against an id list at `+0x705`, reading the parallel power byte at
  `+0x729`.

`FUN_801dd0ac`'s **non-summon** branch (`param_2 != 7`, the arts / physical path) reads
a 26-byte-stride per-move power table at **`0x801F4F5C`** — that is where a genuine
per-move "power" scalar lives, but it feeds melee/arts, not summon magic. The kernel reads
the record's `+0` signed-16-bit field and uses `(i16)power >> 2` as the attacker-roll
modulus (`sll 0x10` then `sra 0x12`); `801f3990` also reads the same `+0` at full (`>> 0`)
and half (`>> 1`) scale.

`param_1` is **not** the raw move id — it is looked up through a 128-byte **id → index
map** at `0x801F4E63` (immediately before the table): the setup site passes
`param_1 = map[actor[+0x1df]]` (`overlay_battle_action_801e09f8`). The map covers move ids
`0x00..=0x7F` and resolves ids `0x04..=0x74` to power indices `0x01..=0x2b` (a `0x00` entry
= the unused record 0, `0xFF` = a no-record sentinel); the full resolution is
`power_table[map[move_id]]`.

Both the table and the map are **static overlay data, pinned on disc**: the
`0x801F4E63..0x801F69D8` window is byte-identical across two unrelated battle save states (a
full-party Gobu Gobu fight and the Tetsu-tutorial command menu), and the bytes live in
**PROT entry 0898** (the battle-action overlay, `overlay_0898`) — both the table window and
the `FUN_801dd0ac` code body map with one consistent base, so the table is at a fixed
raw-entry file offset (`0x26744`; the map at `0x2664B`). Parser `legaia_asset::move_power`
(`asset move-power <PROT-0898.BIN>` prints `idx → power / counter / sfx ← move id`); the
disc-gated `move_power_real` test pins the decoded powers + the id→index map. The clean
26-byte structure runs 44 indices (index 0 unused) before the region transitions to other
overlay data (a float/transform table, then the `data\battle\summon.DAT` / `readef.DAT`
filename strings). Decoded record fields, each code-traced to a battle-action reader: `+0x00`
`i16` power; `+0x04` `u16` an action-timing counter (seeded at `ctx+0x6c6`, decremented by
the SM); `+0x0d` `u8` a sound / voice cue id (handed to the cue dispatcher `FUN_8004fcc8`).
Still open: the `+0x02` u16, the `+0x08` flag halfword, the `+0x0a`/`+0x0b` field (the SM's
most-read), the `+0x0c` category byte (`C`/`E`/`G`/`0x00`), and the `+0x0e`/`+0x12`/`+0x16`
fields.

**What the records are.** Because the move id (`actor[+0x1df]`) is the *same id space* this
spell table is indexed by, joining the two labels every record: power records `0x10..=0x2b`
(move ids `0x25..=0x74`) are the **named monster special-attacks** — every one resolves to a
non-empty spell-table name (Fire Breath `0x25`, the enemy-Gimard *Tail Fire* `0x27`, … the
late-game attacks at `0x61..=0x74`); this is their physical/special-attack *power*, distinct
from the *name* this table carries. Power records `0x01..=0x0f` (move ids `0x04..=0x1f`, all
`< 0x24`) are the unnamed **internal enemy-attack tiers** (escalating-power triplets — the
ids this table leaves nameless). The disc-gated `move_power_real` test pins that named /
unnamed boundary against the live spell table (no Sony name strings embedded).

#### The full damage-roll chain (three stages)

The damage a summon deals is the `attacker_roll - defender_roll` margin after a
three-stage pipeline, all of it now byte-traced:

1. **Roll** — `FUN_801dd0ac` builds an attacker roll
   (`rand % (summon_AGL + 1) + summon_HP + caster_AGL*2`) and a defender roll
   (`rand % ((target_AGL >> 1) + 1) + (target_HP >> 8) + (target_DEFa >> 4) +
   (target_DEFb >> 4) + target_AGL*2`).
2. **Scale** — `FUN_801dd864` scales the attacker roll by the element-affinity
   percent from the 8×8 byte matrix at `0x801F53E8`, then the attacker/defender
   status-weaken bits (`+0x16e & 1` → 9/10, `& 2` → 7/10; defender guard
   `+0x1de == 4` doubles its roll first), and — summon only — the caster's
   magic-power byte (`SC + 0x729`, matched on the spell-id at `+0x705`):
   `roll += roll*(power_byte − 1) >> 3`. `FUN_801dd0ac` then re-rolls the
   attacker as `defender_roll + rand % ((summon_AGL >> 1) + 1) + summon_HP`
   whenever the scaled attacker has not already overwhelmed the defender.
3. **Finish** — `FUN_801ddb30` applies the per-element resistance bits (from the
   defender's SC ability words `+0x6bc`/`+0x6c0`), a `rand % 9 + 8` floor, the
   9999 cap, the spirit-gauge fill, the damage-popup accumulator, MP drain, and
   the per-element stat-debuff for the active field type
   (`*(DAT_801c9358 + 0x1d)`).

The bounded, state-free arithmetic of stages 1 + 2 is ported as pure kernels in
[`legaia_engine_vm::battle_formulas`](../subsystems/battle-formulas.md)
(`summon_attacker_roll` / `summon_defender_roll` / `summon_predamage` /
`apply_element_affinity` / `apply_status_weaken` / `apply_magic_power` /
`heal_summon_amount`). Stage 3 reads ~20 battle globals and mutates live battle
state, so it stays the coupled tail of the live battle context rather than a
pure kernel. See the `FUN_801dd0ac` / `FUN_801dd864` / `FUN_801ddb30` rows in
[`reference/functions.md`](../reference/functions.md).

So the "missing per-spell power scalar" the engine wanted largely **does not exist for
summons**: the game derives summon magnitude from caster/summon battle stats
(`FUN_801dd0ac`) or, for recovery summons, from a per-character magic-power byte. The
genuine per-move power scalar that *does* exist — the `0x801F4F5C` table — feeds the
arts/physical branch and is now located + parsed off the disc (`legaia_asset::move_power`,
above). The `base_power` figures in `legaia_engine_core::retail_magic` stay MP-scaled
placeholders until the `FUN_801dd0ac` summon roll is wired into a live battle context.
(Method: capstone disassembly of the extracted PROT 0900 / 0903..0915 overlays +
byte-matching the resident table against the in-RAM battle save states; `FUN_801dd0ac`
itself is dumped at `ghidra/scripts/funcs/overlay_battle_action_801dd0ac.txt`.)

The mirror lives at `legaia_engine_core::retail_magic` (`SERU_MAGIC` +
`retail_seru_magic_catalog`); the Seru that teach these ids are wired in
`legaia_engine_core::seru_learning::SeruRegistry::retail`.

## See also

- [Item-name table](item-table.md) - the sibling static `SCUS_942.54` name table.
- [`subsystems/battle-formulas.md`](../subsystems/battle-formulas.md) - the MP-cost and damage kernels that read these stats.
- [`reference/gamedata.md`](../reference/gamedata.md) - the curated ground-truth magic tables.
