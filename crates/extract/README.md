# legaia-extract

Top-level pipeline driver. One binary: `legaia-extract`.

## What it does

```text
disc.bin                          // input
   │  legaia-iso         verify SHA-256, walk ISO9660
   ▼
ISO9660 files                     // PROT.DAT, DMY.DAT, SCUS_942.54, MOV/, XA/, ...
   │  legaia-prot        TOC math, name from CDNAME.TXT
   ▼
PROT entries                      // 0865_battle_data.BIN, 0972_move_program_no.BIN, ...
   │  legaia-asset       categorize + streaming sub-asset extract
   ▼
Sub-assets                        // TIM, TMD, VAB, MES, ANM, stage-geom, scene bundles
   │  legaia-tim         TIM → PNG (--skip-png skips)
   │  legaia-xa          CD-XA demux → per-channel WAV (--skip-xa skips)
   │  legaia-asset       TIM catalog → TSV inventory (--skip-catalog skips)
   ▼
extracted/                        // human-browsable output tree
```

The final step writes the flat and deep TIM catalogs as TSVs
(`prot_tim_catalog.tsv` / `prot_tim_deep_catalog.tsv`) into the extract root,
so a headless extract carries the full texture inventory - the raw,
strict-validated TIM set plus the TIMs recovered from inside LZS-compressed
sections. These mirror the committed reference catalogs (metadata + FNV
fingerprints only, no pixel bytes).

The CD-XA step reads the raw disc directly (not the Form-1 dumps under
`extracted/XA/`, which truncate the Form-2 audio sectors and shuffle the
multiplexed channels together) and writes one correctly-paced WAV per
`(file_no, ch_no)` channel under `extracted/XA_WAV/`, each decoded at its
true per-sector rate / stereo mode. The 4-bit XA decoder is bit-exact, so
these WAVs are reference-quality. Non-4-bit channels are skipped with a
warning rather than mis-decoded (the NA corpus is entirely 4-bit).

Each stage is implemented in its own crate; this one wires them
together with a clap CLI and a SHA-256 check on the input.

## Usage

```bash
./target/release/legaia-extract "/path/to/Legend of Legaia (USA).bin" --out extracted

# Common flags:
#   --skip-png       skip the slow PNG conversion
#   --skip-xa        skip the CD-XA demux → WAV step
#   --skip-catalog   skip writing the TIM-catalog TSVs
#   --skip-verify    skip the input SHA-256 check
#   -v               per-file output
```

After it finishes, [`asset-viewer`](../asset-viewer/README.md) reads
straight out of `extracted/` to browse the assets.

## Disc-gated tests

`tests/validation_suite.rs` is the full-pipeline integration test:
PROT entry count, sub-asset totals, TIM round-trip. It runs only when
`LEGAIA_DISC_BIN` points at a valid disc:

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" \
    cargo test -p legaia-extract --release
```

With the env var unset, the test skips and passes - that gating is
intentional so CI works without redistributing Sony data.

## See also

- [`docs/tooling/extraction.md`](../../docs/tooling/extraction.md) -
  per-stage CLI invocations if you want to drive individual binaries.
- The root [`README.md`](../../README.md) - quickstart with disc SHA-256.
