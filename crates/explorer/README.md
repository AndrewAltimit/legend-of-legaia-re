# legaia-explorer

Desktop GUI for browsing, previewing and extracting the assets on a Legend of
Legaia disc. One binary: `legaia-explorer`. It is the native counterpart of the
static site's disc browser and viewer pages, for users who do not want a
terminal: everything the `legaia-extract` / `asset` / `tim` / `vab` command
lines do for a single asset is a button here.

No game data ships with it. The app reads the disc image the user picks, at
runtime, and writes only into folders the user chooses.

## What it does

| Area | What the user gets |
|---|---|
| Open | The start page (nothing open yet): the two ways in as large buttons, the recent list, what each view does, in a centred column that narrows with the window (`start_column`, `start_gaps`). A native file dialog (opened in the last disc's folder), a typed path, a dropped file or the in-app browser ([Controller](#controller)) for a `.bin` / `.cue` image or a `legaia-extract` folder. The image is checked (Mode2/2352, ISO9660, `PROT.DAT`) before loading, a file that is not a raw dump is named for what it is (`psx_iso::sniff`), and one shorter than its file table says is called incomplete (`legaia_app_theme::disc_text`); the last choice is reopened on the next start; a moved last disc is named on the error line. The top bar's *Open* menu holds them all ([below](#the-top-bar)). |
| Browse | The `PROT.DAT` entries grouped under the CDNAME block each belongs to on the disc (the retail block: the filename labels start every block two entries late) and the scene's display name (`place_name`, from `legaia_gamedata`'s scene table), each with its kind in plain words and size; a search whose every term must match the retail block, the place name, the filename label, the kind, the category or the index (`Explorer::entry_matches`), plus a *Show* filter over the kinds with each one's count, with a match count and a hint when nothing matches; arrow / Page / Home / End keys walk the visible rows; the disc's ISO9660 files with a save button. The next start reopens the selected entry. |
| Overview | The pane's heading names the entry's retail block as the tree does; the file name, which can name a neighbour, is on its hover. Where an entry opens when it has nothing for the chosen preview tab; such a tab is greyed out without a count (`tab_label`), and a pane narrower than `TAB_COUNTS_MIN_WIDTH` shows no counts at all, which keeps the tab row to one line at the minimum window width. Plain words first, the technical names behind a toggle ([below](#the-overview-tab)); raw `.BIN` export. |
| Textures | Every TIM in the entry, raw or inside an LZS section (`tim_scan`), with a CLUT slider and zoom (the slider, Ctrl + scroll over the image, or *Fit* for the whole texture in the pane; dragging the image moves it, and the pointer shows the texel it is on); PNG export of one or all. The list takes at most half the pane (`tim_list_width`), so the sliders keep their room at the minimum window width; a row too long for it ends in an ellipsis and shows whole on hover. A 4 / 8 bpp TIM with no CLUT in its file (its palette is another upload's VRAM cell) shows its indices as a grey ramp. |
| 3D model | Every TMD (`tmd_scan`, `scene_tmd_stream`) in an orbit view (drag, scroll, double-click to reset; the triangle count and the hint are drawn inside the view, on the theme's `stage` backdrop) with retail shading (`texel * colour / 128`), as a see-through wireframe, or untextured under a viewer light, picked from one compact toolbar row that holds at the minimum window width. Textures come from the entry's own TIMs or from its **scene** (see below); the view opens on the entry only when its TIMs hold every CLUT and texel the textured prims read (`Model::texture_coverage`), else on the scene; OBJ and textured `.glb` export from the *Export* menu. |
| Audio | VAB banks (spans resolved through the DATA_FIELD body chunk) with per-sample length, waveform (decoded as a row scrolls into view; it narrows to leave the row's Play and WAV buttons on screen at the minimum window width, `wave_width`), play and WAV export; SEQ playback on the `legaia-engine-audio` SPU + sequencer with a chosen bank. The [transport row](#the-audio-transport) shows how far the sound is. |
| Text | Field dialog read out of the scene MAN script and raw carriers the way `legaia-patcher translate export` reads it, in the language-pack markup and with the build's own codec (accented cells shown as letters on the PAL builds, `legaia_font::latin`); MES messages (Compact: per offset-table message; Records: the `0x1F`-led segments of each record); a *Find* field that keeps the lines holding every typed word (`line_matches`) and says how many; text export of every line, whatever is found. |
| Hex | A virtualised hex / ASCII dump of the whole entry, 16 bytes a row, or 8 where 16 do not fit the pane (`hex_row_bytes`), so a row never wraps; *Go to* takes a hex offset (`parse_hex_offset`) and puts its row at the top, or says the entry is shorter. |
| Characters & monsters | The four party members assembled from their player files (PROT 863..866) with a per-section equipment picker named from the build's own executable (`ItemNameTable::from_exe`: the USA table or the French, German or Italian one, accents kept), and the monster archive (PROT 867) listed by name as the font draws it, with its element badge; every battle animation clip, and for Vahn, Noa and Gala every Tactical Art by name, plays or steps frame by frame (`Space` plays and pauses where it stopped, `Left` / `Right` step a frame); animated `.glb` export. The five equipment pickers fold under one *Equipment* header (closed, naming how many pieces are changed) when they would need more than one row. |
| Movies & voice | The STR movies (`MOV/*.STR`), each saying where it plays and which scene the game goes on in (`media::movie_note`, from the FMV table's hand-offs in [`str-fmv-table.md`](../../docs/formats/str-fmv-table.md)): frames decoded on demand (Iki MDEC), played paced by their XA soundtrack's clock (a movie resumed from a paused frame resumes its soundtrack at that frame's sample), PNG frame export, with the same `Space` and `Left` / `Right` keys as the animations; the CD-XA files (`XA/*.XA`) demultiplexed per `(file, channel)` and decoded at their subheader rate, with play and WAV export, each file and channel naming what plays where a cue table does (`media::xa_file_note`, `xa_channel_note`); a channel plays under the [transport row](#the-audio-transport). Disc images only. |
| Play | Starts `legaia-engine` (`command-line-tools/` in the archive, or beside a `cargo build`'s binary) as `play-window --disc <bin> --boot-ui` on the open USA disc, in the engine's per-user data folder, so saves and settings match a game started directly (`launch`). Its output goes to `legaia-explorer-game.log` there; a failed exit puts the reason on the error banner, whose *Copy report* takes the log's end along (`game_exit::report_text`). `F5` presses it, or says why not; the launcher's switches ride along (below). A start also writes the disc into the engine's `launcher.toml` (`record_disc_for_engine`: the `disc` key only), so the game started directly boots it without its picker. Disabled with the reason on hover for an extracted folder, another region's disc, a missing engine. |
| Status line | A settings file that will not save says so on the error banner, once. A confirmation clears itself after a few seconds (the clock restarts while the pointer is on it); an error is a wrapping banner that stays, with *Copy* and *Dismiss* ahead of the text so a long path never pushes them off the window; a game that stopped with an error adds *Copy report* and *Show log* (the folder holding its log). |
| View | Light, dark or follow-system theme, the UI zoom (also Ctrl+= / Ctrl+- / Ctrl+0), both remembered (the zoom is held to what the window has room for, `settings::fit_zoom`, so a small window keeps the layouts of the minimum size and a larger one gets the asked-for zoom back), and the keyboard-shortcuts box (`?`). Both themes carry the Legaia look (`theme`, over the shared [`legaia-app-theme`](../app-theme/README.md) palette the launcher draws in too): blue-grey panels, the palette's button states (a pressed button goes gold, as on the launcher) and the title gold for the selection and the primary buttons. |
| About | Version, license, where `THIRD-PARTY-LICENSES.txt` sits (beside the program, or beside the macOS `.app`). The box scrolls when the window is shorter than it (`about_max_height`), so its title bar stays on screen. |
| Extract | The full `legaia-extract` pipeline (`legaia_extract::run`) on a worker thread into a chosen folder, with the CLI's skip options, a progress bar, a log, Cancel and *Open output folder* (progress and outcome: [below](#extraction-progress)); the folder defaults to one beside the disc image; *Every texture as PNG*, on unless cleared, is the every-TIM sweep (`--all-textures`: `tim_scan/`), without which a run writes almost no images. |

Every view that needs a disc shows the start page until one is open. The page closes
its gaps on a short window (`start_gaps`), so all of it shows at the minimum
size. The *Movies & voice* list opens scrolled to the file open last.

## Kinds

A browse row names what the entry mostly is in plain words (`kind_of_class`:
*Scene textures and models*, *3D models*, *Sound and music*). The raw
`legaia_disc_audit::categorize` category, classified in the background, is on
the row's hover and the Overview tab, and the search finds either. The kind is
a disc-wide label: a scene's model stream carries textures too, which its tabs
count.

## The top bar

One *Open* menu holds the disc dialog (Ctrl+O), the folder dialog, *Type a
path* (Ctrl+L) and the recent list. The path field shows on its own only
while nothing is open; with a disc open the bar is one row of controls plus
the disc line at the minimum window width. Below about 860 points the app's
name leaves the bar (it stays in the window's title bar), and View and About
move to the right edge only when the row has room left for them.
The recent list (also on the start screen) holds up to eight sources, newest
first (`Settings::remember_recent`); gone paths are pruned, and each has a
remove button.

## The audio transport

The Audio tab and an XA file open with one row: *Stop*, how far the playing
sound is against its length, and its name, cut to the room left. A sample
shows a bar on the wall clock; a streamed channel shows a slider on the XA
voice's own cursor, and dragging it (or clicking the rail) restarts the
channel from there (`Player::seek_stream`). A sequence has no length to show,
so it gives its time, tick and tempo. A sound that has played out reads
*Stopped* again (`Player::poll`): a sample or channel past its length, a
sequence that is not looped once its sequencer has finished.

## Extraction progress

The options sit under an *Options* header that a started run folds, which
leaves the log its room at the minimum window height. A run writes raw models
(`.tmd`); one model as OBJ or glTF comes from Export in the views.

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
(same-named files are replaced; no warning for the files the run just finished
wrote) or is a file (`extract_job::OutDir`), and an
*Open* button shows an existing folder. A run goes on behind the other views;
its end is announced once on the status line of whichever view is open, a
finished or cancelled run with *Show folder*, a failure on the error banner
(`ExtractJob::take_finished`).

## How it is built

- **Toolkit:** `eframe` / `egui` on the **glow** (OpenGL) backend. Explorer
  draws only egui's own meshes (the model view is a CPU rasteriser, below),
  so the glow backend keeps `wgpu` out of its build entirely; `eframe`'s
  winit is the workspace's 0.30. `App::logic` polls the background work
  (loads, the extraction, a started game), which goes on while the window is
  minimised; `App::ui` reads the frame's input and lays out the panels on
  the root `Ui`. egui draws text through skrifa, with hinting.
- **No format logic of its own.** `analyze` finds sub-assets with the format
  crates' scanners, `export` calls the existing writers
  (`psx_tim::write_png`, `legaia_actor_formats::scene_gltf`,
  `psx_vab::write_wav`), and extraction is `legaia-extract`'s library.
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
  `psx_tim::Vram` with the page / CLUT addressing the site viewer's shader
  uses; the wgpu renderer is not reachable from a glow context.
- **Meshes baked black.** Some meshes draw near black with retail shading
  although their textures resolve: `town01`'s dark-sky shell (PROT 6) has an
  untextured sky dome authored `(0, 0, 0)` at every corner and near-black
  colour words over a dim palette on the island. When most of a mesh's
  surface is baked that dark (`Model::dark_baked_share`), the model tab says
  the darkness is the disc's (one line, the explanation on hover, with a
  *Show the shape* button); the Lit aid floors colours to a grey so the
  shape still reads.
- **Text codec.** `Library::text_codec` picks the dialog codec from the boot
  executable the way `legaia_translate::build::detect` does (Latin with the
  high accented cells on every Latin build but the USA one).
- **Streamed media** (`media`) reads raw 2352-byte sectors:
  `psx_xa::demux` + `psx_xa::decode` for the XA files, and
  `psx_mdec::str_av::StrAvDemuxer` for a movie. Sound goes out on the
  mixer's XA voice (`AudioSink::play_xa`), whose cursor is the clock the
  movie preview paces its frames by.
- **Play is the launcher's Play.** *Play* runs `legaia-engine launch --disc
  <bin>`, which the engine expands to the launcher's own argument list from
  `launcher.toml`: every launch switch, gameplay tweak and the expansion pack
  (`launch::play_args`). A game started from either program is the same game,
  and a switch added to the launcher needs nothing here. The enhancement
  options live in the game's `legaia-options.toml`, which both starts read.
  *Game options* beside Play (`Shift+F5`; *Options* when the bar is short
  of room) opens the game's own launcher screen on the open disc
  (`launch::launcher_args`, the engine's `--open`): the look, enhancements,
  tweaks, keys and saves a start from here plays with, and its own Play.
- **Dropped files on Wayland.** winit 0.30 has no Wayland drag-and-drop, so
  on a Wayland session that also offers XWayland Explorer opens as an X11
  client, as the launcher does (`legaia_app_theme::drop_backend`), and a disc
  dropped from a Wayland file manager arrives. `LEGAIA_EXPLORER_WAYLAND=1`
  keeps the window native, where only the dialogs and the typed path open a
  disc.
- **What a drop opens.** While files are dragged over the window a frame and
  a line say what a drop opens. Of several dropped paths the first that can
  be a source opens (a `.cue` over its `.bin`, `pick_drop`), and the status
  line says the rest were left. An expansion pack (`.lxpack`, a folder with
  `pack.toml`) or a memory card (`library::CARD_EXTENSIONS`) is named for what
  it is and sent to the game's launcher (`library::not_a_source`), instead of
  being refused as a broken disc image.
- **Settings.** `settings.json` in the platform config folder
  (`LEGAIA_EXPLORER_SETTINGS` overrides the path). One that does not parse is
  renamed to `settings.json.broken` and named on the error banner, never
  overwritten by the defaults; one that can be neither read nor moved is left
  alone and not written. Every save lands whole (temp file, then rename):
  `legaia_app_theme::settings_file`.
- **UI-free model.** `Explorer` (loading, selection, background classification
  and analysis), `ExtractJob`, `media` and the exporters have no egui
  dependency, so `tests/smoke.rs` drives them headlessly.

On Windows the binary is a `windows`-subsystem program (no console window);
it has no command-line contract beyond an optional path to open. The release
archive ships it at its top level as `Legaia Explorer` (`.exe` on Windows,
`.app` on macOS). Its icon (`legaia_app_theme::icons::EXPLORER_PNG`) is the
window icon, and `build.rs` embeds the `.ico` into the Windows executable
(`legaia_app_theme::win_icon`).

## Running

```bash
cargo run -p legaia-explorer --release
legaia-explorer "/path/to/Legend of Legaia (USA).bin"     # open at start
```

Settings (last disc, the recent list, output and export folders, extract
options, window size and position, the view and preview tab last open, the
character or monster and the movie last open, each view's left panel width,
theme and zoom) live in `legaia-explorer/settings.json` under the per-user
config folder. The window position is restored only when its title bar
would land on a monitor that is still connected (`legaia_app_theme::placement`, checked
against the monitors the windowing system reports as the event loop
starts); otherwise the window opens centred.

Keyboard: Ctrl+O opens a disc image, Ctrl+L shows the path field, Ctrl+1 .. Ctrl+4 switch between Browse,
Characters & monsters, Movies & voice and Extract, F5 plays the open disc,
Shift+F5 opens the game's options (its launcher screen),
Ctrl+E presses the shown view's main export (the selected texture's PNG, the
model as textured glTF, the raw `.BIN` on Overview and Hex, the first text
block, the character or monster `.glb`, the movie frame; a view with nothing
to save says so on the status line),
Ctrl+F focuses the archive search (Escape clears it), Ctrl+Tab /
Ctrl+Shift+Tab (or Ctrl+Page Down / Up) step through the preview tabs the
selected entry has something for, F1 toggles the About
box, `?` (or *View*) opens the shortcuts box, Esc closes either, Ctrl+= / Ctrl+- / Ctrl+0 zoom, and Up / Down, Page Up / Down, Home / End
walk the archive rows the search shows, opening blocks as they go, and the
same keys walk the party and monster list and the movie and voice list
(`list_step`).

Controller (`gamepad`): each button is handed to egui as the key a keyboard
user would press, so a pad reaches every view the keyboard does with no arm
of its own - the D-pad or left stick is the arrows, Cross `Enter`, Circle
`Esc`, Triangle `Space`, L1 / R1 `Shift+Tab` / `Tab` (between controls),
L2 / R2 the preview tabs, Select the next view, Start `F5`, and the right
stick the mouse wheel. The pads are read on a thread that wakes the window
only when a button moves, so an idle window with a pad plugged in draws
nothing; a scripted run opens none. The shortcuts box lists the buttons
under the keys, and scrolls in a window too short for both.

### The Overview tab

It leads with the entry's kind and one sentence on what that is
(`kind_blurb`), the place it belongs to (`place_name` over the retail
block), what it holds in words (`contents_plain`: textures, 3D models, sound
banks, lines of dialog, ...), its size and its entry number, each row's
technical name on its hover. *Show technical details* (remembered,
`overview_details`) opens the PROT index, the filename label and the retail
CDNAME block (the two differ by the +2 index shift,
[`cdname.md`](../../docs/formats/cdname.md)), the audit category, location in
`PROT.DAT`, exact size, the sub-assets by format name and the DATA_FIELD
chunk list. A value wider than the panel ends in an ellipsis and shows whole
on hover.

### Controller

The native file dialog stays out of a pad's reach, so while a controller was
the last thing pressed (`pad_active`; a key or a mouse button clears it)
*Open disc image*, *Open extracted folder* and the Extract view's
*Choose...* open the in-app file browser (`file_browser`) instead: the
folder's subfolders and the files it was opened for, the way up, and the
usual places (home, Downloads, Desktop, Documents, the drives), listed by
`legaia_app_theme::folder_list` as the game launcher's finder lists them.
The D-pad moves, Cross or Right opens a folder or takes the file, Left goes
up, Circle closes; the keyboard's arrows, Enter, Backspace and Esc do the
same. It also opens when the desktop's dialog comes back at once (it never
appeared: a desktop with no file-dialog service), and from the start page's
*find it in this window* link. An export with a controller in hand writes
into the remembered export folder (else `Legaia Explorer` in Pictures) under
its own name, never over an earlier file, and the status line says where.
With nothing focused on the start page, a controller's first press puts the
focus on the disc on offer, else on *Open disc image*.

The start page also offers the disc the game itself boots - the `disc` of the
engine's `launcher.toml` (`launch::engine_disc_offer`) - while it is not on the
recent list, so a disc chosen in the game's launcher opens here in one click.
On a first run with nothing to open again, it offers instead a disc image
named for the game found in the program's folder, Downloads, Desktop,
Documents or the home folder (`launch::found_disc_offer`, the search the
game's launcher makes, `legaia_app_theme::folder_list::find_dumps`).

### Scripted runs

These environment variables drive a run without a person at the keyboard
(used for screenshots and smoke checks):

| Variable | Effect |
|---|---|
| `LEGAIA_EXPLORER_OPEN` | Disc image or folder to open (same as the first argument). |
| `LEGAIA_EXPLORER_SELECT` | PROT index to select once loaded. |
| `LEGAIA_EXPLORER_TMD` | Show this TMD of the selected entry in the 3D model tab (the `Model #` picker) instead of the first. |
| `LEGAIA_EXPLORER_TAB` | A preview tab (`overview`, `textures`, `model`, `audio`, `text`, `hex`), a view (`browse`, `actors`, `media`, `extract`), `about` for the About box, or `keys` for the shortcuts box. |
| `LEGAIA_EXPLORER_SEARCH` | Start with this text in the archive search. |
| `LEGAIA_EXPLORER_FIND` | Start with this text in the Text tab's *Find* field. |
| `LEGAIA_EXPLORER_MEDIA` | Open *Movies & voice* on an ISO path (`MOV/MV1.STR`, `XA/XA14.XA`), a movie optionally at `@<frame>`. |
| `LEGAIA_EXPLORER_EXTRACT_TO` | Start an extraction into this folder. |
| `LEGAIA_EXPLORER_SCREENSHOT` | Save a PNG of the window once settled (after the extraction, if one runs), then quit. With nothing to open it captures the start screen. |
| `LEGAIA_EXPLORER_EXPORT_DIR` | Export buttons write here instead of opening a save dialog. |
| `LEGAIA_EXPLORER_ACTOR` | Open *Characters & monsters* on `char:<slot>` (0 Vahn .. 3 Terra) or `monster:<id>`, optionally posed `@<clip>:<frame>`. |
| `LEGAIA_EXPLORER_PLAY` | Press *Play* once the disc has loaded. |
| `LEGAIA_EXPLORER_BROWSE` | Open the in-app file browser at start: `disc`, `folder` (an extracted folder) or `out` (the extraction output folder). |
| `LEGAIA_EXPLORER_SETTINGS` | Settings file to use instead of the per-user one. |

The screenshot reads the window's own framebuffer, so on a shared desktop run
it on a private X server (`Xvfb :97 &`, then `DISPLAY=:97`).

## Tests

```bash
cargo test -p legaia-explorer                        # headless state + extract job
LEGAIA_DISC_BIN=/path/to/disc.bin cargo test -p legaia-explorer
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
