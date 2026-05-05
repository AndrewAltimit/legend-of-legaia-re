# VAB sound bank

Sony's standard `VABp`-magic instrument bank format. Programs (up to 128) × tones (up to 16 per program) point into SPU-ADPCM voice bodies. Implementation: `crates/vab` (header parser + extractor + ADPCM decoder, sharing F0/F1 filter constants with `crates/xa`).

The format itself is documented externally; the Legaia-specific notes are:

- The dominant on-disc carrier is the [scene-VAB-prefixed streaming](scene-bundles.md) shape — the VAB body is preceded by a 4-byte chunk0 header. `crates/vab::parse_header(buf, offset)` accepts a starting offset so callers can skip the wrapper.
- A bulk scan finds 1191 `VABp` headers across 239 PROT entries. Top: `0889_sound_data2` (207), `0891_level_up` (206), `0890_sound_data2` (203) — multi-bank archives. The `vab_01` cluster (1072..1194) is the standard distributed-bank layout: 120 entries with 1–3 banks each.
- Block names from CDNAME can be misleading; trust the `VABp` magic rather than the surrounding cluster name.

## API

```rust
use legaia_vab::parse_header;
let header = parse_header(buf, offset)?;
println!("VAB v{} ps={} ts={}", header.version, header.ps, header.ts);
```

For bulk extraction of every VAB and per-program WAV files, see the `vab` CLI documented in [`tooling/extraction.md`](../tooling/extraction.md).
