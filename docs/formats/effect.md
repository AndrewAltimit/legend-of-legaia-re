# Effect bundles

Two distinct formats share the "effect" name - the on-disc bundle (magic `0x02018B0C`) and the runtime 2-pack wrapper used by `data\battle\efect.dat`. Only one PROT entry uses the on-disc form; the runtime wrapper is what battle code actually consumes.

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
+4  u16 tpage   ; PSX texture-page descriptor, copied verbatim to the primitive
+6  u8  clut    ; CLUT (CBA) id
+7  u8  ?        ; unknown / reserved
```

The texel rectangle is `(u, v)..(u+w-1, v+h-1)`. A typical entry is a 32×32 sprite (`w=h=0x20`) with `tpage=0x7680` (page base (0,0), 8-bit colour mode) and a per-effect CLUT id. The pixels live in VRAM, uploaded once at battle load from the sibling **`etim.dat`** file in the same `befect_data` cluster (see "Battle effect cluster" below). The atlas only carries the VRAM coordinates; the actual texel bytes are blitted by the battle scene loader.

### Battle effect cluster (`befect_data`, CDNAME 872)

`efect.dat` is one of four logical files in the `befect_data` cluster the battle scene loader `FUN_800520F0` pulls in. `FUN_800520F0` is a sequential state machine (sub-state byte at `gp+0xa59`); each state loads one file through the dual-mode loader - retail opens the dev-path string, debug uses a PROT index (`FUN_8003e8a8`):

| Loader state / case | Dev-path string | Role |
|---|---|---|
| case `0x8` | `h:\prot\battle\etim.dat` (`0x80015358`) | Effect TIM images (textures). |
| case `0xb` | `h:\prot\battle\etmd.dat` (`0x80015370`) | Effect 3D models (Legaia TMDs; registered via `FUN_80026b4c`, which asserts magic `0x80000002`). |
| case `0xb` | `h:\prot\battle\vdf.dat` (`0x80015388`) | VDF buffer (asset type `0x07`, appended via `FUN_8001fbcc`). |
| case `0xc` | `data\battle\efect.dat` (`0x800153a0`) | The 2-pack above; initialised by `FUN_801DE914` (offset fixup only). |

#### On-disc layout + the cluster-aware extractor

The per-entry PROT extractor does **not** cleanly separate these files: the four cluster entries (872..875) overlap on disc (each starts only a few sectors into the previous entry's extended footprint), so the naive per-entry `.BIN` files bleed into their neighbours - e.g. `0873_befect_data.BIN` at offset `0x2000` is byte-identical to the start of `0874_befect_data.BIN`. The true per-file size is the **footprint** (`next_lba - this_lba`), which the indexed/extended TOC formula over-reads here.

`asset befect-cluster PROT.DAT --cdname CDNAME.TXT [--out DIR]` (in `legaia-asset`) does the cluster-aware extraction: footprint-bound each entry, expand the one LZS-container entry into its sections, and classify every part. It resolves to:

| Part | Footprint / section | Classification | Notes |
|---|---|---|---|
| entry 872 | `0x4800` | offset pack, 32 entries | Effect billboard geometry (small per-entry records, ~96 B each). |
| entry 873 | `0x2000` | `efect.dat` 2-pack | 144 atlas entries, 14 anim batches, 33 scripts (`pack0@0x488`, `pack1@0x900`). |
| entry 874 §0 | LZS, 46236 B | TMD pack, 5 models | Effect 3D models (`etmd.dat`). |
| entry 874 §1 | LZS, 16864 B | offset pack, 23 entries | (`vdf.dat`.) |
| entry 874 §2 | LZS, 120100 B | 8 effect-texture TIMs | (`etim.dat`.) 4bpp, CLUTs in high VRAM rows 473..478, pixels at `fb_x≥320`. |
| entry 875 | `0x20000` | raw | A 256×256-halfword page blob (see below). |

So `etim.dat` / `etmd.dat` / `vdf.dat` are the three LZS sections of PROT entry 874, and `efect.dat` is entry 873.

#### Texel source - `etim.dat`, pixel-verified

`FUN_800198e0` is the general packed-image → VRAM uploader the loader uses (loader state `9` walks a pack and calls it per entry): it reads a per-chunk tag/flag word, builds a PSX `RECT`, and calls `FUN_800583c8` = `LoadImage` (`0x800156d4`) to DMA pixels into VRAM, maintaining a CLUT cache at `0x8007BEC0`. (Same routine the title / menu / save overlays and the type-`0x01` CLUT walker `FUN_8001fe70` use.)

There are **two independent effect-texel systems** here:

1. **3D effect models.** `etim.dat` (entry 874 §2) holds the textures (4bpp TIMs, pages at `fb_x≥320`, `fb_y=256+`, CLUTs in rows `473..478`) for the 3D effect *models* in `etmd.dat` (entry 874 §0). The `etmd` model primitives reference exactly those `etim` CLUT rows. This is confirmed **pixel-exact** against a live battle VRAM dump captured mid-cast (Gimard's *Tail Fire* - a 3D flame mesh): five of the seven `etim` TIM pixel blocks (the fire tiles at `fb(832,256)`, `(852,256)`, `(872,256)`, `(880,384)`, `(880,448)`) byte-match VRAM at their stated targets, and the CLUT rows match. (The two larger `64×256` blocks at `fb(320,256)`/`(384,256)` hold background / character textures in that capture, so they aren't this effect's tiles.) The engine uploads `etim` into the scene VRAM at scene entry (`seed_effect_vram_from_befect_data` in `engine-core`), making these texels resident for effect-model rendering.

   **Render path (which model is the flame).** Walking the live GPU primitive pool from the same mid-cast capture (decoded with `legaia_mednafen::prim_pool`, filtered to on-screen prims sampling the `etim` page/CLUT) isolates the flame as a tight cluster of ~15 visible **Gouraud-textured** primitives (`POLY_GT3` / `POLY_GT4`) in a ~40×50px screen region, all sampling page `(832,256)` 4bpp + CLUT **row 478** across columns 0/16/32… - a **CLUT-animated** palette (the fire flicker). The `cba`/`tsb` are applied at *render* time (none of the ~33 TMDs registered in `DAT_8007C018` during the cast bake the `etim` CBA).

   **Where the battle flame model comes from (corrected).** It is **not** in `befect_data`. A player Seru-magic cast pages in a **per-summon code overlay** (`FUN_8003EC70(id - 0x79)` → PROT 905..915; Gimard *Tail Fire* `0x81` → PROT 905), and that overlay supplies the summon's 3D models. Confirmed against the live Tail-Fire RAM: `etim` (874 §2) is resident in VRAM (the cluster was loaded), yet **none of 874 §0's five "etmd" TMDs are resident in main RAM**, and the registered models are a 30-entry small-TMD library from the overlay - not the 874 §0 pack. See [`subsystems/battle-action.md`](../subsystems/battle-action.md#seru-magic-summon-overlay-dispatch).

   The 874 §0 pack is therefore mislabeled "etmd" here (its real role is a separate global-TMD-pool head). Its 5th/smallest TMD (2 objects / 18 verts / 25 prims) *does* bake the `etim` CLUT (`cba=0x778E@(224,478)`, `tsb=0x001D@(832,256)`) and looks flame-like, so the engine can render it through the standard VRAM-mesh pipeline as a stand-in flame (`engine-core::scene::ETMD_TAIL_FIRE_MODEL_INDEX`) - but it is a *preview* mesh, **not** the model retail draws in battle, and it is now only a fallback (the engine loads the real PROT 871 library; see below).

   **The real effect-model library is PROT entry 871 (`etmd.dat`).** Verified against the live Tail-Fire RAM: PROT 871 is a 30-entry `asset::pack` of Legaia TMDs (`word[0]=30`, 30 TMD magics), loaded verbatim at `0x800CA25C`, and all 30 register into `DAT_8007C018[3..32]` via `FUN_80026B4C` - the battle scene loader `FUN_800520F0` pulls it at battle init (debug index `0x367`=871, or `0x36d`=877 for battle-type `DAT_8007bd11 == 4`; retail dev path `h:\prot\battle\etmd.dat`). Gimard's flame is `DAT_8007C018[26]` (see [`subsystems/battle-action.md`](../subsystems/battle-action.md#inside-a-summon-overlay-prot-905-decoded)); its fire flicker is **CLUT/palette cycling** (the PROT-905 summon overlay uploads animated CLUT frames each frame via `LoadImage`), not model cycling. PROT 871 (and its texture sibling PROT 870, the 256×256 flame-frame atlas) carry the CDNAME label `sound_data`. So the `etim`/`etmd`/`vdf` dev-path names map to **separate PROT entries** the loader pulls by index, *not* to the three LZS sections of entry 874 - the 874 §0/§1/§2 = etmd/vdf/etim split in the table above is from the standalone cluster extractor and **needs re-verification against the `FUN_800520F0` case→index map** (case `0x8` etim → `0x368`=872; the model load → `0x367`=871). The engine loads PROT 871 at scene entry (`engine-core::scene::seed_effect_model_library_from_etmd`): the uncompressed 30-entry pack is walked directly (the body spans the entry's *extended* footprint, so the indexed-only view truncates it mid-table) and the 30 TMDs register into `World::global_tmd_pool[3..=32]`, overwriting the two §0-head slots `[3]`/`[4]` exactly as retail's battle init does. The Gimard flame is `GIMARD_TAIL_FIRE_MODEL_INDEX = 26` (= pack entry 23); the effect-model render path resolves it through the resident `etim` texels. The remaining parity gap is the per-frame CLUT/palette cycling that animates the fire flicker (driven by the PROT 0905 summon overlay), not the model itself.

2. **2D sprite billboards.** The `efect.dat` sprite atlas (entry 873) drives the per-frame billboard emit in `FUN_801E0088` pass 2. The atlas entry layout is confirmed from that consumer: `u8 u, u8 v, u8 w, u8 h, u16 tpage, u8 clut, u8 unk` - the pass-2 code reads `tpage` from atlas byte 4 (`*(puVar3+0xe)`) and `clut` from byte 6 (`*(puVar3+0x16)`), and builds the sprite UV rectangle from `(u, v)..(u+w-1, v+h-1)`. The atlas's `tpage = 0x7680` decodes to VRAM **page (0,0), 8bpp** - a *different* region than `etim`. So the 2D billboards sample a page-`(0,0)` 8bpp texel source that is **not** `etim` and is **not yet identified** (it isn't entries 872 or 875 either - neither byte-matches that VRAM region). Pinning it is the remaining open thread.

(An earlier note guessed the texel source was the raw blob at entry 875 - wrong; the live-VRAM oracle pins the *model* textures to `etim`. A separate note suggested the atlas layout might be mis-decoded - it is not; the pass-2 consumer confirms it.)

### Consumer cluster

The runtime consumer lives in the battle overlay (`0898_xxx_dat`):

| Function | Span | Role |
|---|---|---|
| `0x801DE914` | 0x138 | **Init / pack-fixup.** Called by `FUN_800520F0` case `0xE` with `(id=0x1000, param=0xA00)`. Zeros the 5008-byte runtime pool at `_DAT_8007BD30`, treats `_DAT_8007BD5C` as the 2-pack wrapper, walks both packs converting offsets→pointers (fixup gated by `byte[3] == 0`). Stores post-fixup state in the table-head 16-byte record `(u16 id, u16 param, u32 buf+8, u32 pack0_data+4, u32 pack1_data+4)`. Sets the init flag `_DAT_8007BD58 = 1`. |
| `0x801DFDF8` | 0x290 | **Public spawn-effect API.** Signature: `(byte effect_id, short* world_pos, ushort angle)`. Reads `pack1[effect_id]` from the head record to find the effect's script. Allocates the first free slot in the 32-entry × 28-byte master pool, writes pos/angle, copies script header bytes, sets script cursor to `entry + 4`. Special-cases `effect_id = 4 → 0x801F5D90` and `effect_id = 0x13 → 0x801F5CF8`. |
| `0x801E0088` | 0x970 | **Per-frame walker (update + render).** Two passes. Pass 1 (32 master slots): for each active slot, decrement state byte; if zero, fetch next 14-byte script instruction (byte 0 indexes pack0 → an anim batch; the rest provides angle/dir/lifetime). Spawns into the 128 child slots; applies sin/cos via lookup tables at `_DAT_8007B7F8` and `_DAT_8007B81C`. Pass 2 (128 child slots): builds a PSX GPU sprite primitive (`0x9000000` opcode + `0x2E000000` RGB), pulls UV/size/page from the inline sprite atlas via the child's anim cursor, submits via `func_0x8003D2C4`. |

Decompiled output: `ghidra/scripts/funcs/overlay_battle_*.txt`.

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

- `data\battle\summon.dat` (PROT `0x37F`) - selected when `_DAT_8007BD24[0x26B] & 0x80 != 0`.
- `data\battle\readef.dat` (PROT `0x380`) - opposite branch.

Buffer size per slot: `0x10800` = 67584 bytes. Format unverified; may share the 2-pack layout but not yet confirmed.

### Open questions

- **Effect-ID → human effect name.** Effect IDs are anonymous; no string table maps id → "fireball / thunder / heal". Reachable by tracing call sites of `FUN_801DFDF8` in damage / battle-action code.
- **2D billboard page-(0,0) texel source.** The 3D-effect-model textures are pinned (`etim.dat`, pixel-verified). Separately, the `efect.dat` 2D sprite atlas samples VRAM **page (0,0), 8bpp** (atlas `tpage = 0x7680`, confirmed via `FUN_801E0088` pass 2). What populates that page at battle load is not yet identified - it isn't `etim` (which textures the 3D models at `fb_x≥320`), nor cluster entries 872 / 875 (neither byte-matches the page-`(0,0)` region in a live battle VRAM). Trace the `LoadImage` site that fills page (0,0) at battle init.
- **summon.dat / readef.dat formats.** Not yet decoded.

## Field-pack format (magic `0x01059B84`)

A small number of PROT entries lead with magic `0x01059B84` followed by a 97-entry strict schema preceding packed TIMs/TMDs. The preamble→slot mapping is unknown - likely runtime-reconstructed from the schema's offset hints. Detector + dispatch live in `crates/asset/src/field_pack.rs`.
