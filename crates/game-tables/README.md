# legaia-game-tables

The game's decoded data tables: fixed-layout records read out of
`SCUS_942.54` or out of an overlay image, with no asset dispatcher or scene
bundle in between.

## The rule

[`legaia-overlay-images`](../overlay-images/README.md) describes **a code
image** (how to detect one, where it loads, how a slot-B module is laid out);
this crate decodes **the data tables inside an image**, wherever the table is
resident. A table module takes its overlay's PROT index and load base from
`legaia_overlay_images::static_overlay` instead of restating them, so this
crate depends on that one and never the reverse.

## What lives here

- **Items + equipment** (`SCUS_942.54`) - `item_names`, `item_effect`,
  `equip_stats`, `accessory_passive`, `steal_table`.
- **Spells** (`SCUS_942.54`) - `spell_names`.
- **Progression** (`SCUS_942.54`) - `level_up_tables`, `new_game`,
  `victory_pose`.
- **Battle overlay tables** (PROT 0898) - `move_power` (per-move power +
  behaviour, [`move-power.md`](../../docs/formats/move-power.md)),
  `element_affinity`, `battle_camera_table`, `battle_attack_camera_table`,
  `spell_anim_pairs`, `seru_side_effect`, `absorb_caption`.
- **Menu overlay tables** (PROT 0899) - `menu_windows` (the window descriptor
  table, [`field-menu.md`](../../docs/subsystems/field-menu.md)),
  `widget_script` (the window-script programs,
  [`window-script.md`](../../docs/formats/window-script.md)), `menu_pool`
  (the string pool's named labels) and `menu_string_refs` (the code site
  that forms each port-read string's address, so a reader follows a string
  a language pack moved; [`ui-strings.md`](../../docs/tooling/translation/ui-strings.md#what-the-port-reads)).
- **Modes, movies, sound** - `mode_table`, `sfx_table`, `xa_cue_table`
  (`SCUS_942.54`); `fmv_dispatch` (STR overlay, PROT 0970); `str_fmv_table`
  (the cutscene player's in-RAM movie lookup table).
- **World map** (`SCUS_942.54`) - `worldmap_menu` (the quick-travel landmark
  menu).
- **Derived** - `seru_trade`, the Seru-trade offer table the patcher and the
  engine both compute from a seed. It is not disc data; it lives here because
  it is keyed on the spell and item id spaces above.

The format pages under [`docs/formats/`](../../docs/formats/overview.md)
(`item-table.md`, `spell-table.md`, `equipment-table.md`, ...) carry each
table's layout and provenance.

## Layering

Depends on `legaia-bytes` and `legaia-overlay-images`. It must not depend on
the container layer or on any scene, UI, minigame or actor format
crate; those sit above it and read these tables.

## Module reference

Entry points, CLI subcommands (`asset ...` is the binary in
[`crates/asset`](../asset/README.md)) and the doc page each module is specified on.

| Module | Covers |
|---|---|
| `worldmap_menu` | The quick-travel landmark menu out of `SCUS_942.54`: 16-entry name table (`DAT_80073B18`) + 6-byte placement records (`DAT_80073A98`). CLI `asset worldmap-menu` (`--json` = the web-viewer shape). |
| `menu_windows` | The menu overlay's 52-entry **window descriptor table** (PROT 0899 file `0x15F20`, VA `0x801E4738`): per-window content id / park edge / class + content rect + content-renderer VA - the caller-supplied rects behind every pause-menu screen (status main panel = id 28 -> `FUN_801D33D8`), plus the per-screen window-id sets pinned from the menu-open captures. See [`docs/subsystems/field-menu.md`](../../docs/subsystems/field-menu.md#window-descriptor-table). |
| `widget_script` | The **window widget scripts** the window-script VM (`FUN_801D6628`, `legaia_engine_vm::run`) interprets: fixed 4-byte `[opcode][window id][operand u16]` programs resident in the menu overlay's own data segment. `parse_at` validates one program; `scan` recovers every program from the image's `jal` sites + `a0` materialisations. See [`docs/formats/window-script.md`](../../docs/formats/window-script.md). |
| `sfx_table` | Sound-effect descriptor table (`DAT_8006F198`, 100 × 8-byte): `SfxTable::from_scus` → per-cue program/VAG, ADSR-region base, voice count + sustained bit, mixer channel. Feeds `SfxBank::from_descriptors`. CLI `asset sfx-table <SCUS>`. See [`sfx-table.md`](../../docs/formats/sfx-table.md). |
| `level_up_tables` | Level-up data: `xp_thresholds_from_scus` (the 98-entry XP increment table) + `xp_correction_divisors_from_scus` (the per-level slots-1/2 threshold-correction divisors at `0x80070A2C`) + `growth_tables_from_scus` (the `DAT_80076918` per-character 8-stat growth curves). CLI `asset level-up <SCUS>`. |
| `item_names` | `SCUS_942.54` item-name table (`PTR_DAT_8007436C[id*3]`, 256 ids): `ItemNameTable::from_scus` → `name(id)`; `from_exe` reads the measured PAL builds too (`table_va_for_exe`); `drawn(level_mark)` is a copy reading the way the font draws it (`legaia_font::cell_text`). The id space a monster record's `drop_item` indexes; used by the web viewer's enemy table. See [`item-table.md`](../../docs/formats/item-table.md). |
| `item_effect` | `SCUS_942.54` item-effect descriptor table (`DAT_800752C0`, 130 records): `ItemEffectTable::from_scus` → `effect(id)` (item id → subtype → `[class, tier, flags]`). Effect class/tier + all-party/field/battle usability, plus the **literal restore amounts** - `heal_amounts()` / `restore_amount(id)` decode the static heal-amount table at `0x8007655C` (HP `[200,800,9999]` / MP `[50,200,20]`) the apply handler `FUN_800402F4` reads - and the **stat-up / buff taxonomy** - `stat_effect(id)` → `StatItemEffect` for the permanent stat-up *Water* line (class 6), the one-battle `×6/5` buff Elixirs (class 7), and Fury Boost (class 5). See [`item-effect-table.md`](../../docs/formats/item-effect-table.md). |
| `equip_stats` | `SCUS_942.54` equipment stat-bonus table (`DAT_80074F68`, 8-byte stride): `EquipStatTable::from_scus` → `bonus(id)` (equippable id → property `+1` byte → record). Attack/def-up/def-down (byte-exact vs gamedata) + equip-character mask + slot type + Ra-Seru flag. See [`equipment-table.md`](../../docs/formats/equipment-table.md). |
| `accessory_passive` | Accessory ("Goods") passive effects: `AccessoryPassiveTable::from_scus` → `passive(id)` (item id → descriptor `+3` / equip `+5` index byte → 64-slot passive index + the `0x8007625C` name/description/scope record). `stat_boosts(index)` mirrors the `FUN_80042558` percent arithmetic, `bit_location(index)` the `char+0xF4` ability-bitfield placement. Byte-validated vs the curated gamedata accessory table. CLI `asset accessory-passive <SCUS>`. See [`accessory-passive-table.md`](../../docs/formats/accessory-passive-table.md). |
| `spell_names` | `SCUS_942.54` spell table (`DAT_800754C8`/`DAT_800754D0`, 256 ids swept over a 190-record table): `SpellNameTable::from_scus` → `name(id)` / `mp(id)`. Resolves a monster's global magic-attack ids (`MonsterRecord::magic_attacks`, record `+0x21..=+0x23`) into the on-screen spell name (`0x27` → `Tail Fire`). `SpellEntry` also carries the `+0`/`+1` **class / sub-class** pair the battle-action commit stamps into `actor[+0x1E8]`, which selects a cast's cue group. `name_field` is one row's writable name extent - the record's own `+8` pointer plus measured NUL slack, mirroring `legaia_art::arts_table::name_field`. CLI `asset spell-names <SCUS>`. See [`spell-table.md`](../../docs/formats/spell-table.md). |
| `steal_table` | `SCUS_942.54` per-monster steal table (`DAT_80077828`, 1-based monster id, 2-byte `[chance, item]`): `StealTable::from_scus` → `entry(id)` / `steal_item(id)`. What the Evil God Icon steals; the item id resolves through `item_names`. NOT in the PROT 867 record. CLI `asset steal-table <SCUS> [--all]`. See [`steal-table.md`](../../docs/formats/steal-table.md). |
| `mode_table` | `SCUS_942.54` game-mode dispatch table (`0x8007078C`, 28 × 24-byte entries): `ModeTable::from_scus` → per-mode handler fn ptr / param / dev name. Recovers the index → retail-handler map from the disc (12 of 14 per-frame modes share `0x80025EEC`; field/town = 2/3 MAIN; world-map = 12/13 MAPDISP). CLI `asset mode-table` (`--json`). See [`boot.md`](../../docs/subsystems/boot.md#game-mode-state-machine). |
| `absorb_caption` | The Seru-absorb caption pieces the battle HUD's screen element `0x59` composes (PROT 0898). |
| `seru_trade` | Seru-trade vendor offers: the deterministic time-bucketed trade table shared by the patcher and the engine. |
| `spell_anim_pairs` | The party cast trigger's per-spell anim-pair lists in PROT 0898 (`FUN_801DBF9C`). |
| `victory_pose` | The static `SCUS_942.54` victory-pose table `FUN_8004E568` indexes. |
| `xa_cue_table` | The static `SCUS_942.54` XA cue duration table at `DAT_800788B8`. |

### `move_power`

Battle-action **per-move power table** (runtime VA `0x801F4F5C`), the
26-byte-stride table `FUN_801dd0ac` reads for the arts/physical attacker roll, plus
the 128-byte **id → index map** at `0x801F4E63` that resolves a battle move id
(`actor[+0x1df]`) to a record (`param_1 = map[move_id]`).

Static battle-overlay data, pinned in PROT 0898 at fixed raw-entry offsets
(byte-matched against the in-RAM table + map across two battle save states).

- `parse` → records (`MoveRecord::power()` = `+0 >> 2`, `counter_init()` = `+0x04`,
  `sound_cue_id()` = `+0x0d`).
- `parse_id_index_map` + `index_for_move_id` / `record_for_move_id` resolve a move
  id. Remaining record fields open.

CLI `asset move-power <PROT 0898 .BIN>` (`--json` for the machine-readable table). See
[`spell-table.md`](../../docs/formats/spell-table.md).

### `battle_attack_camera_table`

Per-art **attack-camera track table** (runtime VA `0x801F4E10`, PROT 0898 file
`0x265F8`): 20 rows of two signed halfwords, addressed
`base + row*4 + phase_cursor*2`. These are the values `FUN_801D71B8`'s per-art
arms fold into the swing framing - the data half of the camera that frames a
Tactical Art. Extent measured two independent ways that agree: the `lhu`
displacements the arms form run `0x00..0x4C` dense, and the disc bytes stop
being camera offsets at `0x801F4E60`.

- `parse` → the row list; disc oracle `tests/battle_attack_camera_table_real.rs`
  (skip-passes without `LEGAIA_DISC_BIN`). See
  [`battle-attack-camera-table.md`](../../docs/formats/battle-attack-camera-table.md).

### `element_affinity`

Battle **element-affinity** matrix (runtime VA `0x801F53E8`) + per-character element
table (`0x801F5480`), the static battle-overlay data `FUN_801dd864` reads to scale
the attacker roll.

- `parse` → the 8×8 matrix (`affinity_pct(atk, def)` = `matrix[attacker][defender]`,
  retail diagonal 96 / opposite-pairs 104 / default 100) + `character_element(char_id_1based)`
  (Vahn=fire, Noa=wind, Gala=thunder, Terra=wind).
- `Element` enum names the ids (2/3/4/7 pinned, 0/1/5/6 inferred).

PROT 0898, same link base as `move_power`. CLI `asset element-affinity <PROT 0898
.BIN>` (`--json` emits the matrix + per-character + summon-power tables). See
[`battle-formulas.md`](../../docs/subsystems/battle-formulas.md#element-affinity-matrix-fun_801dd864-0x801f53e8).

### `seru_side_effect`

The Seru-magic **side-effect table** (runtime VA `0x801F6870`, PROT 0898 file
`0x28058`): per summon element and magic-level band, the percent the damage
finisher `FUN_801DDB30` shaves off the target's stat on every hit (earth DEF /
water AGL / fire ATK / wind SPD / thunder INT / dark MP, `5/10/15/20`), the
cure class on the light row, and the banner-string VA the stager
`FUN_801F3D3C` installs.

- `SeruSideEffectTable::parse` → `amount(element, level)` / `record(..)`;
  `level_band` (`(level - 3) >> 1`, nothing below 3); `SideEffectKind::for_element`.
- `Susceptibility::for_record(stats, scripted)` → per-debuff `EveryHit` / `Once`
  / `Never` for one monster record: in a scripted fight the stager compares the
  target's base stat with the record, and the scripted boost profile has already
  moved ATK / UDF / INT - there is **no per-monster immunity field**.
- `resist_roll_bypassed(summon_element, affinity_pct)` - the scripted fight's
  80% suppression roll is skipped at affinity `>= 101` or for light.

CLI `asset seru-side-effect <PROT 0898 .BIN>`. See
[`battle-formulas.md`](../../docs/subsystems/battle-formulas.md#seru-magic-side-effects---the-element-debuffs-fun_801f3d3c--the-finisher-switch).

### `battle_camera_table`

Per-character **battle-camera height** table (runtime VA `0x801F4D2C`), the
`TR.y` the command-submenu close-up framing `FUN_801D5854` case `0` reads for
whichever character is acting.

- `parse` → `height_for_char_id(char_id_1based)` / `height_for_index` over one
  entry per playable character. Vahn's entry is the value the solo-Vahn camera
  trace pins, so it doubles as the base/stride check.
- The framing keys on **character identity** (`DAT_8007BD10[slot]`), not on
  seat, so this is a per-model eye height rather than a per-position offset.

PROT 0898, same link base as `move_power`. See
[`battle-action.md`](../../docs/subsystems/battle-action.md#case-0---the-submenu-close-up-framing).

### `new_game`

`new_game` - `SCUS_942.54` new-game seed data:

- `StartingParty::from_scus` decodes the starting-party template (`0x80078C4C`, 4
  records, 26-byte stride) into per-member opening stats + name
  (Vahn/Noa/Gala/Terra).
- `StartingInventory::from_scus` decodes the starting-inventory seed code in
  `FUN_80034A6C` (`0x80034b04`) into `(item_id, count)` slots (vanilla = Healing
  Leaf `0x77` ×5), reading back either the original `sb` byte-stores or the
  randomizer's packed `sh` halfword stores.

Seeds for the live `0x80084708 + n*0x414` records + the `0x80085958` bag;
`OPENING_SCENE` = `town01`. CLI `asset new-game <SCUS>`. See
[`new-game-table.md`](../../docs/formats/new-game-table.md).

### `str_fmv_table`

`str_fmv_table` - the compact in-RAM STR FMV file table (`0x801CAE40`,
24-byte stride × 6: name + libcd BCD MSF + size) the cutscene overlay uses to
resolve a movie without an ISO9660 walk. See
[`str-fmv-table.md`](../../docs/formats/str-fmv-table.md).

### `fmv_dispatch`

`fmv_dispatch` - the per-`fmv_id` movie + frame-range dispatch the STR/MDEC
overlay's play loop selects from, decoded straight from the overlay bytes.
