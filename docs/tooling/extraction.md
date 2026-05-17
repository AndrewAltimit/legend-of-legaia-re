# Asset extraction

Tools for extracting assets from a user-supplied disc image. Per the project's clean-room model, no Sony bytes ship in this repo - the user runs the extraction tools against their own disc.

## Top-level pipeline

`legaia-extract` (in `crates/extract`) drives the full pipeline:

```bash
./target/release/legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted
```

The pipeline runs verify → disc → PROT → categorize → streaming-format extract → TIM → PNG. Use `--skip-png` to skip the slowest step; `--skip-verify` to skip the SHA verification.

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
├── tim_scan/                      - every TIM byte-pattern hit
├── tmd_scan/                      - every TMD byte-pattern hit
└── images/                        - TIM-to-PNG conversions
```

## Per-stage tools

When you want to drive a single stage:

### Disc → files (`disc-extract`)

```bash
disc-extract --bin /path/to/disc.bin --out extracted/
```

Walks ISO9660 and writes every file. See [disc + ISO9660](../formats/disc.md).

### PROT.DAT → entries (`prot-extract`)

```bash
prot-extract --in extracted/PROT.DAT --cdname extracted/CDNAME.TXT --out extracted/PROT/
```

Splits PROT.DAT into 1232 numbered entries with CDNAME-derived filenames. Each extracted file's size is the entry's full on-disc footprint — `max(indexed_size, next_start - this_start)` — so trailing-overlay sectors past the TOC-indexed end (e.g. PROT 899's title-screen overlay code) are visible. See [PROT TOC](../formats/prot.md).

### LZS decode (`lzs-decode`)

```bash
lzs-decode raw --size N <file>          # standalone LZS body
lzs-decode container <file>             # multi-section player.lzs container
```

See [LZS compression](../formats/lzs.md).

### TIM → PNG (`tim`)

```bash
tim convert <file> --out <out_dir>
```

### TMD analysis (`tmd`)

```bash
tmd info        <file>            # header + object table
tmd dump-obj    <file> --out <prefix>     # OBJ-with-faces export
tmd validate-prims <DIR>          # bulk-walk every prim group, sanity-check
```

### VAB extraction (`vab`)

```bash
vab info <file> [--offset 0xN]
vab extract <file> --out <out_dir>     # WAV per program × tone
vab play <file> --offset 0xN --sample N --rate Hz
```

### Sub-asset extraction (`asset`)

The format-aware extractor:

```bash
asset categorize     <DIR> [--out categorize.json]    # per-class breakdown
asset extract        <file> --out <out_dir>           # streaming-format chunks → individual files
asset stream         <file>                           # walk DATA_FIELD chunks, no extraction
asset describe       <file>                           # asset-descriptor walk
asset effect-bundle  <file>
asset tmd-scan       <DIR>                            # bulk byte-search for TMD magic
asset tim-scan       <DIR>                            # bulk byte-search for TIM magic
```

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
# sheet, menu-glyph atlas, etc.) — these aren't reachable through
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
