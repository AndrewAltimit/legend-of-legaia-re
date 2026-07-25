# Lane C - the PROT entry-size denominator

`crates/prot` now sizes an entry as `toc[p+3] - toc[p+2]`. `Archive::read_entry`
returns the entry and nothing a neighbour owns. The `max(toc[p+5] - toc[p+3] +
4, footprint)` reading is gone from every parsing path.

## What landed

- **`crates/prot/src/archive.rs`** - `Entry::size_sectors` / `size_bytes` are
  the sector gap to the next entry, taken from
  `runtime_toc::entry_sector_span_from_archive_toc` (the port of
  `FUN_8003E68C`), so the arithmetic has one implementation. The old
  `indexed_size_*` fields are renamed `declared_span_*` and documented as *not*
  a size of entry `p`: they expand to `size(p+1) + size(p+2) + 4`.
- **`crates/prot/src/tiling.rs`** (new) - the property that makes the reading
  self-defending, as a measurement rather than prose. `check(&entries)` reports
  monotonicity, gaps, overlaps, the total, and the covered span.
- **`crates/prot/tests/archive_tiling_real.rs`** (new, disc-gated) - asserts
  the retail entries tile `PROT.DAT` exactly and that the declared spans cannot
  be a partition of it.
- `read_entry_indexed` survives as a forwarding alias of the new
  `read_entry_declared_span` **only** so out-of-scope callers keep compiling -
  see "Still open" below.
- Docs: `docs/formats/prot.md` rewritten around the correct size, with the six
  evidence lines and a section on what reading the neighbours produced;
  `docs/formats/overview.md`, `crates/prot/README.md`, `crates/asset/README.md`,
  `crates/asset/src/{summon_overlay,bse_bank}.rs` follow. Two links into the
  renamed `prot.md` heading fixed (`docs/formats/bse-dat.md`,
  `docs/tooling/disc-coverage.md` - one-line anchor fixes only, no content).

## The measurement

1233 entries, LBA 121..59206 = 59085 sectors, **no gaps, no overlaps**, total
equal to the span, and the span reaching the archive's last sector. The two
words below entry 0 (`toc[0]` = LBA 3, `toc[1]` = 55) tile the rest, so the
whole file from the end of the 3-sector TOC is covered. The declared spans
total 2.08x the archive as the parser clamps them, 2.49x raw.

931 of 1233 entries shrink; none grows.

## Findings - things that were being read off the neighbours

Each of these was load-bearing somewhere, and each is now either corrected or
flagged in place.

- **`scene_scripted_asset_table` does not exist. 79 → 0.** A "prescript-prefixed
  asset table at a 0x800-aligned offset" is, in every one of the 79 cases, a
  sector boundary that *is the next entry's start LBA* - the neighbour's
  ordinary offset-0 table seen through the over-read. Sweeping every
  0x800-aligned offset of every entry under both windows: 247 tables under the
  old one, of which **145 sit at a non-zero offset and 145 of 145 land exactly
  on another entry's start LBA**; under the corrected one, every table is at
  offset 0 with every descriptor payload inside its own entry. The 88 bare
  tables are unchanged, so no content is lost - the 79 were duplicates. The
  carriers reclassify as `scene_event_scripts` (21 → 78). The class is pinned
  at **0** rather than deleted, so a regression that resurrects it fails.
  The `0x1000` variant is the same shape one row further out;
  `scene-v12-table.md` had already caught two instances of it.
- **`init_pak` broke under the correct size, and the detector was wrong.** Its
  parser required `>= 0x30000` bytes - PROT 0895's *old* declared span. The
  entry is 75 sectors (`0x25800`) and all four publisher-logo TIMs sit inside
  it. The floor is now the reach of the last logo TIM; the four TIM headers
  were always the discriminator.
- **Static-overlay pointer-resolution votes were borrowed.** The base
  cross-check counts an image's own LUI+ADDIU self-pointers that resolve inside
  it. A longer window inflates *both* halves - more code, and a wider
  acceptance range. PROT 0901 carries three such pairs on its own sectors, not
  nine, so the flat `total >= 8` bar was being met with a neighbouring
  overlay's code. Re-derived: `total >= 3`, and a thin sample (`< 8`) must
  resolve **all** of them rather than 70%.
- **The residual non-resolvers are `.bss`, not noise.** For PROT 0924 / 0967
  they cluster in `0x801F99xx..0x801FA4xx`, just above each image's end - the
  working storage a PSX overlay reaches past its loaded image inside the slot-B
  buffer, which is by definition not in the file. The over-read window
  swallowed those addresses and scored them as hits. The large-sample bar moved
  0.70 → 0.60 for that reason; 0924 sits at 0.61 and 0967 at 0.76.
- **"0900 and 0901 are shifted copies" is a tautology.** The cited identity
  `0900[0x2800:0x5000] == 0901[0x0:0x2800]` compares entry 0901's bytes with
  entry 0901's bytes: PROT 0900 is `0x2800` bytes and 0901 begins exactly
  `0x2800` past its start. Flagged in the `static-overlays.toml` header, which
  now warns that any `notes` claim comparing two overlays at an offset, or
  quoting a resident span, may be describing the over-read.
- **`clean_copy_bytes = 0x28800` for PROT 0898 is exactly its 81 sectors.** The
  RAM-verified clean-copy prefix and the corrected entry size agree to the
  byte - an independent capture-grade confirmation that 81, not the declared
  83, is the entry.

## Censuses re-pinned (expectations only, no logic)

`crates/extract/tests/validation_suite.rs`:

| | was | now | why |
|---|---|---|---|
| `PINNED_ENTRY` 148 | 172032 | 83968 | 41 sectors |
| `scene_scripted_asset_table` | 79 | 0 | the phantom |
| `scene_event_scripts` | 21 | 78 | the same entries, by their own content |
| `field_map` | 101 | 104 | three more resolve at exactly `0x12000` |
| `scene_tmd_stream` | 182 | 179 | shape was completed by a neighbour |
| `lzs_container` | 34 | 33 | walk no longer completes in-entry |
| `overlay_data_blob` | 26 | 24 | |
| `zero_sector_high_entropy` | 4 | 0 | the high-entropy body was the next entry |
| `all_zeros` | - | 4 | those same four |
| `mostly_zeros` | 0 | 16 | honest verdicts on small entries |
| `unknown_low_entropy` | 0 | 8 | |
| `data_field_truncated` | - | 1 | |
| `EXPECTED_LZS_CONTAINERS_STRICT` | 113 | 110 | |

`scene_asset_table` (88), `scene_vab_stream` (218), `scene_v12_table` (97),
`pochi_filler` (266), `EXPECTED_STREAM_HITS` (34) and
`EXPECTED_TOTAL_SUBASSETS` (583) are unchanged. The classes sum to 1233.

`crates/asset/data/static-overlays.toml`: 26 of 31 `fingerprint_sha256` values
recomputed. Disc-derived hashes, no Sony bytes.

**For the coverage lane:** 29 entries now land in statistical buckets
(`mostly_zeros` 16, `unknown_low_entropy` 8, `all_zeros` 4,
`data_field_truncated` 1) where the previous wave had driven them to 0. They
are small entries whose old classification came from borrowed bytes. Whatever
the by-bytes number turns out to be, the denominator is now a partition of the
archive rather than a 2.5x over-count - re-ratchet against that.

## Still open (out of this lane's scope)

- **`crates/engine-core/src/scene/prot_index.rs`.** `entry_bytes` calls
  `read_entry_indexed`, which still returns the declared-span window - the
  engine's main scene-parse path reads a window that starts correctly and then
  runs into a neighbour (931 entries) or truncates the entry (302). Behaviour
  is bit-identical to before this lane, deliberately: I could not test the
  engine here. Migrating it to `read_entry` is a one-line change plus whatever
  `find_bundle` does when the `SceneScriptedAssetTable` /
  `V12Embedded { table_offset: 0x1000 }` fallbacks stop matching. The survey
  says that should *improve* resolution - the real table is at offset 0 of a
  sibling entry in the same block - but it needs `scene_chain_e2e` to confirm.
  `entry_bytes_extended` already tracks `read_entry` and is correct now.
- **`crates/engine-core/tests/stager_lba_footprint_disc.rs`** asserts
  `entry_bytes_trimmed` equals `unique_content_len(entry_bytes_extended)`. That
  still holds, but the two inputs are now identical, so check whether it has a
  non-vacuity assertion that expects them to differ.
- **`crates/web-viewer/src/disc.rs`** (~line 273) reimplements the old
  `max(indexed, footprint)` math with its own constants. The browser disc
  browser is on the wrong denominator until that mirrors `crates/prot`.
- **`crates/mednafen/tests/static_overlay_clean_copy.rs`** byte-matches the
  committed overlays against resident RAM images. 26 of those images just got
  shorter; the prefix comparisons should still pass but have not been run here.
- **Docs outside this lane's scope that describe the old behaviour**:
  `docs/tooling/extraction.md` (the `.BIN` size rule, the `OVR` column, and
  `--clamp-footprint`, which is now an accepted no-op),
  `docs/guides/extracting-assets.md`, `docs/formats/scene-bundles.md` (the
  "descriptor offsets address the extended footprint" section and the
  `SceneScriptedAssetTable` / `V12Embedded` prose),
  `docs/formats/battle-data-pack.md` (`indexed_size = 7811`),
  `docs/tooling/disc-coverage.md` § "The data denominator counts some disc
  bytes more than once", plus the two `site/_content/` mirrors of the guides.
- **`extracted/` was not regenerated.** Every `.BIN` on disk predates this
  change; 931 of them carry a neighbour's bytes in the tail. `prot-extract
  locate --in-entry N` names that case explicitly.

## Verification

- `cargo test -p legaia-prot`, `-p legaia-asset --release --no-fail-fast` (73
  targets), `-p legaia-extract --release`, `-p legaia-iso --release` all pass.
- `cargo fmt` clean; `cargo clippy --all-targets -- -D warnings` clean on all
  four crates.
- `check-doc-density.py` and `check-md-links.py` both OK over 168 files.
- Disc gating proven by **contrast**, not pass count. `legaia-asset`: **0**
  `[skip]` lines with `LEGAIA_DISC_BIN` set vs **103** with it unset, 73 targets
  ok either way. `legaia-prot` tiling + span + tail oracles: **0** vs **5**.
  `legaia-extract` `validation_suite`: 3.74s with the disc, 0.00s without.
- CLI smoke: `prot-extract list` reports 1233 entries and flags 931 `OVR` rows
  (where the historical expression overshoots); `locate --in-entry 865 0x30000`
  resolves inside entry 865 rather than in the monster archive.
