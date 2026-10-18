# legaia-studio

Desktop GUI for browsing, previewing and extracting the assets on a Legend of
Legaia disc. One binary: `legaia-studio`. It is the native counterpart of the
static site's disc browser and viewer pages, for users who do not want a
terminal: everything the `legaia-extract` / `asset` / `tim` / `vab` command
lines do for a single asset is a button here.

No game data ships with it. The app reads the disc image the user picks, at
runtime, and writes only into folders the user chooses.

## What it does

| Area | What the user gets |
|---|---|
| Open | A native file dialog (or a typed path, or a file dropped on the window) for a `.bin` / `.cue` disc image or a `legaia-extract` output folder. The image is validated (Mode2/2352, ISO9660, `PROT.DAT` present) before loading; the last choice is reopened on the next start. |
| Browse | The `PROT.DAT` entries grouped under their CDNAME block names, each with its category (`legaia_disc_audit::categorize`, classified in the background) and size; a name / index filter and a category filter; the disc's ISO9660 files with a save button. |
| Overview | PROT index, the filename label and the retail CDNAME block (the two differ by the +2 index shift, [`cdname.md`](../../docs/formats/cdname.md)), location in `PROT.DAT`, the DATA_FIELD chunk list, raw `.BIN` export. |
| Textures | Every TIM in the entry, raw or inside an LZS section (`tim_scan`), with a CLUT slider and zoom; PNG export of one or all. |
| 3D model | Every TMD (`tmd_scan`, `scene_tmd_stream`) in an orbit view with retail shading (`texel * colour / 128`) or untextured under a viewer light. Textures come from the entry's own TIMs or from its **scene** (see below); OBJ and textured `.glb` export. |
| Audio | VAB banks (spans resolved through the DATA_FIELD body chunk) with per-sample play and WAV export; SEQ playback on the `legaia-engine-audio` SPU + sequencer with a chosen bank. |
| Text | Field dialog read out of the scene MAN script and raw carriers the way `legaia-patcher translate export` reads it, in the language-pack markup; Compact-layout MES messages; text export. |
| Hex | A virtualised hex / ASCII dump of the whole entry. |
| Characters & monsters | The four party members assembled from their player files (PROT 863..866) with a per-section equipment picker named from the executable's item table, and the monster archive (PROT 867) listed by name; every battle animation clip plays or steps frame by frame; animated `.glb` export. |
| Extract | The full `legaia-extract` pipeline (`legaia_extract::run`) on a worker thread into a chosen folder, with the CLI's skip options, a progress bar, a log and Cancel. |

## How it is built

- **Toolkit:** `eframe` / `egui` on the **glow** (OpenGL) backend. No egui
  release builds against the workspace's `wgpu` 26, and the glow backend keeps
  a second `wgpu` out of the tree; `eframe`'s winit is the workspace's 0.30.
- **No format logic of its own.** `analyze` finds sub-assets with the format
  crates' scanners, `export` calls the existing writers
  (`legaia_tim::write_png`, `legaia_actor_formats::scene_gltf`,
  `legaia_vab::write_wav`), and extraction is `legaia-extract`'s library.
- **Scene textures.** Most scene meshes sample pages and CLUTs another
  entry of their CDNAME block uploads, so a model can be drawn against the
  VRAM the engine builds for its scene: `SceneResources` built the way the
  play host builds a field (every TIM of the block, the shared `init_data` /
  `player_data` blocks, the system-UI bundle layered under, row-479 NPC CLUTs
  merged by the normal upload), with the battle load kind for a
  `scene_tmd_stream` backdrop and the world-map kind for the kingdom maps
  (`scene_vram`). Built on a worker, cached per block.
- **Actors** are `legaia_actor_formats` end to end (`actors`):
  `battle_char_assembly::loadout::build` gives a character's assembled mesh,
  its VRAM band and its clip bank (record[0] actions plus the
  equipment-spliced swings); `monster_archive` gives a monster's mesh, its
  texture pool placed as battle slot 0, and its action clips. Posing applies
  each part's rigid transform per object, the way
  `tmd_to_vram_mesh_posed_rot` does; clips advance at `rate * 7.5` keyframes
  per second (retail: `rate / 8` per 60 Hz tick).
- **Model preview** is a small CPU rasteriser (`raster`) over
  `legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid`, sampling a
  `legaia_tim::Vram` with the page / CLUT addressing the site viewer's shader
  uses; the wgpu renderer is not reachable from a glow context.
- **UI-free model.** `Studio` (loading, selection, background classification
  and analysis), `ExtractJob` and the exporters have no egui dependency, so
  `tests/smoke.rs` drives them headlessly.

On Windows the binary is a `windows`-subsystem program (no console window);
it has no command-line contract beyond an optional path to open.

## Running

```bash
cargo run -p legaia-studio --release
legaia-studio "/path/to/Legend of Legaia (USA).bin"     # open at start
```

Settings (last disc, output and export folders, extract options) live in
`legaia-studio/settings.json` under the per-user config folder.

### Scripted runs

These environment variables drive a run without a person at the keyboard
(used for screenshots and smoke checks):

| Variable | Effect |
|---|---|
| `LEGAIA_STUDIO_OPEN` | Disc image or folder to open (same as the first argument). |
| `LEGAIA_STUDIO_SELECT` | PROT index to select once loaded. |
| `LEGAIA_STUDIO_TAB` | `overview`, `textures`, `model`, `audio`, `text`, `hex` or `extract`. |
| `LEGAIA_STUDIO_EXTRACT_TO` | Start an extraction into this folder. |
| `LEGAIA_STUDIO_SCREENSHOT` | Save a PNG of the window once settled (after the extraction, if one runs), then quit. |
| `LEGAIA_STUDIO_EXPORT_DIR` | Export buttons write here instead of opening a save dialog. |
| `LEGAIA_STUDIO_ACTOR` | Open *Characters & monsters* on `char:<slot>` (0 Vahn .. 3 Terra) or `monster:<id>`, optionally posed `@<clip>:<frame>`. |
| `LEGAIA_STUDIO_SETTINGS` | Settings file to use instead of the per-user one. |

The screenshot reads the window's own framebuffer, so on a shared desktop run
it on a private X server (`Xvfb :97 &`, then `DISPLAY=:97`).

## Tests

```bash
cargo test -p legaia-studio                        # headless state + extract job
LEGAIA_DISC_BIN=/path/to/disc.bin cargo test -p legaia-studio
```

A third disc-gated test assembles all four party members and a monster,
renders them textured, exports their `.glb`s, and checks that town01's
environment pack draws more of itself against the scene VRAM than against
its own (TIM-less) entry.

The disc-gated test loads the real disc, checks the 1233 PROT entries and the
CDNAME grouping, and runs one texture, model and sample through the preview
model and every exporter; a second one cancels an extraction. Both skip and
pass without `LEGAIA_DISC_BIN`.
