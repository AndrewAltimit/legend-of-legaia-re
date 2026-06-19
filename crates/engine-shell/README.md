# legaia-engine-shell

Top-level driver for the clean-room engine reimplementation (Track 2). This
is the crate that turns extracted-disc bytes into a running, rendered scene:
it composes the per-crate primitives - `legaia-engine-core` (world / scene
host / camera), `legaia-engine-render` (wgpu + software PSX VRAM), and
`legaia-engine-audio` (SPU + SsAPI-shape sequencer) - into one `BootSession`
the binary drives per frame, and ships the `legaia-engine` command-line tool.

End-user model: ship the engine, the user supplies the disc image. The
`--disc <bin>` flag on most subcommands reads `PROT.DAT` + `CDNAME.TXT`
straight out of a `.bin` image; `--extracted-root <dir>` reads the output of
`legaia-extract` instead.

## Library: the wiring layer

The crate root (`src/lib.rs`) re-exports the glue every embedding shares:

- [`BootSession`](src/boot.rs) / `BootConfig` - the boot flow. Opens the
  PROT + CDNAME map, loads a starting scene (`town01` by default), uploads
  the scene's primary VAB to the SPU, then drives world + camera + event
  routing each frame. Mirrors the retail boot sequence.
- [`AudioBgmDirector`](src/bgm.rs) - concrete
  `legaia_engine_core::scene::BgmDirector` that parses the SEQ bytes the
  field VM resolves through the BGM table, builds a sequencer, and feeds a
  cpal-backed audio output. Field-VM op `0x35` (BGM start) routes here.
- [`cutscene_av`](src/cutscene_av.rs) - STR (MDEC video + interleaved XA
  audio) windowed playback with the video clock driven off the audio cursor.
- [`replay`](src/replay.rs) - the `j-replay-v1` record/replay format
  (pad-transition capture + deterministic playback).
- [`scenarios`](src/scenarios.rs) - the engine integration-scenario manifest
  runner (boot a scenario headlessly, assert the SHA-256 of its `SaveFile`).

### Parity oracles

Four modules implement the "engine vs. retail" comparison harnesses. Each
boots the engine on a scene, samples a per-frame trace, and (in scenario
mode) compares against a snapshot lifted from a mednafen `.mc{slot}` save:

- [`vram_oracle`](src/vram_oracle.rs) - software-VRAM bytes vs. a runtime
  VRAM dump (per-tile overlap + texpage-region byte-exactness).
- [`mode_trace_oracle`](src/mode_trace_oracle.rs) - `(scene_mode,
  active_scene)` per frame.
- [`audio_trace_oracle`](src/audio_trace_oracle.rs) - `(voice_mask,
  voices[24], master_volume)` per frame, vs. the SPU section.
- [`pcm_oracle`](src/pcm_oracle.rs) - rendered stereo PCM windows from both
  sides (the I2 sibling of the audio trace).

## Binary: `legaia-engine`

The CLI is the user-facing entry point. Run `legaia-engine --help` for the
authoritative list; the broad groups are:

| Group | Subcommands | What they do |
|---|---|---|
| Scene inspection | `info`, `list-scenes`, `clut-trace`, `man-scripts` | Headless reports on a scene's resolved asset chain / dropped CLUTs / MAN field-VM scripts. |
| Run | `play`, `play-window`, `play-str`, `record` | Boot a scene headless (`play`) or in a wgpu window (`play-window`); play an MDEC movie (`play-str`); capture pad input to a replay (`record`). |
| Save / config | `save`, `load`, `config` | Disk-save smoke round-trip + the keyboard→pad input mapping. |
| Parity oracles | `vram-oracle`, `mode-trace`, `audio-trace`, `pcm-trace`, `replay`, `scenarios` | The harnesses above, plus deterministic replay and the scenario-hash suite. |
| Synthetic sessions | `battle`, `inventory`, `equip`, `title`, `save-select`, `encounter`, `target-pick`, `chain-editor`, `seru-capture`, `gte-replay` | Drive one engine subsystem's state machine headless from a scripted input string - no disc required. |

```bash
cargo build --release
# Boot town01 in a window, straight from a disc image:
./target/release/legaia-engine play-window --disc "Legend of Legaia (USA).bin"
# Headless 600-frame tick, logging scene transitions + BGM events:
./target/release/legaia-engine play --scene town01 --disc <bin> --frames 600
# Play a cutscene movie with synced audio:
./target/release/legaia-engine play-str MOV/MV1.STR --disc <bin>
```

In `play-window`, when the booted disc was randomized with `--seru-trade`,
talking to a shop merchant (the field-VM op-`0x49` trigger) shows a top-level
**Buy / Sell / Trade / Exit** menu; the **Trade** row opens that vendor's
seru-for-seru offers (pick an offer, confirm yes/no) - the clean-room UI for the
randomizer's swaps, keyed to the shop you're standing in.

### Binary source layout

The binary is split into modules under `src/bin/legaia-engine/` (the crate
root `src/bin/legaia-engine.rs` keeps only `main` + the clap dispatch):

- `cli.rs` - the clap `Cli` / `Cmd` / `ConfigCmd` definitions (the help text
  doubles as the user-facing per-subcommand docs).
- `commands.rs` - the headless subcommand implementations (scene inspection,
  the oracle drivers, save/load, and the synthetic-session drivers).
- `window.rs` - the winit + wgpu drivers: the `play-window` / `record`
  engine viewer (`PlayWindowApp`) and the `play-str` movie player
  (`StrPlayerApp`), plus their geometry / asset helpers.

## Tests

Integration tests live in `tests/`. The disc-gated ones (scene-asset chains,
VRAM / audio / PCM parity, world-map liveness) skip and pass when
`LEGAIA_DISC_BIN` is unset; the save round-trips key on `~/.mednafen/sav/`.
See the disc-gated-test note in the top-level `CLAUDE.md`.

## See also

- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md) - clean-room
  engine architecture and boundaries.
- [`docs/subsystems/boot.md`](../../docs/subsystems/boot.md) - the retail
  boot sequence this mirrors.
- [`docs/tooling/determinism-replay.md`](../../docs/tooling/determinism-replay.md)
  - the `j-replay-v1` format and the determinism cargo-test.
