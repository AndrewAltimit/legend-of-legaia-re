# legaia-asset-viewer

Combined GUI viewer for everything the extraction pipeline produces.
One binary: `asset-viewer`. Driven by `winit` 0.30 + `wgpu` 26 via
[`legaia-engine-render`], with audio playback via [`legaia-engine-audio`].

## Subcommands

```bash
asset-viewer tim   <input.tim>
asset-viewer tmd   <input> [--shape character] [--sort-by-size] [--bundle battle]
asset-viewer stage <PATH>                       # wireframe stage geometry
asset-viewer vab   <PROT_entry> --offset <H> --sample <N>
asset-viewer prot  <PROT.DAT> [--cdname <CDNAME.TXT>]
```

### `tmd` - textured 3D meshes

Renders Legaia TMDs spinning, lit by a single directional light. Uploads
every sibling TIM into the shared software VRAM model so meshes that
reference textures across multiple VRAM pages render correctly.

Useful flags:

- `--bundle battle` - overlay the correct PROT 865–890 set traced from
  `FUN_800520f0`. Field/town/level_up bundles live in uncaptured
  overlays, so for those you may need `--vram-extra-dir` until the
  runtime asset chain is fully traced.
- `--vram-extra-dir <dir>` - workaround for character meshes whose CLUT
  rows live in *different* PROT entries from their TMD source.
- `--no-textures` (alias `--flat-shaded`) - skip the VRAM path entirely
  and render unlit flat-shaded geometry. Use this when you want to see
  what a mesh's silhouette looks like without battling palette guesses;
  the runtime LoadImage trace for field / town scenes isn't captured
  yet, so some palette rows always render as garbage in textured mode.

When VRAM is built from one or more TIM directories, the `tmd` viewer
drops primitives whose texture page region is empty or whose CLUT row
is populated at the wrong palette depth - those would otherwise
rasterise as solid `CLUT[0]` (a flat green / cyan tint over
correctly-textured geometry) or rainbow-noise stripes (16 entries
sliced out of a 256-entry gradient) and obscure the rest of the model.
The diagnostic logs distinguish each failure mode:

```
skipped N prim(s) (M/N kept)
  missing CLUT data for K prim(s) across rows [r0, r1, ...]
  CLUT row R IS populated but 256 entries wide (8-bit palette);
    prim expects 16 entries (4-bit) - prim dropped to avoid rainbow noise
  missing texture-page data for K prim(s) across tpages [t0, t1, ...]
```

Primitive-section walks are also lenient: a single malformed group near
the end of an object's prim section no longer hides every valid group
that came before it, so multi-object TMDs render every part of the
model that walks cleanly instead of cutting off at the first error
boundary.

For offline diagnostics the same targeted-upload + per-prim verdict
logic is also exposed by the `tmd` CLI: `tmd prims <input> --vram-dir
<dir>` prints a per-prim status tag (`Ok` / `MissingClut` /
`ClutDepthMismatch` / `MissingTexturePage`), and `tmd vram-dump <input>
-o vram.png [--annotate]` writes the simulated post-upload VRAM as a
PNG so collisions are obvious without firing up the GUI.

### `stage` - wireframe stage geometry

Renders the 12-byte-prefix + 8-byte u16 quad records identified by
`legaia_asset::stage_geom`, using the `Lines` pipeline added to
`legaia-engine-render` for this view. `stage-scan` (in the `asset` CLI)
finds candidate entries; this viewer renders one. OBJ export is
supported via `asset stage` proper.

### `prot` - PROT entry browser

Walks `PROT.DAT` end-to-end and pages through every entry, showing
classifier output (TIM hits, TMD hits, scene-bundle membership) and
naming each entry from `CDNAME.TXT`.

## Architecture

The viewer is the only place where Track 1 (asset crates) and Track 2
(engine crates) currently meet:

```text
legaia-iso ─┐                                      ┌─ winit  (input)
legaia-prot ┼─ legaia-asset ─┬─ legaia-tim ──────┐  │
            │                ├─ legaia-tmd ──────┼──┴─ legaia-engine-render
            │                └─ legaia-vab ──────┐
            │                                    └─── legaia-engine-audio
            └─ asset-viewer (this crate)
```

## See also

- [`docs/tooling/extraction.md`](../../docs/tooling/extraction.md) - how
  to populate `extracted/` first.
- [`docs/subsystems/renderer.md`](../../docs/subsystems/renderer.md)
- [`docs/subsystems/asset-loader.md`](../../docs/subsystems/asset-loader.md)
  - explains why `--vram-extra-dir` exists and what's blocking its
  removal (overlay sweep of field/town scene-init).
