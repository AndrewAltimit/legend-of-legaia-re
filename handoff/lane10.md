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

## 2b. A third corpus was missing, and my own classifier was why

The coordinator caught that the first sweep never looked at `saves/library/` -
the **curated** corpus, the states other oracles in this repo are pinned
against. The reason it was invisible is a defect in my script: `classify()`
dispatched on **extension**, and the library's mednafen states are named `.mcr`
(the backup helper keeps the source slot's suffix) - the same suffix as the
memory cards I had just correctly identified as *not* states. Two populations,
one extension, opposite contents.

Fixed by classifying on **content**: sniff the magic, and for a gzip stream
decompress the first block to tell an `MDFNSVST` container from a PCSX-Redux
protobuf. That needed one more guard - a naive content sniff indexed several
hundred `.cpp` and `.md` files as save states, because a default root is
`~/Tools/pcsx-redux`, which is the *emulator's own source tree*, and its files
carry "PCSX" in their license headers. A 512 KiB size floor fixes it: every
state embeds 2 MiB of main RAM.

**Corrected totals: 186 mednafen + 292 pcsx-redux = 478 states, 13 memory cards
skipped, 0 unreadable, 55 scenes.** (The intermediate "941 states" figure was
the un-floored sniff counting source files; it is not a real number.)

## 3. Scene coverage index (new, reusable)

`scripts/mednafen/state-index.py` + `mednafen-state identify` +
`pcsxr-state identify`. Both CLIs read the same anchors via the new shared
`legaia_mednafen::game_anchors` (which `legaia_pcsxr` now delegates to instead of
duplicating), so the two corpora index into one table.

55 scenes covered across the complete corpus. Headline rows for this task:

| scene | states | what they are |
|---|---|---|
| `teien` | 2 | `field-init` + `battle-init` - **still no field-run** |
| `chitei2` | 0 | **still not covered at all** |
| `jouine` | 4 | 2 field-run, but byte-identical (see below) |
| `jouind` | 14 | 2 field-run, byte-identical; 12 battle |
| `edteien` | 3 | field-run **mednafen** - new, see section 5b |

The library's `jouine`/`jouind` field-run states are **not** new cameras: a
library filename *is* the sha256 of its contents, and
`sha256(~/.config/pcsx-redux/SCUS94254.sstate2)` equals the library entry's
name exactly. They are backups of the states already swept, so they cannot
retire the one-frame-one-camera caveat. All three `edteien` states report the
identical player position too.

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
OT.

Limits, stated so nobody relays this as broader than it is: each read is **one
frame, one camera**, and a surface outside that view contributes no packet. The
negative carries weight because a stacked shell would double many adjacent
surfaces at once and none of 1218 + 972 walked packets is doubled anywhere - but
a second field-run state per scene at a different camera would retire the
caveat. **The full corpus does not contain one** - the library's `jouine` /
`jouind` field-run states are byte-identical backups of the ones already read,
so the caveat stands. `chitei2` is not covered at all, so its slope inherits the
conclusion rather than being measured.

**Thread 1 (teien hedge-base ground fill): BLOCKED against the complete corpus.**
No `teien` field-run state exists in any of the three populations. The
`field-init` one is mid-fade - three stacked full-screen untextured quads in a
44-packet chain. Needs **one mednafen state in `teien` at mode `0x03`**;
mednafen specifically, because the per-cell join needs the object grid at
`*(_DAT_1F8003EC)`, which lives in the scratchpad that only mednafen states
carry. `mednafen-state`'s `scratch_ram()` already reads it - the brief's step-2
concern was already implemented.

### 5b. `edteien` is a lead, not an answer

The library turned up three field-run **mednafen** states in `edteien` - the
epilogue variant of the teien garden, and the only garden field-run states in
the corpus. Two things make it interesting and one stops it short of answering
thread 1.

Interesting: its frame draws the *same texture families* as the teien capture
(`0x7C40/0x001A`, `0x7EC0/0x000C`, `0x7F00/0x000B`, `0x7F41/0x001B`), so it is
the same garden art. And reading its live object grid through the scratchpad
(`0x1F8003EC` -> `0x80139530`, grid at `+0x8000`, 0x80x0x80 u16) shows
**exactly the thread's cell pattern**: of 479 nonzero cells, 400 carry `0x1000`,
52 carry `0x0800`, and **45 carry `0x0800` without `0x1000`**. So the
configuration the thread is about is not unique to `teien`, and a mednafen state
that exhibits it does exist.

Stops short: `edteien` is a different CDNAME scene with its own `.MAP`, so a
result there is not a result about teien's cells. And my OT detector only
reaches 254 of that frame's 1624 pool packets - see the limitation below - so I
could not even measure it cleanly. I did **not** answer thread 1 from this, and
nothing in the engine changed.

### 5c. The OT under-read had a cause, and it is fixed

The `edteien` under-read (254 of 1624 packets) was not "a table whose signature
is erased" - it was a one-word over-extension in `find_ot_arrays`.
`is_occupied_bucket` accepted **any** word with a zero length byte, so a word
`0x00002020` sitting directly above a 2048-bucket table joined the run. The head
is taken as the top of the run, so a single junk word put the head on a
non-bucket and the entire walk terminated immediately.

Real links point into a libgpu work buffer, which never lives in the first
64 KiB (that is BIOS and kernel space), so requiring `next >= 0x10000` fixes it.
`edteien` now walks **2368** packets instead of 254. Regression-checked: `jouine`
972 and `jouind` 1218, both still zero coincident groups, so thread 2's result is
unchanged.

The general tell still stands and is documented: a walked count far below the
pool count means under-read, not a small frame.

### 5d. Thread 1 from `edteien`: still cannot tell, and edteien is disqualified

The coordinator's framing is right - `FUN_801f6d48` is scene-independent shared
code, so a ground primitive over an `0x0800`-only cell in *any* scene would be a
finding about the emitter. I could not establish either branch, for two
independent reasons, and the second one matters more than the first.

**The join cannot be made by count.** Retail's ground pass is not 1:1 with grid
cells. `edteien` has 400 cells with `0x1000` and 45 `0x0800`-only, so the two
hypotheses predict 400 and 445 packets - and no texture family in the live chain
is near either. The largest ground-plausible family (`clut=0x7C40 tpage=0x001A`)
carries 651 `POLY_FT4`; summing its whole atlas gives 917. Attributing a packet
to a cell needs the camera transform to forward-project cell centres, which is
not pinned, so no geometric join was possible either.

**`edteien`'s `0x0800`-only cells are not hedge bases.** Their layout is not
row-shaped at all: 36 of the 45 form a solid **6x6 block** (rows 46-51 x cols
40-45) sitting inside the walkable region, 10 more run along row 28, and 3 sit at
row 6 cols 10-12, far outside the `0x1000` area entirely (which spans rows 28-51,
cols 31-54). A solid 6x6 block is a raised platform - which is what the bit means:
the port's own name for `0x0800` is `CELL_ELEVATION_OVERRIDE`. Teien's case is
hedge *rows* whose cutout texels are what make a missing ground quad visible.

So the bit pattern matches while the **feature** does not, and a raised platform
would legitimately have its own mesh drawn over it. Even a clean result here
would not transfer to teien. Recording this so the next wave does not spend a
session on `edteien` as a stand-in: it is not one. Thread 1's row keeps its
capture-blocked status with no measured prior added.

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
