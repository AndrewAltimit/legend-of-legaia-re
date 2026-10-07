# legaia-parity

The engine measured against retail: the parity oracles and the retail
comparison corpus. Each drives the native engine session
(`legaia_engine_session::BootSession` over the cpal output) and scores it
against an emulator save state's own RAM, VRAM or SPU, or against a recorded
trace. Tool code - it reads mednafen / PCSX-Redux states and links the wgpu
renderer for frame captures - so no play host ships it; `legaia-engine`'s
trace subcommands and the retail-compare window seeding call into it.

## Retail comparison corpus

- [`retail_compare`](src/retail_compare.rs) (+ `retail_compare_battle`,
  `retail_compare_image`, `retail_compare_script`, `retail_compare_cli`) -
  the corpus behind `legaia-engine retail-compare`: enumerate the state
  library, seed the engine from each walkable and battle state, score every
  channel, ratchet the result
  ([`docs/tooling/retail-compare.md`](../../docs/tooling/retail-compare.md)).

## Oracles

These modules implement the "engine vs. retail" comparison harnesses. Each
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
- [`sim_trace`](src/sim_trace.rs) - the engine side of the frame-tagged
  differential against the static recomp: per-frame simulation channels in
  **retail** units (PSX 12-bit angles, retail world units), so
  `scripts/recomp/trace_diff.py` can align the two timelines. See
  [`docs/tooling/recomp-differential.md`](../../docs/tooling/recomp-differential.md).

Both audio oracles take the same cold scene-entry sequence the playable hosts
take - free-roam story staging, then `enter_field_live`, then a director that
implements the global-pool start hook - because each of those three is on its
own enough to leave the trace silent. What that silence is *not* evidence of,
and why `converged` on a `.mc` comparand is a statement about the comparand
rather than about playback, is in
[`docs/subsystems/audio.md`](../../docs/subsystems/audio.md#why-converged-is-not-the-audio-oracles-fidelity-measure).
The assertion the `.mc` axis does carry is the floor: where retail had voices,
the engine's own mask must be non-empty.
