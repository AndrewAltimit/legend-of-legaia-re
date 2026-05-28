# Player-character mesh pack

The five player-character TMDs the retail engine keeps resident across every
field scene at `DAT_8007C018[0..=4]`. They live in the head section of PROT
entry **0874** (`befect_data`). Implementation:
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
| 3 | `0x972C` |  3 |   6 488 | Auxiliary actor |
| 4 | `0xB084` |  2 |   1 048 | Auxiliary actor |

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

## Textures

The character TMDs reference texture pages and CLUTs the engine uploads from
**PROT 0876** (`player_data`), the streaming-format file with a VAB + a 256×256
TIM_LIST atlas + a small SEQ trailer. The atlas goes to VRAM `fb=(768, 0)`
with CLUT at `(0, 500)`; both blocks are pinned in
[`FIELD_SHARED_BLOCKS`](../subsystems/asset-loader.md#field_shared_blocks) so
they survive every field-scene transition without being re-uploaded. PROT 0874
itself carries **no character textures** — its remaining sections are the
effect 3D models (etmd.dat) and effect-texture TIMs (etim.dat), unrelated to
the player mesh.

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
| `FUN_80020224` → `FUN_8001F05C` case 2 → `FUN_80026B4C` | Single descriptor-walk that installs PROT 0874 §0's 5 TMDs into `DAT_8007C018[0..=4]` (the engine routes this through [`seed_global_tmd_pool_from_befect_data`](../../crates/engine-core/src/scene.rs)). The retail loader chain that calls FUN_8001F05C with PROT 0874 specifically is **not yet pinned** — open work item in [`world-map-overlay.md`](world-map-overlay.md). |
| `FUN_8001E890` | "DATA_FIELD player loader" — post-install, caps `entry[+0x08] = 10` for the three active-party slots at `DAT_8007C018[DAT_8007B824 + 0..2]`, then dispatches the per-character equipment-conditional patch to `FUN_8001EBEC`. |
| `FUN_8001EBEC` | Per-frame group-descriptor patch. Reads the equipment toggle byte and copies one of the two templates over the visible group descriptor. The full asm trace is decoded in [`ghidra/scripts/funcs/8001ebec.txt`](../../ghidra/scripts/funcs/8001ebec.txt). |

## CLI

```bash
# List the five-slot shape and active-party templates.
asset character-pack extracted/PROT/0874_befect_data.BIN

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
