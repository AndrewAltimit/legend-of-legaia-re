# legaia-asset-viewer

Combined GUI viewer for everything the extraction pipeline produces.
One binary: `asset-viewer`. Driven by `winit` 0.30 + `wgpu` 26 via
[`legaia-engine-render`], with audio playback via [`legaia-engine-audio`].

## Subcommands

`tmd`, `field`, `dialog`, `battle-scene` and `world` read the `legaia-extract`
output tree (`--extracted-root` defaults to `extracted`, resolved against the
current directory); the rest take explicit file paths. `field` and
`dialog` additionally need the dialog font under `extracted/font/`, written by
`legaia-extract` or `font-extract --disc <bin>`.

```bash
asset-viewer tim   <input.tim> [--offset H] [--clut N]
asset-viewer save-icons <PROT_899> [--tile N] [--scale N]  # save-slot portraits
asset-viewer tmd   <input> [--shape character] [--sort-by-size] [--bundle battle] [--scene <CDNAME>]
asset-viewer stage <PATH>                       # wireframe stage geometry
asset-viewer vab   <PROT_entry> --offset <H> --sample <N>
asset-viewer seq   <file.seq> <file.vab> [--vab-offset H] [--looped]
asset-viewer prot  <PROT.DAT> [--cdname <CDNAME.TXT>] [--scus <SCUS_942.54>]
asset-viewer field <SCENE> [--record N] [--cycle-records] [--max-actors N]
asset-viewer dialog <MES_blob> [--message N]    # typewriter-paced dialog box
asset-viewer battle-scene [--queued-action N]   # battle-action SM driver
asset-viewer world <SCENE> [--max-actors N]     # engine-core World composite demo
```

`save-icons` paints the sixteen 16x16 save-slot tiles that sit row-interleaved
across one 256x16 strip, each through its **own** 16-colour palette - which is
exactly what the plain `tim` mode cannot do. Tile N is the memory-card icon
for save N+1; tile 15 is blank padding.

`--cycle-records` is a bare flag (on by default): when the active record
reaches Halt or Unknown the runner advances to the next record, so a single
session exercises every record in order.

### `tmd` - textured 3D meshes

Renders Legaia TMDs spinning. Uploads every sibling TIM into the shared
software VRAM model so meshes that reference textures across multiple VRAM
pages render correctly; the textured path draws the TMD's baked colour words
through the retail texture blend and depth cue, with no light source.

Useful flags:

- `--bundle battle` - overlay the empirically-tuned extraction 865–890
  `tim_scan` set traced from `FUN_800520f0` (battle / `level_up` /
  `monster_se` meshes share its character-body palettes).
- `--scene <CDNAME>` - overlay the `tim_scan` dirs of every PROT entry in
  that CDNAME block, the set the field / town loader co-loads for a scene.
- `--vram-extra-dir <dir>` - workaround for character meshes whose CLUT
  rows live in *different* PROT entries from their TMD source.
- `--no-textures` (alias `--flat-shaded`) - skip the VRAM path entirely
  and render bare flat-shaded geometry under a single directional light (a
  viewer aid, not retail shading). Use this when you want to see
  what a mesh's silhouette looks like without battling palette guesses;
  the runtime LoadImage trace for field / town scenes isn't captured
  yet, so some palette rows always render as garbage in textured mode.

When VRAM is built from one or more TIM directories, the `tmd` viewer
drops primitives whose texture page region or whose own CLUT entries no
loaded TIM has populated - those would otherwise rasterise as solid
`CLUT[0]`, a flat green / cyan tint over correctly-textured geometry that
obscures the rest of the model. It does **not** judge a prim by how wide
its CLUT row is: a row legitimately holds up to 64 packed palettes, so
width cannot separate a spilled texture from a dense palette bank (see
`docs/subsystems/renderer.md`).
The diagnostic logs distinguish each failure mode:

```
skipped N prim(s) (M/N kept)
  missing CLUT data for K prim(s) across rows [r0, r1, ...]
  missing texture-page data for K prim(s) across tpages [t0, t1, ...]
```

Primitive-section walks are lenient: a malformed group near the end of an
object's prim section does not hide the valid groups before it, so a
multi-object TMD renders every part of the model that walks cleanly.

For offline diagnostics the same targeted-upload + per-prim verdict
logic is also exposed by the `tmd` CLI: `tmd prims <input> --vram-dir
<dir>` prints a per-prim status tag (`Ok` / `MissingClut` /
`ClutDepthMismatch` / `MissingTexturePage`), and `tmd vram-dump <input>
-o vram.png [--annotate]` writes the simulated post-upload VRAM as a
PNG so collisions are obvious without firing up the GUI.

### `stage` - wireframe stage geometry

Renders the 12-byte-prefix + 8-byte u16 quad records identified by
`legaia_asset::stage_geom`, through `legaia-engine-render`'s `Lines`
pipeline. `stage-scan` (in the `asset` CLI)
finds candidate entries; this viewer renders one. OBJ export is
supported via `asset stage` proper.

### `prot` - PROT entry browser

Walks `PROT.DAT` end-to-end and pages through every entry, showing
classifier output (TIM hits, TMD hits, scene-bundle membership) and
naming each entry from `CDNAME.TXT`.

A `scene_tmd_stream` entry is previewed the way retail *places* it rather than
drawn raw: object 1 dropped, and a second copy under the stage's transform
(`legaia_asset::battle_backdrop`). The transform comes from `SCUS_942.54`,
found beside `PROT.DAT` or named with `--scus`; without it the preview uses
retail's default and the status line says the transform is unresolved.

### `field` - field-VM scene runner

Boots a CDNAME scene and steps the field VM through the scene's event-script
records. The viewer's frame tick mirrors `World::tick`'s mode dispatch but calls
`engine-core::World::step_field` itself, so the HUD can observe each
`StepResult`. The HUD surfaces:

- Step-outcome tally (`adv / yld / halt / pending / unknown`).
- Last opcode dispatched + a per-opcode top-5 histogram so naturalistic
  playthroughs surface which ops a scene's prescript actually exercises.
- Per-`FieldHost`-callback counter (`Bgm / PlaySfx / OpenDialog / ...`)
  drained from `World::drain_field_events` each tick.

The session-end summary (printed on Esc / window close) dumps the full
top-10 opcode histogram and host-callback tally to stderr - useful
for closing the loop on remaining `Pending` sub-cases by observing
what real scenes do.

## Architecture

The viewer composes Track 1 (asset crates) with Track 2 (engine crates):

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
