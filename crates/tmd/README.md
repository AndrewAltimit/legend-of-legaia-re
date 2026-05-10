# legaia-tmd

PSX TMD (3D mesh) parser, with the Legaia-specific primitive walker and
OBJ exporter.

TMD is Sony's PlayStation 3D model format (PsyQ `libgte` / `libgs`).
Legaia ships a custom variant - distinct enough that a stock TMD parser
won't read it.

## Header layout (12 bytes)

```text
u32 id        // 0x80000002 in Legaia (stock PSX uses 0x00000041)
              // bit 31 (FLIST_BIT) selects pointer addressing mode.
u32 flags     // usually 0
u32 nobj      // number of objects
```

Object table: 28 bytes per object, `nobj` entries:

```text
u32 vert_top
u32 n_vert
u32 normal_top
u32 n_normal
u32 prim_top
u32 n_primitive
i32 scale     // signed log2 scale; always 0x00808080 in Legaia
```

When `FLIST_BIT` is set, all `*_top` values are byte offsets from the end
of the header (i.e. from the start of the object table). The runtime
patches them to absolute RAM addresses via `FUN_800268dc` - **static
parsers must NOT do that patch**.

Vertices and normals are `SVECTOR { i16 x, y, z, pad }` = 8 bytes each.

## Legaia primitive layout (custom)

Primitives are not stock PSX SDK shape. They are grouped: each group has
an **8-byte header** followed by `count × ilen*4` bytes of prim data.

```text
u16 count
u16 flags
u8  olen, ilen, flag, mode
```

Vertex-index byte offset within each prim is looked up from the
6-entry descriptor table at `DAT_8007326c`, keyed on
`((flags >> 1) - 8) >> 1`. The renderer is `FUN_8002735c` (60 GTE ops);
see [`ghidra/scripts/funcs/8002735c.txt`](../../ghidra/scripts/funcs/8002735c.txt).

The walker `legaia_prims::iter_groups` yields each group's metadata plus
a slice over its prim records. `dump-obj` consumes that and emits OBJ
with faces (vertex + face data; materials/UVs are renderer-side, not
exported). For renderer use there's also `iter_groups_lenient`, which
returns every group walked successfully up to a malformed boundary
instead of bailing on the whole walk - the strict variant would otherwise
hide every valid group in the section before the failure, which
manifested in the asset viewer as multi-object TMDs rendering only the
first object's worth of geometry.

`mesh::tmd_to_vram_mesh_filtered` lets a caller drop primitives whose
texture page hasn't been uploaded into VRAM yet via a closure predicate;
the caller can pair it with `legaia_tim::Vram::prim_has_texture_data`
(or any other heuristic) so the mesh upload doesn't include prims that
would rasterise as solid `CLUT[0]` over correctly-textured geometry.

## Targeted VRAM upload + per-prim diagnostics

`vram_targeted::collect_prim_targets` collects the VRAM rectangles every
textured prim in a TMD will sample, and `vram_targeted::build_vram_targeted`
walks the candidate TIM directories and decides per-block whether to
upload each TIM's image and / or CLUT block (suppressing blocks that
would clobber another mesh's CLUT row, which is what causes rainbow
noise). The asset viewer GUI and the `tmd prims` / `tmd vram-dump` CLI
share this logic so on-screen rendering and offline diagnostics agree.

`legaia_tim::vram::Vram::prim_texture_status` returns a structured
verdict per prim: `Ok` / `MissingClut { row }` /
`ClutDepthMismatch { row, populated_width, expected_width }` /
`MissingTexturePage { tpage }`. The CLI prints these as readable trailers
on each prim row when invoked with `--vram-dir`.

## TMD vs TMD2 (asset dispatcher types `0x02` and `0x09`)

Both route through `FUN_80026b4c`. The wrapping differs:

- `TMD` (case 2): payload is a *pack* - `[count, off0, off1, …]` followed
  by `count` independent TMD blobs. Use `legaia_asset::pack` to walk.
- `TMD2` (case 9): payload is a single bare TMD - pass it directly to
  `parse`. **No pack header.**

## CLI

```bash
tmd info           <input>
tmd scan-dir       <dir>             # walk + classify a directory of TMDs
tmd dump-obj       <input> <out.obj>
tmd probe          <input>           # heuristic: is this a Legaia TMD?
tmd prims          <input>           # walk + print primitive groups
tmd prims          <input> --vram-dir extracted/tim_scan/<entry>
                                     # also simulate targeted VRAM upload + per-prim verdict
tmd vram-dump      <input> -o vram.png [--vram-dir ...] [--annotate]
                                     # export simulated post-upload VRAM as PNG
tmd validate-prims <input>           # vertex-index-range sanity check
```

## See also

- [`docs/formats/tmd.md`](../../docs/formats/tmd.md) - full byte-level spec
  with descriptor-table and group-header derivations.
- [`docs/subsystems/renderer.md`](../../docs/subsystems/renderer.md) -
  how `FUN_8002735c` maps to GTE ops.
