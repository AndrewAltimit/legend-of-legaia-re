# Lane R - the assets named by the entry the over-read started in

Seven `legaia-web-viewer` tests were red on this branch and green on `main`.
They share **one** root cause, and it is not a reader bug: it is a naming bug
that `afd3558b` exposed.

## The root cause

`afd3558b` narrowed `Archive::read_entry` to the entry's own sectors, and it is
**right** to. Verified at the disassembly, not the C:

- `ghidra/scripts/funcs/8003e68c.txt` is twelve instructions and returns
  `TABLE[i+3] - TABLE[i+2]`.
- `FUN_8003E8A8` (`prot_toc_resolver`) loads `TABLE[i+2]` into `s2`, `TABLE[i+3]`
  into `v0`, stores the start LBA at `gp+0x8f0`, and returns `subu v0, v1, v0` -
  the same difference, computed beside the LBA it resolves.

So the narrow expression is retail's. Nothing here widens a reader.

What broke is that a family of **constants** of the form "asset X is at PROT `N`
offset `K`" was measured inside the old wide window. Where `K` ran past entry
`N`'s real end, the pair still named a real place on the disc
(`start_lba(N)*0x800 + K`) - just not the entry it claimed. Re-keying each to
the entry whose own sectors hold those bytes changes no byte and fixes the read.

Three coordinates, seven tests:

| Coordinate | Was | Is | Tests |
|---|---|---|---|
| Kingdom bundle | PROT `0085`/`0244`/`0391` | `0086`/`0245`/`0392` | `ocean_assets`, `world_overview_regression` |
| Battle-char atlases | PROT `1204` `0x25804 + k*0x8224`, 7, last truncated | PROT `1205` `4 + k*0x8224`, **8**, none truncated | `battle_palette_overlay` (x2), `muscle_web_real`, `baka_presentation_wasm_api` |
| Title TIM | PROT `0888` `0x1AA28` + "duplicates" at `0889`/`0890` | PROT `0890` `0x14228`, one copy | `new_game_flow_parity` |

`new_game_flow_parity` was **not** in the reported failure list because it
`SIGABRT`s instead of reporting: the missing title art reaches
`crate::console_log`, which builds a `JsValue`, which panics
"function not implemented on non-wasm32 targets" in a `nounwind` frame. A
target that aborts contributes no `FAILED` line, so it is invisible to a
grep-for-`FAILED` triage. Worth remembering.

## Per test: code or expectation?

Six of seven: **the code regressed** - production reads that had been resolving
through the over-read. One (`vahn_battle_palette_lands_on_mesh_rows`) also had a
wrong *expectation*: it read Vahn's palette from PROT `0861` (a one-sector
entry) where the production path `battle_char_vram_bytes_battle` already used
`0863` = `PLAYER1`. Corrected to 0863.

`world_overview_regression`'s fixture digests moved, and **only** because the
digest text carries `prot_base=`. Proven before rebaselining: reading entries
86/245/392 while still printing `prot_base=85`/`244`/`391` reproduces the
committed digests **exactly** (test run green against the old fixture), so every
pack-TMD fingerprint, every MAN placement record and every classification row is
byte-identical. Then the label was made honest and the fixture rewritten.

## The eighth atlas

Entry 1205's own 131 sectors hold **eight** type-`0x00` (TIM) streaming chunks,
`4 + 8*0x8224 = 0x41124` of `0x41800` with the remainder zero. CLUT rows read
off the TIM headers: `490, 491, 492, 493, 494, 495, 497, 496`. The old window
ended at `0x36800` into 1205 - past atlas 6 (`0x30CDC`), short of atlas 7
(`0x38F00`) - so the eighth atlas was invisible and its row read as a gap in a
490..497 run. The live capture already disagreed with "seven": a played battle
uploads party CLUTs at rows 490..497 *including* 496
(`re-settled-threads.md`). `parse_atlases` now walks the chunk chain and takes
each row from the TIM, so the constant can no longer be the source of the answer.

## User-visible symptoms fixed (all three were live, not test-only)

1. **Title screen had no art**, native and browser: `run.rs` and
   `boot_title.rs` both read `title_pak::PROT_INDEX_PRIMARY`, so both fell back
   to the menu-glyph rendering.
2. **Battle party characters lost every texture atlas**:
   `engine-shell/window/battle.rs` bailed out of the whole
   `if let Ok(pack) = battle_char_pack::parse(..)` block, which also carries the
   1204 fallback mesh path.
3. **World-map park/ocean CLUT strips lost their fallback source**:
   `field_render.rs`'s `DRAKE_KINGDOM_BUNDLE_ENTRY` was `85`.

The world-overview viewer itself survived because `scene_geom.rs` already probed
`prot_base` then `prot_base + 1`; the two failing tests hard-coded the base
without that fallback.

## API changes

`battle_char_pack::parse(mesh_entry, atlas_entry)`, plus `parse_slots(entry)` /
`parse_atlases(entry)` for callers that need one half. `ATLAS_COUNT` 7 -> 8,
`FIRST_ATLAS_OFFSET` `0x25804` -> `4`, new `ATLAS_PROT_ENTRY_INDEX` = 1205,
`ATLAS_CHUNK_TYPE` = 0x00. `kingdom_bundle::BUNDLE_ENTRIES` /
`PRESCRIPT_ENTRIES` / `bundle_entry_for`. `title_pak::PROT_INDEX_PRIMARY` 888 ->
890, `TITLE_TIM_OFFSET` `0x1AA28` -> `0x14228`,
`TITLE_TIM_ALTERNATE_SOURCES` now empty. CLI gains
`asset battle-char-pack --atlas-entry`.

## Tests re-aimed off stale `extracted/PROT/*.BIN`

`extracted/` was never regenerated after `afd3558b`, so 931 `.BIN`s still carry
a neighbour's tail - a test reading one of them cannot see this class of defect
at all. `battle_char_pack_real`, `title_pak`'s two disc-gated unit tests and
`engine-core`'s `title_screen_atlas` test now read `extracted/PROT.DAT` through
`Archive::read_entry`. `real_pack_layout` additionally asserts each half fits
its own entry, that the mesh entry is *not* parseable as the atlas entry, and
that nothing follows the eighth chunk.

## Still open (not this lane's failures, flagged with numbers)

- **`extracted/` is stale.** Any test or script keyed to a `.BIN` filename is
  measuring the old window. Re-running `legaia-extract` would close it.
- **Entries that shrank and still carry offset constants**, none currently
  failing a test but each a candidate for the same defect: `0863` `0865` `0866`
  (player battle files), `0888`, `0895`, `0898`, `0975`, `1006`, `1043`, `1048`,
  `1054`, `1199`, `1201`, `1202`, `1206`, `1228`. Checked and clean: `0874`
  (descriptor count is `meta[0]` = 3, all inside its `0x19800`), `1203` (count 4,
  all inside `0x29000`).
- **Site fragments lagged two earlier commits on this branch.**
  `site/_content/formats/prot.html` still documented
  `size = max(toc[p+5]-toc[p+3]+4, gap)` as the entry size, and
  `site/_content/reference/re-do-not-re-walk.html` has no Containers section, so
  the pochi-filler row from `3afb8305` is not mirrored there. `prot.html` is
  fixed here; the pochi row belongs to its own lane.
- **`engine-core` red targets after this lane: 3** -
  `op49_window_census_disc` and `summon_scene_real` (both diagnosed in
  `lane-C.md`, unchanged), plus `opening_progression_oracle`, which fails
  `checked >= 1` because no `.sstate` in `captures/` matches its anchor
  fingerprints - environmental, and it asserts before touching PROT at all.

## Verification

- The seven: green. `cargo test -p legaia-asset -p legaia-web-viewer -p
  legaia-prot --release --no-fail-fast` clean.
- `engine-core`: `title_screen_atlas` green; 3 red targets as above.
- `engine-shell`: builds; `battle_party_pose_live` 3/3.
- Disc gating by **contrast**: the six re-aimed web-viewer targets print **0**
  `[skip]` lines with `LEGAIA_DISC_BIN` set and **12** with it unset (13 tests,
  one not disc-gated), passing either way.
- `cargo fmt --all -- --check` clean; `cargo clippy --all-targets -- -D
  warnings` clean on `legaia-prot`, `legaia-asset`, `legaia-web-viewer`,
  `legaia-engine-core`, `legaia-engine-shell`.
- Both doc gates OK over 169 files; `site/_gen.py` re-run, no broken links.
