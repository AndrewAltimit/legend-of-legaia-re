# Renderer (Legaia TMD)

The renderer is `FUN_8002735C` - 60 GTE ops, per-mode descriptor table at `DAT_8007326C`. Drives the `crates/tmd::legaia_prims` walker and the engine-render port.

## Per-mode descriptor table

The renderer indexes into an 8-byte-stride table at `0x8007326C` using `((flags >> 1) - 8) >> 1`:

| flags | table idx | byte 0 | byte 4 |
|---|---|---|---|
| 0x10/11 | 0 | 0x04 | 0x05 |
| 0x12/13 | 1 | 0x09 | 0x07 |
| 0x14/15 | 2 | 0x04 | 0x00 |
| 0x16/17 | 3 | 0x06 | 0x06 |
| 0x18/19 | 4 | 0x07 | 0x07 |
| 0x1A/1B | 5 | 0x09 | 0x0B |
| 0x20-23 | 4 | (same) | |

Each entry's first u32 has bytes `[?, ?, ?, type_bits]` where the low 2 bits of byte 3 select the OT packet shape (0/1/2/3 → different DrawPolyXX variants). Each entry's second u32 has the vertex-index offset (in u16 units) within the prim in its low byte. See [`formats/tmd.md`](../formats/tmd.md) for the full layout.

## TMD pointer table

`FUN_80026B4C` writes registered TMDs to `*(int **)(idx * 4 + 0x8007C018)`. Consumers in retail (4 functions, all setup-not-render):

- `FUN_80021B04` - actor-spawn helper, builds per-actor OBJECT pointer table.
- `FUN_80024D78` - per-actor OBJECT-table rebuild.
- `FUN_8001EBEC` - per-frame OBJECT[10/11] swap (pose select for player TMDs).
- `FUN_8001E890` - DATA_FIELD player loader; loads `data_field_player_lzs` chains, registers TMDs.

The per-actor `OBJECT[i]` is a 28-byte struct copied into `actor[0x44][i+1]` from `tmd + 12 + i*28` - `sizeof(OBJECT) = 28`.

## VRAM emulation in the engine port

`crates/engine-render` emulates a 1024×512 R16Uint VRAM page so the per-prim CBA/TSB selectors plus 4/8/15bpp + CLUT decoding can happen in a fragment shader. The viewer uploads every sibling TIM into VRAM so multi-page meshes render correctly.

CLUT data scatters across PROT entries - many character meshes reference CLUT rows that live in *different* PROT entries from their TMD source. The viewer's `--vram-extra-dir` is the workaround until the runtime asset chain is fully traced. Battle is fully traced (the bundle loader handles this); field / town / level-up still rely on the workaround.

### Targeted VRAM upload

The TIM corpus on a single PROT entry can run into the hundreds. Uploading every TIM into the 1MB VRAM clobbers regions a different mesh references as its CLUT row, and the paletted decode reads image pixels as palette entries (rainbow noise). The asset viewer and the `tmd` CLI both go through `legaia_tmd::vram_targeted::build_vram_targeted`: for every TIM, the image block and CLUT block are decided *independently* against the prim-target rectangles for the current TMD - a TIM can contribute one block, both, or neither. `legaia_tim::vram::Vram::prim_texture_status` then classifies each prim's `(cba, tsb, uv)` lookup as `Ok` / `MissingClut` / `ClutDepthMismatch { populated_width, expected_width }` / `MissingTexturePage` so the viewer can drop bad prims at mesh-build time and the CLI can explain *why* a prim was dropped (the most common case is a 4bpp prim referencing a CLUT row that's been populated as a 256-entry 8bpp palette by a different TIM).

The same filter is wired into engine-side scene loads through `ResolvedTmd::build_filtered_vram_mesh`, so battle / field actor meshes inherit the same cleanup the asset viewer has.

### Engine-side targeted upload + shared blocks

`SceneResources::build_targeted` is the engine-side mirror of the asset-viewer's targeted-upload path: it parses every TMD in a scene, collects the union of all prim-target rectangles (CLUT rows + texture-page UV bboxes), then walks every TIM and decides per-block whether to write it. This matches what the retail field loader does - DMA only the texture bytes the current scene's meshes need - and avoids the CLUT-row collisions that drop 80%+ of textured prims under the naive "upload every TIM" path.

`build_targeted` also accepts a list of *shared* CDNAME blocks via the [`FIELD_SHARED_BLOCKS`](../../crates/engine-core/src/scene_resources.rs) constant (`init_data` + `player_data`). These are the blocks the retail engine keeps resident across field-scene transitions - `player_data` (PROT 876) holds the player-character TMD + 256x256 atlas at VRAM `fb=(768, 0)` with CLUT at `(0, 500)`; `init_data` (PROT 0) holds shared UI / sprite tiles. The shared blocks are uploaded *first*, so scene-local TIMs win any slot collision (mirrors the retail boot-then-scene order).

`SceneHost::enter_field_scene` calls `build_targeted` with the field shared blocks by default; the legacy `SceneResources::build` / `build_with_shared` paths remain for tests and engines that want the unfiltered upload for diagnostic purposes.

`legaia-engine info --scene <name> --tmd-stats` reports per-TMD `kept / miss_clut / depth_mm / miss_page` counts so future regressions in the targeted-upload pipeline are visible without firing up the windowed viewer. `--vram-png` / `--vram-bin` write the engine VRAM as a 1024x512 PNG / raw BGR555 blob; `--runtime-vram <bin>` (paired with `mednafen-state vram-dump --out-bin`) reports per-region pixel-coverage statistics against the runtime ground truth, and `--vram-diff-png` writes a colour-coded diff (red = runtime has, engine missing; green = engine extras; blue = both populated but different).

### CLUT-depth-mismatch threshold

`Vram::prim_texture_status` flags `ClutDepthMismatch` when a CLUT row is populated past what the prim's color depth could legitimately fill: for 4bpp prims the threshold is `16 * 16 = 256` entries (16 distinct 16-entry palettes packed in one row, picked by the prim's `CBA` low 6 bits - the standard Legaia character-TIM layout); for 8bpp it's `2 * 256` (one palette plus slack for stray pixels). Anything past that indicates another TIM's image bytes have spilled onto the CLUT row, and the paletted decode would index into pixel data. The targeted-upload path in `build_targeted` prevents this spillage, so engine-side scenes hit the mismatch threshold only when a regression breaks the per-TIM block-arbitration.

### Texture-window register

`Renderer::set_texture_window(mask_x, mask_y, off_x, off_y)` maps to GP0(0xE2) "Texture Window setting": four 5-bit values in 8-pixel steps that clamp / wrap texture-coordinate sampling to a smaller window inside the texture page. Default is all-zero (no-op). Retail Legaia leaves the register at zero almost everywhere; the API is wired primarily so future runtime LoadImage / DMA-to-VRAM trace work can replay the register state faithfully. The fragment shader applies the per-pixel `coord = (coord & ~(mask*8)) | ((offset & mask)*8)` transformation before texture-page lookup.

### Asset-viewer flat-shaded fallback

`asset-viewer tmd <PATH> --no-textures` (alias `--flat-shaded`) suppresses the VRAM path entirely and renders unlit flat geometry. Useful for inspecting mesh silhouettes without battling palette guesses (the runtime LoadImage trace for field / town scenes is not yet captured, so some palette rows always render as garbage in textured mode).

### `tmd` CLI VRAM diagnostics

`tmd prims <PATH> --vram-dir extracted/tim_scan/<entry>` simulates the targeted upload and adds a per-prim verdict trailer (`-> Ok` / `-> MISSING CLUT (row N)` / `-> DEPTH MISMATCH (row N populated with K entries; prim expects M)` / `-> MISSING TEXTURE PAGE (tpage 0xNN)`). `tmd vram-dump <PATH> -o vram.png [--vram-dir ...] [--annotate]` exports the post-upload software VRAM as a 1024x512 PNG with optional red CLUT-row + green texture-page outlines, so collisions are obvious without firing up the GUI.

## PSX-faithful rendering knobs

`Renderer::set_psx_mode(true)` enables two retail-faithful rasterisation modes on the VRAM-mesh pipeline:

- **Affine UV interpolation.** Per-vertex UVs interpolate linearly in screen space (no perspective-correct division). This reproduces the texture warping you see on retail surfaces with steep depth gradients - the GP0(0x24)-class triangle commands transmit only `(u, v)` per vertex, the rasteriser does not divide by `1/w`. WGSL `@interpolate(linear)` gives the same behaviour.
- **Sub-pixel vertex snap ("vertex jitter").** Clip-space `x` / `y` are snapped to integer pixel positions inside the vertex shader (NDC → pixel grid → NDC round-trip). Reproduces the GTE's per-vertex sub-pixel-truncation jitter that gives PSX rendering its characteristic shimmer on slowly-moving geometry.

Texture page (`tsb`) and CLUT base address (`cba`) remain `@interpolate(flat)` - they are per-primitive in retail because GP0(0x24) sets them once per draw call, not per vertex.

A fixed-point GTE math module at `crates/engine-render/src/gte.rs` mirrors the retail accumulator shape: q3.12 rotation matrices, q19.12 translation vectors, i64-widened multiply-add to absorb three-term sums without overflow. The module also exposes the GTE's higher-level primitives - a `Camera` bundle that runs `RTPT` (rotate-translate-perspective) end-to-end with PSX-correct saturation on behind-camera vertices, `nclip` for back-face rejection, `avsz3` / `avsz4` for OT-bucket selection, and a small CPU rasterizer scaffold (`raster::rasterize_triangle`, top-left fill rule, integer-pixel bounding-box iterator) downstream tooling uses to validate captured traces. Production rendering still uses f32 wgpu math; this module is the single citation point for code (effect spawners, hit-detection, animation re-targeting, offline regression checks) that needs to reproduce per-vertex GTE behaviour.

### GTE register-state emulator

`Gte` is a register-level cop2 emulator next to the math module, mirroring the PSX hardware register file: V0..V2 input vectors, MAC0..MAC3 wide accumulators (i64), IR0..IR3 saturating shorts, the SXY (3-deep) / SZ (4-deep) / RGB (3-deep) FIFOs, OTZ, and the FLAG sticky-saturation register with hardware-matching bit positions exposed via `gte::flag_bits` (engines comparing against captured FLAG dumps mask the same bits). Control registers cover the rotation matrix, translation, focal length `H`, screen offset `OFX/OFY`, the average-Z scale factors `ZSF3` / `ZSF4`, the depth-cue interpolation slope/intercept `DQA` / `DQB`, the light source matrix `L`, the light color matrix, and the `back_color` / `far_color` triplets used by the depth-cue pipeline.

Instructions covered:

| Mnemonic | Purpose |
|---|---|
| `RTPS` / `RTPT` | Rotate-translate-perspective (single / triple vertex). |
| `NCLIP` | Signed area of the SXY-FIFO triangle (back-face cull). |
| `AVSZ3` / `AVSZ4` | OT-bucket selection from the SZ FIFO. |
| `MVMVA` | Generic matrix × vector + translation, with shift-frac and lower-clamp flags. |
| `NCDS` / `NCDT` | Normal-color depth shading (single / triple vertex). |
| `DCPL` | Depth-cued primary-color blend. |
| `DPCS` / `DPCT` | Depth-cued color blend (single / triple). |
| `INTPL` | Far-color interpolation primitive (used internally by DCPL / DPCS). |
| `SQR` | Squares IR1..IR3 in place. |
| `OP` | Cross product of the rotation-matrix diagonal with IR. |
| `GPF` / `GPL` | General-purpose IR×IR0 multiply / accumulate (alpha-blend kernel). |

Each instruction sets MAC1..MAC3 / IR1..IR3 / FLAG with the same saturation semantics the hardware uses; the `Camera::transform` shim and the cop2 `RTPT` produce identical SXY output (cross-validated by the `gte_rtpt_matches_camera_transform` test).

### GTE register-transfer + memory ops

Beyond the cop2 instruction set the module exposes the four MIPS register-transfer ops (`MFC2` / `MTC2` / `CFC2` / `CTC2`) plus the two memory ops (`LWC2` / `SWC2`) so engines can replay a captured GTE trace without re-deriving the cop2 register layout:

- `read_data(idx)` / `write_data(idx, val)` - map the 32 cop2 data registers to typed fields. Indices 0..5 = V0..V2 (xy packed pairs + sign-extended z), 6 = RGBC, 7 = OTZ, 8..11 = IR0..IR3, 12..14 = SXY0..SXY2, 15 = SXYP (push-only write), 16..19 = SZ0..SZ3, 20..22 = RGB0..RGB2, 23 = RES1 (reserved), 24..27 = MAC0..MAC3, 28..29 = packed `IRGB` / `ORGB` (BGR555), 30 = LZCS, 31 = LZCR (count leading zeros / ones of LZCS).
- `read_ctrl(idx)` / `write_ctrl(idx, val)` - map the 32 cop2 control registers. Rotation / light / light-color matrices live as packed two-i16-per-word entries (RT11RT12 → cop2cr0, RT13RT21 → cop2cr1, RT22RT23 → cop2cr2, RT31RT32 → cop2cr3, RT33 → cop2cr4 sign-extended, etc.); translation triple at 5..7; back-color at 13..15; far-color at 21..23; viewport offsets `OFX` / `OFY` / `H` at 24..26; depth-cue slope / intercept `DQA` / `DQB` at 27..28; `ZSF3` / `ZSF4` at 29..30; `FLAG` at 31 (writable so captured traces can replay the post-instruction FLAG state).
- `LWC2 rd, addr` / `SWC2 rd, addr` - load / store cop2 data register `rd` from memory through the `Cop2Mem` trait. `VecMem` is shipped for replay against captured RAM snapshots; `NullMem` for tests that don't exercise memory at all. `load_vertices(mem, addr)` is a 24-byte bulk-load helper for the canonical retail 3-vertex emit (`LWC2 0..5` covering V0.xy / V0.z / V1.xy / V1.z / V2.xy / V2.z at 8-byte stride).

Each transfer op charges one cycle into `Gte::cycles` (matches the un-pipelined hardware budget). FLAG / cycle bookkeeping is identical between the higher-level instruction methods and the bare register-transfer path.

The per-mode descriptor table from `DAT_8007326C` is also exposed as a typed lookup at `crates/tmd/src/descriptor.rs`: `Descriptor::for_flags(flags)` returns the resolved `PacketShape` (one of `F3` / `FT3` / `G3` / `GT3` / `F4` / `FT4` / `G4` / `GT4`) and the per-prim vertex-index offset. Same on-disc bytes as the older `legaia_prims::vertex_offset_bytes` free function, exposed as typed fields so consumers can branch on shading mode (flat vs gouraud) and texture presence without re-deriving the bit math.

## Stage geometry detector (legacy, signal only)

A "12-byte fixed prefix `00 F0 84 7F 01 F0 1F 00 00 F1 00 00` repeated at 20-byte stride" detector lives at `crates/asset/src/stage_geom.rs`. It's not real stage geometry - it's the standard primitive-group header for Legaia TMD primitive group data when `((flags >> 1) - 8) >> 1 == K` (where K is the group type that uses 20-byte stride).

The detector is preserved as a signal during exploration ("this buffer contains a TMD with effect-style primitives") but for actual geometry extraction use the TMD parser (`crates/tmd::legaia_prims`).
