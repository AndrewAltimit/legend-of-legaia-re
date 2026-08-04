# LANE 10 - proposed replacement text for `docs/reference/open-rev-eng-threads.md`

`open-rev-eng-threads.md` is coordinator-owned. Below is the proposed edit for
the two render threads this lane measured. **Thread 2 closes; Thread 1 does
not** - it stays open with its blocker named precisely enough that one play
session clears it.

Evidence grade for every claim below: **`capture`** (a display-list read out of
a real retail RAM image). No disassembly was re-derived and nothing is inferred
from the disc.

---

## 1. Field / locomotion table row - REPLACE

Current row:

```
| teien hedge-base ground fill (kind-2 tile-trigger cells) | open | [details ↓](#teien-hedge-base-ground-fill) |
```

Proposed row (status unchanged, "what would close it" sharpened):

```
| teien hedge-base ground fill (kind-2 tile-trigger cells) | open - blocked on one `teien` field-run mednafen state | [details ↓](#teien-hedge-base-ground-fill) |
```

## 2. `### teien hedge-base ground fill` - REPLACE the closing paragraph

Replace everything from "If retail really shows grass in those cells" to the end
of the section with:

> If retail really shows grass in those cells, the filler is an **unpinned
> kind-2-cell draw channel**.
>
> **The corpus cannot answer this yet, and the reason is now precise rather than
> assumed.** The state index (`scripts/mednafen/state-index.py`) covers 50
> scenes across both emulators, and `teien` is one of them - but neither of its
> two states carries a `teien` field frame. One is `battle-init`; the other is
> `field-init`, and reading its display list settles what that means: the live
> ordering table holds **44 packets**, three of which are stacked full-screen
> untextured `POLY_F4` quads spanning `(0,-4)..(320,228)`. That is a
> scene-transition fade, not a rendered garden. The several hundred packets also
> present in the pool are stale bytes from the previous frame that no ordering
> table links - which is exactly why the read walks the OT rather than
> scanning the pool.
>
> **What would close it:** one save state in `teien` at game mode `0x03`
> (field-run), captured in **mednafen**, not PCSX-Redux. The emulator choice is
> load-bearing: the question is per-cell, so answering it means joining the
> frame's ground primitives against the live object grid at `*(_DAT_1F8003EC)`,
> and that pointer lives in the scratchpad. Mednafen states carry
> `ScratchRAM.data8`; a PCSX-Redux `.sstate` carries main RAM only. With such a
> state the read is `mednafen-state display-list <state> --list` plus the grid
> slice; see
> [`mednafen-automation.md`](../tooling/mednafen-automation.md#read-a-frames-display-list-libgpu-ordering-table).
>
> Until then the engine must not grow a speculative fill.

## 3. `### Coplanar residual tail: same-position curved-shell stacks` - REPLACE

Replace the section's status line and its first shape (the "same-position stacks
of curved shells" half). Keep the second shape (sub-cluster slivers) as written.

New status line:

> *Status:* partial - the same-position curved-shell half is **answered** by a
> display-list read; the sliver half remains

Replace the text from "First, **same-position stacks of curved shells**" up to
"Second, **sub-cluster slivers**" with:

> First, **same-position stacks of curved shells** - two different env TMDs
> placed at one translation whose curved surfaces coincide (jouine/jouind's
> flesh-cave walls, chitei2's res41/res45 slope). A per-draw *translation*
> cannot separate two coincident curved surfaces everywhere (any direction is
> tangent to some part of the shell), so the offset API is structurally the
> wrong tool.
>
> **Retail does not draw both copies.** Reading the libgpu ordering table out of
> field-run save states inside `jouine` and `jouind` (`mednafen-state
> display-list --coincident`) finds **zero** screen-coincident groups among
> surfaces of at least 16 px² in either scene's live frame - 1218 packets walked
> in `jouind`, 971 in `jouine`, every surface submitted exactly once. The
> scripts swap these meshes as state/morph variants rather than stacking them,
> which is what the thread suspected.
>
> The one place coincidence does appear is not a mesh stack: a small
> 241/302-packet ordering table in the `jouine` image holds four groups of
> **three** copies of a single quad, all in one texture family
> (`clut=0x7F86 tpage=0x001F`), forming a 2x2 patch - the multi-pass
> semi-transparency idiom, one mesh drawn three times. Its members share a
> material; two different env TMDs would not.
>
> Two format properties decide whether such a report means anything, and both
> produce false positives when ignored. Retail **double-buffers**: ordering
> tables come in pairs holding frame N and frame N-1 with near-identical packet
> counts, so merging a pair makes every surface appear to be stacked with itself
> (`--all-ots` does this deliberately; the default walks one). And distant
> geometry projects to 1-3 pixel slivers that coincide with each other
> constantly without saying anything about meshes, hence the `--min-area` floor.
>
> `chitei2` is **not** covered by the state corpus, so its res41/res45 slope is
> asserted only by the two `jou` scenes' result, not measured directly.
>
> Second, **sub-cluster slivers** - [unchanged from here]

## 4. Suggested addition to `re-settled-threads.md`

If the coordinator wants the answered half recorded on the settled page:

> ### Does retail stack coincident curved shells?
>
> No. Grade: `capture`. Field-run display-list reads inside `jouine` and
> `jouind` report zero screen-coincident surface groups above a 16 px² floor;
> every surface in the live ordering table is submitted once. The coplanar
> kernel's same-position curved-shell residual is therefore a property of how
> the port assembles a scene's env TMDs, not something retail resolves by
> ordering - there is nothing for retail to order. The only coincidence in the
> images is one mesh drawn three times in a single texture family (multi-pass
> alpha).
>
> Two false positives this measurement has to avoid: the double-buffered
> ordering-table twin (frame N vs N-1), and sub-pixel slivers of distant
> geometry.
