# legaia-seq

PsyQ **SEQ** sequence-file parser. SEQ is the file format Sony's `libsnd`
SsAPI sequencer (`SsSeqOpen`/`SsSeqPlay`) consumes - a thin MIDI variant
with a 13-byte header.

## Scope

- Header parser (`pQES` magic + version / PPQN / tempo / time signature).
- Event-stream decoder: channel messages, running status, meta events,
  end-of-track.
- VLQ helper (`read_vlq`) usable by other tools.
- CLI (`seq info` / `seq events` / `seq json` / `seq find`) for inspection,
  with `--offset` for SEQ data embedded at a non-zero offset.

## Legaia is not stock PsyQ here

Two deviations bite anyone reading this format against the PsyQ docs:

- The **version field is `u32` big-endian**, not PsyQ's `u16`.
- Meta events carry **no MIDI variable-length `length` field**: tempo is
  `0xFF 0x51` followed by 3 tempo bytes (no `0x03`), and end-of-track is
  `0xFF 0x2F` with no trailing `0x00`. Meta events preserve running status.

Reading a phantom length byte swallows the first tempo override in the body,
which leaves playback pinned to the 240 BPM placeholder in the header - roughly
3x too fast. `ppqn` is 480. Details in
[`docs/formats/seq.md`](../../docs/formats/seq.md).

Most retail BGM is also **not** a bare SEQ file: it sits inside a
`scene_vab_stream` entry as `[u32 chunk_header][VAB][chunk1_header][SEQ]`, so
the SEQ starts at a non-zero offset. On the CLI, `seq find` locates the
offset and `--offset` parses there; library-side,
`legaia_engine_core::scene_assets::SceneAssets` carries the resolved
locations (`seq_in_stream_entries`, plus `bgm_seq_offset` for a given BGM id)
so callers slice past the wrapper.

## CLI

`info` / `events` / `json` take a standalone SEQ or, with `--offset N`
(decimal or `0x`-hex), a wrapped BGM PROT entry from `legaia-extract`
(e.g. `extracted/PROT/0990_music_01.BIN`). Without `--offset` the file is
parsed at 0 and, failing that, at the first parseable `pQES` magic
(auto-scan).

```bash
# Scan any blob for `pQES` magics: each candidate offset + header, and
# whether the full event stream parses - the way to find --offset values.
seq find extracted/PROT/0990_music_01.BIN

# Header + event-count summary
seq info path/to.seq
seq info extracted/PROT/0990_music_01.BIN --offset 0x23E2C

# Disassemble every event in source order (--limit caps the output)
seq events path/to.seq --limit 50

# Full parse as JSON, for tooling
seq json path/to.seq
```

To *hear* one, pair it with its bank through the viewer:
`asset-viewer seq <file.seq> <file.vab>` (see
[`crates/asset-viewer`](../asset-viewer/README.md)).

## Out of scope

- Playback. The runtime side lives in
  [`legaia-engine-audio`](../engine-audio): `Sequencer` consumes an
  already-parsed `Seq` + a `VabBank` and drives the SPU model.

## See also

- [`docs/formats/seq.md`](../../docs/formats/seq.md) - byte-level reference.
- [`docs/subsystems/audio.md`](../../docs/subsystems/audio.md) - how the
  PsyQ audio stack composes (libcd → XA + libsnd → SEQ + libspu → SPU).
