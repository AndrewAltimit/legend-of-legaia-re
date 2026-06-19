# Asset extraction

Tools for extracting assets from a user-supplied disc image. Per the project's clean-room model, no Sony bytes ship in this repo - the user runs the extraction tools against their own disc.

## Top-level pipeline

`legaia-extract` (in `crates/extract`) drives the full pipeline:

```bash
./target/release/legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted
```

The pipeline runs verify → disc → PROT → categorize → streaming-format extract → TIM → PNG → CD-XA demux → WAV → TIM-catalog TSV. Use `--skip-png` to skip the slowest step; `--skip-xa` to skip the CD-XA audio demux; `--skip-catalog` to skip writing the texture-inventory TSVs (`prot_tim_catalog.tsv` + `prot_tim_deep_catalog.tsv`); `--skip-verify` to skip the SHA verification.

Output lands in `./extracted/` (gitignored):

```
extracted/
├── PROT.DAT                       - raw archive copy
├── CDNAME.TXT                     - entry name map
├── SCUS_942.54                    - executable
├── PROT/                          - per-PROT-entry files (1232 entries, named via CDNAME).
│                                     Includes trailing-overlay sectors for entries
│                                     whose on-disc footprint extends past their
│                                     TOC-indexed end (see formats/prot.md).
│   ├── categorize.json            - per-class breakdown
│   └── ####_<name>.BIN
├── streaming/                     - DATA_FIELD streaming sub-assets
│   └── ####_<name>/chunk##_<TYPE>/####.tim
├── XA/                            - raw Form-1 .XA dumps (truncated audio; not listenable)
└── XA_WAV/                        - correctly-paced per-channel WAVs (one per (file_no, ch_no))
    └── XAn_fileN_chM.wav
```

The CD-XA step reads the raw disc directly and demuxes every `*.XA` file
into one WAV per `(file_no, ch_no)` channel, each decoded at its true
per-sector rate / stereo mode. This bypasses the `extracted/XA/` Form-1
dumps from the disc-walk step, which truncate the Form-2 audio sectors
(2324 → 2048) and collapse a file's multiplexed channels into one shuffled
stream. The NA corpus is 34 files / 316 channels, all 4-bit 37.8 kHz;
non-4-bit channels are skipped with a warning (the group decoder is 4-bit
only). The decoder is bit-exact, so the WAVs are reference-quality.

## Per-stage tools

When you want to drive a single stage:

### Disc → files (`disc-extract`)

```bash
disc-extract extract /path/to/disc.bin extracted/
```

Walks ISO9660 and writes every file. See [disc + ISO9660](../formats/disc.md).

### PROT.DAT → entries (`prot-extract`)

```bash
prot-extract extract extracted/PROT.DAT extracted/PROT/ --cdname extracted/CDNAME.TXT
```

Splits PROT.DAT into 1232 numbered entries with CDNAME-derived filenames. Each extracted file's size is the entry's full on-disc footprint - `max(indexed_size, next_start - this_start)` - so trailing-overlay sectors past the TOC-indexed end (e.g. PROT 899's title-screen overlay code) are visible. See [PROT TOC](../formats/prot.md).

### LZS decode (`lzs-decode`)

```bash
lzs-decode raw --size N <file>          # standalone LZS body
lzs-decode container <file> <out_dir>   # multi-section player.lzs container, one file per section
```

See [LZS compression](../formats/lzs.md).

### TIM → PNG (`tim`)

```bash
tim convert <file> [out.png]            # single TIM; out defaults to <file>.png
tim convert-dir <dir>                    # recursively convert every .tim under <dir>
```

### TMD analysis (`tmd`)

```bash
tmd info        <file>            # header + object table
tmd dump-obj    <file> --out <prefix>     # OBJ-with-faces export
tmd validate-prims <DIR>          # bulk-walk every prim group, sanity-check
```

### VAB extraction (`vab`)

```bash
vab list    <file>                                  # find + describe every VAB
vab extract <file> --out <out_dir> [--wav] [--sample-rate 22050]   # VAG bodies (+ optional WAV)
```

### CD-XA demux → WAV (`xa`)

The streamed-audio (`XA*.XA`) decoder. The disc-wide demux is the
correct-pacing path the top-level pipeline uses:

```bash
xa demux-disc-all <disc.bin> --out extracted/XA_WAV   # every .XA → per-channel WAV
xa demux-disc <disc.bin> --lba L --size S --out <dir>  # one .XA by LBA/size
xa info    <file.xa> [--channels stereo] [--sample-rate 37800]
xa convert <file.xa> [-o out.wav]                       # single Form-1 dump (must guess rate)
```

Prefer `demux-disc-all` over `convert`/`convert-dir`: it reads the raw
2352-byte sectors, splits by `(file_no, ch_no)`, and takes the true rate /
channel mode from each sector's CD-XA subheader instead of guessing a
global rate. See [XA audio](../formats/xa.md).

### Sub-asset extraction (`asset`)

The format-aware extractor:

```bash
asset categorize     <DIR> [--out categorize.json]    # per-class breakdown
asset extract        <file> --out <out_dir>           # streaming-format chunks → individual files
asset stream         <file>                           # walk DATA_FIELD chunks, no extraction
asset describe       <file>                           # asset-descriptor walk
asset effect-bundle  <file>
asset tmd-scan       <DIR>                            # bulk byte-search for TMD magic
asset tim-scan       <DIR>                            # bulk byte-search for TIM magic (per-entry, lenient)
asset tim-catalog    <PROT.DAT> [--out f.tsv|f.json]  # flat strict-validated TIM catalog (jPSXdec parity)
asset tim-deep-catalog <PROT.DAT> [--out f.tsv|f.json] # TIMs inside LZS-compressed sections
```

`tim-catalog` scans the whole `PROT.DAT` image (not per-entry), strict-validates
each TIM (see [`formats/tim.md`](../formats/tim.md#strict-validation-what-counts-as-a-tim)),
and maps each hit to its owning PROT entry + offset (or the unindexed system-UI
gap). It recovers the same set an independent reference decoder reports; the
committed catalog + a disc-gated regression pin the result. `--rollup` prints
the count + digest the test pins.

`tim-deep-catalog` covers what the flat catalog can't see: it LZS-decompresses
every entry and catalogs the TIMs inside each decoded section (most character /
scene textures are compressed). Each row is keyed by `(entry, LZS section,
offset-in-section)`. A hit is admitted only when the decompressed bytes
strict-parse **and** decode to RGBA - LZS "decodes without error" is never a
validity signal (the ring buffer inits to zeros). It has its own committed
reference + disc-gated regression. See
[`formats/tim.md`](../formats/tim.md#deep-catalog-tims-inside-lzs-compressed-sections).

### MES dialog (`mes`)

```bash
mes info     <file>
mes disasm   <file>
mes json     <file>
```

### Move tables (`mdt`)

```bash
mdt classify <file>                       # detect runtime-buffer vs flat-record layout
mdt records  <file> --limit 8
mdt slots    <file> --limit 8
```

## Disc-gated tests

Two integration tests touch a real disc and only run when `LEGAIA_DISC_BIN` points at a valid `.bin`:

- `crates/iso/tests/disc_pipeline.rs` - disc walk, file count, key file SHA-256s.
- `crates/extract/tests/validation_suite.rs` - full pipeline, PROT entry count, sub-asset totals, TIM round-trip.

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test --workspace
```

Without the env var, both tests **skip and pass** - that's intentional, so CI works without redistributing Sony data. Don't change that gating.

## Asset viewer

Once assets are extracted, browse them interactively:

```bash
# Single TIM
asset-viewer tim extracted/PROT/tim/<entry>.TIM

# TIM at a non-zero offset within a larger file. Use this for TIMs in
# the unindexed pre-`init_data` gap of PROT.DAT (system-UI sprite
# sheet, menu-glyph atlas, etc.) - these aren't reachable through
# the `prot` browser because no TOC entry covers them.
asset-viewer tim extracted/PROT.DAT --offset 0x018E0 --clut 2   # system-UI panel CLUT
asset-viewer tim extracted/PROT.DAT --offset 0x018E0 --clut 7   # system-UI cursor CLUT
asset-viewer tim extracted/PROT.DAT --offset 0x11218 --clut 13  # menu-glyph atlas, "Load" text CLUT

# A Legaia TMD as a 3D mesh (auto-rotating)
asset-viewer tmd extracted/streaming/<entry>/chunk##_TMD/####.tmd

# Directory of TMDs (N/P/PgDn/PgUp to cycle)
asset-viewer tmd extracted/streaming

# Battle bundle (paired TIMs auto-loaded for correct CLUTs)
asset-viewer tmd extracted/streaming/<character> --bundle battle

# A VAB sample
asset-viewer vab extracted/PROT/<entry>.BIN --offset 0xN --sample N

# PROT entry browser (auto-detects format per entry)
asset-viewer prot extracted/PROT.DAT --cdname extracted/CDNAME.TXT
```

See [`subsystems/engine.md`](../subsystems/engine.md) for the engine port architecture.
