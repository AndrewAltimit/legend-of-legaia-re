# legaia-engine-audio

`cpal`-backed audio output for the engine reimplementation track.

Provides one stream that plays mono 16-bit PCM, queued from any thread.
Resamples linearly into the device's sample rate; downmixes mono into
every output channel by duplication. Designed for the asset viewer's
"play this VAG sample" key binding — not yet a full mixer.

## Channel mapping

A queued mono buffer fans out to every device channel. On a stereo
device that's centre playback; for surround setups it'll be louder than
expected. Good enough for the "does sample N play?" loop.

## Default input rate

`DEFAULT_INPUT_RATE = 22_050` — the rate Legaia's VAB samples run at,
verified across several extracted banks.

## Future iterations

- Mix multiple voices simultaneously (the PSX SPU has 24).
- ADSR envelope shaping (VAB tone metadata is parsed already by
  [`legaia-vab`]).
- Stream XA-ADPCM via the existing [`legaia-xa`] decoder.
- Replace the linear mini-resampler with the full
  PsyQ-equivalent sequencer port (the `SsAPI` cluster is identified at
  `0x80061-0x80067`; see [`docs/subsystems/audio.md`](../../docs/subsystems/audio.md)).

## See also

- [`docs/subsystems/audio.md`](../../docs/subsystems/audio.md)
- [`docs/subsystems/engine.md`](../../docs/subsystems/engine.md)
