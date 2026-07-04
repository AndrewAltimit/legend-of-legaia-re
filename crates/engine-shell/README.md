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
# Deterministic headless-ish screenshot (offscreen readback, no scrot/xdotool):
# open the pause menu, walk down to Status, capture at tick 140.
./target/release/legaia-engine play-window --disc <bin> --scene town01 \
  --screenshot status.png --screenshot-tick 140 \
  --pad-script "40:Start,60:Down,64:Down,68:Down,72:Down,80:Cross"
```

`--screenshot PATH` renders one frame into an offscreen `COPY_SRC` texture
(`Renderer::capture_rgba`) at `--screenshot-tick N` and writes a PNG, then exits -
no `scrot` screen-scrape. `--pad-script "TICK:BUTTON,..."` injects one-tick pad
edges keyed on the world-tick counter, replacing `xdotool` for menu navigation.
Pair with `mednafen-state vram-dump --display-crop` to diff engine output against
retail framebuffers.

In `play-window`, five minigames run as suspending scene modes driven by their
clean-room rules engines: the `K` key starts the Noa dance rhythm minigame
(`legaia_engine_core::dance`, from the dance overlay PROT 0980 - Left/Right/Up
are the three arrows), the `L` key starts the fishing minigame
(`legaia_engine_core::fishing`, from the fishing overlay PROT 0972 - Cross
casts then reels, Circle is the second reel button), the `O` key starts the
casino slot machine (`legaia_engine_core::slot_machine`, from the slot overlay
PROT 0975 - Cross spins / stops each reel / collects; a spin is the retail
flat 3-coin bet across all three paylines, 1 coin during a feature; quitting
cashes the balance out into the casino coin bank), and the `B`
key starts a Baka Fighter duel (`legaia_engine_core::baka_fighter`, from the
Baka Fighter overlay PROT 0976 - Left/Right/Up throw the three
rock-paper-scissors attacks, Down charges the special; a best-of-3 match win
banks the ladder opponent's gold prize into the party money), and the `M` key
starts a Muscle Dome contest (`legaia_engine_core::muscle_dome`, hand tables
from the battle overlay PROT 0898 and card costs from the lead character's
player-file swing records - Left/Right/Up/Down commit the four strike-command
cards under the point budget, Cross confirms/continues; a win credits the
reward Seru through the capture kernel). Each shows its own HUD and restores
the interrupted scene when it ends; press the same key again to quit.

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
