# Player-character mesh packs

The player characters have **two distinct mesh packs**, one per game form:

- **Field form — PROT 0874 §0** (`befect_data`): the low-poly walk/talk models
  the engine keeps resident across every field scene at `DAT_8007C018[0..=4]`.
  Parser [`legaia_asset::character_pack`](../../crates/asset/src/character_pack.rs).
- **Battle form — PROT 1204** (`other5`): the higher-detail party models the
  engine installs into `DAT_8007C018[0..=2]` for every turn-based battle. Parser
  [`legaia_asset::battle_char_pack`](../../crates/asset/src/battle_char_pack.rs).
  The **Baka Fighter** fist-fight minigame reuses this same pack (see
  [§ Battle form](#battle-form--prot-1204)).

The field form is field-only; battle uses the battle form. (An earlier reading
held that battle reused the field pack — falsified by direct save-state
byte-comparison; see the provenance note in § Battle form.)

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

## Battle form — PROT 1204

A real main-game battle renders the party from PROT entry `1204` (`other5`),
**not** the field pack. These are higher-detail Vahn / Noa / Gala meshes plus
two extra fighter slots; the engine installs the active party into
`DAT_8007C018[0..=2]` for the battle.

**Empirical provenance (decisive).** Reading the live party mesh pointers
`DAT_8007C018[0..=2]` out of real-battle save states and byte-comparing each
runtime TMD's (pose-independent) vertex pool against the two candidate disc
packs shows the party meshes byte-match PROT 1204 and **never** the field pack
0874:

| Real battle | slot 0 (Vahn) | slot 1 (Noa) | slot 2 (Gala) |
|---|---|---|---|
| Tetsu tutorial | `nobj=17` → 1204 | (Vahn-only) | (Vahn-only) |
| Gimard Seru-boss (turn-based) | `nobj=17` → 1204 | enemy/aux | enemy/aux |
| Gobu Gobu (full party) | 1204 (12/17) | 1204 (16/18) | 1204 (17/17) |

Gimard is an unambiguous turn-based boss fight, so this is not a minigame
artifact. Reproduce with
[`scripts/verify_battle_char_pack.py`](../../scripts/verify_battle_char_pack.py)
against a battle RAM dump; the disc-only distinctness (battle Vahn geometry is
absent from the field pack) is pinned by the disc-gated
`battle_char_pack_real::battle_pack_is_distinct_from_field_pack`.

Runtime `nobj` is +2 over disc (15/16/15 → 17/18/17): the same
`FUN_8001EBEC` equipment-group patch the field form uses adds visible groups at
battle setup.

**The Baka Fighter minigame reuses this same pack.** Baka Fighter lets you play
*as* Vahn / Noa / Gala, so it borrows the battle character models — its
`overlay_baka_fighter` loads `data\field\other5.lzs` + PROT 1205/1206 (debug
string `"OTHER5 %d %d"`). This is why save states captured *during a Baka
Fighter match* also show `DAT_8007C018[0..=2]` pointing at this archive; it is a
shared battle/minigame pack, not a minigame-exclusive roster. (An earlier
session had this backwards — concluding 1204 was Baka-Fighter-only and that
battle reused the field pack.) Parser: `legaia_asset::battle_char_pack`.

**Loader provenance (partly open).** The captured battle scene loader
`FUN_800520F0` (sub-state byte at `gp+0xa59`) loads PROT `0x367/0x368/0x369/
0x36a/0x36b` and `tmd_register`s `0x36a` — but that registration fills the
**effect/model window `DAT_8007C018[3..]`** (`etmd.dat`), not the party
`[0..=2]` (the party meshes live in a separate high RAM region, e.g. Vahn at
`0x80165f48`). The party-mesh load that installs PROT 1204 into `[0..=2]` for a
normal battle is in an as-yet-uncaptured battle-setup overlay (only
`overlay_baka_fighter` references the `other5` family in the current dumps). A
Lua write-watchpoint on `0x8007C018` during a real battle-entry transition would
pin it. (`ghidra/scripts/funcs/800520f0.txt` for the loader; sibling battle
files `etim.dat` = `0x368`, `efect.dat` = `0x36b`, stage pack `0x367`/`0x36d`.)

### Battle palette is VRAM residue (nominal CBA, no relocation)

The 1204 atlases ship with **bundled CLUTs** that are the **Baka Fighter**
palette (red hair). The **true battle palette** (blue-haired Vahn, etc.) is
*not* the bundled one — but, crucially, the battle character renderer samples
each prim with the mesh's **nominal CBA** (CLUT rows 490..497):
`row = (cba>>6)&0x1ff`, `col = (cba&0x3f)*16`. There is **no texpage→row
relocation for characters.** (The `0x8007BEC0` table that `FUN_800198E0` builds
— `table[image_texpage] = clut_y` — drives the *scene/background* renderer, not
the party meshes. An earlier reading that relocated character CLUTs through it
was an artifact of a contaminated mid-battle save and is **falsified**.)

So how does the *true* palette get to rows 490..497 if the pack ships the Baka
one? **It is VRAM residue.** Every field/world-map scene's `tim.dat` (driver
`FUN_8002541C` → per-TIM `FUN_800198E0`) uploads CLUT-only TIMs whose `(cx, cy)`
is **baked into the TIM header** (`cy` 490..497, matching the nominal CBA), and
battle entry **never clears those rows** — so the party palette the player's
scenes uploaded simply persists into the fight. Party nominal rows:
**Vahn 490/491, Noa 492/493, Gala 494/495, aux 496/497.**

#### Battle palette provenance — scattered scene residue

A corpus-wide scan (raw + LZS-decoded, every PROT entry, validated against a
clean-battle VRAM) places the band across the scenes a world-map battle passed
through — no single asset holds it:

- Rows **490, 495, 496, 497** — LZS-compressed CLUT-only TIMs in the **map01**
  bundle (PROT 0086).
- Rows **491, 492, 493** — raw CLUT-only TIMs in **town** bundles (town01
  `0003`/`0005`, town0b/0c/0d, urudre1, town0e); row 493 also in **dolk** (0061).
- Row **494** (Gala's body) — **runtime-generated; absent from the disc
  entirely** (raw + LZS, all 1232 entries) and from main RAM at stable saves
  (transient decompress→DMA→free). Its generator is in the uncaptured
  party-setup overlay — an open thread
  (see [`open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md)).

**Reproduction recipe** (clean-room, from the disc): replay the CLUT uploads of
the visited scenes — for each, decompress its LZS sections, find CLUT-bearing
TIMs (`magic 0x10`, `flags & 8`, `cy` 490..497, `ch`≤4) and paint each at its
baked `(cx, cy)`; then sample the mesh's nominal CBA. Replaying
`dolk → town01 → map01` reproduces rows 490–493, 495, 496 **byte-exact** vs a
real battle; row 494 is the only gap. The web viewer's
`battle_char_true_vram_bytes` implements exactly this (uploading the bundled
1204 CLUTs first as the row-494 fallback, then overlaying the replayed band),
sampled with the nominal `battle_char_mesh_cba_tsb`.

> Use a **clean** battle capture as ground truth (command-menu / Begin-menu, no
> effect animation). Mid-battle captures (e.g. a Drake fight paused during an
> effect) overwrite the character CLUT rows and read back garbage — that
> contamination produced the earlier, wrong relocation reading.

### Equipment groups (battle only)

A live battle character carries +2 `nobj` over the disc form (Vahn 15→17).
The equipment swap (`FUN_8001EBEC`, the same mechanism the field pack uses)
replaces several visible groups at battle setup; the replacement geometry
(the equipped weapon/gear) is **not present in the 1204 TMD** — it is sourced
externally (a separate weapon mesh), so the in-battle silhouette differs from
both the unarmed disc form and the Baka Fighter form (a fist-fight, which
keeps the unarmed mesh). The external weapon-mesh source is an open thread.

### On-disc layout (PROT 1204)

PROT 1204 is a flat streaming-format container (no LZS wrapper) with five
chunks of **asset type `0x09` (TMD2)** plus a terminator plus seven trailing
TIMs at fixed `0x8224` stride:

| Region   | Offset      | Type | Size       | Role                                           |
|----------|-------------|------|------------|------------------------------------------------|
| chunk 0  | `0x000004`  | TMD2 | 33 516     | Vahn battle (`nobj=15`)                        |
| chunk 1  | `0x0082F4`  | TMD2 | 33 636     | Noa battle (`nobj=16`)                         |
| chunk 2  | `0x01065C`  | TMD2 | 24 780     | Gala battle (`nobj=15`)                         |
| chunk 3  | `0x01672C`  | TMD2 | 27 036     | Extra fighter (`nobj=20`)                       |
| chunk 4  | `0x01D0CC`  | TMD2 | 33 340     | Extra fighter (`nobj=15`)                       |
| atlas 0  | `0x025804`  | TIM  | ~33 312    | 256×256 4bpp + 256×1 CLUT @ `(0, 490)`         |
| atlas 1  | `0x02DA28`  | TIM  | ~33 312    | CLUT @ `(0, 491)`                              |
| atlas 2  | `0x035C4C`  | TIM  | ~33 312    | CLUT @ `(0, 492)`                              |
| atlas 3  | `0x03DE70`  | TIM  | ~33 312    | CLUT @ `(0, 493)`                              |
| atlas 4  | `0x046094`  | TIM  | ~33 312    | CLUT @ `(0, 494)`                              |
| atlas 5  | `0x04E2B8`  | TIM  | ~33 312    | CLUT @ `(0, 495)`                              |
| atlas 6  | `0x0564DC`  | TIM  | ~23 332    | CLUT @ `(0, 497)` — truncated, last in pack    |

The bundled CLUTs are the pack's own palettes (the roster renders to match the
in-game characters — also the Baka Fighter PLAYER SELECT screen, which reuses
this pack). The streaming chunk type `0x09`
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
| `FUN_80020224` → `FUN_8001F05C` case 2 → `FUN_80026B4C` | Single descriptor-walk that installs PROT 0874 §0's 5 **field-form** TMDs into `DAT_8007C018[0..=4]` (the engine routes this through [`seed_global_tmd_pool_from_befect_data`](../../crates/engine-core/src/scene.rs)). The field caller is `FUN_801D6704` → `FUN_80020118` → `FUN_8001E890`. (The battle-form party meshes come from PROT 1204 via an uncaptured loader — see [§ Battle form](#battle-form--prot-1204). `FUN_800520F0` state `0xc` `tmd_register`s PROT `0x36a` into the *effect* window `[3..]`, not the party.) |
| `FUN_8001E890` | "DATA_FIELD player loader" — post-install, caps `entry[+0x08] = 10` for the three active-party slots at `DAT_8007C018[DAT_8007B824 + 0..2]`, then dispatches the per-character equipment-conditional patch to `FUN_8001EBEC`. |
| `FUN_8001EBEC` | Per-frame group-descriptor patch. Reads the equipment toggle byte and copies one of the two templates over the visible group descriptor. The full asm trace is decoded in [`ghidra/scripts/funcs/8001ebec.txt`](../../ghidra/scripts/funcs/8001ebec.txt). |

## CLI

```bash
# Field-form pack (PROT 0874 §0): list the five-slot shape + active-party templates.
asset character-pack extracted/PROT/0874_befect_data.BIN

# Battle-form pack (PROT 1204, also the Baka Fighter pack): list the five TMD2 chunks + seven character atlases.
asset battle-char-pack extracted/PROT/1204_other5.BIN

# Export one battle character TMD and one atlas TIM.
asset battle-char-pack extracted/PROT/1204_other5.BIN --slot 0 --out-tmd vahn_battle.tmd
asset battle-char-pack extracted/PROT/1204_other5.BIN --atlas 0 --out-tim vahn_atlas.tim

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
