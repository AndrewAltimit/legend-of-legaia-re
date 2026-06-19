# Save record (per-character runtime layout)

The runtime per-character record is `0x414` bytes. Four records sit
contiguously in main RAM at `0x80084708 + slot * 0x414`:

| Slot | Base | Owner |
|---:|---|---|
| 0 | `0x80084708` | Vahn |
| 1 | `0x80084B1C` | Noa |
| 2 | `0x80084F30` | Gala |
| 3 | `0x80085344` | Terra (New Game template's 4th roster entry; never a savable battle-party member) |

The display name sits at record offset `+0x2A7` (9 bytes, NUL-padded),
bounded by the active-spell table at `+0x2B0`.

Slot 3's `0x414` footprint runs into the global story-flag bitmap
(RAM `0x80085600`, record offset `+0x2BC`) and inventory, so only its
leading fields (name, live stats `+0x104`, RecordStats `+0x11C`) are
exclusive; the tail aliases the globals. This is benign - Terra is never
saved as an active member.

The on-disc save block (PSX memory-card record) is a verbatim dump of
this resident region: record `n`'s base is `block + 0x5C8 + n*0x414`
(`game_data + 0x3C8`), and the `+0x2A7` display name therefore surfaces
at `block + 0x86F + n*0x414`. See
[`docs/subsystems/save-screen.md`](../subsystems/save-screen.md)
for the wrapper around it.

Field offsets are pinned by a fusion of three sources:

- decompiled function dumps under `ghidra/scripts/funcs/` (notably
  `FUN_80042558` per-frame stat aggregator, `FUN_800431D0`
  ability-bitmap reader, `FUN_80042DBC` spell-list pop, and the
  level-up overlay applier);
- captured save-state observations in
  `engine_core::capture_observations::char_level_up` and the
  `levelup::observations::{noa,gala}_4_level_jump` triplets;
- the GameShark cheat database under [`data/cheats/`](../../data/cheats/)
  whose every effect is a labelled `(addr, value)` write.

## Top-level layout

```text
+0x000  u32 LE   xp_cumulative            ; cumulative EXPERIENCE ("Max Exp" cheat
                                           ; target). The displayed combat level
                                           ; derives from this (vanilla New Game
                                           ; leaves it 0 -> level 1). See note.
+0x004  u32 LE   xp_next_threshold        ; next-level XP threshold - the status
                                           ; screen's "next" readout; the level-up
                                           ; applier compares +0x0 against it. NOT
                                           ; cumulative XP (a live randomized ROM
                                           ; writing here showed it as "next level"
                                           ; while experience stayed 0). Seeded per
                                           ; char to reach(L2): Vahn 121, Noa 102,
                                           ; Gala 140.
+0x006  u8[10]   header_tail              ; (function unmapped)
+0x010  u8[228]  stat_block_unmapped      ; (function partially mapped via
                                           ; FUN_80042558; not all bytes named)
+0x0F4  u8[16]   ability_bits             ; OR'd into 0x80074358 by the
                                           ; per-frame aggregator
+0x104  u16 LE   hp_curr_live             ; "Infinite HP" / "Max HP" cheat target
+0x106  u16 LE   hp_max_live              ;
+0x108  u16 LE   mp_curr_live             ;
+0x10A  u16 LE   mp_max_live              ;
+0x10C  u16 LE   sp_curr_live             ;
+0x10E  u16 LE   sp_max_live              ; "100 AP" cheat target
+0x110  u16 LE   agl_live                 ; "Max AGL" target (live copy)
+0x112  u16 LE   atk_live                 ; "Max ATK"
+0x114  u16 LE   udf_live                 ; "Max UDF"
+0x116  u16 LE   ldf_live                 ; "Max LDF"
+0x118  u16 LE   spd_live                 ; "Max SPD"
+0x11A  u16 LE   int_live                 ; "Max INT" (also legacy
                                           ; `stat_cap()` accessor target)
+0x11C  u16 LE   hp_max_record            ; "Max HP" target (record copy)
+0x11E  u16 LE   mp_max_record            ; "Max MP" target (record copy)
+0x120  u16 LE   stat_cap_constant        ; per-stat record cap, value `100`
                                           ; in every captured save - see below
+0x122  u16 LE   agl_record               ; "Max AGL" (record copy)
+0x124  u16 LE   atk_record               ; "Max ATK"
+0x126  u16 LE   udf_record               ; "Max UDF"
+0x128  u16 LE   ldf_record               ; "Max LDF"
+0x12A  u16 LE   spd_record               ; "Max SPD"
+0x12C  u16 LE   int_record               ; "Max INT"
+0x12E  u8[2]    stat_window_tail
+0x130  u8       magic_rank               ; "Level 99" cheat target. See note.
+0x131  u8[11]   post_level_unmapped
+0x13C  u8       magic_slot_activator     ; "Magic Slot Activator" target,
                                           ; cheat sets to 0x24 to enable
                                           ; the spell list
+0x13D  u8[12]   magic_group_0            ; "Magic Modifier 1..12" hits
+0x149  u8[12]   magic_group_1            ; second copy of the slot
+0x155  u8[12]   magic_group_2            ; third copy of the slot
+0x161  u8[16]   summon_levels            ; "All Summons Level 9" target.
                                           ; Distinct from the spell list -
                                           ; the prior `levels` parallel array
                                           ; interpretation was wrong.
+0x179  u8[12]   post_summon_unmapped
+0x185  u8       displayed_skill_count    ; menu-overlay skill roster count
+0x186  u8[16]   displayed_skill_ids      ; "Has all Arts" target. Head insert
                                           ; on item-use spell-learn.
+0x196  u8       armor_id                 ; "Armor Modifier" / "Best Equipment"
+0x197  u8       head_gear_id             ; "Head Gear Modifier"
+0x198  u8       weapon_id                ; "Weapon Modifier"
+0x199  u8       accessory_or_seru_lock   ; "Activate Meta/Terra/Ozma at Lv9"
                                           ; - the "Seru lock byte" the menu
                                           ; reads to gate summon access
+0x19A  u8       leg_gear_id              ; "Leg Gear Modifier"
+0x19B  u8       accessory_1_id           ; "Accessory 1 Modifier"
+0x19C  u8       accessory_2_id           ; "Accessory 2 Modifier"
+0x19D  u8       accessory_3_id           ; "Accessory 3 Modifier"
+0x19E  u8[274]  post_equipment_unmapped
+0x2B0  ...      active_spell_slots[14]   ; 14 × 0x14-byte active-spell
                                           ; runtime slots - covered by the
                                           ; `active_spell_slot()` accessor
+0x380  u8[148]  tail_unmapped
```

## Cheat-database citations

Every offset above is named after an empirical write target in the
GameShark cheat database shipped at [`data/cheats/`](../../data/cheats/).
The mapping from cheat description to record offset is:

| Cheat | Address (Vahn) | Offset | Field |
|---|---|---:|---|
| `100 AP` | `0x80084816` | `+0x10E` | sp_max_live |
| `Max HP` | `0x8008480C` / `0x8008480E` / `0x80084824` | `+0x104` / `+0x106` / `+0x11C` | hp_curr_live / hp_max_live / hp_max_record |
| `Max MP` | `0x80084810` / `0x80084812` / `0x80084826` | `+0x108` / `+0x10A` / `+0x11E` | mp_curr_live / mp_max_live / mp_max_record |
| `Max AGL` | `0x80084818` / `0x8008482A` | `+0x110` / `+0x122` | agl_live / agl_record |
| `Max ATK` | `0x8008481A` / `0x8008482C` | `+0x112` / `+0x124` | atk_live / atk_record |
| `Max UDF` | `0x8008481C` / `0x8008482E` | `+0x114` / `+0x126` | udf_live / udf_record |
| `Max LDF` | `0x8008481E` / `0x80084830` | `+0x116` / `+0x128` | ldf_live / ldf_record |
| `Max SPD` | `0x80084820` / `0x80084832` | `+0x118` / `+0x12A` | spd_live / spd_record |
| `Max INT` | `0x80084822` / `0x80084834` | `+0x11A` / `+0x12C` | int_live / int_record |
| `Level 99` | `0x80084838` | `+0x130` | magic_rank |
| `Magic Slot Activator` | `0x80084844` | `+0x13C` | magic_slot_activator |
| `Magic Modifier 1` | `0x80084845` / `0x80084851` / `0x8008485D` | `+0x13D` / `+0x149` / `+0x155` | three magic-group copies |
| `All Summons Level 9` | `0x80084869..0x80084886` | `+0x161..+0x178` | summon_levels |
| `Has all Arts` | `0x8008488D..0x8008489A` | `+0x185..+0x192` | displayed_skill_count + ids |
| `Best Equipment` | `0x8008489E` / `0x800848A0` / `0x800848A2` | `+0x196` / `+0x198` / `+0x19A` | armor / weapon / leg gear |
| `Activate Meta at Lv9` | `0x800848A1` | `+0x199` | accessory_or_seru_lock |
| `Accessory 1 Modifier` | `0x800848A3` | `+0x19B` | accessory_1_id |
| `Max Exp` | `0x80084708` / `0x8008470A` | `+0x000` / `+0x002` | xp_cumulative (combined u32) |

The Noa (`+0x414`) and Gala (`+0x828`) bases shift every address by
one record stride; same offsets, same fields.

## Notes on contradictions reconciled by this document

Two prior interpretations had to be reconciled:

### `+0x130` is the displayed character level.

The status screen reads `+0x130` as "LV" and the `Level 99` GameShark cheat sets it
to `0x63` (99). **Boot-confirmed via the starting-level randomizer**: a New Game
record with level-10 cumulative experience (`+0x0`), level-10 stats, and the correct
next-level threshold (`+0x4`) but `+0x130 == 1` still displays **LV 1**, and setting
`+0x130 = 10` makes it display **LV 10** - so the shown level is read from `+0x130`
directly, *not* re-derived from cumulative XP at a New Game.

The retail level-up applier maintains `+0x130` by incrementing it `+1` per level-up
event (the captured `engine_core::levelup::observations::{noa,gala}_4_level_jump`
bumped it by one across a four-level grant, so it can momentarily lag the XP-derived
level after a rare multi-level jump), but for single-level play and the new-game seed
it equals the level. This supersedes the earlier "`+0x130` = Magic Rank" reading for
the level question; whether a separate magic-rank byte lives at the adjacent `+0x131`
(which the new-game seed also inits to 1) is unconfirmed. The runtime accessor
`legaia_save::CharacterRecord::magic_rank()` reads this byte, so it is in fact the
**level** byte under a legacy name; the crate's `level()` reads `+0x100`, which is
always zero in retail (the engine port uses it as its own internal level cell).

### `+0x4` is the *next-level threshold*, not cumulative XP.

`+0x0` is the cumulative experience (the "Max Exp" cheat target and the "Experience"
readout, and the value the level-up applier compares against the threshold); `+0x4`
is the **next-level XP threshold** - the "next" readout on the status screen. (The
displayed level itself is read from `+0x130`, above, not derived from `+0x0`.)
Confirmed live: a randomized ROM that wrote a level-10 XP value into `+0x4` showed
it as "next level: 11195" while *experience* stayed `0`, leaving the derived level
at 1 (the earlier note that "the level-up logic reads +0x4 as XP" conflated the
threshold the applier reads with the cumulative XP it reads it against). The
starting-level randomizer therefore seeds `+0x0`, not `+0x4`.

The runtime accessor in [`legaia_save::CharacterRecord`] is
`magic_rank()` / `set_magic_rank()`. The legacy `stat_cap()` call
sites (which read `+0x11A`) are preserved for back-compat - that
field is now understood to be the live INT stat (`int_live`),
which the engine clamps at `0x3E7` and the cheat database calls
`Max INT`.

### `+0x161..+0x178` is the summon-level array, not spell levels.

The prior `SpellList::levels` field claimed `+0x161..+0x184` was a
parallel "spell level" array. The GameShark `All Summons Level 9`
cheat stamps `0x09` into 16 bytes at `+0x161..+0x178` - with one
byte per summon ID, not per spell. The corrected layout puts the
per-spell levels (if any) in the magic-group window at `+0x13D..+0x160`
where the cheat database has 12 named modifiers, and treats
`+0x161..+0x178` as the 16-summon level array.

The original `SpellList::levels` accessor is preserved for
backwards compatibility; new code should prefer `summon_levels()`.

### `+0x004` vs `+0x000` for cumulative XP.

The captured Noa + Gala level-up triplets show the `+0x004` u16
moving by the expected XP delta per level-up event. The GameShark
`Max Exp` cheat writes a u32 spanning `+0x000..+0x003`. Both
co-exist in the engine: `cumulative_xp()` reads u16 LE at `+0x004`
(empirical, used for level inference); the `xp_low_word_alt` window
at `+0x000..+0x003` holds a separate per-level XP cell that the
runtime uses for "next level" displays.

## The `+0x120` cap constant

Across every captured save, `+0x120` reads as `100`. The level-up
overlay writes this constant on every level-up event regardless of
character or stat. The interpretation is that this byte is the
hard ceiling each individual record-side stat clamps against - the
runtime then re-projects through `FUN_80042558`'s `0x3E7` clamp on
the live copy, allowing equipment / buffs to push past the
record-side cap up to `999` in battle.

## See also

- [`legaia_save::character::CharacterRecord`] for the typed Rust
  accessors.
- [`docs/subsystems/level-up.md`](../subsystems/level-up.md) for the
  multi-frame write split during a level-up event.
- [`docs/subsystems/battle-formulas.md`](../subsystems/battle-formulas.md)
  for how the live stats feed damage / accuracy / RNG.
- [`docs/reference/cheats.md`](../reference/cheats.md) for the
  full cheat-database citation table and the runtime applier.
