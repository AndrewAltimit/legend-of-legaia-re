# Player-character mesh pack

The five player-character TMDs the retail engine keeps resident across every
field scene at `DAT_8007C018[0..=4]`. They live in the head section of PROT
entry **0874** (`befect_data`). The **same** pack also supplies the party
meshes in battle — the battle scene loader reloads it as `etmd.dat`; there is
no separate battle-character mesh (see
[§ Battle reuses the field form](#battle-reuses-the-field-form--there-is-no-separate-battle-character-pack)).
Implementation:
[`legaia_asset::character_pack`](../../crates/asset/src/character_pack.rs).

## On-disc layout

PROT 0874 is a [`parse_player_lzs(buf, 3)`](asset-descriptor.md)-shaped
container with three LZS-compressed sections. Section 0 decompresses to a
canonical [`asset::pack`](pack.md) TMD pack with **five** Legaia TMDs:

| Pack slot | Body offset | `nobj` (disc) | Body bytes (runtime) | Active-party role |
|---:|---:|---:|---:|---|
| 0 | `0x0018` | 12 |  13 220 | Vahn (party slot 0) |
| 1 | `0x33BC` | 12 |  13 800 | Noa (party slot 1) |
| 2 | `0x69A4` | 12 |  11 656 | Gala (party slot 2) |
| 3 | `0x972C` |  3 |   6 488 | Savepoint (save crystal) |
| 4 | `0xB084` |  2 |   1 048 | Auxiliary actor (untriaged) |

The "body bytes (runtime)" column is the length the engine allocates for each
slot — the descriptor's compressed-size hint bounds the LZS decode to ~46 KB
total, so slot 4 receives only its 1 048-byte TMD prefix even though the
underlying compressed stream would expand to ~65 KB of trailing zero
padding. This is byte-equality-verified against the live `DAT_8007C018[4]`
allocation in retail (see
[`world-map-overlay.md` § Disc-side source of `[0..4]`](world-map-overlay.md#disc-side-source-of-04)).

Byte-equality verified against a settled field-scene RAM snapshot at
`DAT_8007C018[0..=4]` — see
[`world-map-overlay.md` § Disc-side source of `[0..4]`](world-map-overlay.md#disc-side-source-of-04)
for the full match. The character pack is **shared across every field scene**,
not kingdom-specific; only the trailing field-pack `[5..]` window changes
per scene.

The slot-to-character mapping (slot 0 → Vahn, slot 1 → Noa, slot 2 → Gala)
is asserted by retail's `FUN_8001EBEC` patch loop: those three slots are
the only ones with `nobj=12` and the equipment-conditional group templates
the player-equipment swap pass needs. Slots 3 / 4 carry the small
auxiliary-actor meshes (no equipment swap).

## TMD shape (per slot)

Each pack body is a [Legaia TMD](tmd.md) with the canonical 12-byte header
followed by `nobj × 0x1C` group descriptors:

```text
+0x00  u32  magic = 0x80000002
+0x04  u32  flags (= 1 post-fixup)
+0x08  u32  nobj                  ; 12 / 12 / 12 / 3 / 2 on disc
+0x0C  group descriptors          ; 0x1C bytes each
```

Inside the active-party slots (0..=2), groups 10 and 11 are *templates* for
the equipment-conditional swap below; the engine caps live `group_count` to
10 at install time so the templates aren't drawn directly.

## 10-group cap + equipment-conditional swap

`FUN_8001E890` overwrites `DAT_8007C018[party_base + 0..2]`'s `entry[+0x08]`
(TMD `group_count`) to **10** after the install, capping each active-party
TMD at 10 live groups. The last two disc groups (10 and 11) are the
*equipment-conditional* templates the per-frame patch loop picks between.

`FUN_8001EBEC` runs that loop. For each of the three active-party slots:

1. Read the equipment toggle byte at the character record's per-slot offset.
2. If the byte is **non-zero**, source the group-10 template at TMD `+0x124`.
   If it's **zero**, source the group-11 template at TMD `+0x140`.
3. Overwrite the visible group descriptor at the slot's `patched_group_index`
   with that 28-byte template.

| Party slot | Character | Patched group | Equip-byte record offset | Template-zero (TMD `+0x140`) | Template-nonzero (TMD `+0x124`) |
|---:|---|:---:|:---:|---|---|
| 0 | Vahn | 0 | `+0x196` | group 11 | group 10 |
| 1 | Noa  | 3 | `+0x199` | group 11 | group 10 |
| 2 | Gala | 5 | `+0x19B` | group 11 | group 10 |

(The "patched group index" and the offset-within-the-equip-byte-window are
the same three numbers `{0, 3, 5}` — retail's `FUN_8001EBEC` reuses one tiny
stack table for both roles. See the asm trace in
[`ghidra/scripts/funcs/8001ebec.txt`](../../ghidra/scripts/funcs/8001ebec.txt).)

The swap is **binary**: each character has exactly one visible mesh group
that toggles between two pre-baked variants. Different equipped items don't
each get their own mesh swap; the toggle is a single bit ("weapon-bearing
group is on / off") and item identity is conveyed by the character's
[texture atlas](#textures), not by mesh changes.

`legaia_asset::character_pack::equipment_swap::apply` is the clean-room
equivalent: given a slot's disc-form TMD bytes, a [`PatchSlot`], and the
character's equipment toggle byte, it returns the patched TMD buffer.

## Textures (field form)

The field-form character TMDs reference texture pages and CLUTs the engine
uploads from **PROT 0876** (`player_data`), the streaming-format file with a
VAB + a 256×256 TIM_LIST atlas + a small SEQ trailer. The atlas goes to VRAM
`fb=(768, 0)` with CLUT at `(0, 500)`; both blocks are pinned in
[`FIELD_SHARED_BLOCKS`](../subsystems/asset-loader.md#field_shared_blocks) so
they survive every field-scene transition without being re-uploaded. PROT 0874
itself carries **no character textures** — its remaining sections are the
effect 3D models (etmd.dat) and effect-texture TIMs (etim.dat), unrelated to
the player mesh.

## Battle reuses the field form — there is no separate battle-character pack

There is **no distinct battle-character mesh**. A real main-game battle renders
the party from the **same PROT 0874 §0 pack** as the field. The battle scene
loader `FUN_800520F0` is a multi-step async state machine (sub-state byte at
`gp+0xa59`); its character-pack steps are:

- **state `0xb`** loads the battle model pack — dev path `FUN_8003e68c(0x36a)`
  (PROT index `0x36a` = **874**) into the work buffer, with `0x369` (873) as the
  index; retail path opens `h:\prot\battle\etmd.dat` (the dual-mode loader pulls
  the same data from ISO9660 vs. the PROT TOC).
- **state `0xc`** walks that pack and calls `tmd_register` on every entry —
  `tmd_register` is `jal 0x80026b4c` = **`FUN_80026B4C`**, the sole
  `DAT_8007C018` installer. So the battle installs PROT 874's TMDs into
  `DAT_8007C018[0..=4]` exactly the way the field initializer does.

The companion battle files are sibling PROT entries in the same `befect_data`
block, **not** sections of 874: `etim.dat` = PROT `0x368` (872, battle TIMs),
`efect.dat` = PROT `0x36b` (875, effects), plus a paired stage pack at
`0x367`/`0x36d` (871/877). Confirmed in `ghidra/scripts/funcs/800520f0.txt`.

Empirically: in settled-battle save states the party actors' mesh pointer
`actor[+0x230]` equals `DAT_8007C018[0..=2]`, and during a real battle's load
transition (`_DAT_8007B83C` mode `0x9`) the field-form `DAT_8007C018[1..=4]`
entries are still resident. The field-form `nobj` (12/12/12/3/2 on disc,
≈10/10/10/3/2 after the equipment swap) is what renders in battle — the higher
`nobj` 17/18/17 pack belongs to the Baka Fighter minigame (below), not battle.

## Baka Fighter minigame roster — PROT 1203-1221 (`other5`)

PROT entry `1204` (`other5`) is the character pack for the **Baka Fighter**
fist-fight minigame, **not** the main battle party. Baka Fighter lets you play
*as* Vahn / Noa / Gala, so its roster reuses recognizably the same characters —
which is why a save state captured during a Baka Fighter match shows
`DAT_8007C018[0..=4]` repointed at this archive (the minigame's
`overlay_baka_fighter` loads `data\field\other5.lzs` + PROT 1205/1206; debug
string `"OTHER5 %d %d"`). The earlier reading of PROT 1204 as a "battle form"
that repoints `DAT_8007C018` on `game_mode = 0x15` was a misidentification: the
`game_mode 0x15` save states that pinned it were Baka Fighter sessions (their
"enemy" actor's mesh also resolves to `other5`, PROT 1208/1209 — a real enemy
would come from the monster archive PROT 867). Parser:
`legaia_asset::baka_fighter_pack` (the name is a historical misnomer — it parses
the Baka Fighter pack).

PROT 1204 is a flat streaming-format container (no LZS wrapper) with five
chunks of **asset type `0x09` (TMD2)** plus a terminator plus seven trailing
TIMs at fixed `0x8224` stride:

| Region   | Offset      | Type | Size       | Role                                           |
|----------|-------------|------|------------|------------------------------------------------|
| chunk 0  | `0x000004`  | TMD2 | 33 516     | Vahn fighter (`nobj=15`)                       |
| chunk 1  | `0x0082F4`  | TMD2 | 33 636     | Noa fighter (`nobj=16`)                        |
| chunk 2  | `0x01065C`  | TMD2 | 24 780     | Gala fighter (`nobj=15`)                        |
| chunk 3  | `0x01672C`  | TMD2 | 27 036     | Extra fighter (`nobj=20`)                       |
| chunk 4  | `0x01D0CC`  | TMD2 | 33 340     | Extra fighter (`nobj=15`)                       |
| atlas 0  | `0x025804`  | TIM  | ~33 312    | 256×256 4bpp + 256×1 CLUT @ `(0, 490)`         |
| atlas 1  | `0x02DA28`  | TIM  | ~33 312    | CLUT @ `(0, 491)`                              |
| atlas 2  | `0x035C4C`  | TIM  | ~33 312    | CLUT @ `(0, 492)`                              |
| atlas 3  | `0x03DE70`  | TIM  | ~33 312    | CLUT @ `(0, 493)`                              |
| atlas 4  | `0x046094`  | TIM  | ~33 312    | CLUT @ `(0, 494)`                              |
| atlas 5  | `0x04E2B8`  | TIM  | ~33 312    | CLUT @ `(0, 495)`                              |
| atlas 6  | `0x0564DC`  | TIM  | ~23 332    | CLUT @ `(0, 497)` — truncated, last in pack    |

The bundled CLUTs are the correct Baka-Fighter palettes (the roster renders
to match the in-game PLAYER SELECT screen). The streaming chunk type `0x09`
(TMD2) is recognized in [`AssetType`](../../crates/asset/src/lib.rs) as a
distinct dispatcher tag from the regular TMD (type `0x02`); the TMD body shape
is identical (magic `0x80000002`).

## Animation

Per-character animation data is **not** in PROT 0874. The runtime per-action
record consumed by the actor tick `FUN_80021DF4` and the overlay-resident
per-frame animator lives in the [ANM container](anm.md) (asset type `0x06`);
the actor receives a record pointer via `FUN_80024CFC`
(`actor[+0x4C] = anm_base + record_offset`). Battle actions feed through a
parallel consumer struct at `actor[+0x234]` — see `anm.md` § Per-actor anim
state offsets.

## Readers (retail)

| Function | Role |
|---|---|
| `FUN_80020224` → `FUN_8001F05C` case 2 → `FUN_80026B4C` | Single descriptor-walk that installs PROT 0874 §0's 5 TMDs into `DAT_8007C018[0..=4]` (the engine routes this through [`seed_global_tmd_pool_from_befect_data`](../../crates/engine-core/src/scene.rs)). The **field** caller is `FUN_801D6704` → `FUN_80020118` → `FUN_8001E890`. The **battle** caller is the battle scene loader `FUN_800520F0` (state `0xb` loads PROT 874 / `etmd.dat`, state `0xc` `tmd_register`s it via `jal 0x80026b4c`) — see [§ Battle reuses the field form](#battle-reuses-the-field-form--there-is-no-separate-battle-character-pack). |
| `FUN_8001E890` | "DATA_FIELD player loader" — post-install, caps `entry[+0x08] = 10` for the three active-party slots at `DAT_8007C018[DAT_8007B824 + 0..2]`, then dispatches the per-character equipment-conditional patch to `FUN_8001EBEC`. |
| `FUN_8001EBEC` | Per-frame group-descriptor patch. Reads the equipment toggle byte and copies one of the two templates over the visible group descriptor. The full asm trace is decoded in [`ghidra/scripts/funcs/8001ebec.txt`](../../ghidra/scripts/funcs/8001ebec.txt). |

## CLI

```bash
# Field-form pack (PROT 0874 §0): list the five-slot shape + active-party templates.
asset character-pack extracted/PROT/0874_befect_data.BIN

# Baka Fighter minigame pack (PROT 1204): list the five TMD2 chunks + seven character atlases.
asset baka-fighter-pack extracted/PROT/1204_other5.BIN

# Export one Baka Fighter character TMD and one atlas TIM.
asset baka-fighter-pack extracted/PROT/1204_other5.BIN --slot 0 --out-tmd vahn_baka.tmd
asset baka-fighter-pack extracted/PROT/1204_other5.BIN --atlas 0 --out-tim vahn_atlas.tim

# Apply the equipment swap for a single slot + export the patched TMD.
asset character-pack extracted/PROT/0874_befect_data.BIN \
    --slot 0 --equip 1 --out vahn_equipped.tmd
```

## See also

- [Legaia TMD](tmd.md) — the per-slot mesh format.
- [`world-map-overlay.md` § Disc-side source of `[0..4]`](world-map-overlay.md#disc-side-source-of-04) — the byte-equality provenance against `DAT_8007C018[0..=4]`.
- [`subsystems/asset-loader.md`](../subsystems/asset-loader.md) — the `FIELD_SHARED_BLOCKS` invariant that keeps `player_data` resident.
- [`ANM animation`](anm.md) — the per-actor animation container that drives these meshes.
- [`art-data.md`](art-data.md) — the per-character art tables (animation indices map into the player ANM pack).
