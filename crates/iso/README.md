# legaia-iso

PSX disc image reader and ISO9660 walker.

The Legend of Legaia disc is shipped as Mode2/2352 (`.bin` + `.cue`). Each
2352-byte sector wraps 2048 bytes of user data behind a 24-byte header
(sync + Mode2/Form1 subheader) and an 8-byte EDC/ECC trailer. This crate
strips that wrapper and exposes a clean ISO9660 view.

## What it provides

- `raw::RawDisc` — sector-addressed reader. `read_sector(lba)` returns the
  2048-byte user payload only; the caller never sees raw 2352-byte sectors.
- `iso9660` — primary volume descriptor + directory walker. Yields
  `(name, lba, size)` tuples for every file on the disc.
- `region` — TCRF-derived heuristics for identifying which retail build
  (USA / JP / EU / debug) you're holding.

The single binary, `disc-extract`, drives all of the above.

## CLI

```bash
disc-extract list   <disc.bin>             # ISO9660 listing
disc-extract extract <disc.bin> <out_dir>  # walk + dump every file
disc-extract verify  <disc.bin>            # SHA-256 of the .bin
```

Verification table for the canonical NA build is in the root [`README.md`](../../README.md);
cross-reference [Redump](http://redump.org/disc/425/) for per-track hashes.

## Tests

`tests/disc_pipeline.rs` is a disc-gated integration test: it asserts file
count, key file SHA-256s, and that the ISO9660 walk reaches every entry.
The test reads `LEGAIA_DISC_BIN`; with the env var unset it skips (and
passes) so CI runs without redistributing Sony data.

```bash
LEGAIA_DISC_BIN="/path/to/Legend of Legaia (USA).bin" cargo test -p legaia-iso
```

## See also

- [`docs/formats/disc.md`](../../docs/formats/disc.md) — Mode2/2352 layout
  and the iso9660 primary volume descriptor.
- [`docs/reference/builds.md`](../../docs/reference/builds.md) — TCRF
  region table.
