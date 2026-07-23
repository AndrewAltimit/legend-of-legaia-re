# VAB sound bank

Sony's standard `VABp`-magic instrument bank format. Programs (up to 128) × tones (up to 16 per program) point into SPU-ADPCM voice bodies. Implementation: `crates/vab` (header parser + extractor + ADPCM decoder, sharing F0/F1 filter constants with `crates/xa`).

The format itself is documented externally; the Legaia-specific notes are:

- The dominant on-disc carrier is the [scene-VAB-prefixed streaming](scene-bundles.md) shape - the VAB body is preceded by a 4-byte chunk0 header. `crates/vab::parse_header(buf, offset)` accepts a starting offset so callers can skip the wrapper.
- A bulk scan finds 1191 `VABp` headers across 239 PROT entries. Top: `0889_sound_data2` (207), `0891_level_up` (206), `0890_sound_data2` (203) - multi-bank archives. The `vab_01` cluster (1072..1194) is the standard distributed-bank layout: 120 entries with 1–3 banks each.
- Block names from CDNAME can be misleading; trust the `VABp` magic rather than the surrounding cluster name.
- The trailing VAG size table (256 × `u16`) is **1-indexed**: `vag_table[1..=vs]` hold each sample's size in 8-byte units, so `vag_table[0]` is a reserved leading spacer. It is universally `0` across the retail corpus (986 / 986 VABs, asserted by the disc-gated `corpus_vag_spacer` test) - it is **not** a master pitch / sample-rate shift, so no pitch offset is derived from it (`VabReport::vag_table_spacer` surfaces the raw byte only).

### Program slots vs packed tone pages

The 128-slot `ProgAtr` table is indexed by **program number** - the value a SEQ ProgramChange or an SFX descriptor names. The tone-attribute region that follows is **packed**: one 16-tone page per *used* program (`ProgAtr.tones != 0`), `ps` pages total, in slot order. A program number therefore resolves to its page by **rank among the used slots**, not by its own value.

Retail computes the mapping once at VAB open: `FUN_80068D94` (`SsVabOpenHead`) walks the full ProgAtr table writing the running used-program count into each entry's `+8` reserved word, and the program-change `FUN_80068B98` reads that byte back as the page index (the open also stashes each VAG's SPU address `>>3` into the ProgAtr `+0xC`/`+0xE` reserved slots).

The distinction is load-bearing on this disc: 66 of the 217 wrapped PROT-entry banks - 43 of the 77 `music_01` banks - author *sparse* (non-contiguous) used-program sets, so indexing the packed pages with the raw program number mis-tones or silently drops most of their programs. The engine expands the pages into slot space at upload (`engine-audio::VabBank::upload`); the law is asserted corpus-wide by the disc-gated `engine-audio/tests/real_vab_program_mapping.rs`.

Retail quirk, reproduced: the rank counter is stored *before* the used check increments it, so a program-change to an unused slot aliases onto the next used slot's page. The engine reproduces this - the unused slot borrows that page while keeping its own `ProgAtr` mvol/mpan - because real BGM exercises it (e.g. `music_01` PROT 868 program 5 and PROT 996 program 19 select gap slots that retail plays via the alias; `engine-audio/tests/real_seq_program_change_coverage.rs` pins the census and the resolution). The one case *not* reproduced is a program-change past the last used slot, where retail's index runs beyond the tone region and reads garbage: the engine leaves those slots empty (silent) rather than replay undefined bytes.

### Tone attributes the engine uses (and the ones it can ignore)

Each 32-byte tone (`VagAtr`) carries the standard Sony fields. A disc-wide census of every tone (986 banks, ~44.8k tones; `engine-audio/tests/real_vab_tone_attributes.rs`) fixes which the retail data actually populates:

- **Used by playback:** `vol`/`pan` (mix), `center`/`shift` (key → pitch), `min`/`max` (note range → tone select), `adsr1`/`adsr2` (envelope), and **`pbmin`/`pbmax`** - the per-tone pitch-bend range in semitones (`pbmin` down, `pbmax` up). Only some tones carry a non-zero range; the common value is 2 (the GM-default ±2 semitones), with a few at 4/12/24/40. The sequencer scales a `0xEn` wheel event by the **sounding tone's** range (`VabBank::pitch_bend_range`), so a `(0, 0)` tone does not bend - see [`subsystems/audio.md`](../subsystems/audio.md).
- **Always zero in retail (no consumer needed):** `vibw`/`vibt` (vibrato) and `porw`/`port` (portamento) are zero on every tone, so the clean-room voice model needs no LFO.

## API

```rust
use legaia_vab::parse_header;
let header = parse_header(buf, offset)?;
println!("VAB v{} ps={} ts={}", header.version, header.ps, header.ts);
```

For bulk extraction of every VAB and per-program WAV files, see the `vab` CLI documented in [`tooling/extraction.md`](../tooling/extraction.md).

## See also

- [SEQ sequence](seq.md) - the sequenced music that plays against this bank.
- [Sound-driver outputs](sound-driver.md) - the related driver-output formats.
- [XA audio](xa.md) - the streamed-audio format for FMV/cutscenes.
- [`subsystems/audio.md`](../subsystems/audio.md) - the PsyQ libspu/libsnd stack.
