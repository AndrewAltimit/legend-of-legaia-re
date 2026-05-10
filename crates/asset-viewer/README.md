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
