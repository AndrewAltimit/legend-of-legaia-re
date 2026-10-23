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
| Open | The start page (nothing open yet): the two ways in as large buttons, the recent list, what each view does. A native file dialog (opened in the last disc's folder; or a typed path, or a file dropped on the window) for a `.bin` / `.cue` disc image or a `legaia-extract` output folder. The image is validated (Mode2/2352, ISO9660, `PROT.DAT` present) before loading; the last choice is reopened on the next start, and a last disc that has moved is named on the error line. Every view that needs a disc shows the start page until one is open. *Recent* (and the start screen) lists up to eight recent sources, newest first (`Settings::remember_recent`); gone paths are pruned, and each has a remove button. |
| Browse | The `PROT.DAT` entries grouped under their CDNAME block names and the scene's display name (`place_name`, from `legaia_gamedata`'s scene table), each with its category (`legaia_disc_audit::categorize`, classified in the background) and size; a search whose every term must match the CDNAME label, the place name, the retail block, the category or the index (`Studio::entry_matches`), plus a category filter, with a match count and a hint when nothing matches; arrow / Page / Home / End keys walk the visible rows; the disc's ISO9660 files with a save button. A preview tab the entry has nothing for shows its Overview. |
| Overview | PROT index, the filename label and the retail CDNAME block (the two differ by the +2 index shift, [`cdname.md`](../../docs/formats/cdname.md)), location in `PROT.DAT`, the DATA_FIELD chunk list, raw `.BIN` export. |
| Textures | Every TIM in the entry, raw or inside an LZS section (`tim_scan`), with a CLUT slider and zoom; PNG export of one or all. A 4 / 8 bpp TIM with no CLUT in its file (its palette is another upload's VRAM cell) shows its indices as a grey ramp. |
| 3D model | Every TMD (`tmd_scan`, `scene_tmd_stream`) in an orbit view with retail shading (`texel * colour / 128`) or untextured under a viewer light. Textures come from the entry's own TIMs or from its **scene** (see below); the view opens on the entry only when its TIMs hold every CLUT and texel the textured prims read (`Model::texture_coverage`), else on the scene; OBJ and textured `.glb` export. |
| Audio | VAB banks (spans resolved through the DATA_FIELD body chunk) with per-sample play and WAV export; SEQ playback on the `legaia-engine-audio` SPU + sequencer with a chosen bank. |
| Text | Field dialog read out of the scene MAN script and raw carriers the way `legaia-patcher translate export` reads it, in the language-pack markup and with the build's own codec (accented cells shown as letters on the PAL builds, `legaia_font::latin`); MES messages (Compact: per offset-table message; Records: the `0x1F`-led segments of each record); text export. |
| Hex | A virtualised hex / ASCII dump of the whole entry. |
| Characters & monsters | The four party members assembled from their player files (PROT 863..866) with a per-section equipment picker named from the build's own executable (`ItemNameTable::from_exe`: the USA table or the French, German or Italian one, accents kept), and the monster archive (PROT 867) listed by name as the font draws it, with its element badge; every battle animation clip, and for Vahn, Noa and Gala every Tactical Art by name, plays or steps frame by frame; animated `.glb` export. |
| Movies & voice | The STR movies (`MOV/*.STR`): frames decoded on demand (Iki MDEC), played paced by their XA soundtrack's clock (a movie resumed from a paused frame resumes its soundtrack at that frame's sample), PNG frame export; the CD-XA files (`XA/*.XA`) demultiplexed per `(file, channel)` and decoded at their subheader rate, with play and WAV export. Disc images only. |
| Play | Starts `legaia-engine` (found beside this binary, or beside the macOS `.app`) as `play-window --disc <bin> --boot-ui` on the open USA disc, in the engine's per-user data folder, so saves and settings are shared with a game started directly (`launch`). Its output goes to `legaia-studio-game.log` there; a failed exit puts the log's last lines on the error line. `F5` presses it; the launcher's switches ride along (below). A start also writes the disc into the engine's `launcher.toml` (`record_disc_for_engine`: the `disc` key only, other keys kept), so the game started directly boots it without its picker. Disabled with the reason on hover for an extracted folder, another region's disc, or a missing engine. |
| Status line | A confirmation clears itself after a few seconds (the clock restarts while the pointer is on it); an error is a wrapping banner that stays, with *Copy* and *Dismiss* ahead of the text so a long path never pushes them off the window. |
| View | Light, dark or follow-system theme, and the UI zoom (also Ctrl+= / Ctrl+- / Ctrl+0); both remembered. Both themes carry the Legaia look (`theme`, over the shared [`legaia-app-theme`](../app-theme/README.md) palette the launcher draws in too): blue-grey panels and the title gold for the selection and the primary buttons. |
| About | Version, license, where `THIRD-PARTY-LICENSES.txt` sits (beside the binary, or beside the macOS `.app`), the keyboard shortcuts. |
| Extract | The full `legaia-extract` pipeline (`legaia_extract::run`) on a worker thread into a chosen folder, with the CLI's skip options, a progress bar, a log, Cancel and *Open output folder* (progress and outcome: [below](#extraction-progress)); the output folder defaults to one beside the disc image; *Export all textures* adds the every-TIM sweep (`--all-textures`: `tim_scan/` as `asset tim-scan` + `tim convert-dir` write it). |

## Extraction progress

The Extract view numbers the steps the way the log does (verification is
step 0, the rest count from 1; the every-texture sweep adds one) and fills
the bar over the steps the run has, so a run without the sweep ends at 100%.
Under the bar sit the time so far and, once the current step has done a
twentieth of its units over at least two seconds, that step's remaining time
extrapolated from its own rate (`extract_job::Progress::step_remaining`).
There is no estimate for the whole run: the steps differ too much in cost.

Warnings are tagged in the log (`extract_job::WARNING_PREFIX`), counted, drawn
in the warning colour, and can be shown on their own. Before a run, a note
under the output folder says whether it is new, empty, already holds files
(same-named files are replaced) or is a file (`extract_job::OutDir`), and an
*Open* button shows an existing folder. A run goes on behind the other views;
its end is announced once on the status line of whichever view is open, a
finished or cancelled run with *Show folder*, a failure on the error banner
(`ExtractJob::take_finished`).

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
  (`scene_vram`). Over it go the two layers the native play host adds: the
  field-character atlas (PROT 874 section 2 entries 1..=3, strip CLUTs) for
  field and world-map loads, and the effect-texture pool (`etim`) laid
  under every load. Built on a worker, cached per block.
- **Actors** are `legaia_actor_formats` end to end (`actors`):
  `battle_char_assembly::loadout::build` gives a character's assembled mesh,
  its VRAM band and its clip bank (record[0] actions, the equipment-spliced
  swings, and the Tactical Arts `loadout::decode_art_clips` resolves from
  the art bank and the `readef.DAT` ME archives to curated names through
  `legaia_art::clip_ladder`, the same call the site's characters page
  makes);
  `monster_archive` gives a monster's mesh, its
  texture pool placed as battle slot 0, and its action clips. A monster name
  is shown as the font draws it: cell `0x24` (`$`) is the level mark (`Lv`,
  `Nv` on the French page), `*` the multiplication sign, and the `^X`
  escape is the plaque's element badge. Posing applies
  each part's rigid transform per object, the way
  `tmd_to_vram_mesh_posed_rot` does; clips advance at `rate * 7.5` keyframes
  per second (retail: `rate / 8` per 60 Hz tick).
- **Model preview** is a small CPU rasteriser (`raster`) over
  `legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid`, sampling a
  `legaia_tim::Vram` with the page / CLUT addressing the site viewer's shader
  uses; the wgpu renderer is not reachable from a glow context.
- **Meshes baked black.** Some meshes draw near black with retail shading
  although their textures resolve: `town01`'s dark-sky shell (PROT 6) has an
  untextured sky dome authored `(0, 0, 0)` at every corner and near-black
  colour words over a dim palette on the island. When most of a mesh's
  surface is baked that dark (`Model::dark_baked_share`), the model tab says
  the darkness is the disc's; the Lit aid floors colours to a grey so the
  shape still reads.
- **Text codec.** `Library::text_codec` picks the dialog codec from the boot
  executable the way `legaia_translate::build::detect` does (Latin with the
  high accented cells on every Latin build but the USA one).
- **Streamed media** (`media`) reads raw 2352-byte sectors:
  `legaia_xa::demux` + `legaia_xa::decode` for the XA files, and
  `legaia_mdec::str_av::StrAvDemuxer` for a movie. Sound goes out on the
  mixer's XA voice (`AudioSink::play_xa`), whose cursor is the clock the
  movie preview paces its frames by.
- **Play shares the launcher's switches.** The game's own launcher keeps
  two launch switches in `launcher.toml` (`psx_render`, `retail`) beside the
  disc; *Play* passes them too (`launch::launch_switches`), so a game started
  from either program looks the same. The enhancement options live in the
  game's `legaia-options.toml`, which both starts read.
- **UI-free model.** `Studio` (loading, selection, background classification
  and analysis), `ExtractJob`, `media` and the exporters have no egui
  dependency, so `tests/smoke.rs` drives them headlessly.

On Windows the binary is a `windows`-subsystem program (no console window);
it has no command-line contract beyond an optional path to open.

## Running

```bash
cargo run -p legaia-studio --release
legaia-studio "/path/to/Legend of Legaia (USA).bin"     # open at start
```

Settings (last disc, the recent list, output and export folders, extract
options, window size and position, the view and preview tab last open,
theme and zoom) live in `legaia-studio/settings.json` under the per-user
config folder. The window position is restored only when its title bar
would land on a monitor that is still connected (`placement`, checked
against the monitors the windowing system reports as the event loop
starts); otherwise the window opens centred.

Keyboard: Ctrl+O opens a disc image, Ctrl+1 .. Ctrl+4 switch between Browse,
Characters & monsters, Movies & voice and Extract, F5 plays the open disc,
Ctrl+F focuses the archive search (Escape clears it), F1 toggles the About
box, Ctrl+= / Ctrl+- / Ctrl+0 zoom, and Up / Down, Page Up / Down, Home / End
walk the archive rows the search shows, opening blocks as they go.

### Scripted runs

These environment variables drive a run without a person at the keyboard
(used for screenshots and smoke checks):

| Variable | Effect |
|---|---|
| `LEGAIA_STUDIO_OPEN` | Disc image or folder to open (same as the first argument). |
| `LEGAIA_STUDIO_SELECT` | PROT index to select once loaded. |
| `LEGAIA_STUDIO_TMD` | Show this TMD of the selected entry in the 3D model tab (the `TMD #` picker) instead of the first. |
| `LEGAIA_STUDIO_TAB` | A preview tab (`overview`, `textures`, `model`, `audio`, `text`, `hex`), a view (`browse`, `actors`, `media`, `extract`), or `about` for the About box. |
| `LEGAIA_STUDIO_SEARCH` | Start with this text in the archive search. |
| `LEGAIA_STUDIO_MEDIA` | Open *Movies & voice* on an ISO path (`MOV/MV1.STR`, `XA/XA14.XA`), a movie optionally at `@<frame>`. |
| `LEGAIA_STUDIO_EXTRACT_TO` | Start an extraction into this folder. |
| `LEGAIA_STUDIO_SCREENSHOT` | Save a PNG of the window once settled (after the extraction, if one runs), then quit. With nothing to open it captures the start screen. |
| `LEGAIA_STUDIO_EXPORT_DIR` | Export buttons write here instead of opening a save dialog. |
| `LEGAIA_STUDIO_ACTOR` | Open *Characters & monsters* on `char:<slot>` (0 Vahn .. 3 Terra) or `monster:<id>`, optionally posed `@<clip>:<frame>`. |
| `LEGAIA_STUDIO_PLAY` | Press *Play* once the disc has loaded. |
| `LEGAIA_STUDIO_SETTINGS` | Settings file to use instead of the per-user one. |

The screenshot reads the window's own framebuffer, so on a shared desktop run
it on a private X server (`Xvfb :97 &`, then `DISPLAY=:97`).

## Tests

```bash
cargo test -p legaia-studio                        # headless state + extract job
LEGAIA_DISC_BIN=/path/to/disc.bin cargo test -p legaia-studio
```

The disc-gated tests skip and pass without `LEGAIA_DISC_BIN`. One loads the
real disc, checks the 1233 PROT entries and the CDNAME grouping, and runs one
texture, model and sample through the preview model and every exporter; one
cancels an extraction. One assembles all four party members (Vahn, Noa and
Gala with their Tactical Arts clips) and a monster, checks the monster names
read as the font draws them, renders them textured, exports their `.glb`s,
and checks that town01's environment pack draws more of itself against the
scene VRAM (with both host layers loaded) than against its own (TIM-less)
entry. One checks the search forms and decodes an XA channel and a movie
frame with its soundtrack.
