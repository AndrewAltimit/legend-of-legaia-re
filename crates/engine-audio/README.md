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

The `audio-webaudio` cargo feature (off by default) adds `WebAudioOut`, a
`ScriptProcessorNode`-backed twin of the `AudioOut` API for `wasm32` targets.
It must be opened from inside a user-gesture handler; the browser play page
enables the feature and drives it.

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
# the retail SsSeqCalc tier over the same bytes, instead of the engine one
note-trace --extracted extracted --track 2 --frames 3600 --seq-calc
```

`--seq-calc` is the host for [`seq_calc`](src/seq_calc.rs) +
[`seq_events`](src/seq_events.rs): it seeds one retail channel record off the
track's SEQ header and runs `SsSeqCalc`'s own dispatch frame by frame, printing
the decoded events. Nothing plays - the point is to see what retail's transport
makes of a real body, and a trace reporting an unknown status or an overrun is
reporting that the port disagrees with the disc.

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
| [`seq_calc`](src/seq_calc.rs) | The retail **SsAPI per-frame calc tier** - `SsSeqCalc`'s dispatch (`FUN_80062F98`) plus the tempo slide (`FUN_800649B0`), the ascending / descending volume slides (`FUN_8006320C` / `FUN_8006352C`) and the track-end / loop-repeat handler (`FUN_80063AA8`). Pure kernels over a `SeqChannel`; `tick_budget` is the one place wall-clock tempo becomes an integer tick step. |
| [`seq_events`](src/seq_events.rs) | The rest of that tier - everything in `SsSeqCalc`'s fan-out that reads a stream byte: the stop / start arms (`FUN_800638D8` / `FUN_8006418C`), the delta-time pump (`FUN_80063974` / `FUN_800639A0`), the SEQ event decoder (`FUN_80063CEC`) and the chained-channel restart (`FUN_80064090`). `run_handler_tail` completes a walk by advancing past what the installed handler consumes; without it a trailing delta is re-read as the next status byte. |
| [`anim_cue`](src/anim_cue.rs) | `walk_anim_cues` / `AnimCueState` - the per-frame walker over a playing battle action's 8-slot `(frame, cue)` track (`FUN_800508DC`). Resolves the party `0xC8..=0xFF` band into the `>= 0x100` arts-voice namespace, the three per-character shout ids into a two-take XA channel pick, and the CD-busy fallback into a fixed ring cue. Emits `AnimCueEmit` decisions rather than calling anything - `NOT WIRED`, see the module docs. |
| [`sfx_ring`](src/sfx_ring.rs) | The retail 4-slot cue ring, byte-faithful: the id array `DAT_8007B6D8[4]` beside its per-slot vsync countdown `DAT_8007C338[4]`, plus both halves of the per-frame walk (`FUN_8001698c` aging, `FUN_80016b6c` drain). Two parallel fixed arrays, not a queue. |
| [`seq_slots`](src/seq_slots.rs) | The SEQ resource-slot table at `0x80091508` (12-byte stride): which side-band SEQ/VAB resources hold an open libsnd handle. Pure bookkeeping - the hardware side of a close is a caller-supplied closure. |
| [`battle_voice`](src/battle_voice.rs) | `battle_voice_step` (`FUN_8004DA00`) - which whole-clip XA voice stream, if any, a battle action arms this frame. `NOT WIRED`; retail reaches it through a static actor template rather than a call. |
| [`footstep`](src/footstep.rs) | The field movement cadence kernel (`FUN_80018DB0`). **Deliberately mislabelled**: retail's cadence drives the libpad *actuators*, not audio - it plays no footstep sound. The module docs enumerate each name that reads as audio and what it really is. |
| [`test_sink`](src/test_sink.rs) | `TestAudioSink` - the device-free stand-in for `AudioOut`, so a headless test can drive the BGM/SFX plumbing. See below. |

## Driving the mixer without a device

`AudioOut` is the only handle that carries the BGM half of the mixing core
(attach / pause / crossfade / swap), and opening one needs a real output
device - so a headless test could not tick the mixer at all, which is what
left the SFX enqueue, the VAB upload and the sequencer's voice allocator
without a test-side host.

[`test_sink::TestAudioSink`](src/test_sink.rs) closes that. It owns the
**same** `StreamResampler` the cpal callback owns and drives it by pulling
frames, so every kernel on the output path runs exactly as under a device.
Two rules keep it honest:

- **No second copy of the mixing math.** Every method delegates to the same
  private `StreamResampler` method `AudioOut` delegates to, so the sink cannot
  drift from the device path and start asserting about itself.
- **Measure at the output.** `render_frames` returns a `SinkMeasure` over the
  emitted PCM (peak, non-zero frames, level integral), not a count of calls -
  a wired kernel that runs and produces silence is indistinguishable from an
  unwired one at the call site, and distinguishable at the samples.

The session ladder over it is `tests/w1e_audio_session_ladder.rs` (disc-gated):
real BGM staged and played, paused and resumed, and a real SFX cue fired
through the frame scheduler, each asserted on the PCM that came out.

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
