# Player-character mesh packs

The player characters have **two distinct mesh packs**, one per game form:

- **Field form — PROT 0874 §0** (extraction label `befect_data`; retail-space
  the entry is the `player_data` define's `player.lzs`, see
  [`cdname.md` § numbering space](cdname.md#numbering-space)): the low-poly walk/talk models
  the engine keeps resident across every field scene at `DAT_8007C018[0..=4]`.
  Parser [`legaia_asset::character_pack`](../../crates/asset/src/character_pack.rs).
- **Battle form — assembled per character from the player battle files**
  (`data\battle\PLAYER1..4`, extraction 0863..0866 — see
  [`battle-data-pack.md`](battle-data-pack.md)): at battle setup the engine
  **builds** each party member's higher-detail TMD by splicing together the
  five equipment-selected sections of that character's file, and installs
  the result into `DAT_8007C018[0..=2]` (see
  [§ Battle form](#battle-form--assembled-from-the-player-files)).
  **PROT 1204** (`other5`) is a sibling pack carrying pre-assembled copies of
  the same characters with default equipment — it is what the **Baka
  Fighter** fist-fight minigame loads, and most default-section geometry is
  byte-shared between the two sources. Parser
  [`legaia_asset::battle_char_pack`](../../crates/asset/src/battle_char_pack.rs).

The field form is field-only; battle uses the battle form. (Two earlier
readings — "battle reuses the field pack" and "battle renders PROT 1204
directly" — are both superseded by the assembly chain in § Battle form; the
1204 attribution rested on the default-section geometry the two sources
share.)

## Contents

- [On-disc layout](#on-disc-layout) (field form, PROT 0874 §0)
- [TMD shape (per slot)](#tmd-shape-per-slot)
- [10-group cap + equipment-conditional swap](#10-group-cap--equipment-conditional-swap)
- [Textures (field form)](#textures-field-form)
  - [CLUT upload semantic (`FUN_800198e0`)](#clut-upload-semantic-fun_800198e0)
  - [Hybrid render (textured + untextured prims)](#hybrid-render-textured--untextured-prims)
- [Battle form — assembled from the player files](#battle-form--assembled-from-the-player-files)
  - [Assembly — object-local pieces posed by the character's own battle streams](#assembly--object-local-pieces-posed-by-the-characters-own-battle-streams)
  - [Battle render: load-time TSB/CBA relocation](#battle-render-load-time-tsbcba-relocation)
  - [Equipment groups (battle only)](#equipment-groups-battle-only)
  - [On-disc layout (PROT 1204)](#on-disc-layout-prot-1204)
- [Animation](#animation)
- [Readers (retail)](#readers-retail)
- [CLI](#cli)
- [See also](#see-also)

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
[texture atlas](#textures-field-form), not by mesh changes.

`legaia_asset::character_pack::equipment_swap::apply` is the clean-room
equivalent: given a slot's disc-form TMD bytes, a [`PatchSlot`], and the
character's equipment toggle byte, it returns the patched TMD buffer.

## Textures (field form)

The field-form character textures live in **PROT 0874 section 2** — the third
LZS descriptor of the `player.lzs` container (the "etim.dat" texture section),
parser [`legaia_asset::field_char_textures`](../../crates/asset/src/field_char_textures.rs).
They are **not** in extraction PROT 0876 (a VAB + empty TIM_LIST + SEQ stream
carrying neither the atlas nor the CLUTs, raw or LZS) — that entry was only
searched because its *filename label* says `player_data`; the retail
`player_data` define (876) names extraction 0874 itself under the
[−2 numbering correction](cdname.md#numbering-space).

`FUN_8001E890` (the field player loader) loads `player.lzs` (disc index `0x36c`,
the same 3-descriptor container the extractor labels PROT 0874) and LZS-decodes
all three sections (`piVar2[2..7]`): §0 → the 5-TMD mesh pack
([§ On-disc layout](#on-disc-layout)), §1 → effect / `vdf` models, and **§2 → a
[`pack`](pack.md) of eight asset chunks, each uploaded to VRAM via
`FUN_800198e0`.** The eight entries (byte-exact against a live field-scene VRAM
dump):

| entry | image `(x, y, w_words, h)` | CLUT `(x, y, colours)` | role |
|---:|---|---|---|
| 0 | `(448, 0, 64, 256)` | `(0, 473, 256)` | shared 256-colour page |
| 1 | `(832, 256, 20, 128)` | `(0, 478, 64)` | **Vahn** atlas + palettes cols 0..63 |
| 2 | `(852, 256, 20, 128)` | `(64, 478, 64)` | **Noa** atlas + palettes cols 64..127 |
| 3 | `(872, 256, 20, 128)` | `(128, 478, 64)` | **Gala** atlas + palettes cols 128..191 |
| 4 | `(320, 256, 64, 256)` | `(0, 475, 256)` | shared 256-colour page |
| 5 | `(384, 256, 64, 256)` | `(0, 475, 256)` | shared 256-colour page |
| 6 | `(880, 384, 16, 64)` | `(192, 478, 32)` | atlas extension (lower) |
| 7 | `(880, 448, 16, 64)` | `(224, 478, 32)` | atlas extension (lower) |

Entries 1/2/3 tile horizontally (`832 + 20 + 20 = 872`) to fill the 4bpp
texpage `(832, 256)` (`tsb 0x3D`) that every field-form character primitive
samples; their CLUTs occupy VRAM **row 478**, columns 0..191 (Vahn 0..63 / Noa
64..127 / Gala 128..191) — exactly the per-primitive CBA columns the meshes
carry (Vahn 0/16/32/48, Noa 64/80, Gala 128/144). The textures are
character-intrinsic and resident: byte-identical across every field scene, kept
across transitions by the [`FIELD_SHARED_BLOCKS`](../subsystems/asset-loader.md#field-shared-cdname-blocks)
residency, not re-uploaded per scene.

### CLUT upload semantic (`FUN_800198e0`)

Each entry is a standard PSX TIM (`magic 0x10`, `flags & 8` = has CLUT, 4bpp).
The image block is uploaded verbatim at its declared rect, but the CLUT block is
written as a **flat horizontal strip** — `LoadImage(rect = { x = clut_x, y =
clut_y, w = clut_w * clut_h, h = 1 })`, **not** the declared `clut_w × clut_h`
rectangle. So a CLUT header of `(0, 478, 16, 4)` lands as 64 colours at row 478
columns 0..63 (four 16-colour palettes side by side), which is why a single
character occupies several CBA *columns* of one row. STP (`| 0x8000` on non-zero
colours) is applied only when `_DAT_8007b998 != 0`; the field upload runs with
that flag **0**, so field CLUTs are bit-15-clear (the row-479 NPC CLUTs are
STP-set by a separate upload — see [`npc-palette.md`](npc-palette.md)).

Verified byte-exact: `legaia_asset::field_char_textures::parse` +
`upload_to_vram(stp = false)` reproduces the live field VRAM at every uploaded
rect (disc-gated `field_char_textures_real`, FNV-pinned). CLI:
`asset field-char-tex extracted/PROT/0874_befect_data.BIN`.

### Hybrid render (textured + untextured prims)

A field-form character mesh is **not** fully textured. Only ~⅓ of its
primitives are textured (`FT*`/`GT*` — the face, eyes, skin, and parts of the
clothing, sampling the atlas above); the rest are **untextured** flat / gouraud
prims (`F*`/`G*` — hair, vest, boots) that carry **per-vertex RGB in the TMD**,
not UVs. A texture-only renderer drops those (their `(cba, tsb)` is `(0, 0)`, so
they sample empty VRAM → transparent), leaving holes where the body should be.

The untextured-prim colour block sits at the **start of the prim**, immediately
before the vertex indices (the same slot a textured prim's texture block
occupies). Its length is the descriptor's `vertex_offset`
([`tmd.md`](tmd.md) / [`legaia_tmd::descriptor`](../../crates/tmd/src/descriptor.rs)):

- **Flat** (`F3`/`F4`): one RGB (`[r, g, b]` + a code byte) shared by every corner.
- **Gouraud** (`G3`/`G4`): one RGB per corner at a 4-byte stride
  (`colour[v] = bytes[v*4 .. v*4+3]`).

`legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid` returns the per-vertex
[`VertexShading`] (flat/gouraud RGB + a textured flag) parallel to the mesh, so a
renderer can sample VRAM for textured verts and use the stored colour for
untextured ones. The web viewer's [`/characters.html`](../../site/_content/characters.html)
Field form does exactly this (a `u_use_flat_colors`-gated branch in the shared
`TmdRenderer` fragment shader); a software-rasterizer replica of that path
renders all three party members in their real colours (blue-haired Vahn, auburn
green-eyed Noa, orange-haired Gala). Field TMDs are model-space, so the pieces
assemble without an ANM rest pose.

## Battle form — assembled from the player files

A real main-game battle does not render any disc TMD directly: at battle
setup the engine **assembles** each active party member's mesh from that
character's player battle file (`data\battle\PLAYER<n>`, extraction
0863..0866 — format: [`battle-data-pack.md`](battle-data-pack.md)), picking
one section per equipment slot by the character's **equipped item ids**, and
installs the merged TMD into `DAT_8007C018[0..=2]`.

**The assembly chain** (all static SCUS; decomps in `ghidra/scripts/funcs/`):

1. **`FUN_80052770`** — player-file streaming state machine. Case 1 opens
   `data\battle\PLAYER<n>` by raw TOC index `member_id + 0x360`
   (= extraction 863..866) and reads the head; **case 4 selects the five
   sections** by matching descriptor ids against the character record's
   equipped-item bytes (`+0x196..+0x19A`, `id = 0` defaults — see
   [`battle-data-pack.md` § Descriptor table](battle-data-pack.md#descriptor-table));
   later cases stream the selected sections into RAM (per-slot context
   `0x801C92F0 + slot*0x1C`).
2. **`FUN_80052FA0`** — per-character assembler. LZS-decodes `record[0]`
   (the palette chain) plus the five selected sections, then builds the
   merged TMD at `ctx + 0x50` (`ctx = *(0x801C9360 + slot*4)`): writes magic
   `0x80000002` at `blob+0x18`, `nobj = 0` at `blob+0x20`, and splices each
   section in with `FUN_800536BC`.
3. **`FUN_800536BC`** — the object splice (**the thing that grows `nobj`**).
   Appends the section's 7-word TMD object entries (relocating vertex /
   normal / primitive offsets into the merged pool), copies the section's
   data words, accumulates `nobj += section_nobj`, and writes one bone-id
   byte per object at `blob+0` from the section's attach list
   ([`battle-data-pack.md` § Decompressed slot layout](battle-data-pack.md#decompressed-slot-layout));
   objects past the attach list get tag `0xFF` / `0xFE` — these are the
   **equipment visual meshes** (weapon, Ra-Seru).
4. **`FUN_80053898`** — post-pass: retags `0xFF` → 200/201 and `0xFE` → 100+,
   records each extra's attach bone at `blob + nobj`, and selection-sorts the
   object table by tag so the extras land at indices `nobj-2`, `nobj-1`.
5. **`FUN_800513F0`** — battle init registers `blob + 0x18` into
   `DAT_8007C018[slot]` (the watchpoint-pinned `*(actor+0x50)+0x18` install
   below), runs the per-slot TSB/CBA rewrite (`FUN_80053a28` — see
   [§ Battle render](#battle-render-load-time-tsbcba-relocation)), and caches
   the two extras' vertex-pool pointers (battle ctx `+0x1030..0x103C`) +
   attach bones (`+0x23A/0x23B`).

So runtime `nobj` = skeleton bone count + equipment extras — Vahn's 15 bones
+ weapon + Ra-Seru = the observed 17. (**Not** `FUN_8001EBEC`, which only
toggles a pose transform on the *field* mesh — see
[10-group cap + equipment-conditional swap](#10-group-cap--equipment-conditional-swap).)

**Empirical provenance (byte-verified, full-party Gobu Gobu save).**
`DAT_8007C018[0] = 0x80165E38` = the assembler's `ctx+0x50` blob + 0x18
exactly; the assembled TMD reads `nobj = 17`, bone-id bytes `[0..14, 200,
201]`, attach array `[5, 8]` at `blob+17`. With Vahn equipped `[0x43 Hunter
Clothes, —, 0x22 Survival Knife, 0x01 Ra-Seru Meta, —]`, **every one of the
17 object vertex pools byte-matches a PLAYER1 section**, selectively: the
body objects match only the `id = 0x43` section, the weapon objects (bone 5
+ extra 200) only the `id = 0x22` section, the Ra-Seru extra the Meta-tier
sections, and the unequipped slots match their `id = 0` defaults.

**PROT 1204 (`other5`) is the Baka Fighter pack, not the battle source.**
The five equipped-variant objects (Hunter Clothes body, Survival Knife
piece + extra, the equipped Meta piece) appear **nowhere** in PROT 1204;
the remaining 12 default-section objects are byte-shared between the player
files and 1204 — which is what the earlier "battle party byte-matches
PROT 1204 (12/17)" partial-match table was actually seeing. Baka Fighter
loads 1204 explicitly (`overlay_baka_fighter` loads `data\field\other5.lzs`
+ PROT 1205/1206, debug string `"OTHER5 %d %d"`), and its bundled meshes are
the same characters with default equipment. The field-pack distinctness
finding stands (battle geometry is absent from the field pack 0874;
disc-gated `battle_char_pack_real::battle_pack_is_distinct_from_field_pack`).
Parser for the 1204 pack: `legaia_asset::battle_char_pack`.

### Assembly — object-local pieces posed by the character's own battle streams

Each battle TMD is a set of **object-local** pieces (head, torso, limbs), not a
pre-assembled mesh. Every object authors its vertices relative to its own joint
origin, and the engine places each piece with a **flat per-object transform** —
no skeleton hierarchy:

```text
v_world = R_bone · v_local + T_bone        (rotation about the object's local origin)
```

In a real battle the `(T, R)` come from the **character's own action-animation
streams in `record[0]` of the same player file** — the monster-format packed
keyframe stream `[u8 parts][u8 frames][9-byte TRS records]`
([`monster-animation.md`](monster-animation.md), shared decoder), reached
through the u32 action-offset table at the head of the decoded `record[0]`
(slot 0 = the idle loop) with the stream at **entry `+0xAC`** (the monster
archive's sibling entries keep theirs at `+0x8C`). `parts` equals the
**skeleton bone count** (15 Vahn / 16 Noa / 15 Gala / 17 Terra): channel `i`
drives assembled object `i` (post-sort, object index == bone tag), and the
equipment extras past `parts` ride their **attach bone's** channel via the
blob-header side tables — which is why the duplicate weapon/Ra-Seru pieces
coincide exactly with their attach piece instead of rendering as second
copies. Frame 0 of the idle is the combat-stance rest pose. There is **no
pivot/centroid subtraction** — placing a piece anywhere other than its local
origin pulls the joints apart. Live-pinned against a full-party capture: each
party render node's anim context (`node +0x4C`, consumed by `FUN_80047430` →
`FUN_8004AD80`) points its `+0x88` stream pointer at
`record0_image + action_table[0] + 0xAC`, and the whole stream byte-matches
the disc decode (`crates/engine-shell/tests/battle_party_pose_live.rs`); no
PROT 1203 record is resident in battle RAM.

The **PROT 1203** ANM bundle (`other5`, decoded per
[`anm.md`](anm.md#per-bone-frame-8-byte-encoding)) is the rig for the
**PROT 1204 pack's own object order** — the Baka Fighter / viewer
configuration, drawn by `FUN_8001B964` → `FUN_8001BE80` → `FUN_8002735C`
(same `Rz·Ry·Rx · v + T` composition, ANM bone `i` → model object `i`,
gated on `bone_count == nobj`). Its 30 records form per-character banks:
records 0–8 the 15-bone Vahn set, 9–17 the 16-bone Noa set, 18–26 the
15-bone Gala set, 27–29 a 10-bone simplified rig; the first record of each
bank is that character's idle, and its frame 0 agrees with the player-file
idle's frame 0 up to rotation quantisation (1203 stores `u8 << 4` angles,
the player streams full 12-bit). **1204's object order differs from the
assembled blob's sorted bone-tag order per character** (Vahn/Gala permute
their head/torso and limb-chain triples; Noa happens to coincide), so posing
the *assembled* mesh from 1203 — or the 1204 mesh from a player-file stream —
mis-sockets the rig.

The site `/characters.html` viewer poses the 1204 meshes from the 1203 banks
(the `BattleMeshView` path). The clean-room engine assembles the real thing:
`legaia-engine play-window` splices each party member from their player file
(`battle_char_assembly`, equipped ids from the roster record), applies the
registration-time TSB/CBA relocation (`relocate_tsb_cba`), decodes the same
file's idle stream (`idle_battle_animation`), expands it per object
(`expand_animation_for_objects` over the assembler's `anm_bones` channel
map — skeleton objects on their own bone, extras on their attach bone), poses
the rest mesh with `tmd_to_vram_mesh_posed_rot`, and loops the clip through
the same `MonsterAnimPlayer` the enemy meshes use. The PROT 1204 mesh stays
as the per-member fallback, posed from its 1203 bank idle with the identity
object→bone mapping (see
[`subsystems/battle.md` § Battle party meshes](../subsystems/battle.md#battle-party-meshes-assembled)).

**Loader provenance — pinned (write-watchpoint).** The party meshes are
installed by the generic registrar `tmd_register` (`FUN_80026B4C`, the store at
`0x80026BA8`) called from **two static SCUS functions** — not an overlay. A Lua
write-watchpoint on `DAT_8007C018[0..2]` across a live field→battle transition
(the auto-starting Rim Elm Queen Bee fight) catches all three installs at
`game_mode 0x15`, and the installed pointers byte-match the battle form (Vahn →
`0x80165F48`, the exact value a real battle save holds in `DAT_8007C018[0]`):

- **`FUN_800513F0`** (the battle scene-loader state handler) registers the
  active-party meshes in a `while (i < 3)` loop, **gated per slot** by the
  active-member-ID array `DAT_8007bd10[i]`:
  `if (DAT_8007bd10[i] != 0) tmd_register(*(actor + 0x50) + 0x18, 0)`, where
  `actor = *(0x801C9360 + i*4)` (the active-actor pointer table). The *same*
  function runs the party-palette decode `FUN_80052FA0` immediately before the
  loop. (Caught installing Vahn / slot 0; caller `ra = 0x8005148C`.)
- **`FUN_800542C8`** (the battle archive loader) registers each additional party
  member in a per-member loop bounded by the member count at `*(rec + 0x4a)`:
  `tmd_register(*(*rec + 4), 0)`. (Caught installing Noa + Gala / slots 1–2;
  caller `ra = 0x80054804`.)

`DAT_8007bd10[i]` is the **per-slot active-member ID** — `1`=Vahn, `2`=Noa,
`3`=Gala, `0`=empty — not a 0/1 flag. A Vahn-solo fight has `[1,0,0,0]`, so
`FUN_800513F0`'s loop installs only slot 0 and `FUN_800542C8` fills the rest
(this is what the live Queen Bee capture shows, since every PCSX-Redux library
save is `party_count=1`). A **full party has `[1,2,3,0]`**, so the loop's guard
passes for all three slots and `FUN_800513F0` installs Vahn/Noa/Gala itself —
confirmed against the full-party battle save states `mc1`/`mc6`/`mc7`
(`game_mode 0x15`, `party_count=3`, `DAT_8007bd10=[1,2,3,0]`, and
`DAT_8007C018[0..2] = 0x80165E38 / 0x8017A908 / 0x8018D550`, all three
battle-form meshes). So the `while (i<3)` loop is gated by the active-member
array, not hardcoded to the lead.

Both functions are reached **indirectly** (battle state-handler dispatch), which
is why a static cross-reference on `0x8007C018` finds no writer and the install
was long assumed to live in an overlay. The captured loader `FUN_800520F0`
separately `tmd_register`s PROT `0x36a` into the **effect/model window
`DAT_8007C018[3..]`** (`etmd.dat`) — that is the effect window, not the party
slots. Probe:
[`autorun_battle_party_mesh_install.lua`](../../scripts/pcsx-redux/autorun_battle_party_mesh_install.lua);
dumps [`ghidra/scripts/funcs/800513f0.txt`](../../ghidra/scripts/funcs/800513f0.txt)
+ [`800542c8.txt`](../../ghidra/scripts/funcs/800542c8.txt). (Sibling battle
files, raw indices → extraction entries: `etim.dat` `0x368` → 0870,
`efect.dat` `0x36b` → 0873, and the battle-type-conditional pair
`0x367`/`0x36d` → 0869/0875, both VAB-prefixed streaming files — see
[`effect.md` § Battle effect cluster](effect.md#battle-effect-cluster-befect_data).)

### Battle render: load-time TSB/CBA relocation

At battle entry the party setup does three things to each party character:
registers the assembled mesh (`flags` 0→1, object-table pointers fixed to
absolute RAM — see [§ Battle form](#battle-form--assembled-from-the-player-files)
for the assembly that produces it, including the two equipment extras), and —
crucially — **rewrites every primitive's TSB (texpage) and CBA (CLUT)
fields** to a packed per-party-slot runtime VRAM band. The TSB/CBA stored on disc are an **authoring
layout** (the one the Baka Fighter minigame renders directly, with the bundled
CLUTs); a normal battle relocates them. The remap is fixed and scene-independent
(byte-identical across a town battle and a world-map battle):

| slot | char | disc texpages (authoring) | runtime texpages | disc CBA rows | runtime CBA row |
|---|---|---|---|---|---|
| 0 | Vahn | (640,0) + (704,0) | **(512,256) + (576,256)** | 490 / 491 | **481** |
| 1 | Noa  | (640,256) + (704,256) | (640,256) + (704,256) | 492 / 493 | **482** |
| 2 | Gala | (512,0) + (576,0) | **(768,256) + (832,256)** | 494 / 495 | **483** |

The CBA **column is preserved** (`(cba & 0x3f) * 16`); only the page and row
change. Both disc CBA rows of a character collapse to a single runtime row, so
each character ends up with **one 256-colour palette** at its runtime row. The
party textures pack into the band `x ∈ [512, 896), y = 256` (one 128-px,
two-page slot each).

The rewrite itself is **`FUN_80053a28`**, called by the battle scene-loader
state `FUN_800513F0` per party slot right after registering the assembled
blob: it walks each object's primitive groups (gated on the group mode byte's
TME bit), and per textured prim sets the CBA word's CLUT row to `0x1E1 + slot`
(`& 0x803fffff | (0x1e1+slot) << 22` — column + high bit preserved) and the
TSB word's 5-bit texpage index to `0x18 + 2*slot` when the authoring page is
`0x15`, else `0x19 + 2*slot` (`& 0xffe0ffff` — ABR / depth bits preserved).
The **assembled player-file meshes** all author at texpages `0x15`/`0x16` =
`(320, 256)`/`(384, 256)` and CLUT row 480, so on them the pass is a uniform
`+3` texpage / `+0x40` CLUT-id rewrite — exactly the residual delta between
the disc assembly and the live registered blob. Clean-room port:
[`legaia_asset::battle_char_assembly::relocate_tsb_cba`](../../crates/asset/src/battle_char_assembly.rs)
(dump [`ghidra/scripts/funcs/80053a28.txt`](../../ghidra/scripts/funcs/80053a28.txt);
disc-gated `battle_char_assembly_real::relocates_each_character_into_its_runtime_band`).

There is no involvement of the `0x8007BEC0` texpage→row table for party
characters — that table (`FUN_800198E0`: `table[image_texpage] = clut_y`) is the
*scene/background* renderer's. (Earlier readings of this format claimed "nominal
CBA, no relocation, palette is scene VRAM residue at rows 490..497" — that is
**falsified**. Rows 490..497 hold *scene environment* palette, shared between a
scene's field and battle modes, which is why field and battle matched there; the
party palette is at rows 481..483 and is uploaded by the battle loader, not
inherited as residue. Both prior errors came from reading the disc mesh's
authoring TSB/CBA, or from a world-map save whose authoring pages happen to hold
terrain.)

**Textures** come from the **player files themselves**: the equipped
sections' post-TMD texture pools + the two `record[0]` image blocks, each
LoadImaged into the band at a static per-section rect
(`SCUS_942.54` table `0x800775B8`, banded by the party ordinal) — the
placement is fully pinned and reproduces live battle VRAM at 99.7–100 % per
member; see
[`battle-data-pack.md` § Texture-pool VRAM placement](battle-data-pack.md#texture-pool-vram-placement).
The 1204 atlases byte-match the band only at **73–98 %** (they carry the
default-equipment texels; the shortfall is the equipped-variant texels), so
they serve as the engine's fallback approximation, not the source.

**Palette** is a resident party-palette block in main RAM that the loader DMAs
to VRAM rows 481/482/483. In a clean full-party battle save the blocks are
contiguous at **`0x800ebee8` (Vahn) / `0x800ec0c8` (Noa) / `0x800ec2a8` (Gala)**,
a fixed **`0x1E0` (480-byte) stride** — exactly **15 × 16-colour sub-CLUTs per
character, one per disc mesh object**; the per-object CBA columns read back off
the runtime TMD land at the scattered columns of that character's row. It is
**battle-allocated** (the same RAM address holds unrelated data in a field save)
and is **not the field character palette** (a set test puts only 10 of Vahn's
130 battle-novel colours — and **0** of Noa's / Gala's — in any field-pack CLUT).

The palette is **produced fresh at battle load** — it is absent from main RAM in
the pre-battle field saves (name-entry, standing-in-front-of-Tetsu, and the
load-initiating frame all miss) and present as a **single** copy only once the
battle is up, byte-identical between the Tetsu tutorial fight and the Drake-castle
fight (so it is **character-intrinsic**, deterministic per character). The
work-arena holding it is `memset`-zeroed at load by the `sw $zero` loop at SCUS
`0x80055F14` (`base = *(0x8007BD3C)`, `0x1e8d` words), then sparsely filled — the
palette sits at `arena_base + 0x4048` as an isolated non-zero island.

**It is built by a CLUT-copy routine that OR-sets the STP bit.** A write-watchpoint
on `0x800EBEE8` (`scripts/pcsx-redux/autorun_battle_palette_writer.lua`, run on a
clean Tetsu fight) pins the assembler: **`FUN_80053B9C`** (per-colour store at
`0x80053C6C`: `sh a0, 0x894(v0)`). It reads a source CLUT struct
`[u16 base][u16 count][count × BGR555]` at a pointer `s0`, and for each colour
copies it into the per-character palette block at
`dst = arena + slot*0x1E0 + (base+idx)*2` while **`OR`-ing in `0xFFFF8000`
(bit 15, the PSX semi-transparency / STP flag) on every non-zero colour**
(`or vX, vY, 0xFFFF8000`, written back in place). So the **runtime** palette has
bit 15 set (`0x9D40…`); the **disc-stored source has bit 15 clear** (`0x1D40…`).
The source pointer derives from the battle overlay context:
`s0 = *(*(0x801C92F0) + 8) + per-char-offset`, into a transient buffer (the
`0x800Dxxxx` region, freed after the copy).

**The source CLUTs are LZS-compressed inside the PLAYER file — SOLVED.** A second
write-watchpoint, on the source struct header `0x800D6C98`, shows it is written by
`FUN_8001A55C` (the LZS decoder). The loaded `data\battle\PLAYER1` buffer is
**extraction PROT entry `0863`** (`edstati3` filename label = the
[+2 label shift](cdname.md#numbering-space); raw TOC index `0x361`). Early
`lzs-decode find` probes located the streams through entry `0861`'s
*extended* window — the 1-sector stub entries `0859..0862` precede the true
file, so `0861`'s over-read copy holds the same record at window offset
`0x1000`. Running the full
`FUN_80052FA0` decode+assembly, the decompressed records hold the party CLUT
structs `[u16 base][u16 count][BGR555]`,
and running the full `FUN_80052FA0` decode+assembly then applying the runtime
STP-set (`colour |= 0x8000` on non-zero) reproduces the live Vahn battle palette
**byte-exact, all 3 bands** (no residual "equipment-patch" colours — that earlier
3-colour discrepancy was the budget-less scratch decoder, since corrected). Every
earlier byte-search missed only because it used the bit-15-**set** runtime needle
(`40 9d 70 90…`) instead of the disc's bit-15-**clear** form (`40 1d 70 10…`). The
full chain: PLAYER-file raw-load → `FUN_8001A55C` LZS-decompress (→ CLUT structs,
bit-15-clear) → `FUN_80053B9C` per-CLUT copy with STP-set (→ palette block
`arena + slot*0x1E0`) → DMA to VRAM rows 481/482/483.

> **Retraction.** An earlier reading ("LZS-decompressed from the `town0c` scene
> bundle at `0x23430`") was wrong: that write-watchpoint caught the **scene
> bundle's** decompression into the *shared* work-arena (its `0x800ebee8` value
> `0x7965481F` ≠ the Vahn palette). The real source is the `PLAYER1` file
> (extraction PROT `0863`), above.

**Per-character structure (Exec-BP on `FUN_80053B9C`, `autorun_clut_copy_calls.lua`).**
The copy routine is called **once per CLUT struct, several times per character**,
with `a3 = slot` → VRAM row `481 + slot` and `a0` = the source struct. For Vahn
it fires for **three** structs — `base=0x00 count=0x20`, `base=0x40 count=0x30`,
`base=0x70 count=0x20` (colours `0..0x8F`) — plus two `count=0` no-ops. The CLUTs
ride *inside* `record[0]` and the sub-records (each a trailing CLUT at the record's
own `+adv`); the sub-records are scattered at descriptor-driven offsets within the
player file (Vahn: `0x1C000 / 0x28800 / 0x66000 / 0x85800 / 0xA2000`). The parser
resolves them from the descriptor table — see the on-disc layout below.

**The parser/loader is `FUN_80052770` + `FUN_80052FA0` (static trace).**
`FUN_80052770` sets `_DAT_801c92f0` to the asset-table base, points each
character's 28-byte entry at the player files **`data\battle\PLAYER1..4`**
(Vahn/Noa/Gala/Terra), and loads each via the disc resolver at index
`char_id + 0x360` (`FUN_8003e8a8`). The loaded PLAYER1 buffer **byte-matches
extraction entry `0863`** (raw index `0x361` − 2; the historical `0861`
attribution matched the same bytes through that entry's over-read window). `FUN_80052FA0` then LZS-decodes `record[0]` (`len@+0xC`,
`data@+0x10`) into a `0x19000` work buffer, decodes 5 sub-records into it at
advancing offsets, and STP-copies CLUTs from offsets *within* the decoded buffer
(`buf + *(record[0]+4)` / `+8`, plus each flagged sub-record) to VRAM row
`481+slot` via `FUN_80053B9C`. So the CLUTs are embedded in the decoded character
records.

**Extraction status — SOLVED, byte-exact.** Running `FUN_80052FA0`'s
decode+assembly *as a unit* (not a per-stream extract) reproduces the live battle
VRAM exactly. The earlier "sub-record decode diverges past ~`0x1C00`" was a bug in
a throwaway scratch decoder, **not** a data problem: `FUN_8001A55C`'s first
argument is an **output-byte budget** (decremented once per literal *and* once per
match-copied byte; loop runs `while budget > 0`). A decoder that ignores the
budget runs off the stream into the next record. `legaia_lzs::decompress` already
honors this (`while out.len() < size`), so the port is just one
`decompress(stream, budget)` per record. Clean-room parser:
[`legaia_asset::battle_char_palette`].

**Where the player files load from (traced).** Each party member's CLUTs live in
its `data\battle\PLAYERn` file. The loader (`FUN_80052770`) opens it through the
dual-mode wrapper `FUN_800558fc(path, …, char+0x360)`: the retail ISO9660 branch
`FUN_800608f0` is a **`trap` stub** on this build, so it always takes the debug
branch → `FUN_8003e8a8(char+0x360)`, which reads `toc[idx+2]` from the in-RAM PROT
TOC (`0x801C70F0`) as a **sector offset into `PROT.DAT`** (the disc filesystem
itself holds only `SYSTEM.CNF`, `SCUS_942.54`, `PROT.DAT`, `DMY.DAT`,
`CDNAME.TXT`, `MOV/`, `XA/` — there is no `DATA\` tree). The four player files are
contiguous in `PROT.DAT`:

| Player | `idx = char+0x360` | `PROT.DAT` offset | size |
|---|---|---|---|
| Vahn  | `0x361` | `0x36E8000` | 338 sec |
| Noa   | `0x362` | `0x3791000` | 303 sec |
| Gala  | `0x363` | `0x3828800` | 222 sec (`0x6F000`) |
| Terra | `0x364` | `0x3897800` | 47 sec |

The TOC start offsets of extraction entries `0863`/`0864`/`0865`/`0866` equal
those four player-file offsets exactly, so the parser reads them by extraction
index. (The historical Vahn label `0861` was the over-read window of a
preceding 1-sector stub entry, not the file's own slot.)

**On-disc record layout (self-describing relative to `record[0]`):** (the
whole-file container — header words, descriptor table, TMD slot region — is
documented in [`battle-data-pack.md`](battle-data-pack.md))

```text
rec0+0x00  u32  desc_off    descriptor-table offset (rec0-relative)
rec0+0x04  u32  clut_a_off  offset of CLUT A within record[0]'s DECODED output
rec0+0x08  u32  clut_b_off  offset of CLUT B
rec0+0x0C  u32  budget      record[0] decoded size; LZS stream begins at +0x10
desc_off: 12-byte entries [u32 id, u32 running_a, u32 size]; the table runs while
          `a[i+1] == a[i] + size[i]`; `id == 0` marks a section boundary.
```

On disc the five sub-records are **scattered**. Their section base is:

```text
sec_base = rec0 + align_up(recbase - rec0, 0x2000)   (recbase = end of the table)
```

The `0x2000` alignment is **rec0-relative** — a `0x1000` alignment gives the same
answer for Vahn (`0x587C → 0x6000`) and Noa (`0x781C → 0x8000`) but misplaces Gala
(`0x6E6C → 0x7000`, where his sub region is zero-padded; the real base is `0x8000`).
Each sub-record is `[u32 budget][LZS stream]`. At load time `FUN_80052770` streams
the five into one buffer at a **`0x2000` stride** (the runtime layout the decode
loop's `ra = 0x80053130` callsite walks via `a0 = *a1 (budget); FUN_8001A55C(a0,
a1+4, dst)`), but the parser reconstructs the scattered disc offsets directly — no
capture needed. See [`legaia_asset::battle_char_palette`] + the disc-gated
`battle_char_palette_real` / `battle_palette_overlay` tests.

**Assembly.** Decode `record[0]` at work offset 0; read CLUT A @`clut_a_off` and
CLUT B @`clut_b_off` *immediately* (the sub-records overwrite the region from
`clut_a_off` on). Set `cur = clut_a_off`; for each of the 5 sub-records, decode it
at `cur`, take `adv = u32[cur+0x0C]` and `flag = u16[cur+0x12]`, and if `flag != 0`
its trailing CLUT is at `cur + adv`; then `cur += adv`. A CLUT struct is `[u16
base][u16 count][count × BGR555]`; upload sets **bit 15 on every non-zero colour**.
For Vahn this yields **3 bands**: `base=0x00 count=0x20` = `record[0]`'s CLUT B,
`0x40 count=0x30` = sub#0, `0x70 count=0x20` = sub#4 (CLUT A and sub#1 are
`count=0` no-ops). Those 3 cover exactly the CBA columns Vahn's disc `1204` mesh
samples (`0,16,64,80,112,128`); the runtime mesh's extra columns
(`176/192/208/224`) belong to the `+2` equipment groups it doesn't have.

**Equipment variants + the general parser.** The descriptor isn't a fixed list:
each section ships **one CLUT per equipment id** *plus* an `id == 0` **separator**
(the unequipped default), and `FUN_80052770` case 4 picks, per section, an
equipment-id-matched entry or the separator. So there's no single "the" palette —
it depends on equipment. Two parser entry points in
[`legaia_asset::battle_char_palette`]:

- `parse_record` reproduces one *specific* configuration via the fixed
  `sec_base + a[post-separator head]` / `rec0 + total` offsets. Exact for Vahn's
  tutorial-equipped state (byte-exact vs the live capture), but a character with
  more equipment variants overflows the `0x19000` work buffer.
- `collect_palette` is the equipment-robust path: gather `record[0]`'s CLUT A/B +
  each section **separator**'s flagged trailing CLUT + the final record, then keep
  only the bands whose base is a column the character's battle mesh samples
  (`(cba & 0x3F) * 16`). The mesh-column filter resolves which variant belongs to
  the character (Vahn samples col `0x70` not `0x90`, so his `0x70` band is kept).

**All three party palettes decode from the disc** (`FUN_80052FA0` ported as a
unit), validated against a full-party battle VRAM capture (mednafen mc1/mc7/mc9,
rows 481/482/483 all populated): **Vahn (PROT `0863`) byte-exact, Noa (PROT `0864`)
~98%, Gala (PROT `0865`) 100%** (the 1-2 % misses on Noa are equipment patches in
the late-game reference). Each is overlaid onto the VRAM rows its mesh's CBA
samples (Vahn 490/491, Noa 492/493, Gala 494/495 — runtime collapses each pair to
one row `481+slot`). Party order / row mapping confirmed by reading the mc7 char
names (ASCII at `0x80084708 + n*0x414 + 0x2A7` = Vahn/Noa/Gala/Terra).

**How the relocation was pinned (reproduction):** read `DAT_8007C018[slot]` from
a clean battle save → dump the runtime TMD (it has `flags=1`, absolute object
pointers); convert each object pointer `p → (p − base − 12)` and clear `flags`,
then walk it as a normal Legaia TMD — the resulting prims carry the *relocated*
TSB/CBA. Sampling the save's VRAM with those prims renders the correct
characters (blue-haired Vahn, pink-haired Noa, brown-haired Gala). The disc mesh
walked as-is samples the authoring pages and renders incoherently.

> Use a **clean** battle capture as ground truth (command-menu / Begin-menu, no
> effect animation). Mid-battle captures paused during an effect can overwrite
> VRAM regions and read back garbage.

### Equipment groups (battle only)

A live battle character carries +2 `nobj` over the 1204 disc form (Vahn
15→17), so the in-battle silhouette differs from both the unarmed disc form
and the Baka Fighter form (a fist-fight, which keeps the unarmed mesh). The
equipped-weapon/gear geometry behind that `+2` is **not present in the 1204
TMD** — it is the per-equipment-id section of the character's player battle
file, spliced in by the assembler (resolved; see
[§ Battle form](#battle-form--assembled-from-the-player-files)).

`FUN_8001EBEC` is **not** that loader, and it does **not** grow `nobj`. The
decomp ([`8001ebec.txt`](../../ghidra/scripts/funcs/8001ebec.txt); see also
[§ 10-group cap + equipment-conditional swap](#10-group-cap--equipment-conditional-swap))
shows it loops over the three party slots and, per a per-character
equipment-condition byte, copies a **28-byte (7 × u32) transform** from one of
two *in-TMD* group templates (group 10 at `TMD+0x124` for "equipped" ↔ group 11
at `TMD+0x140` for "unequipped") **into an existing visible group descriptor**
(`group[selector] = base + 0xC + sel*0x1C`, `sel ∈ {0,3,5}`). It writes seven
words (`puVar1[0..6]`) and **never touches the object/group count** — a binary
pose toggle on geometry already in the field mesh, not an object add and not
an external-mesh upload. The mechanism that actually raises the battle object
count is the player-file section splice `FUN_800536BC` (see
[§ Battle form](#battle-form--assembled-from-the-player-files)); the earlier
"the equipment swap `FUN_8001EBEC` sources it / adds the groups" framing
conflated the two.

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

The bundled CLUTs (declared at rows 490..497) are the pack's **authoring
palette** — what the Baka Fighter minigame renders with directly. A normal
battle does **not** use them: it relocates the mesh to rows 481..483 and uploads
a different, battle-allocated party palette there (see
[§ Battle render: load-time TSB/CBA relocation](#battle-render-load-time-tsbcba-relocation)).
The streaming chunk type `0x09`
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
| `FUN_80020224` → `FUN_8001F05C` case 2 → `FUN_80026B4C` | Single descriptor-walk that installs PROT 0874 §0's 5 **field-form** TMDs into `DAT_8007C018[0..=4]` (the engine routes this through [`seed_global_tmd_pool_from_befect_data`](../../crates/engine-core/src/scene.rs)). The field caller is `FUN_801D6704` → `FUN_80020118` → `FUN_8001E890`. |
| `FUN_800513F0` → `FUN_80026B4C` | **Battle-form party install (lead/active actors).** Battle scene-loader state handler; `while (i<3)` loop registering `*(actor+0x50)+0x18` (`actor = *(0x801C9360 + i*4)`) into `DAT_8007C018[0..]`, after the party-palette decode `FUN_80052FA0`. Pinned by a `DAT_8007C018[0..2]` write-watchpoint at battle entry — full trace in [§ Battle form, Loader provenance](#assembly--object-local-pieces-posed-by-the-characters-own-battle-streams). |
| `FUN_800542C8` → `FUN_80026B4C` | **Battle-form party install (additional members).** Battle archive loader; per-member loop bounded by `*(rec+0x4a)`, registering `*(*rec+4)`. Dispatched indirectly (no static `0x8007C018` xref). `FUN_800520F0` state `0xc` separately `tmd_register`s PROT `0x36a` into the *effect* window `[3..]`, not the party. |
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
