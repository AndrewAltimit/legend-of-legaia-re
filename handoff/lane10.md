# LANE 10 handoff - display-list reads for the two open render threads

Scope kept to `scripts/mednafen/**`, `crates/mednafen/src/**`, `crates/pcsxr/**`,
`docs/tooling/mednafen-automation.md`. The coplanar kernel was **not** touched -
per the brief, acting on the finding is the next wave's work.

Proposed doc text for the coordinator-owned threads page is in
[`lane10-threads.md`](lane10-threads.md).

---

## 1. The brief's corpus premise was wrong (grade: `capture`)

> "`~/.mednafen/sav/*.mcr` - 60 of these; they are mednafen save STATES despite
> the extension, not memory cards"

They are **memory cards**. Every one is exactly 131072 bytes and opens with the
`MC` block-allocation magic. They carry save blocks, not main RAM - no scene
anchor, no display list, nothing this task could use. The real mednafen state
corpus is `~/.mednafen/mcs/*.mc{0..9}` (90 files including `backup/`), which the
brief did not mention.

`captures/` also holds far more than the brief implied: **211** PCSX-Redux
states, not 11.

Corrected totals swept: **90 mednafen + 211 PCSX-Redux = 301 states**, 7 memory
cards skipped.

## 2. The PCSX-Redux reader was dropping ~2/3 of the corpus (grade: `capture`)

`legaia_pcsxr::SaveState::from_sstate_bytes` assumed gzip. PCSX-Redux writes a
`.sstate` **either** gzipped (emulator save-state slots) **or** bare protobuf
(anything written from a Lua probe's snapshot call - magic
`0a 1b 0a 17 "PCSX"`, ~19 MB each). Every `captures/**/snap_*.sstate` and
`autosave_*.sstate` is bare, so they all failed with `invalid gzip header`.

Fixed by dispatching on the `1f 8b` magic. Before: 21 scenes indexed. After:
**50**. Both threads' scenes only became visible after this fix - it is the
single change that unblocked the task.

## 3. Scene coverage index (new, reusable)

`scripts/mednafen/state-index.py` + `mednafen-state identify` +
`pcsxr-state identify`. Both CLIs read the same anchors via the new shared
`legaia_mednafen::game_anchors` (which `legaia_pcsxr` now delegates to instead of
duplicating), so the two corpora index into one table.

50 scenes covered. Headline rows for this task:

| scene | states | modes |
|---|---|---|
| `teien` | 2 | `field-init`, `battle-init` - **no field-run** |
| `jou` | 10 | 2 `mode_01`, 8 `battle` |
| `jouine` | 2 | `field-run`, `battle` |
| `jouind` | 1 | `field-run` |
| `chitei2` | 0 | not covered |

Full table: run the script, or `--json` it.

**Trap worth knowing:** a `snap_*_scene_<name>` filename is the scene the *probe*
tagged, not proof the frame is that scene's. 42 of 65 such snapshots are
field-run, but 13 are `field-init` - and `teien`'s is one of them. Read the
state's own mode byte.

## 4. Display-list reader (extension, not new)

`crates/mednafen/src/prim_pool.rs` already decoded textured polys and sprites.
Extended with what a scene frame actually needs:

- **Untextured families** `POLY_F3/F4/G3/G4`. These are a large share of a real
  field frame (1440 `POLY_G3` in one `jouine` frame) and a textured-only reader
  drops them silently.
- **`find_pools`** - the old `POOL_BASE_DEFAULT` is a world-map constant; any
  other scene needs the pool found.
- **`find_ot_arrays`** - locates the ordering table by its `ClearOTagR`
  signature (an empty bucket holds its own predecessor's address). Retail's are
  2048 buckets.
- **`chain_walk`** - follows `next_addr`, cycle-guarded.
- CLI `mednafen-state display-list` (+ `scripts/mednafen/display-list.py` which
  dispatches a `.sstate` through `pcsxr-state extract`).

Three properties that make the difference between a real read and a wrong one,
all learned the hard way here:

1. **The pool is not the frame.** Stale packets from earlier frames sit in the
   pool. Only the chain reachable from an ordering table is live. The `teien`
   state shows 564 pool packets and a 44-packet live chain.
2. **Draw order is chain order.** No depth buffer; the OT *is* the depth policy.
3. **Retail double-buffers.** OT pairs hold frame N and N-1 at near-identical
   counts. Merging them makes every surface appear stacked with itself - the
   exact false positive a coincidence test must not have. Default walks one.

## 5. Thread verdicts

**Thread 2 (coplanar residual, curved-shell stacks): ANSWERED - retail does not
draw both copies.** `jouine` and `jouind` field-run frames report **zero**
screen-coincident groups above 16 px². The only coincidence found is one mesh
drawn three times in a single texture family (multi-pass alpha), in a different
OT. `chitei2` is not covered, so its slope inherits the conclusion rather than
being measured.

**Thread 1 (teien hedge-base ground fill): BLOCKED, with the blocker pinned.**
No `teien` field-run state exists. The `field-init` one is mid-fade - three
stacked full-screen untextured quads in a 44-packet chain. Needs **one mednafen
state in `teien` at mode `0x03`**; mednafen specifically, because the per-cell
join needs the object grid at `*(_DAT_1F8003EC)`, which lives in the scratchpad
that only mednafen states carry. `mednafen-state`'s `scratch_ram()` already
reads it - the brief's step-2 concern was already implemented.

## 6. Decoder defect found and fixed, with a downstream visual consequence

`prim_pool` had the sprite opcode ranges **inverted**: it decoded `0x74..0x77` as
`Sprt16` and `0x7C..0x7F` as `Sprt8`. Per the GP0 encoding (bits 4-3 select size:
`10`=8x8, `11`=16x16), it is the reverse. Grade: spec + `capture`.

Consumer impact: `crates/web-viewer/src/inspect.rs` calls
`sprite_to_quad(.., 16)` for `Sprt16` and `(.., 8)` for `Sprt8`, so **every 8x8
sprite was rendered at double size** in that viewer's prim-pool replay.

Fixed by swapping the ranges in the decoder, which corrects the consumer with no
edit to its own logic. Covered by `sprite_opcode_sizes_follow_the_gp0_encoding`.
**Someone should eyeball the web-viewer page** - this lane changed what that
page draws and verified only that it compiles, not that it looks right.

## 7. One edit outside scope, deliberately

Adding the untextured `Prim` variants is a breaking change for exhaustive
matches, and it broke `crates/web-viewer/src/inspect.rs` (`non-exhaustive
patterns`). `web-viewer` is not claimed by any sibling lane, and leaving the
workspace uncompilable is worse than a scoped edit, so the four new variants got
an explicit skip arm there.

The arm **preserves web-viewer's exact prior behaviour**: before this change the
decoder never produced those variants, so the viewer never saw them. It emits a
textured-vertex stream (every vertex carries a CLUT + texpage the shader
samples) with no untextured flag to fall back on, so rendering flat/Gouraud
polys needs an untextured path on both sides of the buffer - a feature, not a
compile fix. Left for whoever owns that viewer.

## 8. Also noticed, not actioned

`scripts/mednafen/widget-draw-sweep.py` dispatches on `.mcr` for mednafen states.
`.mcr` files are memory cards; real mednafen states are `.mc{0..9}`, which that
script's `main_ram()` will treat as a raw RAM dump and misread. Left alone
because the file is shared surface and this lane had no failing case for it, but
it is wrong.

## 9. Reproduction

```bash
export LEGAIA_SCUS=extracted/SCUS_942.54
cargo build --release -p legaia-mednafen -p legaia-pcsxr

scripts/mednafen/state-index.py --root captures --json /tmp/idx.json
scripts/mednafen/display-list.py ~/.config/pcsx-redux/SCUS94254.sstate1 --coincident  # jouind
scripts/mednafen/display-list.py ~/.config/pcsx-redux/SCUS94254.sstate2 --coincident  # jouine
scripts/mednafen/display-list.py \
  captures/state_poll/2026-07-29T22-21-04Z/snap_0125158_scene_teien_teien.sstate --coincident
```

Gates run: `cargo test --release -p legaia-mednafen --lib` (73 pass, 11 new),
`cargo fmt --all`, `cargo clippy -p legaia-mednafen -p legaia-pcsxr --all-targets`
(clean).
