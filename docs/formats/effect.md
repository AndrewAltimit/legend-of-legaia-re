# Effect bundles

Two distinct formats share the "effect" name - the on-disc bundle (magic `0x02018B0C`) and the runtime 2-pack wrapper used by `data\battle\efect.dat`. Only one PROT entry uses the on-disc form; the runtime wrapper is what battle code actually consumes.

## Contents

- [On-disc effect bundle (magic `0x02018B0C`)](#on-disc-effect-bundle-magic-0x02018b0c)
- [Runtime effect format - 2-pack wrapper](#runtime-effect-format---2-pack-wrapper)
  - [Battle effect cluster (`befect_data`)](#battle-effect-cluster-befect_data)
  - [Entry contents (byte-checked)](#entry-contents-byte-checked)
  - [Effect texels in VRAM - pixel-verified](#effect-texels-in-vram---pixel-verified)
  - [Consumer cluster](#consumer-cluster)
  - [Runtime pool layout](#runtime-pool-layout-_dat_8007bd30-5008-bytes-total)
  - [Side-band streaming-effect handler](#side-band-streaming-effect-handler)
  - [Open questions](#open-questions)
- [Field-pack format (magic `0x01059B84`)](#field-pack-format-magic-0x01059b84)
- [See also](#see-also)

## On-disc effect bundle (magic `0x02018B0C`)

A bulk scan over the PROT corpus finds this format in exactly one entry: `0000_init_data` (engine bootstrap data).

### Header (12 bytes)

```
+0   u32 LE = 0x02018B0C       ; magic
+4   u32 LE = 0x0000001D       ; HEADER_A - TMD slot count (1 master + 28 sub = 29)
+8   u32 LE = 0x0000001E       ; HEADER_B - HEADER_A + 1 (sentinel/terminator)
```

Both header words are constant within the format. **HEADER_A = 29 = the maximum TMD count the format reserves space for** - 1 master TMD plus 0..28 sub-effect TMDs.

### Offset table (112 bytes after the header)

28 strictly-ascending u32 LE values:

```
0x17F4, 0x1832, 0x198F, 0x1B9B, 0x1D75, 0x1EFD, 0x20BB, 0x224B,
0x2438, 0x260B, 0x26DD, 0x27C3, 0x2982, 0x2AA1, 0x2C44, 0x2D9F,
0x2F77, 0x30E6, 0x3300, 0x34BE, 0x36BE, 0x3805, 0x39B6, 0x3AB4,
0x3C4A, 0x3E23, 0x3F78, 0x404D
```

Slot sizes derive from `offset[i+1] - offset[i]`. The 28th slot's size depends on the post-table asset-region layout.

### Asset region (after the table)

The asset region begins with a **master Legaia TMD** at `assets_start`. The single observed master carries `1 object, 382 verts, 760 normals, 760 primitives`. The 28 schema offsets do not byte-align with sub-TMD file positions; they likely index into an abstract runtime buffer whose semantics live in a consumer that hasn't been reached.

### API

```rust
use legaia_asset::effect_bundle;
if let Some(eb) = effect_bundle::detect(&buf) {
    println!("magic @ 0x{:X}, asset region 0x{:X}..0x{:X}",
             eb.magic_offset, eb.assets_start, eb.file_size);
    for (i, slot) in eb.slots.iter().enumerate() {
        let size = slot.size.map(|s| format!("{}", s)).unwrap_or("?".into());
        println!("  slot[{}] off=0x{:X} size={}", i, slot.offset, size);
    }
}
```

Implementation: `crates/asset/src/effect_bundle.rs`.

## Runtime effect format - 2-pack wrapper

The format `data\battle\efect.dat` (PROT entry 873) actually uses at runtime. Cross-validated against a live battle save state - the post-init buffer at RAM `_DAT_8007BD5C = 0x800E425C` is byte-identical to PROT.DAT bytes at sector `0x9086`.

### Buffer layout

```
+0    u32   pack0_offset    ← fixed up to absolute pointer on first init
+4    u32   pack1_offset    ← fixed up to absolute pointer on first init
+8    [N × 8-byte sprite atlas entries]
...   [pack0]  u32 count, u32 entry_offsets[count]   ← packs of frame-batched anim entries
...   [pack1]  u32 count, u32 entry_offsets[count]   ← packs of effect scripts
```

Each **pack0 entry** is a frame-batch animation record:

```
+0   u8 frame_count     ← number of 6-byte frames
+1   u8 flags
+2   [N × 6-byte frame records]
   each frame:
    +0  u8  sprite_atlas_index   (indexes the inline 8-byte atlas at buffer+8)
    +1..+5 (timing / dir bits)
```

Each **pack1 entry** is an effect-ID script:

```
+0   u8   child_count         ← N - number of child sprites to spawn
+1   u8   flags               (bit 0 = use random child distribution)
+2   u16  spread              ← half-range modulo for random child position (signed 8.8 fixed)
+4   [N × 14-byte child sprite descriptors]
   each descriptor (retail offsets from the per-frame walker):
    +0x00  u16  sprite_id     ← indexes pack0; copied to master slot on spawn
    +0x02  i16  width         ← half-width of random X distribution (8.8 fixed)
    +0x04  u16  anim_flags    ← animation / shading flags read by the per-frame walker
    +0x06  i16  depth         ← half-width of random Z distribution (8.8 fixed)
    +0x08  u8[6] tail         ← animation curves / sound-id / timing (per-frame walker only)
```

The retail random-distribution loop (`FUN_801E0088` pass 1) reads only `+0x02` (width) and `+0x06` (depth) per child - those two govern where a child sprite spawns relative to the effect origin. `anim_flags` and `tail` are consumed later by the per-frame walker when advancing a live child slot's animation state.

A live `0873_befect_data` sample carries 14 entries in pack0 and 33 entries in pack1. The pack0/pack1 offset tables hold **absolute file offsets** (not the `word*4` offsets of `asset::pack`). Parser: `legaia_engine_vm::effect_vm::EffectCatalog::from_efect_dat_bytes`.

Inline sprite atlas entries (between `buffer+8` and pack0) are 8 bytes each. The layout is pinned from the consumer (`FUN_801E0088` pass 2, the sprite-emit block ~`0x801E0840`), which reads them byte-wise to build each child's GPU sprite primitive:

```
+0  u8  u       ; source texel U within the texture page
+1  u8  v       ; source texel V
+2  u8  w       ; sprite width in texels
+3  u8  h       ; sprite height
+4  u16 clut    ; CLUT (CBA) id  -> primitive CLUT field (POLY_FT4 word3 high)
+6  u8  tpage   ; texture-page descriptor byte -> primitive tpage (word5 high)
+7  u8  ?        ; unknown / reserved
```

The texel rectangle is `(u, v)..(u+w-1, v+h-1)`.

**Field order note.** The emit at ~`0x801E0980` reads the atlas fields in this order:

- the **u16 at `+4`** copies into the primitive's **CLUT field** (`sh atlas[4..5] -> prim+0xe`);
- the **byte at `+6`** copies into the **tpage field** (`sh atlas[6] -> prim+0x16`).

This is the reverse of an earlier reading. Consequences:

- The oft-cited `0x7680` is the **CLUT**, not the tpage: as a CBA it decodes to fb `(0, 474)`, an effect-CLUT row (PROT 870 TIM0's CLUT), *not* page `(0,0)`.
- The real tpage is the single byte at `+6` (e.g. `0x25` = page `(320,0)`, 4bpp).
- So effect sprites sample the loaded effect-texture pages (PROT 870 / `etim`, `fb_x≥320`) with effect-band CLUTs - confirmed against a melee hit-spark battle capture (the live impact quads sample exactly pages `(320,0)`/`(448,0)` with CLUT rows 473..480).

The pixels live in VRAM, blitted at battle load; the atlas carries only the VRAM coordinates.

<a id="battle-effect-cluster-befect_data-cdname-872"></a>

### Battle effect cluster (`befect_data`)

`efect.dat` is one of four dev-named files in the retail `befect_data` CDNAME block (defines `872..875` → **extraction entries 870..873** under the [−2 numbering correction](cdname.md#numbering-space)). The battle scene loader `FUN_800520F0` pulls them as a sequential state machine (sub-state byte at `gp+0xa59`); each state loads one file through the dual-mode loader - retail opens the dev-path string (a trap stub on this build), so the load resolves through the **raw TOC index** (`FUN_8003e8a8`; raw = extraction + 2). The full case → index → entry map, read off the decomp (`ghidra/scripts/funcs/800520f0.txt`) and byte-checked per entry below:

| Loader state / case | Dev-path string | Raw index | Extraction entry | Content |
|---|---|---|---|---|
| case `0x8` | `h:\prot\battle\etim.dat` (`0x80015358`) | `0x368` | **0870** | Effect texture pages (3-TIM pack). |
| case `0xb` | `h:\prot\battle\etmd.dat` (`0x80015370`) | `0x369` | **0871** | Effect 3D-model library (30-TMD `asset::pack`; registered via `FUN_80026b4c`). |
| case `0xb` | `h:\prot\battle\vdf.dat` (`0x80015388`) | `0x36A` | **0872** | VDF buffer (32-entry offset pack; asset type `0x07`, appended via `FUN_8001fbcc`). |
| case `0xc` | `data\battle\efect.dat` (`0x800153a0`) | `0x36B` | **0873** | The 2-pack above; initialised by `FUN_801DE914` (offset fixup only). |
| case `0x4` | - (battle-type-conditional) | `0x367` / `0x36D` | 0869 / 0875 | Streaming files `[type-0 VAB chunk][type-3 chunk]`; `0x36D` when `DAT_8007bd11 == 4`. Outside the befect block (retail `sound_data` / `sound_data2`). |

So the dev names map 1:1 onto the four retail `befect_data` slots. Earlier readings that applied the raw indices as extraction indices ("etim = entry 872", "etmd loads at `0x367`", "PROT 870 = index `0x366`, blitted by a separate unpinned site") are superseded - extraction 0870 **is** the case-`0x8` `etim.dat` load. The extraction filename labels for 0870/0871 say `sound_data` (the +2 label shift), and extraction **0874** - which an earlier cluster analysis read as "etmd/vdf/etim LZS sections" - is the retail `player_data` file (`player.lzs`): its three LZS sections are the **field character mesh pack / auxiliary models / field-character textures** (see [`character-mesh.md`](character-mesh.md)).

#### Entry contents (byte-checked)

| Extraction entry | Dev name | Structure (verified against the extracted bytes) |
|---|---|---|
| 0870 | `etim.dat` | 16-byte pack header `[u32 3][u32 word_offsets 0x4, 0x208C, 0x4114]` (×4 → `0x10`, `0x8230`, `0x10450`); three 64×256 4bpp TIMs targeting VRAM `(320,0)` / `(384,0)` / `(448,0)`, CLUTs `(0,474)` / `(0,475)` / `(0,476)`. The first TIM's flags word is `0x00010008` (a bit-16 quirk; strict TIM parsers reject it). |
| 0871 | `etmd.dat` | `asset::pack`, `word[0] = 30`, 30 Legaia-TMD magics at the declared word offsets. The body spans the entry's *extended* footprint (the indexed view truncates it mid-table). |
| 0872 | `vdf.dat` | Offset pack, 32 strictly-ascending entries (~96 B records). |
| 0873 | `efect.dat` | The 2-pack: 144 atlas entries, `pack0@0x488` (14 anim batches), `pack1@0x900` (33 scripts). Byte-identical to the live post-init buffer at `_DAT_8007BD5C` (PROT.DAT sector `0x9086` = this entry's start). |

The per-entry PROT extractor over-reads here (neighbouring entries' extended footprints overlap), so naive `.BIN` files bleed into their neighbours. `asset befect-cluster PROT.DAT --cdname CDNAME.TXT [--out DIR]` footprint-bounds and classifies a window of entries; note it resolves the CDNAME symbol in define-number space, so its "befect" window is extraction 872..875 - retail `vdf.dat`, `efect.dat`, `player.lzs` (= `player_data`), and a `sound_data2` VAB stream. Its LZS-section expansion of entry 874 is the **player.lzs** split, not the battle effect files.

#### Effect texels in VRAM - pixel-verified

`FUN_800198e0` is the general packed-image → VRAM uploader the loader uses (loader state `9` walks a pack and calls it per entry): it reads a per-chunk tag/flag word, builds a PSX `RECT`, and calls `FUN_800583c8` = `LoadImage` (`0x800156d4`) to DMA pixels into VRAM, maintaining a CLUT cache at `0x8007BEC0`. (Same routine the title / menu / save overlays and the type-`0x01` CLUT walker `FUN_8001fe70` use.)

**Two texture pools serve battle effects** (an earlier reading conflated them under one "etim" label):

1. **`etim.dat` = extraction 0870** (the 3-TIM pack above). **Battle-only**: byte-verified pixel-exact in VRAM against every stable Rim Elm battle capture (command-menu / submenu / pre- and post-Seru-capture frames match 100%; a still-loading frame matches partially - the mid-DMA snapshot). Its pages sit at `fb_y=0` in the same VRAM columns the field uses for town stage textures, so the town01 *field* captures hold unrelated texels there. The engine uploads it on **battle entry** (`engine-core::scene::upload_flame_atlas_into_vram`, into a throwaway VRAM copy that battle exit discards, so field VRAM is never clobbered).

2. **The `player_data` §2 band (extraction 0874 §2)** - eight TIMs at `fb_y=256+`:
   the three field-character atlases at `(832,256)`/`(852,256)`/`(872,256)`, two
   shared 256-colour pages at `(320,256)`/`(384,256)`, two 16×64 extension tiles
   at `(880,384)`/`(880,448)`, CLUTs in rows 473/475/478 (full table:
   [`character-mesh.md` § Textures](character-mesh.md#textures-field-form),
   byte-exact vs a live field VRAM dump). These were previously mislabeled
   "etim" here. They are **field-resident** (uploaded at field entry, kept
   through battle), which is why mid-cast battle captures byte-match them:
   during a Gimard *Tail Fire* cast, five of the blocks - `(832,256)`,
   `(852,256)`, `(872,256)`, `(880,384)`, `(880,448)` - match VRAM at their
   rect-header targets, and the `(320,256)`/`(384,256)` pages match a `town01`
   field capture 256 rows byte-exact. The engine uploads them at scene entry
   (`scene::upload_effect_textures_into_vram`); the field VRAM-parity oracle
   applies the same upload image-pages-only (`upload_clut = false`). They are
   invisible to the per-entry `tim_scan` (the overlapping windows mis-slice
   them); `befect_cluster::scan_tims` resolves all eight with correct
   `fb_x/fb_y/CLUT`.

   **Render path (which model is the flame).** Walking the live GPU primitive pool from the same mid-cast capture (decoded with `legaia_mednafen::prim_pool`) isolates the flame:

   - It is a tight cluster of ~15 visible **Gouraud-textured** primitives (`POLY_GT3` / `POLY_GT4`) in a ~40×50px screen region.
   - All sample page `(832,256)` 4bpp + CLUT **row 478** across columns 0/16/32 *simultaneously* (a static multi-shade look, **not** a temporal CLUT cycle: see "Animation is geometric" below) - i.e. the flame samples the **`player_data` §2 band**, not the 0870 pages.
   - The `cba`/`tsb` are applied at *render* time (none of the ~33 TMDs registered in `DAT_8007C018` during the cast bake that CBA).

   **Where the battle flame model comes from.**

   - A player Seru-magic cast pages in a **per-summon code overlay** (`FUN_8003EC70(id - 0x79)` → extraction PROT 903..913 under the corrected loader index math; Gimard *Tail Fire* `0x81` → PROT 903), and that overlay supplies the summon's spawn logic.
   - Confirmed against the live Tail-Fire RAM: the `player_data` §2 texels are resident in VRAM, yet **none of 874 §0's five TMDs are resident in main RAM** - 874 §0 is the *field* character pack (Vahn / Noa / Gala / savepoint / auxiliary; [`character-mesh.md` § On-disc layout](character-mesh.md#on-disc-layout)), not an effect-model pack. Its 5th/smallest TMD (2 objects / 18 verts / 25 prims) *does* bake `cba=0x778E@(224,478)` / `tsb=0x001D@(832,256)` and looks flame-like; the engine keeps it only as a preview fallback (`engine-core::scene::ETMD_TAIL_FIRE_MODEL_INDEX`).
   - See [`subsystems/battle-action.md`](../subsystems/battle-action.md#seru-magic-summon-overlay-dispatch).

   **The real effect-model library is extraction entry 0871 (`etmd.dat`).** Verified against the live Tail-Fire RAM:

   - The 30-entry TMD pack loads verbatim at `0x800CA25C`, and all 30 register into `DAT_8007C018[3..32]` via `FUN_80026B4C` (battle init, loader case `0xb`, raw index `0x369`).
   - Gimard's flame is `DAT_8007C018[26]` (see [`subsystems/battle-action.md`](../subsystems/battle-action.md#seru-magic-summon-overlay-dispatch)); its animation is **geometric** (the summon stager overlay spawns 8 flame part-actors via `FUN_80021B04` and the actor system ticks them - **not** the move VM, and **not** CLUT cycling), see "Animation is geometric" below. (The "8 parts" phase loop is decoded from the extraction-905 stager file - the spell-`0x83` slot under the corrected loader math; Gimard's own file is 903.)
   - The engine loads it at scene entry (`engine-core::scene::seed_effect_model_library_from_etmd`): the uncompressed pack is walked over the entry's extended footprint and the 30 TMDs register into `World::global_tmd_pool[3..=32]`, overwriting the two field-pack tail slots `[3]`/`[4]` exactly as retail's battle init does. The Gimard flame is `GIMARD_TAIL_FIRE_MODEL_INDEX = 26` (= pack entry 23).

   **Animation is geometric, not CLUT cycling (earlier reading falsified).**

   - Two animation-distinct Tail Fire capture frames (catalogued `battle_gimard_tail_fire_a`/`_b`) have a **byte-identical CLUT band** (VRAM rows 470..499) while their framebuffers differ ~21% - so *no* per-frame CLUT/CBA cycling occurs. The visible flame motion is **geometric**.
   - A live PCSX-Redux trace of a player Gimard *Burning Attack* cast pins the render path: the **battle per-actor draw `FUN_80048A08` fires 35-64×/frame** → the per-object rigid-TRS keyframe decoder `FUN_8004998C` → cluster-A `FUN_80043390`, while `FUN_801F7088` fires **0×** and the move VM `FUN_80023070` only **2-3×** (noise).
   - So the **player** summon is posed like an enemy monster body (per-object rigid TRS keyframes), and the faithful render is the battle TRS-keyframe draw ported in `engine-vm/anim_vm.rs` (`FUN_80048A08` / `FUN_8004998C`).
   - The summon stager overlays (extraction 903..913) *do* carry real move-VM part records - recovered under the corrected link base `0x801F69D8` by `legaia_asset::summon_overlay`, which supersedes the earlier wrong-link-base "PROT 905 has zero `jal 0x80023070` → no move VM" reading (the `jal` is in the SCUS stager `FUN_80021B04`, not inside the overlay); the engine drives them as a stand-in (`summon::SummonScene`), but the trace shows that scene-graph is not the player summon's per-frame render path.
   - The overlay's 3 conditional `LoadImage` (`0x800583C8`) CLUT uploads - `RECT = {x=0, y=481+s5, w=240, h=1}`, source `a2 + s5*480 + 0x894` - target VRAM row **481+** (the character/party-CLUT region), not the flame's row 478, and that region is byte-identical across the two frames.
   - SCOPE: the trace covers the **player** "Burning Attack"; the **enemy** Gimard *Fire Tail* boss move is untraced. The engine renders the static flame mesh with the correct row-478 CLUT.

2. **2D sprite billboards.** The `efect.dat` sprite atlas (entry 873) drives the per-frame billboard emit in `FUN_801E0088` pass 2.

   - The atlas entry layout is confirmed from that consumer: `u8 u, u8 v, u8 w, u8 h, u16 clut, u8 tpage, u8 unk` - the pass-2 code copies the u16 at atlas `+4` into the primitive's CLUT field (`*(puVar3+0xe)`) and the byte at `+6` into its tpage field (`*(puVar3+0x16)`), and builds the sprite UV rectangle from `(u, v)..(u+w-1, v+h-1)` (see the Field order note above).
   - The "billboards sample page `(0,0)`, 8bpp" reading was a **field-order misread of the atlas entry** (see the Field order note above): the `0x7680` everyone decoded as a tpage is actually the **CLUT** (the u16 at `+4`), and the real tpage is the byte at `+6`. So the billboards sample the **loaded effect-texture pages**, not page `(0,0)`.
   - Confirmed from a live battle capture on the impact frame of a melee attack (the small white hit-spark): walking the GPU prim pool, **no on-screen prim samples page `(0,0)`, 8bpp, or `tpage 0x7680` anywhere** (there are zero 8bpp textured prims in the whole battle).
   - The white hit-spark is drawn as **textured quads** (`POLY_FT4`/`POLY_GT4`, cmds `0x2c`/`0x2e`/`0x3c`/`0x3e`) sampling the **extraction-0870 `etim.dat` pages `(320,0)` and `(448,0)`** (4bpp, effect-band CLUTs in rows 473..480) - pages that appear *only* in the impact frame (absent from a no-spark command-menu frame from the same fight), confirming they are the effect, not the persistent scene (party/monster meshes sit at pages `(512,256)`/`(576,256)`/`(832,0)`).
   - The engine reads the atlas in the correct order now (`engine-vm` `SpriteAtlasEntry`: CLUT u16 `+4`, tpage byte `+6`), so `World::active_effect_sprites` yields the real effect page + CLUT and the billboards sample the resident `etim` texels.

(An earlier note guessed the billboard texel source was extraction entry 875 - wrong twice over: the live-VRAM oracle pins the texels to `etim` (0870) and the `player_data` §2 band, and 875 is the battle-type-4 conditional stream in the loader table above. A separate note suggested the atlas layout might be mis-decoded - it is not; the pass-2 consumer confirms it.)

### Consumer cluster

The runtime consumer lives in the battle overlay (`0898_xxx_dat`) and is three functions - init, spawn, and the per-frame walker.

| Function | Span | Role |
|---|---|---|
| `0x801DE914` | 0x138 | Init / pack-fixup |
| `0x801DFDF8` | 0x290 | Public spawn-effect API |
| `0x801E0088` | 0x970 | Per-frame walker (update + render) |

**`0x801DE914` - init / pack-fixup.** Called by `FUN_800520F0` case `0xE` with `(id=0x1000, param=0xA00)`. It zeros the 5008-byte runtime pool at `_DAT_8007BD30`, treats `_DAT_8007BD5C` as the 2-pack wrapper, and walks both packs converting offsets to pointers (the fixup is gated by `byte[3] == 0`). Post-fixup state goes in the table-head 16-byte record `(u16 id, u16 param, u32 buf+8, u32 pack0_data+4, u32 pack1_data+4)`, and the init flag `_DAT_8007BD58` is set to `1`.

**`0x801DFDF8` - public spawn-effect API.** Signature `(byte effect_id, short* world_pos, ushort angle)`. It reads `pack1[effect_id]` from the head record to find the effect's script, allocates the first free slot in the 32-entry × 28-byte master pool, writes pos/angle, copies the script header bytes, and sets the script cursor to `entry + 4`. Two ids are special-cased: `effect_id = 4 → 0x801F5D90` and `effect_id = 0x13 → 0x801F5CF8`.

**`0x801E0088` - per-frame walker.** Runs two passes over the pools.

Pass 1 covers the 32 master slots. For each active slot it decrements the state byte; on zero it fetches the next 14-byte script instruction, where byte 0 indexes `pack0` to select an anim batch and the rest supplies angle, direction, and lifetime. It spawns into the 128 child slots, applying sin/cos through the lookup tables at `_DAT_8007B7F8` and `_DAT_8007B81C`.

Pass 2 covers the 128 child slots. For each it builds a PSX GPU sprite primitive (`0x9000000` opcode + `0x2E000000` RGB), pulls UV / size / page from the inline sprite atlas via the child's anim cursor, and submits through `func_0x8003D2C4`.

Decompiled output: `ghidra/scripts/funcs/overlay_battle_*.txt`.

#### How a move reaches this 2D pool - the bit-7 multiplex

A battle move's effect-id lists ([move-power.md](move-power.md) record `+0x12` /
`+0x16`) are the main producers of `FUN_801DFDF8` spawns. Each list byte
**multiplexes two id spaces by bit 7** in the dispatch loop `FUN_801e09f8`:

- **bit 7 clear** (`0x01..=0x63`) → the **3D move-FX** path: prototype
  `0x801F6324[id]` staged through `FUN_80050ED4` → `FUN_80021B04` (the move-VM
  scene-graph, *not* this pool); `0x64` (`100`) is a hardcoded screen-flash.
- **bit 7 set** (`0x80..=0xFE`) → **this 2D pool**: `FUN_801DFDF0(id & 0x7F)`
  spawns the `efect.dat` `pack1[id & 0x7F]` billboard through the public API
  above, whose two special-cased ids (`0x04` → `0x801F5D90`, `0x13` →
  `0x801F5CF8`) apply here.

So only the bit-7-set half of a move's effect list reaches `pack1` / this pool;
the bit-7-clear half is the move-VM effect-model path. The engine models the
split at `engine-core::move_power::EffectListEntry` (`Spawn` vs `AltEffect`).

### Runtime pool layout (`_DAT_8007BD30`, 5008 bytes total)

```
+0x000  16 bytes   table-head record set by init
+0x010  4096 bytes 128 × 32-byte child slots - per-sprite render state
+0x1010 896 bytes  32 × 28-byte master slots - per-effect-instance state
+0x1390 1968 bytes (unused / future expansion)
```

32 max simultaneous effects × ~4 sprites avg = 128-child sprite pool.

### Side-band streaming-effect handler

`0x801F17F8`, called from `FUN_800520F0` case `0xFF`, streams two specific runtime-only files via `FUN_800558FC`:

- `data\battle\summon.dat` - selected when `_DAT_8007BD24[0x26B] & 0x80 != 0`.
- `data\battle\readef.dat` - opposite branch.

**Resolved** - both entries are pinned and the format is decoded; full reference
in [`summon-readef.md`](summon-readef.md). In retail `FUN_800558FC` ignores the
path string (the ISO9660 open is a trap stub) and consumes its fourth argument
as a **retail TOC index** directly: `summon.dat` = `0x37F`, `readef.DAT` =
`0x380`. The retail index space includes the PROT.DAT 8-byte header in the
in-RAM TOC copy, so those map to **extraction entries 893 / 894** (retail
index − 2) - exactly 103 / 78 slots of `0x10800` bytes. Byte-verified in the
`battle_gimard_tail_fire_a` save state (stream buffer ↔ disc slot, slot-0
CLUT + texture page ↔ VRAM `(0,488)` / `(512,0)`). The slots carry
per-special-attack CLUT rows + 4bpp texture pages and summon-creature actor
records (TMD + texture pool installed via `FUN_80055468`); parser
`legaia_asset::summon_readef`. The earlier reading that placed `0x37F/0x380`
at extraction entries 895 / 896 (init pak / `0896` blob) failed because it
compared against the extraction numbering - the two index spaces differ by 2.

### Open questions

- **Effect-ID → human effect name.** Effect IDs are anonymous and there is **no
  string table** that maps id → "fireball / thunder / heal" - the ids are pure
  disc data. The call sites are now traced (the move-power `+0x12`/`+0x16` effect
  lists' bit-7-set entries route here via `FUN_801e09f8` → `FUN_801DFDF0`; see the
  bit-7 multiplex above), so the only "name" available is an id → triggering-move
  join off the move-power table (disc-gated), not a symbolic effect name.
- **2D billboard texel source - RESOLVED (page-(0,0) was a field-order misread).**
  - The atlas entry's `+4`/`+6` fields are CLUT/tpage, not tpage/CLUT (see the Field order note): `0x7680` is the CLUT (CBA → fb `(0,474)`), and the real tpage is the byte at `+6`.
  - A melee hit-spark capture confirms it - the spark draws as textured quads sampling the **PROT 870 flame atlas at `(320,0)`/`(448,0)`** (effect-band CLUTs), with no prim anywhere sampling page (0,0)/8bpp.
  - The engine now reads the atlas in the right order, so the billboards sample the resident PROT 870 / `etim` texels.
- **summon.dat / readef.dat formats - RESOLVED.** Pinned to extraction PROT entries 893 / 894 and decoded; see [`summon-readef.md`](summon-readef.md). Still open there: the consumer of the low-band `readef.DAT` aux slots.

## Field-pack format (magic `0x01059B84`)

A small number of PROT entries lead with magic `0x01059B84` followed by a 97-entry strict schema preceding packed TIMs/TMDs. The preamble→slot mapping is unknown - likely runtime-reconstructed from the schema's offset hints. Detector + dispatch live in `crates/asset/src/field_pack.rs`.

## See also

- [`subsystems/effect-vm.md`](../subsystems/effect-vm.md) - the effect-bundle pool and spawn API.
- [PSX TIM](tim.md) - the sprite-anim texture format inside the bundle.
- [Legaia TMD](tmd.md) - the effect-model meshes inside the bundle.
- [`subsystems/battle.md`](../subsystems/battle.md) - the battle scene that spawns these effects.
