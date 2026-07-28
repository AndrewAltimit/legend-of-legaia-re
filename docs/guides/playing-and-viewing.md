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
rendering, BGM, NPC scripts, dialog, and the full gameplay loop - walking
rolls the scene's own random encounters, a battle opens the command menu, and
victory returns you to the field with XP, gold and drops. Keyboard defaults:
arrows = D-pad, `Z` = Cross, `Esc` = quit. In-window extras: left-mouse drag
orbits the camera, `T` cycles the camera-distance preset, `R` toggles precise
free-angle movement (an opt-in enhancement - retail-style movement is the
default), `V` mutes audio. `--boot-ui` starts at the title screen →
save-select flow instead of jumping into the scene.

Two flags turn parts of that back off:

- `--no-live-loop` stops random encounters from rolling, leaving field VM +
  locomotion only - the mode to use when you are inspecting a scene rather
  than playing it. A battle the engine is already in still resolves.
- `--no-player-battle` auto-attacks each party turn instead of opening the
  command menu.

**Towns have no random encounters, by design.** Rim Elm is one of them, and so
are most scenes with a shop in them: their encounter regions carry a zero
trigger rate, exactly as on the retail disc. The window says so in the corner
(`no random encounters in this scene`) so a quiet walk does not look like a
broken engine - step onto the overworld, or `--scene map03`, to fight.

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

## 4. Start a minigame from a live scene

Every ported minigame is a **mode suspend** on the running world, not a
separate program: the field scene stays loaded underneath and comes back when
you leave, so you start one from wherever you happen to be standing.

In `play-window` each is one key, and the same key leaves again: `L` fishing,
`K` dance, `O` the casino slot machine, `B` Baka Fighter, `M` Muscle Dome. Each
loads that minigame's overlay off the disc and installs a session, so the rules,
tables and scoring all come from the disc rather than from hardcoded numbers.

The browser play page carries the fishing entry as a **Fish here** button under
the canvas. Cross casts and reels, Square reels harder, Cross recasts once a
catch resolves - the same pad the field controller reads, because the driver is
the shared per-frame fishing tick rather than a per-host input handler. Points
bank into the world's persistent pool, so they survive leaving and re-entering,
and the prize-exchange rows come off the same overlay with retail availability
gating. Background: [minigame-fishing.md](../subsystems/minigame-fishing.md).

One HUD caveat that looks like a bug and is not: the fishing sprite page is the
one asset in that chain nobody has decoded, so both hosts draw the HUD's digit
and caption rows from the dialog font and skip its glyphs. The captions are
engine-side English placeholders at the retail pen positions, so a long
placeholder can overlap the count beside it.

## 5. What you hear

Both hosts decode their audio from the disc you supply - no samples ship with
the engine or the site. Music is a SEQ played through the clean-room SPU against
the scene's own sound bank; sound effects come from the executable's descriptor
table plus the resident program bank, and both hosts key them into the **same**
SPU the music uses, so a cue shares the voice pool exactly as it does on
hardware.

Cue *provenance* is worth knowing before you judge a sound. Retail fires an
effect by writing an id into a ring, and only a handful of those writes have
been traced, so a cue is either `disc` (the id is retail's, at the place retail
writes it) or `site` (retail's id there is unpinned and the port reuses the
closest one). The browser page reports the split per event rather than presenting
a guess as the game's sound. The footstep is the clearest example: its *timing*
is the retail cadence - a faster walk steps more often, standing still is silent
- while the cue id it fires is the port's pick.

## 6. Saves and config live next to you

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

## 7. Record and replay a session

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

## 8. Browse assets interactively

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

## 9. Read the game's scripts

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

## 10. Run the browser version locally

The same engine runs in a browser as the static site's play page, sharing the
simulation kernels with the native window. That build is **not in the clone** -
`site/wasm/` is generated output. Build it once:

```bash
scripts/ci/build-wasm.sh            # ~9 min cold; needs wasm-pack
python3 -m http.server -d site      # then open /play.html
```

Nothing is uploaded: the disc image you pick stays in the tab.

Rebuild after changing anything the page compiles - which is most of the
workspace, not just `crates/web-viewer`. To check whether the bundle you built
still matches your sources:

```bash
python3 scripts/ci/check-wasm-freshness.py
```

Worth running before concluding a change did or didn't work in the browser: a
stale bundle looks exactly like a fix that had no effect. See
[shipped-bundle-freshness.md](../tooling/shipped-bundle-freshness.md).

## Related docs

- [engine.md](../subsystems/engine.md) - the engine's architecture and clean-room boundaries.
- [shipped-bundle-freshness.md](../tooling/shipped-bundle-freshness.md) - why `site/wasm/` is generated, not committed.
- [renderer.md](../subsystems/renderer.md) - what "retail-faithful rendering" means here.
- [determinism-replay.md](../tooling/determinism-replay.md) - the replay format.
- [modding-and-translation.md](modding-and-translation.md) - randomize or translate the disc you just booted.
