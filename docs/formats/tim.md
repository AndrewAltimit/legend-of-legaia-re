# PSX TIM (texture)

A standard PlayStation texture format. The format is well-documented externally; we don't reimplement the parser. Magic check: first u32 == `0x00000010`.

```
u32  id          // 0x00000010
u32  flags       // bits 0..2 = pixel mode (0=4bit, 1=8bit, 2=16bit, 3=24bit)
                 // bit 3   = CLUT present
[CLUT block if flag bit 3 set]
[image block]
```

Each block has its own header `(u32 size, u16 dx, u16 dy, u16 w, u16 h)` followed by pixel data.

In the extracted streaming files, all observed TIMs use **type 8** (4-bit indexed with CLUT). They're VRAM-ready textures.

## VRAM emulation in the engine port

`crates/engine-render` emulates a 1024×512 R16Uint VRAM page so per-prim CBA/TSB selectors plus 4/8/15bpp + CLUT decoding can be done in a fragment shader. The viewer uploads every sibling TIM into VRAM so multi-page meshes render with the correct CLUT bindings.

Some character meshes reference CLUT rows that live in **different PROT entries** from their TMD source (the runtime asset chain stitches them together). The viewer's `--vram-extra-dir` flag is the workaround until the chain is fully traced for every scene type.

## Multi-row CLUT blocks

The PSX TIM spec allows a 4bpp TIM's CLUT block to contain multiple CLUT rows (each row is 16 BGR555 entries = 32 bytes), so the same indexed pixel data can be re-rendered under different palettes. Legaia uses this extensively for system-UI sprite sheets:

| Source TIM | Layout | CLUT-row usage |
|---|---|---|
| **System-UI sprite sheet** at `PROT.DAT[0x018E0]` (4bpp, 256×192, 16×16 CLUT block) | Lives in the unindexed pre-`init_data` gap — not reachable through the per-PROT-entry walker. Constants in `legaia_asset::title_pak::OVERLAY_SYSTEM_UI_TIM_*`. | **Row 2** = the load-screen panel chrome (gold-bronze 9-slice border + dark-blue marbled interior region). **Row 7** = the pointing-finger cursor (white ink + grey shadow). Other rows render HP/MP/money panels, battle chrome, equipment frames, etc. |
| **Menu-glyph atlas** at `PROT.DAT[0x11218]` (4bpp, 256×256, multi-row CLUT block) | Same pre-`init_data` gap. See `legaia_asset::menu_glyph_atlas`. | **Row 13** carries the "Load" text glyphs the load screen draws inside its panel. Other rows render NEW GAME / CONTINUE / OPTIONS strings + smaller menu labels. |

Both TIMs are byte-confirmed against retail VRAM dumps; see [`subsystems/save-screen.md`](../subsystems/save-screen.md#sprite-asset-sources-continue--load-screen) for the pinning method (PCSX-Redux save state → `extract_vram_from_sstate.py` → CLUT-row byte cross-reference against `PROT.DAT`).

Browse them in the asset viewer with `asset-viewer tim extracted/PROT.DAT --offset 0x018E0 --clut <row>` (any of 0..15).
