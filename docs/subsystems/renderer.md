# Renderer (Legaia TMD)

The renderer is `FUN_8002735C` — 60 GTE ops, per-mode descriptor table at `DAT_8007326C`. Drives the `crates/tmd::legaia_prims` walker and the engine-render port.

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

- `FUN_80021B04` — actor-spawn helper, builds per-actor OBJECT pointer table.
- `FUN_80024D78` — per-actor OBJECT-table rebuild.
- `FUN_8001EBEC` — per-frame OBJECT[10/11] swap (pose select for player TMDs).
- `FUN_8001E890` — DATA_FIELD player loader; loads `data_field_player_lzs` chains, registers TMDs.

The per-actor `OBJECT[i]` is a 28-byte struct copied into `actor[0x44][i+1]` from `tmd + 12 + i*28` — `sizeof(OBJECT) = 28`.

## VRAM emulation in the engine port

`crates/engine-render` emulates a 1024×512 R16Uint VRAM page so the per-prim CBA/TSB selectors plus 4/8/15bpp + CLUT decoding can happen in a fragment shader. The viewer uploads every sibling TIM into VRAM so multi-page meshes render correctly.

CLUT data scatters across PROT entries — many character meshes reference CLUT rows that live in *different* PROT entries from their TMD source. The viewer's `--vram-extra-dir` is the workaround until the runtime asset chain is fully traced. Battle is fully traced (the bundle loader handles this); field / town / level-up still rely on the workaround.

## Stage geometry detector (legacy, signal only)

A "12-byte fixed prefix `00 F0 84 7F 01 F0 1F 00 00 F1 00 00` repeated at 20-byte stride" detector lives at `crates/asset/src/stage_geom.rs`. It's not real stage geometry — it's the standard primitive-group header for Legaia TMD primitive group data when `((flags >> 1) - 8) >> 1 == K` (where K is the group type that uses 20-byte stride).

The detector is preserved as a signal during exploration ("this buffer contains a TMD with effect-style primitives") but for actual geometry extraction use the TMD parser (`crates/tmd::legaia_prims`).
