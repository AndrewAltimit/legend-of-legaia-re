# legaia-engine-audio

`cpal`-backed audio output for the engine reimplementation track. Two
layers:

1. A clean-room PSX **SPU** model - 24 voices, 512 KB SPU RAM, ADSR
   envelopes, libspu-shaped transfer engine.
2. An `AudioOut` handle that owns one cpal output stream and ticks the
   SPU at 44.1 kHz internal rate, resampling linearly to the host device
   rate.

Engines push voice attributes / key-on masks / sample uploads through
`AudioOut::with_spu(|spu| ...)`.

`AudioOut` also carries two output-side switches: `set_mono` (the retail
options screen's Stereo/Monaural downmix) and `set_muted` - an engine-only
master gate that zeroes the rendered frames while the sequencer, SPU
voices, XA stream and fade engine all keep ticking, so unmuting resumes
playback in sync without tearing down the stream.

## SPU model

| Module | Surface |
|---|---|
| [`spu`](src/spu/mod.rs) | Top-level `Spu` struct with `tick`, `key_on_mask`, `key_off_mask`, `find_idle_voice`. |
| [`spu::voice`](src/spu/voice.rs) | Per-voice `Voice` (sample addr, loop addr, pitch, ADSR, L/R volume). Resamples through the hardware's 4-point Gaussian interpolator. |
| [`spu::gauss`](src/spu/gauss.rs) | The SPU's 512-entry Gaussian interpolation coefficient ROM + the 4-tap mix (published hardware spec; matters because Legaia's 22.05 kHz VAGs make every voice run at a non-unity pitch step). |
| [`spu::adsr`](src/spu/adsr.rs) | 5-phase envelope state machine matching the PSX `(adsr1, adsr2)` word layout (linear / exponential / increase / decrease). |
| [`spu::adpcm`](src/spu/adpcm.rs) | Streaming SPU-ADPCM block decoder - 28 samples per 16-byte block, stateful across blocks. |
| [`spu::ram`](src/spu/ram.rs) | 512 KB SPU RAM model + libspu-style transfer pointer / direction + a first-fit `SpuAllocator` for `SsSpuMalloc` / `SpuFree`. |
| [`spu::reverb`](src/spu/reverb.rs) | Faithful register-driven reverb network (same/different-side IIR + 4-tap comb + 2 all-pass), with the 9 standard libspu mode presets. Per-voice opt-in via `Voice::set_reverb_send`. |
| [`vab_bind`](src/vab_bind.rs) | `VabBank::upload(spu, alloc, report, buf)` drops every VAG body into SPU RAM; `VabBank::play_note(spu, voice, prog, note, velocity)` translates a MIDI key into voice config + key-on. |
| [`sfx`](src/sfx.rs) | `SfxBank` maps cue IDs (the `HitCue::kind` byte from art records, plus engine-extended slots for menu blips / footsteps) to per-cue `SfxEntry` descriptors. `play_one_shot` delegates to `VabBank::play_note`. `SfxScheduler::tick_frame` drains queued `PendingCue`s with retail-style `timing_frames` offsets so cues fire on the right anim frame. |

## Default input rate

`DEFAULT_INPUT_RATE = 22_050` - the rate Legaia's VAB samples run at,
verified across several extracted banks.

## Out of scope (first port pass)

- **SPU-level pitch modulation, noise, FM.** None of these *hardware* voice
  modes are used by Legaia (verified against the libspu calls in the SCUS
  dumps). Sequencer-level **MIDI pitch-bend** (`0xEn`) *is* used and *is*
  handled - the [`Sequencer`](src/sequencer.rs) applies it by scaling the
  voice's pitch register over the tone's own `pbmin`/`pbmax` range, alongside
  dynamic channel volume (CC7) and pan (CC10). See the audio subsystem doc.
- **Asynchronous DMA timing.** The transfer engine here is synchronous;
  the API shape (`set_transfer_start_units_8` / `set_direction` /
  `write`) preserves the libspu surface.

## See also

- [`docs/subsystems/audio.md`](../../docs/subsystems/audio.md) - full byte-level audio reference + the `engine-audio model` section that documents this crate.
- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md)
- [`legaia-vab`](../vab) for the VAB parser this crate consumes.
- [`legaia-xa`](../xa) for the F0/F1 filter constants the ADPCM decoder
  shares.
