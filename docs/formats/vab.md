# VAB sound bank

Sony's standard `VABp`-magic instrument bank format. Programs (up to 128) × tones (up to 16 per program) point into SPU-ADPCM voice bodies. Implementation: `crates/vab` (header parser + extractor + ADPCM decoder, sharing F0/F1 filter constants with `crates/xa`).

The format itself is documented externally; the Legaia-specific notes are:

- The dominant on-disc carrier is the [scene-VAB-prefixed streaming](scene-bundles.md) shape - the VAB is preceded by a 4-byte chunk-0 header, and it is **split across two chunks** rather than stored whole (see [below](#a-vab-is-carried-as-two-chunks-and-the-bodies-are-the-second)). `crates/vab::parse_header(buf, offset)` accepts a starting offset so callers can skip the wrapper.
- A bulk scan of each entry's own sectors finds 424 `VABp` headers across 219 PROT entries. Exactly **one** is a multi-bank archive - `0891_level_up`, holding 206 banks; every other carrier holds exactly one. The `vab_01` cluster (1072..1194) is the standard distributed-bank layout: 119 of its 123 entries, one bank each.
- **The "three multi-bank archives" reading was an over-read.** `0889_sound_data2` (207 banks) and `0890_sound_data2` (203) were counted through the superseded `toc[p+5] - toc[p+3] + 4` window ([`prot.md`](prot.md#tocp5---tocp3--4-is-not-an-entrys-size)), which spans both of those small entries into `0891`'s 6 MB archive; on their own sectors they hold 1 and 0. The whole-corpus figures from that window (1191 headers, 239 entries, 120 `vab_01` carriers) are artefacts of the same window.
- Block names from CDNAME can be misleading; trust the `VABp` magic rather than the surrounding cluster name.
- The trailing VAG size table (256 × `u16`) is **1-indexed**: `vag_table[1..=vs]` hold each sample's size in 8-byte units, so `vag_table[0]` is a reserved leading spacer. It is universally `0` across the retail corpus (424 / 424 VABs, asserted by the disc-gated `corpus_vag_spacer` test) - it is **not** a master pitch / sample-rate shift, so no pitch offset is derived from it (`VabReport::vag_table_spacer` surfaces the raw byte only).

### A VAB is carried as two chunks, and the bodies are the second

The 4-byte word in front of `pBAV` is a [DATA_FIELD](data-field.md) chunk header
`(type << 24) | payload_len` with type `0x00`, and its payload length is the
VAB's **header part** only - `VabHdr` + the 128-slot program table + `ps` tone
pages + the 256-slot VAG size table, i.e. `0x20 + 0x800 + 0x200 * ps + 0x200`.
The VAG bodies are a *different chunk* of the same stream, so a second chunk
header sits between the size table and the first ADPCM block:

```text
[u32 (0x00 << 24) | header_part_len]   chunk 0 header
[VabHdr .. VAG size table]             chunk 0 payload
[u32 (type << 24) | body_len]          chunk N header   <- inside the VAB
[VAG bodies]                           chunk N payload
```

`header_part_len + body_len == VabHdr.fsize`, which is what identifies the body
chunk: it is the one whose payload length is `fsize - header_part_len`, a
length the container states rather than a magic to hunt for.

The law holds on all 424 retail VABs for the leading header, and the two-chunk
carriage is asserted over every top-level VAB by the disc-gated
`crates/vab/tests/corpus_chunk_carriage.rs`. The body chunk's type byte is
`0x01` in almost every case and `0x03` in ten.

**Why this matters to a reader.** `legaia_vab::parse` walks straight on from the
size table, so the `byte_offset` it reports for each VAG body is that chunk
header's address, not the body's - four bytes early wherever the body chunk
comes next. That skew was first measured from the other side: an SPU-ADPCM
block's high nibble is a filter index and only `0..=4` is legal, and the grid at
`+4` scores 100 % legal filters where the reported origin scores about chance.
`legaia_vab::vag_body_origin` reads the real origin off the stream instead, and
`decode_vag_aligned`'s `{0, 4}` probe is the fallback for callers that hold only
the body.

Six entries - `0886`, `1058`, `1059`, `1063`, `1064`, `1065` - put the SEQ chunk
**before** the body chunk, so their skew is thousands of bytes and no
small-alignment probe recovers it. Anything that indexes a VAG body out of a
stream entry has to walk the chunks.

### The multi-bank archive (`monster.snd`)

PROT extraction `0891` is the disc's one multi-bank VAB: 206 independent banks,
one per monster SE set, streamed a bank at a time because the whole archive is
5.7 MB. Its head is the bank index:

| Offset | Field |
|---|---|
| `+0x00` | `u32` reserved, zero |
| `+0x04` | `u32 count` - 206 |
| `+0x08` | `u32 start_sector[count + 1]` - each bank's first sector relative to the entry start; the last word is the archive's own sector count |

Bank `i` occupies `[start_sector[i] << 11, start_sector[i + 1] << 11)`, so the
table is bounded the way a [PROT TOC](prot.md) entry is - by its successor. Each
bank is itself a two-chunk stream in exactly the shape above, then a zero
terminator, then sector slack the builder left its previous buffer contents in
(banks 61 and 62 carry bank 60's bytes from the same buffer offset). Nothing
reads past `fsize`.

The reader is `FUN_8003E104(bank, slot, dest)`, which bounds `bank` against the
count word, forms `start` and `end` from `table[bank]` / `table[bank + 1]`, and
shifts both left by 11 (`see ghidra/scripts/funcs/8003e104.txt`). Its three
callers pass `monster_id - 1`. The table is resident at `0x801C8980` because
the boot image stages it: PROT `0895` reads one sector of raw TOC `0x37D`
(= extraction 891) and `memcpy`s `0x400` bytes of it there. Parser
`legaia_asset::vab_multi_bank`; the whole entry accounts structurally in
[`byte-accounting.md`](../tooling/byte-accounting.md).

### Program slots vs packed tone pages

The 128-slot `ProgAtr` table is indexed by **program number** - the value a SEQ ProgramChange or an SFX descriptor names. The tone-attribute region that follows is **packed**: one 16-tone page per *used* program (`ProgAtr.tones != 0`), `ps` pages total, in slot order. A program number therefore resolves to its page by **rank among the used slots**, not by its own value.

Retail computes the mapping once at VAB open: `FUN_80068D94` (`SsVabOpenHead`) walks the full ProgAtr table writing the running used-program count into each entry's `+8` reserved word, and the program-change `FUN_80068B98` reads that byte back as the page index (the open also stashes each VAG's SPU address `>>3` into the ProgAtr `+0xC`/`+0xE` reserved slots).

The distinction is load-bearing on this disc: 66 of the 218 wrapped PROT-entry banks - 43 of the 77 `music_01` banks - author *sparse* (non-contiguous) used-program sets, so indexing the packed pages with the raw program number mis-tones or silently drops most of their programs. The engine expands the pages into slot space at upload (`engine-audio::VabBank::upload`); the law is asserted corpus-wide by the disc-gated `engine-audio/tests/real_vab_program_mapping.rs`.

Retail quirk, reproduced: the rank counter is stored *before* the used check increments it, so a program-change to an unused slot aliases onto the next used slot's page. The engine reproduces this - the unused slot borrows that page while keeping its own `ProgAtr` mvol/mpan - because real BGM exercises it (e.g. `music_01` PROT 868 program 5 and PROT 996 program 19 select gap slots that retail plays via the alias; `engine-audio/tests/real_seq_program_change_coverage.rs` pins the census and the resolution). The one case *not* reproduced is a program-change past the last used slot, where retail's index runs beyond the tone region and reads garbage: the engine leaves those slots empty (silent) rather than replay undefined bytes.

### Tone attributes the engine uses (and the ones it can ignore)

Each 32-byte tone (`VagAtr`) carries the standard Sony fields. A disc-wide census of every tone (424 banks, ~23k tones; `engine-audio/tests/real_vab_tone_attributes.rs`) fixes which the retail data actually populates:

- **Used by playback:** `vol`/`pan` (mix), `center`/`shift` (key → pitch), `min`/`max` (note range → tone select), `adsr1`/`adsr2` (envelope), and **`pbmin`/`pbmax`** - the per-tone pitch-bend range in semitones (`pbmin` down, `pbmax` up). Only some tones carry a non-zero range; the common value is 2 (the GM-default ±2 semitones), with a few at 4/12/24/40. The sequencer scales a `0xEn` wheel event by the **sounding tone's** range (`VabBank::pitch_bend_range`), so a `(0, 0)` tone does not bend - see [`subsystems/audio.md`](../subsystems/audio.md).
- **Always zero in retail (no consumer needed):** `vibw`/`vibt` (vibrato) and `porw`/`port` (portamento) are zero on every tone, so the from-scratch voice model needs no LFO.

#### `center` / `shift` are the whole of the pitch, rate included

`center` is the key at which the tone plays back at **unity** - SPU pitch
`0x1000`, 44.1 kHz - and `shift` raises that by `shift/128` of a semitone
(positive, quantised to 1/16 by the driver). The full law, both key-on paths and
the table they index, is in
[`subsystems/audio.md`](../subsystems/audio.md#the-key-on-pitch-law---note-against-the-tones-center).

The consequence worth stating on this page: **a VAG body's sample rate is
encoded in `center`, not anywhere else.** The bank header carries no per-sample
rate and the 1-indexed size table's leading spacer is not one either (above);
a body recorded at 22.05 kHz is authored with `center` twelve semitones above
the key it is meant to sound at. So a port that multiplies the key-on pitch by a
nominal `22050/44100` *on top of* `note - center` double-counts the resampling
and plays every voice an octave low. `crates/vab`'s WAV writer hard-codes 22050
for the standalone-extraction case, which is a separate question from playback.

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
