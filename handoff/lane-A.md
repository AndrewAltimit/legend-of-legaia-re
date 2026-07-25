# Lane A handoff - the two instrument fixes, applied

Both changes were agreed in the previous wave and blocked only because the
instruments were frozen mid-wave. Both are applied here, each validated against
its per-row artifact rather than against a summary count.

## 1. `disc-coverage.py` - byte-level overlay attribution

`scripts/ghidra-analysis/dump-extent-attribution.csv` (committed, 754 rows,
keyed by `(entry, bytes)`) is now read by the gate. `--attribution` points at it
and can be pointed elsewhere or nowhere; absent, every overlay extent stays
ambiguous by address.

### Before / after

```
before   overlay battle_action   not meaningful (71% of its dumps are VA-ambiguous)
         overlay menu            not meaningful (100% of its dumps are VA-ambiguous)

after    overlay battle_action   99.8% (<=, 32.0% VA-ambiguous)
         overlay menu            not meaningful (50.2% of its extents are VA-ambiguous)
```

`SCUS_942.54` 95.4% and PROT data 92.3% are unchanged, which is the control:
the SCUS row is passed `attrib=None` by construction, and the data half is a
different measurement entirely.

### The ambiguity figure is 32.0%, not the projected 34.9%

The projection in `lane-2.md` and the CSV's own consumer-action table disagree
about one class, and the CSV is right. `identical` means several images hold
byte-identical code at that VA; the stated action is *credit each named image*,
and there are exactly 18 such rows. The projection instead left all 18 in the
residue. Every one of them names only `baka_fighter|dance`,
`cutscene_str|debug_menu` or `cutscene_str|fishing` - never a measured image -
so under the CSV's own rule all 18 leave both measured rows.

| | projected | applied | delta |
|---|---:|---:|---|
| `battle_action` residue / kept | 148 / 424 = 34.9% | 130 / 406 = **32.0%** | the 18 `identical` rows |
| `menu` residue / kept | 148 / 277 = 53.4% | 130 / 259 = **50.2%** | same 18 |

Both verdicts are the ones the projection called: `battle_action` crosses under
50 and becomes reportable, `menu` does not and stays **not meaningful**.

### Per-row validation

Not a count comparison. Every one of the 754 CSV rows was replayed through
`cover_image` for both measured images and its verdict asserted against its
class:

| class | `battle_action` | `menu` |
|---|---|---|
| `unique` | 68 kept, 380 dropped | 129 kept, 319 dropped |
| `identical` | 18 dropped | 18 dropped |
| `misbased` / `gapped` / `data` | 141 / 15 / 2 dropped | same |
| `short` / `unresolved` / `no_disassembly` | 75 / 33 / 22 kept (residue) | same |

The `unique` keep counts (68 / 129) and the credit-nobody total (158) reproduce
`lane-2.md`'s projection exactly, which is what pins the divergence above to the
`identical` class alone. Two controls also hold: an extent absent from the CSV
is never dropped, and `attrib=None` drops nothing at all.

### `menu` is structurally unfixable, not corpus-starved

`menu`'s span (`0x801CE818`, `0x15E8C`) is **wholly inside** `battle_action`'s
(`0x801CE818`, `0x28800`), so every extent in it falls in both spans by
construction. Most of what it loses is loss to the outer image, which is why the
same 130-extent residue is a much larger share of what remains. Dumping more
does not move it. That is on the page and in the script, so nobody re-opens it.

### The two denominators, reconciled

The report's old `71% / 100%` were per **dump file**; the CSV's shares are per
**distinct extent**. The report now uses distinct extents for the ambiguity
statistic - one extent can back dozens of dump files, and the mis-based print
batches are large enough that weighting by dump count measures the corpus rather
than the image. Both denominators are named in the report and in
`docs/tooling/disc-coverage.md`, alongside the `dumps` column, which stays per
dump file.

Concretely, per dump file the same post-attribution figures are 12.5% and 28.0%
- low enough to make even the inner nested span read as reportable. That is the
number not to quote, and the page says so.

One consequence for the CSV-absent path: it now prints `78.4% / 100%` where it
used to print `71% / 100%`. Same phenomenon, correct denominator; the verdict
(`not meaningful` on both rows) and the baseline are unaffected.

### Baseline

Refreshed in the same commit. `snapshot()` skips rows at `>= 50%`, so
`battle_action` is newly ratchetable and now sits at `99.82`.

One thing for whoever repairs dumps next: that figure is an **upper bound**
(`<=` on the row, 32.0% disclosed beside it) and 3,388 of its covered bytes come
from residue extents. Re-dumping the residue can therefore legitimately push it
*down*, and the ratchet will fire. That is the ratchet working - refresh the
baseline and say why - not a regression.

### The stale cache that bit twice

`extracted/PROT/categorize.json` is a cache nothing regenerates. This lane's
copy was stale on arrival: regenerating it changed the file (`5e9146ff` ->
`274c7a79`) while leaving `pct_parsed` at 92.28, so the headline was right by
luck and the per-class table was not. The trap is now recorded in the script
docstring and in `docs/tooling/disc-coverage.md` under "Running it".

### Not touched, deliberately

`lane-10.md`'s finding that the PROT *data* denominator itself double-counts
(`max()` totals 2.49x the archive) is a sibling lane's. Nothing here reconciles
against the 99.5% that correction implies, and the data half is untouched apart
from the cache warning.

## 2. `check-ui-host-drift.py` - transitive used-set

Host references now seed the used-set and propagate along engine-ui's own
builder-to-builder edges to a fixpoint. Body extraction is a Rust-aware brace
match (comments, string / raw-string / char literals skipped) so a
`format!("{}", ..)` cannot unbalance the scan; only builder-to-builder edges are
claimed, nothing further.

### Before / after

```
before   75 builders: 52 both / 8 native-only / 0 web-only / 15 unused
after    75 builders: 54 both / 7 native-only / 0 web-only / 14 unused
```

`unused` falls and `both` rises - the honest direction. No builder moved into
`native-only`, so nothing here inflates the drift bucket.

### Per-row validation

The whole builder call graph was dumped and the delta read off it, rather than
inferred from the counts. Exactly two builders changed bucket, each with a named
referencing builder that both hosts already draw:

| builder | was | now | reached through |
|---|---|---|---|
| `ap_gauge_sprites` | unused | native,web | `field_menu_icon_sprites_for`, `status_icon_sprites_for` |
| `title_tab_draws_for` | native-only | native,web | `tab_label_draws` |

Both were exactly the cases `lane-8.md` predicted. The graph's only other edges
are the shared text primitive `text_draws_for` (already `native,web`, so it
propagates nothing) and `dialog_panel_draws_for -> dialog_box_draws_for`, both
of which stay orphans - which is now *measured* rather than asserted in that
waiver's prose.

Note one correction to the deleted `ap_gauge_sprites` waiver: it named
`status_screen_draws_for` as a caller. The real edge is
`status_icon_sprites_for`.

### Waivers re-examined

The checker fails on a stale waiver, which is the proof. It flagged exactly two,
and both are deleted:

- **`ap_gauge_sprites`** (`kind = "orphan"`) - existed only because of this
  blind spot; its own reason said so.
- **`title_tab_draws_for`** (`kind = "web_missing"`) - the browser page draws
  its tabs through `tab_label_draws`, which delegates here, so the geometry was
  already reaching web. Reached one call deep is reached.

The other 14 `orphan` waivers were each re-read. All survive: they are screens
no host opens, not widgets composed into a drawn screen. One reason was rewritten
rather than deleted - **`sprite_draws_for`** cited the same blind spot ("the same
reason `ap_gauge_sprites` is waived"), which is now false. It is genuinely
uncalled: its only reference outside its own definition is a unit test under
`engine-render/src/tests/`, which the checker excludes by design. The new reason
draws that distinction explicitly, so the next reader does not re-derive it.

The waiver file header now states that "reaches" is transitive, so a future
waiver is not written for a builder some drawn screen already folds in.

## Files

- `scripts/ci/disc-coverage.py`, `scripts/ci/disc-coverage-baseline.json`
- `scripts/ci/check-ui-host-drift.py`, `scripts/ci/ui-host-drift-waivers.toml`
- `docs/tooling/disc-coverage.md`, `site/_content/tooling/disc-coverage.html`

Gates run clean: `check-doc-density.py`, `check-md-links.py`,
`check-site-links.py`, `check-shell-observer-traps.py`,
`disc-coverage.py --check`, `check-ui-host-drift.py`.
