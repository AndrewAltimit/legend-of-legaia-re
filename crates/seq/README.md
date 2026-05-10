# legaia-seq

PsyQ **SEQ** sequence-file parser. SEQ is the file format Sony's `libsnd`
SsAPI sequencer (`SsSeqOpen`/`SsSeqPlay`) consumes - a thin MIDI variant
with a 13-byte header.

## Scope

- Header parser (`pQES` magic + version / PPQN / tempo / time signature).
- Event-stream decoder: channel messages, running status, meta events,
  end-of-track.
- VLQ helper (`read_vlq`) usable by other tools.
- CLI (`seq info` / `seq events` / `seq json`) for inspection.

## Out of scope

- Playback. The runtime side lives in
  [`legaia-engine-audio`](../engine-audio): `Sequencer` consumes an
  already-parsed `Seq` + a `VabBank` and drives the SPU model.

## See also

- [`docs/formats/seq.md`](../../docs/formats/seq.md) - byte-level reference.
- [`docs/subsystems/audio.md`](../../docs/subsystems/audio.md) - how the
  PsyQ audio stack composes (libcd → XA + libsnd → SEQ + libspu → SPU).
