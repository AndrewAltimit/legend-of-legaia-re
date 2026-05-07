# Builds + region data

External research from TCRF (`https://tcrf.net/Legend_of_Legaia`), GameHacking.org, and GameFAQs save-state guides — distilled into the technical bits useful for cross-region testing and runtime validation.

## Region enum

`legaia_prot::Region` captures the three retail regions. `ProtIndex` carries this as metadata — the TOC formula and CDNAME layout are region-agnostic.

| `Region` variant | Product codes | Notes |
|---|---|---|
| `Region::Na` | `SCUS-94254` | NA retail; anchor build for this project. Default. |
| `Region::Jp` | `SCPS-10059`, `SCPS-91246` | JP retail + "PlayStation The Best" reissue. RAM addresses shifted by `+0x1B90` relative to NA. |
| `Region::Eu` | `SCES-01752`, `SCES-01944–47` | EU EN + FR/DE/IT/ES localisations. Same binary layout as NA; MES tables swapped. |

`Region::from_product_code(code)` maps a disc ID prefix to the right variant.  
`Region::translate_addr(na_addr)` applies the per-region shift to an NA-anchor address.

## Developer attribution

- **Prokion** (also styled "Procyon" — appears in Japanese as ぷろきおん) is the primary studio.
- **Contrail** is co-credited.
- Publisher: SCEI (JP), SCEA (US), SCEE (EU).

This explains the dev paths visible in `SCUS_942.54` strings (`H:\PROT\FIELD\<stage>\tim.dat` etc.) — Sony PsyQ on a Windows host with `H:` mapped to the project tree.

## Known builds

| ID | Date | Notes |
|---|---|---|
| PAPX-90040 | 1996-06-02 | Trial on PrePre Vol. 14 demo disc |
| PAPX-90055 | 1998-08-19 | Demo 2 (JP) |
| PCPX-96130 | 1998-08-18 | Demo 1 / store kiosk video (JP) |
| SCPS-10059 | 1998-09-09 | JP retail (Regaia Densetsu) |
| SCPS-10059 | 1998-11-16 | NA prototype (uses JP serial) |
| SCUS-94366 | 1998-12-21 | NA demo |
| **SCUS-94254** | 1999-01-29 | **NA retail (anchor build for this project)** |
| SCES-01752 | 1999-09-27 | EU English retail |
| SCES-01944 | 2000-04-05 | French |
| SCES-01945 | 2000-05-10 | German |
| SCES-01946 | 2000-04-26 | Italian |
| SCES-01947 | 2000-??-?? | Spanish |
| SCPS-91246 | ? | "PlayStation The Best" reissue |
| SCPS-45340 | ? | Unknown JP variant |

The four EU localisations (FR/DE/IT/ES) are likely identical engine + swapped MES tables. Cross-region testing priority: SCPS-10059 (original JP) → SCUS-94254 (anchor) → SCES-01752 (EU English).

## Debug flags

Two distinct flags in the same RAM page:

| Flag | NA address | JP address | Effect |
|---|---|---|---|
| `_DAT_8007B8C2` | `0x8007B8C2` | (build-shifted by 0x1B90) | Dev/retail loader-path flag. Read by 26 SCUS functions. Controls `h:\PROT\FIELD\<stage>\…` dev-path branch in `FUN_800255B8` (and 25 sister branches in other loaders). |
| `_DAT_8007B98F` | `0x8007B98F` | `0x8007D51F` | In-game debug menu enable. Accessed as the high byte of the word at `0x8007B98C` — the GameShark byte-write `8007B98F 0001` makes the LE word non-zero (`0x01000000`), enabling every debug branch with one check. |

Both have **zero static writers in `SCUS_942.54`**. The writers must live in an unswept overlay or come from external POKE — TCRF GameShark codes prove both flags are runtime-writable.

The 0x1B90-byte build shift between JP and NA addresses is consistent with same-data, different-binary-layout. Implies the JP and NA executables have the same RAM-resident layout, just relocated.

## Debug input bindings

Once `_DAT_8007B98F` is set, these button combos are live. Dispatcher: `FUN_8001822C`.

| Combo | Effect |
|---|---|
| SELECT + △ | Open Debug Menu (item-give, map-change, stats, event flags, …) |
| SELECT + START | Start game in Debug Mode (TMD/sprite/sound/music testers) |
| R1 + R2 + X | Coordinates debug overlay + free camera |
| (in coords mode) ○ | Dim lighting |
| (in coords mode) ▢ | Toggle verbose overlay |

`FUN_8001822C` per-frame:
- Reads BIOS pad data at `0x800840F8`, builds 32-bit button mask `_DAT_8007B850`.
- `if (_DAT_8007B98C == 0) _DAT_8007B850 &= 0xFFFF` — the upper 16 bits (controller 2 / debug bindings) are stripped when debug is OFF. **This is the master gate.**
- Then a large block guarded by `if (_DAT_8007B98C != 0)` handles the button combos above.

GameShark D-code globals:
- `DAT_8007B7C0` — previous-frame button mask (so `D007B7C0 XXXX` D-codes mean "wait for the user to press XXXX").
- `_DAT_8007B874 = mask & ~prev` — newly pressed this frame (edge detection).
- `DAT_8007B7C4 = mask ^ prev` — buttons that changed.

## Camera control axes

The debug coordinate overlay uses cinematography axis names — useful as a string-search anchor for finding the camera-debug renderer:

```
VX (truck), VY (pedestal), VZ (zoom)
WX (track), WZ (dolly)
AX (crane), AY (tongue/pan), AZ (roll)
SC (FOV)
```

These exact strings appear in the executable as debug-overlay text.

## CDNAME subsystem semantics

| Block range | Likely contents |
|---|---|
| 0..864 | Stage / field entries (towns, dungeons, world maps, cutscenes) — keyed by map name |
| 865..868 | `battle_data` — battle subsystem core |
| 869 | `monster_data` |
| 870..871 | `sound_data` |
| 872..875 | `befect_data` (battle effects) |
| 876 | `player_data` |
| 877..890 | `sound_data2` |
| 891..892 | `level_up` (XP / leveling tables) |
| 893 | `monster_se` (monster sound effects) |
| 894 | `card_data` (menu / inventory UI) |
| 895..896 | `bat_back_dat` (battle backgrounds) |
| 897..971 | `xxx_dat` — mini-game cluster |
| 972..973 | `move_program_no` |
| 974..979 | `other_game` |
| 980..989 | `monster_test` (debug fixture) |
| 990..1071 | `music_test` / `music_01` (sequence + bank data) |
| 1072..1194 | `vab_01` (VAB instrument banks) |
| 1195..1232 | `other1`/4/5/6/7 — minigame fields |

## Dispatch RAM addresses (GameHacking.org)

PSX RAM (KSEG0, `0x80000000` base). Useful as runtime-tracing anchors.

| Address | Purpose |
|---|---|
| `0x80084540` | Current map ID (writable; "Map Modifier" / "View Credits" use this) |
| `0x8007B6F4` | "Small maps" debug mode flag |
| `0x8007B7C0` | Debug-dispatch trigger — selects which menu/state is active |
| `0x8007B450` | Debug-dispatch parameter slot — sub-action within the trigger's mode |
| `0x800422F4` | "Force item quantity 99 on pickup/buy" flag |

The `0x8007B7C0` + `0x8007B450` pair forms a built-in menu/debug-action dispatcher. Sample parameter values when trigger == `0x0100`:

| Parameter | Effect |
|---|---|
| `0x0001` | Open Status Modifier Menu |
| `0x000D` | Open name modifier for Gala |
| `0x0011` | Open name modifier for Vahn |
| `0x0085` | Show End-of-Game stat page |
| `0x4097` | "Save Anywhere" prompt |

When trigger == `0x0003`, writing to `0x80084540` warps to that map ID.

## Mini-game state regions

Each mini-game gets its own ~64 KB slab of upper RAM, loaded fresh when entered. The `xxx_dat` cluster (PROT 897..971) holds mini-game data; different mini-games' state regions don't overlap because they're loaded sequentially.

| Address | Mini-game | Purpose |
|---|---|---|
| `0x801D9168` | Fishing | Tension |
| `0x801D9274` | Fishing | Casting power |
| `0x801D9298` | Fishing | Life |
| `0x801D91CC` | Fishing | Fish ID modifier |
| `0x801D53CC` | Dancing | Dance points |
| `0x801DBFC4` | Baka Fighter | Player life |
| `0x801DC06C` | Baka Fighter | Computer life |

## Character struct

Stride: **0x414 bytes (1044)**. Confirmed by file-offset deltas in ePSXe save states (Vahn / Noa / Gala). 4 slots (Vahn / Noa / Gala / Terra-the-wolf) gives a party-data block of ~4096 bytes — exactly one PSX RAM page.

Layout (relative to Level offset, all `u16` LE unless noted):

| Offset | Field |
|---|---|
| `-0x2C` | HP max |
| `-0x2A` | HP current |
| `-0x28` | MP max |
| `-0x26` | MP current |
| `-0x20` | AGL current |
| `-0x1E` | ATK current |
| `-0x1C` | UDF current |
| `-0x1A` | LDF current |
| `-0x18` | SPD current |
| `-0x16` | INT current |
| `-0x14` | HP unequipped (base) |
| `-0x12` | MP unequipped (base) |
| `-0x0E..-0x04` | AGL/ATK/UDF/LDF/SPD/INT unequipped (base) |
| `+0x00` | Level |
| `+0x55` | Moves-learned count (u8) |
| `+0x56...` | Moves array (u8 IDs, contiguous) |
| `+0x177` | Name (string) |

The "current" stats are post-equipment; "unequipped" are the base values. Stat order (AGL/ATK/UDF/LDF/SPD/INT) is fixed across both blocks and across all 4 slots.

## Money and inventory

- **Money**: 24-bit (3 consecutive u8 bytes, LE). Cap = 0x00FFFFFF = 16,777,215 G.
- **Inventory**: single shared list, 2 bytes per entry `[item_id, quantity]`. Weapons, armor, items, and Ra-Seru levels are all in the same list.

## Item ID table

256 max. Canonical NA assignments:

```
Items (consumables, books, flutes):    0x65-0x9A, 0xA3
Ra-Seru weapons (per char × levels):   0x01-0x19  (Meta=Vahn 1-9, Terra=Noa 1-8, Ozma=Gala 1-7, plus 0x12=empty Noa slot)
Normal weapons:                        0x1A-0x33, 0x52 (empty), 0xBA (Astral Sword)
Helmets:                               0x34-0x42
Armors:                                0x43-0x57
Boots:                                 0x58+
```

ID `0x12` is "empty wp. for Noa" and `0x52` is "empty wp. for Vahn" — the inventory has reserved-but-empty slots.

## Move tables (Tactical Arts)

14 moves per character (IDs `0x01-0x0E`); slot `0x0F` is reserved for Miracle Arts (gated by a story flag).

**Vahn:**
```
0x01 Burning Flare    0x02 Fire Blow      0x03 Tornado Flame  0x04 Cyclone
0x05 Hurricane        0x06 PK Combo       0x07 Spin Combo     0x08 Pyro Pummel
0x09 Cross-Kick       0x0A Power Punch    0x0B Slash Kick     0x0C Somersault
0x0D Charging Scorch  0x0E Hyper Elbow
```

**Noa** (in display order): Lizard Tail, Acrobatic Blitz, Sonic Javelin, Blizzard Bash, Mirage Lancer, Dolphin Attack, Bird Step, Swan Diver, Tough Love, Rushing Gale, Tempest Break, Frost Breath, Vulture Blade, Hurricane Kick.

**Gala**: Flying Knee Attack, Battering Ram, Ironhead, Back Punch, Guillotine, Head-Splitter, Side Kick, Black Rain, Neo Raising, Electro Thrash, Bull Horns, Thunder Punch, Lightning Storm, Explosive Fist.

The script VM's [`EXEC_MOVE` (op 0x22)](../subsystems/script-vm.md) feeds these IDs into the move-table consumer at `FUN_800204F8`; the move-table file format itself is documented at [`formats/mdt.md`](../formats/mdt.md).

## Translation notes

Character-name discrepancies between JP and NA scripts:

- Gala (NA) = Gaara (JP)
- Vahn (NA) = Vann (JP)
- Noa (NA) = Noah (JP)
- Tetsu (NA) = Todd (JP)

Will matter when extracting MES tables and comparing across regions.

## Unused content

Useful as fixtures for validating extractors:

- **Unused enemies**: index 78 ("Comm") and 140 ("Evil Bat") in the monster table.
- **Unused items**: "Something Good" (sells for 50,000G) and an unnamed accessory (forces Seru-only encounters).
- **Placeholder battle background**: a graphic containing `ぷろきおん` in hiragana — loaded when a real battle background is missing.
- **Placeholder music**: tracks copied from Alundra (Zazan battle theme) and Wild ARMs (battle theme) sit in the SEQ/VAB data.
