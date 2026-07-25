# Lane 2 handoff - worklist classification, ignore-list merges, `--audit-ignored`

Findings that land in files this lane does not own. Nothing here is a request to
change lane 2's own files (`classify-worklist.py`, the classification CSV,
`port-catalog-ignore.toml`, `proposed-ignore-additions.toml`,
`worklist-classification.md`, `phantom-print-index.md`, `re-do-not-re-walk.md`).

## Stand-down list for the porting lanes: empty

Every one of the 25 addresses reserved for L3-L6 passes the entry-boundary test -
some extracted image begins a routine at the VA at its mapped base. None is a
phantom, a label-call slice or a data region. No lane should stand down.

The same is true of the whole worklist: of the 44 rows the wave started from,
exactly one (`801dfb10`) was not a port site, and it is now ignored. Treat the
residue as real work.

## `docs/subsystems/field-locomotion.md` carries a phantom address

Line ~1055 describes `FUN_801dfb10` as "a scripted player-turn cutscene state
machine (also keyed on `+0x54`)". The behaviour is accurately described; the
address does not exist. `0x801DFB10` is a `+0xE818` mis-based print from the
`overlay_0897_xxx_dat` batch and its bytes are field (0897) `0x801EE328` - the
world-map `ON RULA` travel-art actor, already documented in
`docs/subsystems/world-map.md` and ported in `engine-vm::travel_art_actor`. In
the images the printed VA is interior everywhere: the fall-through of
`bnez v0,0x801dfb28` in 0898, a branch label in 0897, the delay slot of
`jal 0x8003ce64` in 0899.

The same routine is also printed at `0x801E8B10` by the `overlay_0896` batch at
its `+0x5818` delta, which is the cross-check that pins it. Both phantoms are
now on `docs/tooling/phantom-print-index.md`; the field-locomotion sentence
should either re-address to `FUN_801EE328` or drop, and if it drops the
`documented` flag for `801dfb10` goes with it.

## `801d84b4` is now a worklist row again

It was ignored as `worklist_minigame_structural` with the reason "PADDING, not a
routine". That is true of the fishing / dance / debug-menu / slot-machine
extractions (17 `nop`; 32 in baka_fighter) and false of the field overlay, which
is the image the ignore was costing. Read out of `overlay_field_0897.bin` at base
`0x801CE818`: `jr ra; addiu sp,sp,0x20` closes the predecessor at
`0x801D84AC`/`0x801D84B0`, then a six-instruction leaf stores master game mode
`_DAT_8007B83C = 0x16` (22, CARD INIT), raises `_DAT_8007BB00 = 1`, and returns.
Exactly one `jal 0x801D84B4` in that image. Two base-tagged dumps carry the same
seven-word body - `overlay_cutscene_dialogue_801d84b4` and
`overlay_cutscene_mapview_801d84b4`, both field-overlay captures - so the padding
reason was not even the only dump evidence at the VA.

That is the overlay-local twin of the SCUS scripted game-over trigger
`FUN_8003C7EC`, which the engine already ports (`engine-core::world`
`op4c_n_e_sub_a_call_c7ec`). A porting lane may reasonably fold it into that port
with a second `// PORT:` tag rather than write a new function - the call is
theirs, not this lane's. It classifies `VA_ALIASED`, so the ignore list will not
take it back.

## Three unassigned rows that are cheap and real

Not on any lane's list, all confirmed `REAL` from the images:

- **`801d0290`** - battle overlay (0898) overlay-local PRNG. Twelve instructions,
  no frame: `v = s*12 + 2` built as `s<<2 + s<<3`, then `s = (v<<16) + (v>>16)`
  over the single state word `0x801F6950`. Already correctly written up in
  `docs/reference/functions/battle.md`, which warns that the `overlay_0897` dump
  holds a different body at the VA. The classifier used to call this `UNCERTAIN`
  because its only dump is the field VM's `addiu s8,s8,2` label-call idiom
  printed at the wrong base.
- **`801d56e4`** - fishing overlay (0972) 2-D segment clipper, documented in
  `docs/subsystems/minigame-fishing.md` and `functions/minigames-debug.md`. Its
  dump is **truncated**: Ghidra bounded the function at 524 bytes and the stream
  stops mid-body at `0x801D58EC` with no `jr ra`. A re-dump would help whoever
  ports it.
- **`801cf5d0`** - menu overlay (0899) per-character equipment snapshot, a
  32-instruction frameless leaf. The earlier `INTERIOR` reading came from the
  battle-overlay image, where the VA decodes data as code. The ignore list
  already records the removal; this run confirms it independently from the
  entry-boundary test.

## Eight rows arrived mid-wave from a concurrent lane's dumps

The worklist was 44 at the start of this lane and 52 by the end, with no port
landing and no ignore change accounting for the difference. The added rows are
`8002149c`, `8002174c`, `80059e10`, `80062f98`, `8006320c`, `8006352c`,
`80063aa8`, `80063cec`: each was already `documented` and became `dumped` when
new dumps appeared under `ghidra/scripts/funcs/` during the wave (3812 dumps at
the start, 3896 at the end). All eight classify `REAL` with substantial bodies.

Five of them are the SsAPI sequencer cluster `docs/subsystems/audio.md` documents
as per-slot fan-out and per-channel note/expression handlers - real logic rather
than libsnd shims, and that page explicitly corrects an older "fixed-point div"
label on `8006320C` / `8006352C`. Whether the engine's own sequencer makes them a
scope exclusion is the audio owner's call, not a classification question, so this
lane left them on the worklist.

Two operational notes for the coordinator:

- The shared `target/port-catalog/catalog.csv` in the main checkout is rewritten
  by whichever lane runs `port-catalog.py` there, so a worklist count read off it
  is not reproducible mid-wave. Symlinking `ghidra/scripts/funcs` into a worktree
  (it is in `.git/info/exclude`) lets a lane run the catalog against its own tree
  and its own `target/`, which is what this lane did.
- Any wave-level "worklist went from N to M" claim needs the dump corpus pinned,
  or the denominator moves under it.

---

# Extent attribution for `disc-coverage.py`

## Method, since the two are no longer interchangeable

Every number below is **raw capstone over `extracted/overlays/*.bin` at the bases
in `static-overlays.toml`**, compared against the dump's own printed disassembly
text. `disasm-overlay-fn.py` was not used to produce any of it, so none of it
inherits that script's truncation defect. The 18 classification rows in the first
half of this file were read the same way.

## The ambiguity is two nested spans, not many overlays

`disc-coverage.py` measures a row only when it has `base_va`, `clean_copy_bytes`
and an extracted image. **Exactly two rows qualify** - `menu` (0899) and
`battle_action` (0898) - and both are based at `0x801CE818`:

| image | span | end |
|---|---|---|
| `menu(899)` | `0x15E8C` | `0x801E46A4` |
| `battle_action(898)` | `0x28800` | `0x801F7018` |

`menu`'s span is **wholly inside** `battle_action`'s. So every extent in
`menu`'s range falls in both spans by construction, which is why `menu` reports
100% VA-ambiguous - it is not a corpus problem and no amount of dumping moves
it. The other 29 overlay rows have no `clean_copy_bytes`, so they are not
measured at all, and dumps whose bytes belong to them are currently being
counted against `menu` and `battle_action` anyway.

## The artifact

`scripts/ghidra-analysis/dump-extent-attribution.csv`, regenerated by
`scripts/ghidra-analysis/attribute-dump-extents.py`. Keyed by `(entry, bytes)` -
the extent, matching the key `read_dump_extents` builds - not by dump filename,
so it does not rot when a dump is added. Addresses, image labels and one-line
reasons only.

754 distinct ambiguous extents:

| class | n | consumer action |
|---|---:|---|
| `unique` | 448 | credit the named image only |
| `misbased` | 141 | credit nobody - the extent is fiction |
| `short` | 75 | leave ambiguous |
| `unresolved` | 33 | leave ambiguous |
| `no_disassembly` | 22 | leave ambiguous |
| `identical` | 18 | credit each named image |
| `gapped` | 15 | credit nobody |
| `data` | 2 | credit nobody |

Of the 448 unique attributions, **245 go to images `disc-coverage.py` does not
measure** - `field(897)` 98, `fishing(972)` 58, `baka_fighter(976)` 40,
`slot_machine(975)` 25, `dance(980)` 25, `debug_menu(971)` 3, `gameover(902)` 1,
`cutscene_str(970)` 1. Those extents are inflating both measured rows today.

## Projected effect, and the honest ceiling

| image | extents | keep | other | exclude | residue | amb% now | after |
|---|---:|---:|---:|---:|---:|---:|---:|
| `battle_action(898)` | 962 | 68 | 380 | 158 | 148 | 78.4% | **34.9%** |
| `menu(899)` | 754 | 129 | 319 | 158 | 148 | 100.0% | **53.4%** |

`other` and `exclude` leave the image's set entirely, so `after` divides the
residue by what remains. Percentages here are per **distinct extent**; the
report's own `71%` / `100%` are per **dump**, so the "now" column will not match
it exactly - same phenomenon, different denominator. Worth reconciling in one
place when you apply this.

**`battle_action` crosses the 50% threshold and becomes reportable.
`menu` does not, and I recommend leaving it "not meaningful".** It is the inner
of two nested spans: most of what it loses is loss to other images, so the same
148-extent residue is a much larger share of what remains. Forcing a figure
would mean asserting the residue belongs to `menu`, and nothing in the bytes
says so.

The residue does not yield to more of the same effort. It is dominated by *dump*
defects rather than corpus gaps - windows too short to sign, dumps carrying only
decompiled C, gapped streams - so it is repaired by re-dumping, not by
extracting another overlay. Lowering the signature floor is measured, not
assumed: 8 → 5 instructions moves `battle_action` 34.9% → 34.3% and `menu`
53.4% → 52.7%, while making every verdict rest on less evidence. Not worth it.

## Proposed diff to `scripts/ci/disc-coverage.py` (yours to apply)

Attribution is **optional**: absent CSV must reproduce today's output verbatim.

**1. Loader.** New helper, called once in `main()`:

```python
ATTRIBUTION = os.path.join(REPO, "scripts", "ghidra-analysis",
                           "dump-extent-attribution.csv")

# Extents whose owning image was established by BYTES rather than by address.
# `image` is `label(prot)`; join on the label half. Classes that credit nobody
# map to a sentinel so a caller can drop the extent from every image.
CREDIT_NOBODY = {"misbased", "data", "gapped"}

def read_attribution(path=ATTRIBUTION):
    """{(entry, end): set_of_labels_or_None}. None = credit nobody."""
    if not os.path.exists(path):
        return {}
    out = {}
    with open(path, newline="") as fh:
        for row in csv.DictReader(fh):
            entry, nbytes = int(row["entry"], 16), int(row["bytes"])
            cls = row["class"]
            if cls in CREDIT_NOBODY:
                out[(entry, entry + nbytes)] = None
            elif cls in ("unique", "identical"):
                out[(entry, entry + nbytes)] = {
                    n.split("(")[0] for n in row["image"].split("|") if n != "-"}
            # every other class is residue: left out, so it stays ambiguous
    return out
```

**2. `cover_image` gains an `attrib` argument** and filters `mine`:

```python
def cover_image(name, image, base_va, span, extents, attrib=None):
    lo, hi = base_va, base_va + span
    mine, dropped = [], 0
    for a, b in extents:
        if not (lo <= a < hi):
            continue
        owners = (attrib or {}).get((a, b), "residue")
        if owners == "residue":          # unattributed, or no CSV at all
            mine.append((a, min(b, hi)))
        elif owners is None or name not in owners:
            dropped += 1                 # belongs elsewhere, or nowhere
        else:
            mine.append((a, min(b, hi)))
    ...
```

and returns `"attributed_out": dropped` alongside the existing keys. Pass
`attrib=None` for the `SCUS_942.54` call - it has no aliasing and the filter must
not touch it.

**3. `overlay_reports` recomputes `ambiguous_pct` over what survives.** An extent
that the bytes assign elsewhere is no longer ambiguous *for this image*; it is
simply not this image's:

```python
    for row in out:
        lo, hi = row.pop("_image_span")
        mine = [(a, b) for a, b in extents if lo <= a < hi]
        resid = sum(1 for k in mine if (attrib or {}).get(k, "residue") == "residue")
        kept  = len(mine) - row["attributed_out"]
        row["ambiguous"] = resid
        row["ambiguous_pct"] = (100.0 * resid / kept) if kept else 0.0
```

**4. What it prints.** With the CSV present, the overlay caveat becomes a
statement about the residue rather than the whole band, and the table's
`VA-ambiguous` column means "share of this image's extents the bytes could not
place". Suggested replacement for the `### Overlay caveat` block:

> Overlay images alias in VA space - several share base `0x801CE818`, and the two
> measured spans are nested - so an extent in that band cannot be attributed by
> address. `N` of them are resolved by bytes against the extracted images
> (`dump-extent-attribution.csv`); `M` remain unattributable and keep those rows
> an upper bound. See `dump-corpus-integrity.md`.

With the CSV absent, keep today's wording exactly, and add one line so a reader
knows the better number exists:

> Byte-level attribution is not available (`dump-extent-attribution.csv` absent);
> overlay rows are attributed by address alone.

**5. Baseline.** `snapshot()` skips rows at `ambiguous_pct >= 50`, so applying
this makes `battle_action` newly ratchetable. Take a fresh baseline in the same
commit or the first CI run after it will compare against nothing.

## For Lane 1's 452 dump repairs

Per-**dump** attribution - which program to re-dump each file against - is
printed, never committed (it is filename-keyed over a gitignored corpus):

```bash
scripts/ghidra-analysis/attribute-dump-extents.py --per-dump
```

The important structural point: **attribution is not constant within a dump-stem
family**, and it is least constant for the biggest families. Constant ones:

| family | resolves to |
|---|---|
| `overlay_menu`, `overlay_shop_save`, `overlay_save_ui*` | `menu(899)` |
| `overlay_battle_action`, `overlay_muscle_dome`, `overlay_magic_level_up` | `battle_action(898)` |
| `overlay_cutscene_dialogue`, `overlay_cutscene_mapview`, `overlay_world_map*`, `overlay_dialog*` | `field(897)` |

Split ones, where a family-level rule would be wrong - a capture spans slot A
plus the resident executable, so its dumps land in two different images:

| family | split |
|---|---|
| `overlay_debug_menu` | `fishing(972)` 53, `field(897)` 39 |
| `overlay_dance` | `field(897)` 39, `dance(980)` 33 |
| `overlay_slot_machine` | `field(897)` 39, `fishing(972)` 29 |
| `overlay_fishing` | `fishing(972)` 52, `field(897)` 39 |
| `overlay_baka_fighter` | `baka_fighter(976)` 48, `field(897)` 25 |

`overlay_debug_menu` is the validation case you named, confirmed: 53 of its
dumps are **fishing** bytes, because PROT 0971's own content stops at `0x1800`
and those VAs are past it.

`overlay_0897` (226 dumps, 140 `misbased`), `overlay_0897_xxx_dat` and
`overlay_0896` are the phantom-print batches. **They are not re-dump targets at
the printed VA** - there is no routine there to re-dump. Re-key them first
(`phantom-print-index.md`), then dump the real VA.

`overlay_magic_capture`: all 58 in the band are `no_disassembly` - they carry no
instruction stream, so byte attribution has nothing to work with. Not
permanently unattributable for the reason you expected, though: a re-dump *would*
produce a disassembly. **Inference, clearly labelled as one:** its sibling
capture `overlay_magic_level_up` is the same scenario family and all 58 of its
band dumps resolve to `battle_action(898)`, so slot A in the magic captures is
almost certainly 0898 too. Verify on the first repaired dump before trusting it
for the other 57.

## Two more deleted port sites, recovered

Lane 1's corpus repairs made `--audit-ignored` re-raise two rows that had been
ignored as phantoms on the strength of dumps that were bounded short. Both are
real, both verified from `extracted/SCUS_942.54` directly, both now back on the
worklist and unassigned:

- **`80021934`** - 116 instructions, the scene-transition streaming actor SM.
  The phantom reading rested on a 3-instruction dump, and those three
  instructions are the routine's own pre-prologue setup (`lui v0,0x1f80 /
  lbu v0,0x393(v0) / lw v1,0x710(gp)`) before `addiu sp,sp,-0x120` at
  `0x80021940`. **The ignore list contradicted itself here**: the `80021940`
  row in `worklist_interior` says "Port 80021934 instead", while `80021934` sat
  in `worklist_phantom` - so the routine was ignored at both of its addresses
  and vanished. Documented in `functions/game-modes.md` and
  `formats/scene-v12-table.md`.
- **`80055b4c`** - an 8-instruction leaf arming the one-slot side-band stream
  request (`*(_DAT_8007BD24)[0x26B] = a0 + 1`, `[0x26C] = 0`). The
  0-instruction dump was a bounds failure, not a stub.
  `formats/battle-data-pack.md` and `functions/renderer.md` both already
  describe it by name.

Worklist moves 52 → 54 as a result. Both are cheap and neither is assigned.

## Two defects found in tools I do not own

**`check-dump-base-integrity.py` does not fold `break`'s operand.** Ghidra prints
the full code field (`break 0x1c00`), capstone a sub-field (`break 7`). A survey
of near-miss windows found this is **24 of the 25** systematic disagreements -
it matters out of proportion because `div; bne; break 0x1c00` is the overflow
check emitted at every integer divide. `dump-corpus-integrity.md` already
prescribes treating a lone `break`-immediate mismatch as noise; the tool does not
yet. Folding it moved 29 extents out of `unresolved` here, so your `NOT_FOUND`
count is inflated by roughly that shape. My tool folds it locally
(`norm_tokens`), symmetrically on both sides - the file is yours to change.

It cost a **worklist** verdict too, which is how visible the gap is: your
repaired `801d56e4` dump is 338 instructions and its opening window contains the
division overflow check, so the unfolded comparison stopped matching the fishing
image and the row fell from `REAL` to `UNCERTAIN` - "no extracted image holds
the dumped bytes at this VA", a statement about the corpus produced by a
comparison bug. `classify-worklist.py` folds it now (`StaticArbiter._fold`) and
the row reads `REAL … fishing(972) (captures: debug_menu/fishing)`, which
reproduces your mislabelling finding from the other direction.

**Independent cross-check of your 142.** Reading `entry=` from headers and
comparing against the filename address across the whole corpus: **198 mismatches,
and all 198 resolve BELOW the requested address.** Different tool, different
denominator (whole corpus vs cited set), same decisive signature. Printed by
`attribute-dump-extents.py` on every run.

## `disasm-overlay-fn.py` fixed

The walk now ends the body at a `jr ra` or an outbound `j` **only once nothing
already walked branches past it**, tracking the highest forward branch target
seen. That covers both blind spots at once: the first-`j` rule truncates a
forward jump to a shared epilogue, the first-`jr ra` rule truncates an early-exit
arm. Any other ending - instruction cap, end of input, `--max-size` - now prints
a loud `INCOMPLETE BODY` marker with the count flagged as a lower bound.

Validated against four independently-known bodies: `801d84c0` 85 → **259
instructions / 0x40C** (matches Lane 3 and the dump header), `801d56e4` →
**338 / 0x548** (matches your repair), `801d0290` → 12 (leaf), `801cf5d0` → 32
(leaf). A fifth turned up while validating: `801ee328` reads **171**
instructions, where `overlay_0897_xxx_dat_801dfb10` records 133 - that dump is
truncated as well as mis-based.

The same `jr ra`-walk caveat you flagged on `repair_truncated_dumps.py` applies
to it and is not fixed there: if it stops at the first return in address order,
the frontier condition above is the cheapest correction.

## One more shared-environment trap

The session scratchpad is shared across lanes, and `sys.path[0]` is the running
script's directory. A `.py` file another lane drops there can shadow a module a
dependency imports internally - it cost me a debugging cycle with `capstone`
failing as a *partially initialized module* (circular import) only when a script
ran from the scratchpad. Same class as the self-matching observers in
`shell-observer-traps.md`. Run analysis scripts from the worktree, not from the
scratchpad.
