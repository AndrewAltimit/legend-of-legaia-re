# RECONCILE — items the legibility refactor did NOT resolve

This is a scratch list for the human maintainer. The documentation legibility
refactor is a **pure structural pass** (same facts, more readable layout). Per
its prime directive it does **not** adjudicate truth: where two passages appear
to disagree, or where a link was already broken before the refactor, the item is
recorded here instead of being "fixed" by guessing.

## Content contradictions

**None found.** Every passage that reads like a reversal (a `FALSIFIED` /
`corrected` / "the old X was wrong" note) is a *self-labeled* falsification of a
prior hypothesis — the doc states both the old reading and why it is wrong, on
purpose. That is documented provenance, not two sources disagreeing, so nothing
was collapsed or picked-between. No genuine fact-level contradiction surfaced in
any stream (`docs/formats`, `docs/subsystems`, `docs/tooling`, `docs/reference`,
`crates/*/README.md`).

## Pre-existing dangling internal links (NOT introduced by this refactor)

These links were already broken on `HEAD` (present in the prior version, but
their target heading did not exist anywhere in the target file). They have all
been **resolved** — each had a single defensible intended target once its
context was read:

- `docs/formats/world-map-overlay.md` → `#live-snapshot-drake-post-warp-settled`
  — the link's surrounding text describes "real TMDs in **steady state**", which
  matches `### Live snapshot (settled field scene)` (the `drake_world.bin` settled
  scene), not `### Live snapshot (Sebucus mid-warp)`. Repointed to
  `#live-snapshot-settled-field-scene`.
- `docs/subsystems/world-map.md` → `../formats/encounter.md#engine-port-region-keyed-roll`
  — `encounter.md` had the matching material under a **bold paragraph lead**
  (`**Engine port (region-keyed roll).**`) rather than a heading, so no anchor
  existed even though the link's slug matched the lead text exactly. Promoted that
  lead to a `#### Engine port (region-keyed roll)` heading, which both makes the
  existing link resolve and gives the engine-port subsection a navigable anchor.

Three further pre-existing dangling anchors were likewise repaired because their
intended target was unambiguous (the section had simply been renamed): the
`FIELD_SHARED_BLOCKS`, `BGM lookup`, and `befect_data` cluster links now resolve.

No outstanding link issues remain.

## Note on one intentional de-duplication

In `docs/formats/character-mesh.md` the over-budget "Readers (retail)" table cell
for `FUN_800513F0` was reduced to a one-line summary + an anchor link to its full
`§ Battle form, Loader provenance` trace on the same page. Every fact (including
`actor = *(0x801C9360 + i*4)` and the `DAT_8007C018[0..2]` write-watchpoint
pinning) survives in that section and at the other in-page mentions; the cell now
points to it rather than repeating the full trace.
