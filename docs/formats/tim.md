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
