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

## Note-level tracing

The engine half of the note-level BGM differential against the static
recomp. [`note_trace`](src/note_trace.rs) records every voice key transition
with the state the voice was programmed with - ADPCM start address, pitch,
per-voice volumes, raw ADSR words - which is the same thing the recomp
runtime's semantic key-on ring records, so the two timelines compare
directly. It is opt-in: `Spu::note_trace` is `None` by default and the
normal audio path never touches it.

Recording hangs off explicit `Spu::record_key_on` / `record_key_off` calls
placed next to the real key transitions, **not** off `key_on_mask` - the
sequencer's voice path keys voices on directly through `Voice`, so a hook on
the mask API would miss every BGM note.

The `note-trace` binary emits a track's timeline as canonical JSONL:

```bash
note-trace --extracted extracted --list
note-trace --extracted extracted --track 0 --frames 1800 --out notes.jsonl
```

Anything driving the SPU for a trace must call `Spu::tick` per sample even
when the rendered audio is discarded: `tick` is what advances the ADSR, and
a voice only becomes reusable once its envelope reaches `Phase::Off`.
Ticking the sequencer alone leaves every voice permanently busy, which both
drops notes the allocator can no longer place and flattens the voice
distribution - an artifact that reads exactly like a voice-allocation bug.

Capture, diff and the retail side are documented in
[`docs/tooling/recomp-differential.md`](../../docs/tooling/recomp-differential.md).

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
| [`vab_bind`](src/vab_bind.rs) | `VabBank::upload(spu, alloc, report, buf)` drops every VAG body into SPU RAM and expands the file's packed tone pages into **program-number space** by rank among used `ProgAtr` slots (retail builds the same map at VAB open - see [`formats/vab.md`](../../docs/formats/vab.md#program-slots-vs-packed-tone-pages)); `VabBank::play_note(spu, voice, prog, note, velocity)` translates a MIDI key into voice config + key-on through the retail key-on volume chain incl. program `mvol`/`mpan` (the sequencer's key-range path); `VabBank::play_tone(spu, voice, prog, tone_index, note, velocity)` keys an **explicit** tone-region index (the SFX path). |
| [`shout`](src/shout.rs) | `ArtsShoutBank` - the battle Tactical-Arts **shout** clips (per-character CD-XA banks `XA2`/`XA4`/`XA6`, demuxed per channel + decoded by the host) plus the per-art candidate-channel pools from the SCUS cue tables. Resolves `(cslot, action_constant)` to a clip with the retail no-immediate-repeat channel pick (`FUN_8004C140`); played through `AudioOut::play_xa_shout` with the modeled CD-response start delay so the shout trails the art animation. `OfflineMixer` (lib.rs) is the device-free twin of the cpal mixing core for asserting what reaches the output. |
| [`sfx`](src/sfx.rs) | `SfxBank` maps cue IDs (the `HitCue::kind` byte from art records, plus engine-extended slots for menu blips / footsteps) to per-cue `SfxEntry` descriptors carrying the retail descriptor's program + tone-region index + note + voice count (`from_descriptors`). `play_one_shot` fires via `VabBank::play_tone` across the cue's `voices` consecutive regions - the retail SFX shape, which names a tone by index, not by key-range window (a `play_note` resolve renders silence for cues whose note falls outside the tone's window, e.g. the strike cue `0x1A`). `SfxScheduler::tick_frame` drains queued `PendingCue`s with retail-style `timing_frames` offsets so cues fire on the right anim frame. |

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
