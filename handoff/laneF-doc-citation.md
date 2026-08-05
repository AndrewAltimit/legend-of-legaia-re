# Lane F - `dual-label` refinement + all 65 `doc-citation` findings

Two commits on `worktree-agent-a03efc29128b1c513`:

- `1e0430b3` - `dual-label` stops filing the index entry as a rival of the page
  it indexes (`check-port-provenance.py`, waivers, `port-provenance.md`).
- `e2e245d2` - the `doc-citation` review: the function-entry exemption fixed in
  the checker, nine directory rows corrected, twenty-three waived.

Every verdict below that says `disassembly` rests on a fresh read of
`/home/mikunpc/Documents/repos/legend-of-legaia-re/ghidra/scripts/funcs/`,
reached read-only through a gitignored symlink and never copied into the
worktree. `doc` means it rests on two committed pages already agreeing.

---

## Part 1 - the `dual-label` refinement

### What Lane C proposed, and why it does not do what it says

Lane C's rule: *treat a directory row that links to its counterpart page as a
pointer, not an independent definition*, and it predicted this "would clear the
great majority of the 39 while leaving `FUN_801D5DE0` firing".

Measured on the current tree, it clears **14** of the 39 and silences **four of
the six real mislabels**. The reason is structural and worth writing down,
because it is the trap in the brief's own validation clause:

> Lane C's corrections *added* the cross-links the rule keys on.

`git show e0ca3812` is the evidence. Before that commit, `script-vms.md`'s
`801EAD98` row read "Field subsystem hub (town overlay). 5.9 KB / 35 calls." -
no link. After, it reads "**World-map debug-menu list renderer** … Full label
table in [`world-map.md`](…)". Same for `game-modes.md`'s `80034A6C` row and
`minigames-debug.md`'s `801D0748`. Fixing a mislabel *is* adding the pointer, so
after any correction the row becomes indistinguishable from a row that was never
wrong.

That is not a defect in the rule. It is the ceiling on what any structural rule
can do: **on the corrected tree the six are byte-shape-identical to the
thirty-nine.** The cleanest proof pair -

| Address | Carriers | In the brief's set |
|---|---|---|
| `801EAD98` | `functions/script-vms.md` row → links to `subsystems/world-map.md`; one `###` there | must fire |
| `801CFE4C` | `functions/script-vms.md` row → links to `subsystems/field-locomotion.md`; one `###` there | must stop |

No predicate over page names, link graph, site kind or cited addresses
distinguishes them, because after Lane C's work nothing distinguishes them.

### What was implemented

Two exclusions in `find_dual_labels`, both structural:

1. **Pointer rule** (Lane C's, correctly scoped). A page is a pointer for an
   address when **every** site it has for that address links to the counterpart
   page. Scoped per pair, so a row pointing at `save-screen.md` is still an
   independent claim against `world-map.md`; scoped over all sites, so a page
   that defines a routine twice and links once still counts as a rival. That
   "all sites" clause is what keeps `801D8DE8` firing.
2. **Defining pages only.** `DEFINING_DOC_ROOTS = docs/subsystems/`,
   `docs/formats/`, `docs/reference/functions/`. A thread ledger
   (`re-settled-threads.md`, `re-do-not-re-walk.md`) records which readings are
   falsified and a `docs/tooling/` page describes an instrument; naming an
   address there is not a second label. Two of the rows this drops are titled
   "X is **not** Y", which Lane C had already argued in prose.

Link scope is the **site's own line**, not the whole `###` section. The section
variant was measured (it clears two more duplicates) and rejected: it also
silences two of the four per-image aliases, which are the rows Lane C flagged as
still needing a human.

### Validation, run both ways

`dual-label` is validated against the doc state the conflicts were found in -
`2f5b1e67`, the commit before `e0ca3812` landed the corrections - because that is
the only tree on which the six are still mislabels.

| Tree | Before | After | The 6 | `FUN_801D5DE0` | aliases |
|---|---|---|---|---|---|
| `2f5b1e67` (pre-correction) | 50 | 31 | **6/6 fire** | fires | 3/4 |
| current | 47 | 26 | 4/6 fire | fires | 3/4 |

The 6 are `80034A6C`, `801CFC40`, `801EAD98`, `801D0748`, `801D8DE8`,
`801DA51C`. On the pre-correction tree every one still fires, verified by
running the refined checker against `git checkout 2f5b1e67 -- docs/`. On the
current tree `80034A6C` and `801EAD98` go quiet, and each does so through the
pointer rule reaching a link that the correction itself introduced - not through
the signal losing the shape.

**Precision.** On the corpus Lane C actually triaged (the pre-correction tree,
50 findings, 6 real): **12% → 19%**. On the current tree every finding has been
reviewed, so precision there is not a meaningful number - what matters is that
21 rows stopped being printed and 21 waivers stopped being needed.

### A variant measured and rejected

Replacing filename-token relatedness with **page-graph relatedness** (two pages
are related when either links to the other anywhere) takes the current tree to
11 findings - the most attractive number in the sweep. It is wrong: on the
pre-correction tree it silences `801CFC40`, `801EAD98` and `801D8DE8`, three of
the six real defects, because a directory page links to the subsystem page it
indexes for unrelated reasons. That is lowering a number by loosening a signal,
and it is recorded here so it is not re-proposed.

### Waivers deleted as redundant

21, all of them `dual-label`, all confirmed to name no firing finding
(`--emit-waivers` diffed against the waiver keys; no other orphans exist):

`80034a6c` `8003ce9c` `8005126c` `8005567c` `801cf388` `801cfe4c` `801d0750`
`801d1288` `801d1ec4` `801d3380` `801d4a60` `801d5ae8` `801d7bb8` `801da2a0`
`801dbc30` `801de840` `801e1c1c` `801ead98` `801f12d0` `801f3990` `801f69d8`

Two carried real disassembly evidence (`80034a6c`'s flag-bank read,
`801ead98`'s jump table + label strings). Both are already in the committed page
the correction landed on - `game-modes.md`'s row states the `SC+0x45C` store and
the `0x80085758..0x80085957` clear; `script-vms.md`'s row states the 24-entry
table at `0x801CF46C` over the labels at `0x801CF344`. Nothing was lost.

---

## Part 2 - the 65 `doc-citation` findings

### The class defect: 33 of 65

`_unsupported()` exempts a cited **function entry**, and the docstring gives the
right reason - prose about a routine names its callers, its siblings and the
dispatcher that reaches it, and none of those is in its own bytes. The
implementation keyed that exemption on `c in entries`, the set of addresses that
**have a dump**. That inverts the policy: a row naming a caller we had read
passed, and the same row naming a caller we had not read as unsupported
evidence. The test is now the citation's **written form** (`FUN_`, `func_0x`,
`LAB_`), which is what the prose is asserting either way.

This is *not* Lane E's synthesised-address bug. `doc-citation` decides absence
partly by a literal string search over the whole dump file, so a register-tracker
defect cannot produce a false absence here; every one of the 65 is a case where
the eight hex digits appear nowhere in any dump of the routine. The two signals
do share `_unsupported`, but Lane E's fix is in `parse_dump`. **The only shared
function I touched is `_unsupported`** (one added `if c in func_refs: continue`
plus the `FUNC_REF_RE` constant next to `CITED_ADDR_RE`), and my `dual-label`
edits are confined to `find_dual_labels` plus two module-level constants. If
Lane E's diff collides, those are the three hunks.

### Verdict counts

| Verdict | N | Disposition |
|---|---|---|
| `GATE-ARTIFACT` - function entry cited, exemption keyed on the corpus | 33 | checker fixed; rows unchanged |
| `GATE-ARTIFACT` - function entry cited in bare `0x` form | 1 | row rewritten to `FUN_` form |
| `GATE-ARTIFACT` - citation belongs to another routine, no written form separates the cases | 19 | waived, each with what was read |
| `CITATION-WRONG` | 6 | row corrected to the address the body forms |
| `CLAIM-WRONG` | 3 | row rewritten from the disassembly |
| `CORPUS-GAP` | 3 | waived, each naming what would settle it |

65 unwaived → **0 unwaived**. Whole report: 65 → 0 unwaived, 113 waived.
`--strict` passes.

### `CITATION-WRONG` - six rows, each a hex digit out

Every one is contradicted by a sibling page that had it right, and every one was
re-derived from the `lui`/displacement pair. Evidence grade `disassembly`.

| Row | Cited | Body forms | How |
|---|---|---|---|
| `audio.md:62` `80026234` | `0x801D2220` | `0x801CD220` | `lui v1,0x801d` / `sw v0,-0x2de0(v1)`; `subsystems/audio.md` and this page's own event table already said `0x801CD220` |
| `audio.md:73` `8006688C` | `0x801D1CBC` | `0x801CE344` | `lui v0,0x801d` / `lbu v0,-0x1cbc(v0)` - the displacement is negative and the row added it |
| `audio.md:87` `8006E8D4` | `0x801D1A5C` `0x801D1A74` | `0x801CE55C` `0x801CE574` | `sw v0,-0x1aa4(at)` / `sw v0,-0x1a8c(at)`; `DAT_801D1A5C` is the Muscle Dome outcome table |
| `runtime-libs.md:339` `8005ED64` | `0x801DD210` `0x801DD208` | `0x801CD210` `0x801CD208` | `lw v0,-0x2df8` (busy) and `addiu a0,a0,-0x2df0` (MSF) off `lui 0x801d` |
| `runtime-libs.md:343` `8005EF04` | `0x801DADD8` | `0x801CADD8` | `lw v1,-0x5228(v1)` off `lui 0x801d`; the `0x801CAxxx` libcd cluster `_DAT_801CADB0` sits in |
| `menus.md:147` `801E37CC` | `&DAT_801D04B8` | `&DAT_801CF4B8` | `lui a1,0x801d` / `addiu a1,a1,-0xb48` |

### `CLAIM-WRONG` - three rows describing a routine no dump carries

| Row | What the row said | What every dump shows | Evidence |
|---|---|---|---|
| `battle.md:184` `801D829C` | "composes per-actor transforms over the actor table (`DAT_801C9370`)" | 137 instructions forming exactly three bases - `0x8007B790`, `0x800840B8`, `0x80089118` - and nothing in `0x801C____`. `subsystems/battle-action.md` already described it correctly as the angle-tween over the rotation / translation / focus trios | disassembly |
| `script-vms.md:76` `801DDFE4` | "3-instruction tail-call wrapper: writes `local_stack[+0x10] = 0x100` then jumps to `0x801EC96C` → `FUN_801D6274`" | eight instructions - frame, `jal 0x801de004`, return - identical in all eight images that carry the VA. `sw ra,0x10(sp)` is the frame's own return-address save | disassembly |
| `script-vms.md:79` `801DE3E0` | "6-instruction wrapper. Calls `func_0x80035A4C(0x37)` … tail-calls `FUN_801ECCAC`" | 38 instructions in all eight images: `jal FUN_801DBA20`, and on zero it stamps the camera-config block `0x8007B607..0x8007B618` with `0x10,0x10,0x30,0x51,0x20` / `0x1B8` / `0` / `0x4000` / `0x300` | disassembly |

`801DE3E0` is the one to notice: its citation was written `FUN_801ECCAC`, so the
class fix stops the gate ever reporting it. **It was found by hand while
sampling the class, not by the gate.** Anyone re-running the numbers will see 33
rows dropped by the checker fix and should know one of them was a real defect -
which is the honest cost of an exemption that cannot read English.

Each corrected row keeps the falsified address in the text so the wrong reading
stays searchable, which is why two of the three still fire and are waived.

### `CORPUS-GAP` - three

| Row | Gap | What would settle it |
|---|---|---|
| `minigames-debug.md:75` `801CF870` | its one dump reports 437 instructions / 1748 bytes, prints across 2088 and stops at `0x801D0094`, below the cited `0x801D00B8` | a re-dump from `0977_other_game.BIN` |
| `renderer.md:29` `801F69D8` | row is image-qualified to `overlay_world_map_top_ext`; the only dump at the VA is `overlay_muscle_dome`, PROT 0900's slot-B link base | a dump of the VA from `world_map_top_ext` |
| `script-vms.md:11` `801CF650` | row claims an emitter ramp-actor allocator calling `FUN_80020DE0`; both dumps at the VA (`overlay_menu`, `overlay_shop_save`) are the 272-byte equipment stat aggregator this same page files further down | a dump from the field/town overlay image |

`801CF650` is a same-page dual label - two rows, one address, contradicting
labels - which `dual-label` cannot see because it only compares across pages.
The row is now marked image-unverified in place.

### The 19 waived `GATE-ARTIFACT` rows

All the same shape: the row cites an address that belongs to a **different**
routine, in a form (`0x…`, `DAT_…`) that carries no marker for it. Five caller
call sites were verified against the caller's own dump, which is the strongest of
these and worth listing:

| Row | Citation | Verified at | Grade |
|---|---|---|---|
| `audio.md:26` `800267A8` | `0x801E01B4` | `overlay_world_map_801de840.txt:1634` = `801e01b4  jal 0x800267a8` | disassembly |
| `audio.md:33` `80035BAC` | `0x801E03D8` | same dump, line 1771, `jal 0x80035bac` | disassembly |
| `battle.md:93` `801F8004` | `0x801DF918` | same dump, line 1083, `jal 0x801f8004` | disassembly |
| `battle.md:92` `801F8D4C` | `0x801DF974` | same dump, line 1106, `jal 0x801f8d4c` | disassembly |
| `battle.md:110` `801F3990` | `0x801E3E04` | `overlay_muscle_dome_801e295c.txt:1327`, `jal 0x801f3990` | disassembly |
| `runtime-libs.md:398` `801D9C3C` | `0x8003B444` | `8003aeb0.txt:362`, `jal 0x801d9c3c` | disassembly |

The rest, by sub-shape:

- **A table its dispatcher reads, not it.** `script-vms.md:38` `801D33D8` /
  `0x801E4738` (`field-menu.md`: `FUN_801D6628` passes `0x801E4738 + id*0x10`);
  `script-vms.md:26` `801E30E4` / `0x801D0A6C` (the FMV table the *player* forms,
  per `str-fmv-table.md` and this page's own `801CF098` row); `battle.md:76`
  `801D8DE8` / `PTR_DAT_801F4D34` (walked at `FUN_801D388C`'s tail);
  `script-vms.md:18` `801CF9F4` and `:19` `801D5B5C` / `DAT_801F2254` (neither
  forms anything in `0x801F____`; the interact dispatch owns it);
  `minigames-debug.md:73` `801D1184` / `DAT_801D1AC8` (this routine stores
  `801D1ACC` / `801D1AD0` / `801D1AD4` / `801D1AAC`; the hub state collects them).
  Grade `disassembly`.
- **A load base named as provenance.** `battle.md:53` `801D0F60` /
  `0x801CE818` + `0x801C0000`; `battle.md:113` `801F452C` / `0x801C0000`;
  `script-vms.md:71` `801D5A24` / `0x801CE818`. A base cannot occur in the bytes
  it bases. Grade `doc`.
- **An interior of the neighbour in the same tail.** `battle.md:188` `801F0450`
  / `0x801F02D0` - `overlay_muscle_dome_801efe44.txt` is 1284 bytes from
  `0x801EFE44`, so `+0x48C` lands inside it. Grade `disassembly`.
- **A jump-table target, table external.** `game-modes.md:34` `801D362C` /
  `0x801D52D0` - the JT is at `0x801CE868` (PROT 0897 file `+0x50`). Two of the
  sub-handlers the same sentence lists do appear in the print as branch targets,
  which is why only one is flagged. Grade `disassembly`.
- **A call-site pair one level up.** `battle.md:182` `801E1D98` / `0x801E0CA0` +
  `0x801E0CD0` - the dispatcher's two arms, recorded in `subsystems/battle.md`
  and `formats/move-power.md`. Grade `doc`.
- **A section link.** `audio.md:64` `8001D230` / `0x801CE628` - libpad's
  two-port context block, cleared by the `PadInitDirect` this routine calls.
  Grade `doc`.

### Also fixed: the row Lane B handed over

`battle.md`'s `801DA34C` said the block is restored "from `record+0x1B7`".
`overlay_battle_action_801da34c.txt` reads **both** slots - `lbu v0,0x76f(v1)`
at `0x801DA3CC` / `0x801DA3F8` and `lbu v0,0x77f(…)` at `0x801DA41C` /
`0x801DA448` / `0x801DA4C4` / `0x801DA4F8` - i.e. record-relative `+0x1A7` and
`+0x1B7`, picked on `u16[+0x156] < u16[+0x154]`. Its save-side inverse
`801DA59C` has the matching `sb v1,0x76f` / `sb v1,0x77f` pair and its row named
one slot too. Both rows corrected. Grade `disassembly`.

### Spillover, not fixed here

`docs/subsystems/audio.md` lists the `FUN_8006E8D4` pair as
`_DAT_801CE564 / _DAT_801CE574`. The disassembly forms `0x801CE55C` and
`0x801CE574`, so the first of the two is eight bytes out on that page. It is
outside `docs/reference/functions/`, so `doc-citation` cannot see it; the
corrected directory row now cites the dump, which is enough for a reader to tell
which is right. Worth a pass by whoever next owns `audio.md`.

---

## Gate state after this lane

    check-port-provenance.py            0 unwaived, 113 waived
    check-port-provenance.py --strict   exit 0

`module-orphan` = 63 waived (Lanes A/B), `dual-label` = 26 waived,
`absent-citation` = 1 waived, `doc-citation` = 23 waived. No waiver in the file
fails to name a firing finding.

## Reproducing

    ln -s <main-checkout>/ghidra/scripts/funcs ghidra/scripts/funcs   # gitignored
    python3 scripts/ci/check-port-provenance.py --signal doc-citation
    git checkout 2f5b1e67 -- docs/ && \
      python3 scripts/ci/check-port-provenance.py --signal dual-label --show-waived
    git checkout HEAD -- docs/
