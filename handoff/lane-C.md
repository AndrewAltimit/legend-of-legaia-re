# Lane C - the PROT entry-size denominator

> **Read the second pass first** if you are picking this up:
> [Is a scene bundle one entry or a span?](#is-a-scene-bundle-one-entry-or-a-span)
> answers the question the integration run raised and lists what is still red.

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

## Still open after the first pass (each addressed in the second - see below)

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

---

# Second pass - the engine

## Is a scene bundle one entry, or a span?

**One entry, always.** The integration run raised this as genuinely open, with
the multi-entry reading looking plausible: the entries tile exactly, so a
bundle spanning several of them would be coherent. It isn't what the disc says.

Sweeping every CDNAME block, reading each entry as its own sectors and probing
offset 0 for a MAN-bearing asset table:

- **90 blocks carry exactly one.** Not one block carries two.
- **Zero tables have a descriptor payload that reaches past their own entry.**
- 35 blocks carry none. 22 of those are not scene blocks at all (`battle_data`,
  `monster_data`, `sound_data`, `befect_data`, `player_data`, the VAB / music
  banks, …); the 13 that are scenes take the streaming-MAN fallback, which is
  the documented path for `rikuroa` / `dolk2` already.

The block layout is uniform and positional:

```text
[.MAP  36 sectors] [v12 header  1] [prescript  1..3] [bundle  N] [...]
   e.g. map02: 242, 243, 244, 245      geremi: 163, 164, 165, 166
```

A v12 header is one sector, so its documented prescript at `+0x800` is the
**next entry** at offset 0, and a table at `+0x1000` is the entry after that.
That is the whole mechanism behind the phantom `scene_scripted_asset_table`
and `V12Embedded { table_offset: 0x1000 }` readings, and it also explains
`dolk` / `keikoku`: their "scripted bundle at 0x800" and their plain bundle at
offset 0 are the *same disc sector*, named twice.

So there is no span reader to build. What there was is a **split**: the engine
detected bundles against `ProtIndex::entry_bytes` (still the declared-span
window, which I had deliberately left alone) and extracted against
`entry_bytes_extended` (already the corrected entry). Detection found a
one-sector prescript entry's "table"; extraction then resolved its descriptor
offsets against 2048 bytes and failed. The two reported failures are exactly
that split, and closing it is the fix.

## What the second pass changed

- **`ProtIndex::entry_bytes` reads the entry** (`Archive::read_entry`). One
  view now, for detection and extraction alike. `entry_bytes_extended` and
  `entry_bytes_lba_footprint` stay as names that say what a call site needs;
  both return the same bytes.
- **`Scene::load` stopped special-casing** `SceneAssetTable` / `LzsContainer`
  to a wider window. There is no wider window.
- **`Scene::find_event_scripts` gained a positional fallback.** The standalone
  `scene_event_scripts` detector's frame-opener-rate gate is what makes it
  zero-false-positive on a context-free buffer, and it rejects the small
  prescripts - `geremi`'s is 3 records with none opening on the `-1`
  transform-node sentinel, `opurud`'s is 8 with 3. Inside a scene there *is*
  context: the entry immediately after a `SceneV12Table` and immediately
  before the bundle is the prescript, so take the structural read there. This
  is what the phantom class used to supply.
- `Archive::read_entry_indexed` deleted from `crates/prot` - the alias existed
  only for the engine call site that just went away.
- `crates/web-viewer/src/disc.rs` no longer reimplements the size rule: its
  zero-copy TOC walk calls the same ported span routine `crates/prot` does.
- Docs: `docs/formats/scene-bundles.md` (descriptor offsets, `Scene::load`,
  the scripted / V12Embedded sections, `scene_event_scripts`),
  `crates/asset/src/scene_asset_table.rs`, and the `prot.md` section this all
  hangs off.

## What the migration exposed, and what it costs

Making the engine consistent is not free: it moved a lot more than the two
reported failures. The taxonomy, from the full `-p legaia-engine-core --release
--no-fail-fast` run:

1. **"scene `X` has no event scripts" - 18 scenes** (`bylon`, `cave01`,
   `geremi`, `izumi`, `jiji`, `koin1`/`1b`/`2`/`3`/`4`/`6`, `kor`, `kor3`,
   `kor4`, `opurud`, `tunnela`, `tunnelb`, `urudre2`). Not a data loss: the
   prescript is at offset 0 of its own entry in every case, and the phantom
   class was what used to claim it. Closed by the positional fallback above -
   `scene_change_destinations_all_enter_without_error` goes 52/70 → 70/70.
   Everything downstream of scene entry (the opening chain, the `0x4C 0xD8`
   census, the cold-spawn and round-trip drivers) rides on this one.
2. **`dolk` / `keikoku` bundle identity.** They resolve `Plain` at offset 0 of
   the bundle entry instead of `Scripted` at `0x800` of the prescript entry.
   Same sector; the test expectation moved.
3. **Resource-sweep counts - the one that is *not* just an expectation.**
   `town01`'s TMD pool goes 119 → 148 against a live-RAM anchor that says
   **119**. This is not the bundle entry growing (it shrinks, 224 → 111
   sectors): it is the *other* entries in the block, which the old reader
   truncated. `town01` entry 5, the block's `field_pack`, goes 83 → 180
   sectors, and a byte scan over its now-complete content finds meshes the old
   window cut off. The 119 agreement was a coincidence of two errors: the
   engine builds its pool by **byte-scanning every entry in the block**, where
   retail populates it from the **asset-table descriptor walk**. With correct
   extents the heuristic over-collects, and re-pinning it to 148 would bless
   the wrong method. The fix is to build the pool from the descriptor walk -
   real work, and the reason this is more than one pass. Same shape:
   `gimard_summon_spawns_and_ticks_through_the_move_vm` / `world_spawns_...`
   ("mesh parts produce draws"), `pochi_leftovers_never_reach_the_ground_atlas_page`.
4. **Corpus censuses that count entries or sites** -
   `man_variant_carrier_census_disc` (`gameover_data`'s dev-copy sibling of the
   Rim Elm `0x225` latch no longer appears; the `0x225` gate-variant count is
   3, not >= 4), `op49_window_census_pins_the_corpus_shape`. Each needs its
   own look at whether the old number counted a neighbour's bytes.

## Where it landed: 11 known-red, each with a diagnosis

`cargo test -p legaia-engine-core --release --no-fail-fast`: **159 targets ok,
7 red (11 tests)**, from 17 red targets (23 tests) before the event-scripts
fallback. Everything functional recovered - all 70 scene-change destinations
enter, the opening chain runs to Rim Elm, cold spawn / round-trip / hub-sweep
drivers pass, `dolk` / `keikoku` / `rikuroa` / `dolk2` all resolve their MAN.

I did **not** re-pin the remaining 11. Each is a number that was measured
through a neighbour's bytes, and blessing the new number would bless the method
that produced it:

| Test(s) | Diagnosis |
|---|---|
| `town01_npc_placements_resolve_models_and_anim_records` | TMD pool 119 → 148 against a live-RAM anchor of **119**. The engine byte-scans every entry in the block; retail populates the pool from the asset-table descriptor walk. Correct extents make the heuristic over-collect (block entry 5 alone goes 83 → 180 sectors). Fix the method, not the number. |
| `catalogued_field_states_match_retail_npc_and_flag_state`, `pochi_leftovers_never_reach_the_ground_atlas_page` | Same sweep, same cause. |
| `gimard_summon_spawns_and_ticks_through_the_move_vm`, `world_spawns_and_ticks_the_gimard_summon` | `mesh_part_count() >= 1` on PROT 0905. Its own 4 sectors carry only transform nodes; the mesh-bearing parts came from 0906/0907, and `summon_overlay`'s own docs say a neighbour's spawn sites resolve record pointers valid only for that neighbour's load. Zero mesh parts is consistent with the corpus (`enemy_stager_real` asserts only `nodes > meshes`). Settle it with the mid-cast capture that already exists (`summon_binding_base_high`), not a re-pin. |
| `disc_corpus_contains_4c_d8_opcode_pattern`, `drives_real_balden2_4c_d8_into_synchronous_spawn` | 0 hits. `0x4C` is a **field-VM** opcode and the field VM's scripts live in the scene MAN - but the census scans *event-script* entries, which are move-VM prescripts. It only ever found hits because the prescript entry's window ran into the bundle. The census is pointed at the wrong carrier. |
| `flag_549_reader_is_the_rim_elm_p2_gate`, `flag_549_writer_is_the_rim_elm_p2_3_self_latch`, `flags_0x5a1_and_0x6c3_are_write_only_cutscene_toggles` | MAN-variant census. `gameover_data`'s dev-copy of the Rim Elm `0x225` latch is gone and the `0x225` gate-variant count is 3 rather than >= 4 - the extra "variants" were the same MAN reached through more than one entry's window. Confirm that reading before re-pinning. |
| `op49_window_census_pins_the_corpus_shape` | Site counts over the same corpus; it already prints `[past-footprint]` for sites it could not window. |

Two threads, then, for whoever takes this next: **rebuild the scene TMD/TIM
pool from the asset-table descriptor walk** (closes rows 1-3), and **re-point
the `0x4C` census at the MAN** (closes row 4). The MAN-variant census is a
third, smaller one.

## Also closed in this pass

- **`crates/web-viewer/src/disc.rs`** now calls the ported span routine instead
  of reimplementing `max(indexed, footprint)`. The browser disc browser is on
  the same denominator as everything else.
- **`crates/mednafen/tests/static_overlay_clean_copy.rs`** run: 4/4 pass, and
  `battle_action` reports **RAM-matched `0x28800` bytes** - the whole corrected
  entry is byte-identical to the resident image. That is the entry size
  confirmed against live RAM, end to end.
- `crates/engine-core/tests/stager_lba_footprint_disc.rs` passes; its two
  inputs are now the same bytes and it has no assertion that they differ.

## Verification (second pass)

- `cargo test -p legaia-engine-core --release --no-fail-fast`: 159 ok / 7 red
  targets, as above. `-p legaia-mednafen --release --test
  static_overlay_clean_copy`: 4/4.
- `cargo fmt` clean; `cargo clippy --all-targets -- -D warnings` clean on
  `legaia-prot`, `legaia-asset`, `legaia-engine-core`, `legaia-web-viewer`.
- `check-doc-density.py` + `check-md-links.py` OK over 168 files.
- Disc gating by **contrast**: the three engine-core targets re-run at the end
  print **0** `[skip]` lines with `LEGAIA_DISC_BIN` set and **7** with it
  unset, passing either way.
