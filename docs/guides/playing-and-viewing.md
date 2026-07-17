# Playing and viewing

`legaia-engine` is the clean-room engine reimplementation: point it at your
disc and it boots a scene, renders it, and takes pad input.
`asset-viewer` is its museum-mode sibling for browsing individual assets. Both
ship in the release archive; commands use the bare `./tool` form (source
builds live at `target/release/`).

Every subcommand accepts the disc directly via `--disc` - no extraction step
required to play. Without `--disc`, tools read an `extracted/` tree
(`--extracted-root`, default `extracted`, resolved against the current
directory) produced by `legaia-extract`
([getting-started.md](getting-started.md)).

## 1. Boot the engine

```bash
./legaia-engine play-window --disc "/path/to/Legend of Legaia (USA).bin"
```

This boots the scene `town01` (Rim Elm) straight off the disc: field
rendering, BGM, NPC scripts, dialog. Keyboard defaults: arrows = D-pad,
`Z` = Cross, `Esc` = quit. In-window extras: left-mouse drag orbits the
camera, `T` cycles the camera-distance preset, `R` toggles precise free-angle
movement (an opt-in enhancement - retail-style movement is the default),
`V` mutes audio. `--boot-ui` starts at the title screen → save-select flow
instead of jumping into the scene.

## 2. Pick a scene

```bash
./legaia-engine list-scenes --disc "/path/to/disc.bin"
./legaia-engine play-window --disc "/path/to/disc.bin" --scene town04
```

`list-scenes` prints every scene name the game's file map exposes with the
PROT entry range each covers - the same names feed `--scene`, and a range
start is where that scene's files land in `extracted/PROT/`.

## 3. Play the FMVs

```bash
./legaia-engine play-str MOV/MV1.STR --disc "/path/to/disc.bin"
```

With `--disc`, the argument is the movie's path *inside* the disc image and
the interleaved XA audio track plays in sync (the video clock is driven off
the audio cursor). Without `--disc` it plays a raw extracted `.STR` file
(video only). To dump frames as PNGs instead, use `mdec`
([extracting-assets.md](extracting-assets.md)). Background:
[cutscene.md](../subsystems/cutscene.md).

## 4. Saves and config live next to you

The engine resolves its files against the **current directory**: key bindings
in `legaia-input.toml`, options (camera preset, movement mode) in
`legaia-options.toml`, and save slots under `saves/`. Run from the same
directory each time - or pass explicit paths where supported
(`--save-dir`, `config set --config-file`).

Rebind keys with `config`:

```bash
./legaia-engine config show
./legaia-engine config set --binding Space=Cross
./legaia-engine config set --binding Enter=Start
```

`KEY=BUTTON` uses friendly key names (`Z`, `Up`, `Enter`, `RShift`) and PSX
pad button names (`Cross`, `Circle`, `Start`, `L1`).

## 5. Record and replay a session

```bash
./legaia-engine record --disc "/path/to/disc.bin" --out session.toml
./legaia-engine replay --input session.toml
```

`record` is `play-window` plus input capture into a small `j-replay-v1` TOML
file. The file is checkpointed to disk about once a second and finalized on
window close (`Esc`), so an interrupted session still yields a valid file up
to the last checkpoint. `replay` runs it back headless and deterministic - the
same file always produces a bit-identical engine trace, and it needs no disc
at all. Details: [determinism-replay.md](../tooling/determinism-replay.md).

`legaia-engine --help` lists many more subcommands; the `COMMAND GROUPS`
footer separates the player-facing ones above from the development
diagnostics (parity oracles, synthetic state drivers) you can ignore.

## 6. Browse assets interactively

`asset-viewer` reads the `extracted/` tree (there is no `--disc` here - run
`legaia-extract` first). The `field` and `dialog` demos additionally need the
dialog font at `extracted/font/`, which the pipeline writes by default (or
`font-extract --disc` rebuilds).

```bash
./asset-viewer prot extracted/PROT.DAT --cdname extracted/CDNAME.TXT   # archive browser
./asset-viewer tim  extracted/tim_scan/<entry>/raw_off<HEX>.tim        # one texture
./asset-viewer tmd  extracted/tmd_scan                                 # cycle 3D meshes
./asset-viewer vab  extracted/PROT/<entry>.BIN --sample 0              # play a sample
./asset-viewer field --scene town01                                    # playable field demo
```

In the PROT browser: `N` / `P` = next/prev entry, `PgDn` / `PgUp` = jump 10,
`Esc` = quit; each entry's format is auto-detected and the first viewable
sub-asset is shown. `tmd` pointed at a directory walks every mesh with the
same keys. The `tim` subcommand also takes `extracted/PROT.DAT` itself with
`--offset`/`--clut` for the system-UI textures that live outside any TOC
entry.

## 7. Read the game's scripts

The field/event VM ([script-vm.md](../subsystems/script-vm.md)) drives every
scene. Its disassembler is a release binary too:

```bash
./field-disasm scan-prot --prot extracted/PROT.DAT     # sweep for event scripts + FMV triggers
./field-disasm file <extracted-script-body>            # walk one raw script linearly
```

For a specific scene's per-scene scripts (LZS-compressed inside the scene's
MAN sub-asset), the engine has the direct path:

```bash
./legaia-engine man-scripts --scene town01 --disc "/path/to/disc.bin"
```

## Related docs

- [engine.md](../subsystems/engine.md) - the engine's architecture and clean-room boundaries.
- [renderer.md](../subsystems/renderer.md) - what "retail-faithful rendering" means here.
- [determinism-replay.md](../tooling/determinism-replay.md) - the replay format.
- [modding-and-translation.md](modding-and-translation.md) - randomize or translate the disc you just booted.
