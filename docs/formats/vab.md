# VAB sound bank

A VAB is Sony's instrument bank: the samples a piece of sequenced music or a sound effect plays, plus the tables that say how to play them. It has **programs** (instruments, up to 128), each with up to 16 **tones** (a key range, volume, pan, envelope and a pointer to one sample), and the samples themselves as SPU-ADPCM bodies called VAGs. A [SEQ](seq.md) names a program number; the bank turns note + program into a sample and a pitch.

The file layout is Sony's standard `VABp` format. What is specific to Legaia is how the bank is **carried** on disc - split across two chunks of a stream, so the sample bodies do not start where a plain parse expects - and a few properties of the retail data.

Implementation: [`crates/vab`](../../crates/vab/README.md) (header parser, extractor, ADPCM decoder; shares the filter constants with `crates/xa`).

## Layout

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `0x00` | u32 | `magic` | `0x56414270`, the bytes `pBAV` on disc | Confirmed |
| `0x04` | u32 | `version` | Format version | Confirmed |
| `0x08` | u32 | `vab_id` | Bank id | Confirmed |
| `0x0C` | u32 | `fsize` | Total bank size: header part + VAG bodies | Confirmed |
| `0x10` | u16 | reserved | Not read by the parser | Confirmed |
| `0x12` | u16 | `ps` | Number of used programs (= tone pages) | Confirmed |
| `0x14` | u16 | `ts` | Total tone count | Confirmed |
| `0x16` | u16 | `vs` | Number of VAG samples | Confirmed |
| `0x18` | u8 x 4 | `mvol`, `pan`, `attr1`, `attr2` | Master volume / pan / attributes | Confirmed |
| `0x1C` | u32 | reserved | Not read by the parser | Confirmed |
| `0x20` | 128 x 16 | `ProgAtr[128]` | Program table, indexed by **program number** | Confirmed |
| `0x820` | `ps` x 16 x 32 | `VagAtr` pages | One 16-tone page per **used** program, packed in slot order | Confirmed |
| `0x820 + 0x200*ps` | 256 x u16 | VAG size table | **1-indexed**: `[1..=vs]` are sample sizes in 8-byte units; `[0]` is a spacer | Confirmed |
| after the table | - | VAG bodies | SPU-ADPCM, 16-byte blocks of 28 samples | Confirmed |

The header part is therefore `0x20 + 0x800 + 0x200 * ps + 0x200` bytes (`legaia_vab::header_part_size`).

The size table's leading spacer is `0` in all 424 retail VABs (asserted by the disc-gated `corpus_vag_spacer` test). It is **not** a master pitch or sample-rate shift; `VabReport::vag_table_spacer` surfaces the raw value only.

## Where VABs sit on disc

- The dominant carrier is the [scene-VAB-prefixed streaming](scene-bundles.md) shape: a 4-byte chunk header, then the VAB, split across two chunks ([below](#a-vab-is-carried-as-two-chunks-and-the-bodies-are-the-second)).
- A scan of each entry's own sectors finds 424 `VABp` headers across 219 PROT entries. Exactly **one** entry is a multi-bank archive - extraction `0891` (`monster.snd`), holding 206 banks. Every other carrier holds exactly one.
- The `vab_01` block (raw-TOC `1072..1194`) is the standard one-bank-per-entry layout: 119 of its 123 entries carry a bank.
- Extraction `0889` and `0890` hold 1 and 0 banks. Counts of 207 and 203 for them, and whole-corpus figures of 1191 headers / 239 entries, come from the `toc[p+5] - toc[p+3] + 4` window running on into `0891` ([`prot.md`](prot.md#tocp5---tocp3--4-is-not-an-entrys-size)).
- CDNAME block names can mislead; trust the `VABp` magic over the surrounding block name.

## A VAB is carried as two chunks, and the bodies are the second

The 4-byte word in front of `pBAV` is a [DATA_FIELD](data-field.md) chunk header
`(type << 24) | payload_len` with type `0x00`, and its payload length is the
VAB's **header part** only - `VabHdr` + the 128-slot program table + `ps` tone
pages + the 256-slot VAG size table, i.e. `0x20 + 0x800 + 0x200 * ps + 0x200`.
The VAG bodies are a *different chunk* of the same stream, so a second chunk
header sits between the size table and the first ADPCM block:

```text
stream entry
+--------------------+---------------------------------+--------------------+------------------+
| chunk 0 header     | chunk 0 payload                 | chunk N header     | chunk N payload  |
| (0x00<<24)         | VabHdr, ProgAtr[128],           | (type<<24)         | VAG bodies       |
|  | header_part_len |  VagAtr pages, VAG size table   |  | body_len        |                  |
+--------------------+---------------------------------+--------------------+------------------+
0                    4  <- pBAV                        ^                    ^
                                                       |                    real body origin
                                      where a plain parse puts the bodies (4 bytes early)

header_part_len + body_len == VabHdr.fsize
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
comes next. The data shows it directly: an SPU-ADPCM block's high nibble is a
filter index and only `0..=4` is legal, and the grid at `+4` scores 100 % legal
filters where the reported origin scores about chance.
`legaia_vab::vag_body_origin` reads the real origin off the stream instead, and
`decode_vag_aligned`'s `{0, 4}` probe is the fallback for callers that hold only
the body.

Six entries - `0886`, `1058`, `1059`, `1063`, `1064`, `1065` - put the SEQ chunk
**before** the body chunk, so their skew is thousands of bytes and no
small-alignment probe recovers it. Anything that indexes a VAG body out of a
stream entry has to walk the chunks.

### Two wrongs that cancel, and the six entries where they do not

The skew has a twin on the consumer side, and the pair is why neither was
visible. A caller that parses at the VAB (`parse(entry, 4)`) gets spans that are
absolute in `entry`, and then hands the upload a buffer **re-sliced at the VAB**
- so every span is indexed four bytes too far on. That is exactly the four bytes
the parsed origin is short, and on 212 of the 218 stream-carried VABs the two
errors land on the same byte. They do not on the six above: there the upload
reads inside the SEQ chunk and puts a slice of the sequence into SPU RAM as a
sample body, which measures as a legal-filter share of roughly a coin flip
against 100 % at the resolved origin.

The two halves have to move together. Three entry points cover the cases:

| Entry point | Use when |
|---|---|
| `parse(buf, offset)` | The bank sits at a known offset and the caller compensates for the body origin itself |
| `parse_in_stream(buf, stream_start)` | The bank is inside a DATA_FIELD stream; spans come back already resolved |
| `vag_body_origin_at(buf, vab_offset)` | Only the body origin is needed, from a buffer that may start at `pBAV` |

`legaia_vab::vag_body_origin_at` finds the body chunk by walking **forward**
from the end of the header part (no chunk-0 header needed, so it works on a
buffer that starts at `pBAV`), and `legaia_vab::parse_in_stream` returns a
report whose spans are already resolved. The `pBAV` magic tells a consumer which
buffer convention it was handed. `parse`'s own spans stay where they are -
several callers compensate with a hard-coded `+4` of their own.

## The multi-bank archive (`monster.snd`)

PROT extraction `0891` is the disc's one multi-bank VAB: 206 independent banks,
one per monster SE set, streamed a bank at a time because the whole archive is
5.7 MB. Its head is the bank index:

| Offset | Size | Field | Meaning | Confidence |
|---|---|---|---|---|
| `+0x00` | u32 | reserved | Zero | Confirmed |
| `+0x04` | u32 | `count` | 206 | Confirmed |
| `+0x08` | u32 x (`count` + 1) | `start_sector[]` | Each bank's first sector relative to the entry start; the last word is the archive's own sector count | Confirmed |

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

## Program slots vs packed tone pages

The 128-slot `ProgAtr` table is indexed by **program number** - the value a SEQ ProgramChange or an SFX descriptor names. The tone-attribute region that follows is **packed**: one 16-tone page per *used* program (`ProgAtr.tones != 0`), `ps` pages total, in slot order. A program number therefore resolves to its page by **rank among the used slots**, not by its own value.

Retail computes the mapping once at VAB open: `FUN_80068D94` (`SsVabOpenHead`) walks the full ProgAtr table writing the running used-program count into each entry's `+8` reserved word, and the program-change `FUN_80068B98` reads that byte back as the page index (the open also stashes each VAG's SPU address `>>3` into the ProgAtr `+0xC`/`+0xE` reserved slots).

The distinction is load-bearing on this disc: 66 of the 218 wrapped PROT-entry banks - 43 of the 77 `music_01` banks - author *sparse* (non-contiguous) used-program sets, so indexing the packed pages with the raw program number mis-tones or silently drops most of their programs. The engine expands the pages into slot space at upload (`engine-audio::VabBank::upload`); the law is asserted corpus-wide by the disc-gated `engine-audio/tests/real_vab_program_mapping.rs`.

Retail quirk: the rank counter is stored *before* the used check increments it, so a program-change to an unused slot aliases onto the next used slot's page, and the engine keeps that alias (the unused slot borrows the page while keeping its own `ProgAtr` mvol/mpan).

The alias is **silent for the score**, though. A note-on searches the page for covering tones only `ProgAtr.tones` rows deep, and that count is the slot's own byte: `FUN_80066308` stages `ProgAtr[prog]+0` at `0x801CE348` (`0x80066474..0x80066484`) and `FUN_80068568` loops over exactly that many rows. An unused slot's count is zero, so its notes key no voice.
Real BGM selects gap slots (PROT 868 program 5, PROT 996 program 19; `engine-audio/tests/real_seq_program_change_coverage.rs` pins the census), and a breakpoint census of PROT 996 (`uru`, BGM `2008`) sees the program-19 note reach no voice allocation between its neighbours'.
The engine keys from `VabProgram::key_range_tones`, the first `key_tones` rows. A program-change past the last used slot, where retail's index runs beyond the tone region, is left an empty page.

## Tone attributes the engine uses (and the ones it can ignore)

Each 32-byte tone (`VagAtr`) carries the standard Sony fields. A disc-wide census of every tone (424 banks, ~23k tones; `engine-audio/tests/real_vab_tone_attributes.rs`) fixes which the retail data actually populates:

- **Used by playback:** `vol`/`pan` (mix), `center`/`shift` (key → pitch), `min`/`max` (note range → tone select), `adsr1`/`adsr2` (envelope), and **`pbmin`/`pbmax`** - the per-tone pitch-bend range in semitones (`pbmin` down, `pbmax` up). Only some tones carry a non-zero range; the common value is 2 (the GM-default ±2 semitones), with a few at 4/12/24/40. The sequencer scales a `0xEn` wheel event by the **sounding tone's** range (`VabBank::pitch_bend_range`), so a `(0, 0)` tone does not bend - see [`subsystems/audio.md`](../subsystems/audio.md).
- **Always zero in retail (no consumer needed):** `vibw`/`vibt` (vibrato) and `porw`/`port` (portamento) are zero on every tone, so the from-scratch voice model needs no LFO.

### `center` / `shift` are the whole of the pitch, rate included

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
