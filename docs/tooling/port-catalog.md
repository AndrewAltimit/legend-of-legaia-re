# Port catalog

Answers "what is left to do?" for one function, or for the whole project, from
evidence rather than from a hand-maintained checklist.

**Reach for it when** you are picking up work and want a real worklist: which
functions are understood but not ported, which are ported but undocumented,
which nobody has looked at. Run `--dashboard` and it prints the open work as a
single page.

The trick is that it **derives** status instead of tracking it. Each column below
is measured live from the tree, so the catalog cannot rot the way a status table
in a doc does - if you port a function and tag it, the catalog knows on the next
run.

```bash
python3 scripts/ci/port-catalog.py --dashboard
```

## The columns

Signals across the decompilation and engine-port tracks, plus an axis for
scope-excluded addresses (statically-linked PsyQ library code the engine maps to
native equivalents rather than porting line-by-line):

| Column | Source of truth |
|---|---|
| **dumped** | A Ghidra decompiler dump exists under `ghidra/scripts/funcs/` (gitignored - regenerable from the Ghidra project). |
| **documented** | The address is cited from at least one file under `docs/` (`FUN_<addr>` or `0x<addr>`, case-insensitive). |
| **ported** | A Rust source under `crates/` carries a `// PORT: FUN_<addr>` tag for that address. |
| **live** | The Rust symbol carrying that tag is reachable, through non-test code, from a host entry point. Opt-in (`--live`); see [Reachability](#reachability-the-live-axis). |
| **ignored** | The address is listed in `scripts/ci/port-catalog-ignore.toml` as a non-port-site (BIOS thunk / libc shim / libgte / libgs / libgpu / libcd / libsnd / libspu / libapi / libetc). Excluded from `--missing-ports` by default. |

**`ported` and `live` are different axes.** A `// PORT:` tag is a provenance
marker: it records that a Rust function implements a Ghidra function. It says
nothing about whether anything ever calls that Rust function. A port can be
faithful, tested, documented - and never execute. Without the second axis,
"how much of the game is covered" cannot be answered from the tree at all,
only estimated.

Tool: [`scripts/ci/port-catalog.py`](../../scripts/ci/port-catalog.py). Reuses helpers
from [`scripts/ci/function-coverage.py`](../../scripts/ci/function-coverage.py) and
shares the same code-range filter (SCUS `0x80010000-0x8006FFFF`, overlays
`0x801C0000-0x8020FFFF`).

## The `// PORT:` tag

The catalog's "ported" column keys off a structured comment in Rust source:

```rust
// PORT: FUN_801dd35c                       // single address
// PORT: FUN_801dd35c, FUN_801cf244         // multiple on one line
// PORT: FUN_801dd35c (sub-mode jump table) // trailing context allowed
//! PORT: FUN_801dd35c                      // inside `//!` module doc
/// PORT: FUN_801dd35c                      // inside `///` outer doc
```

The tag may appear as plain `//`, doc `//!`, or outer-doc `///` - putting it in
the doc block keeps the provenance co-located with the rustdoc description and
makes it visible in generated docs.

Rules:

- The tag is the only signal trusted for "ported". Plain mentions of
  `FUN_<addr>` in module docs or comments are ignored - they show up in many
  contexts that don't imply a port (cross-refs, "inspired by", "not yet
  ported", etc.) and noisily inflate the column.
- Address must be lowercase hex in the SCUS / overlay code range.
- Match is line-local - put the tag on its own line or as a trailing comment.
- A single Rust file can carry many tags. The catalog records the crate name
  each tag appears in.
- One Ghidra function can be ported into more than one crate (e.g. a
  formula shared between `engine-vm::battle_formulas` and a helper in
  `engine-core`). The catalog lists every crate that tags the address.

When porting a Ghidra function, add the tag once in the Rust function that
*implements* its behaviour. Don't tag every caller of the ported function.

Prefer a `///` tag on that function to a `//!` tag on the module. A module tag
anchors the address to the whole *file*, which is coarse enough to report the
address wired on the strength of any other function in the same file - so it is
only safe while the whole file shares one wiring status. Same for a `PORT:` tag
on a plain data struct: tag the function that computes the value, and let the
struct carry a `REF:` instead. See
[Anchors](#anchors) for how each form resolves and
[`stale-not-wired-triage.md`](stale-not-wired-triage.md#anchor-granularity) for
the edit that splits a module tag safely.

## The `// REF:` tag

Sibling of `// PORT:`. Marks an address as a **cross-reference citation** -
the file mentions `FUN_<addr>` in a docstring or comment but isn't claiming
to port it. Same comment shapes as `// PORT:` (plain `//`, doc `//!`,
outer-doc `///`) and same multi-address syntax.

```rust
//! PORT: FUN_801E30E4
//! REF: FUN_801E7320, FUN_801CF098  -- callees, not yet ported
```

`port-catalog.py` ignores REF tags - they don't set the "ported" column -
but the drift checker (`scripts/ci/check-port-tags.py`, see below) treats them
as equivalent to PORT for warning suppression.

## Reachability: the `live` axis

`--live` adds a reachability column by building a call graph over
`crates/**/src/**.rs` and asking, for each `// PORT:` tag, whether the symbol
it attaches to can be reached from a declared host entry point.

```bash
python3 scripts/ci/port-catalog.py --live            # add the `live` column
python3 scripts/ci/port-catalog.py --not-live        # ported but unreachable
python3 scripts/ci/port-catalog.py --live-only       # ported and reachable
python3 scripts/ci/port-catalog.py --live-audit      # the audit page (below)
```

The pass parses every Rust file in the workspace, so it is markedly slower than
the other modes and stays opt-in.

### Roots

The BFS starts from these, and nothing else. A `pub fn` that no host reaches is
exactly the inert-port case the axis exists to find, so being public is not a
root:

| Root family | What it covers |
|---|---|
| `fn main` in a `[[bin]]` target (`src/bin/**`, `src/main.rs`) | Every CLI subcommand across the 20-odd tool binaries, plus each GUI binary's command dispatch and window-loop *setup*. |
| `#[wasm_bindgen]` exports in the WASM crates | The browser's entry points into the static site's viewer, play and patcher pages. |
| Methods of an `impl ApplicationHandler for T` block | The whole per-frame native GUI surface - redraw, input, HUD build - and everything in `engine-core` / `engine-vm` those reach. |

The third family exists because the second call in the chain leaves the tree.
`fn main` reaches `cmd_play_window`, which builds the app and hands it to
`event_loop.run_app(&mut app)`; winit then calls `window_event` /
`about_to_wait` / `resumed` back into the tree from outside it. Without those
methods in the root set the BFS stops at `run_app`, and every per-frame,
redraw and input path below it reads as inert. The trait set is a literal in
`EXTERNAL_DISPATCH_TRAITS`; add to it when another externally-dispatched
callback trait appears.

Treating these as roots is deliberately over-permissive: an
`impl ApplicationHandler` block counts even if nothing constructs the app. That
is the same direction every other ambiguity resolves in, and it is what keeps
`--not-live` a floor.

Test code is excluded on purpose: `crates/*/tests/`, `benches/`, `examples/`,
`#[cfg(test)]` modules, `#[test]` functions, and files named `tests.rs`. "Called
only by a unit test" is precisely the condition a `NOT WIRED:` tag reports.

### Anchors

A tag is resolved to the symbol it sits on, most precise form first:

| Tag form | Anchor | Live when |
|---|---|---|
| `///` / `//` above a `fn` | that function | the function is reachable |
| `///` / `//` above a `struct` / `enum` / `impl` | that type | any method in the type's `impl` blocks is reachable, or - when the file gives that type no `impl` block at all - any non-test `fn` in the file is |
| `//` inside a function body | the enclosing function | that function is reachable |
| `//! PORT:` (module doc) | the file, widened to its submodule subtree when the file declares no functions of its own | any non-test function in scope is reachable |

Module-level tags are the coarse case and the main source of over-reporting: a
`//!` block on a crate root claims the whole crate, so one wired function in it
reports every address on that block as live.

The type anchor's fallback covers the tag that sits on a plain data struct
whose behaviour lives in free functions, or in an `impl` of a *different* type
in the same file. Without it such a tag could never be live however wired the
port is, because the rule has no method to look at.

### Precision

The graph resolves calls by **name**, not by type. Qualified calls (`Type::f`,
`module::f`) resolve against in-tree types and module stems; method calls
(`.f(...)`) resolve against every in-tree method of that name; bare `f(...)`
resolves against every function of that name. There is no type inference, no
trait-impl selection and no monomorphisation.

A **trait default method** counts as a method of its trait, so `.f(...)` finds
it. It stays listed among the free functions too, which is purely additive: a
host that does not override the default runs exactly that body, and the port
tag on it is a claim about code that really executes.

The consequences are asymmetric, and the asymmetry is what makes the axis
usable:

- **False positives on `live` are expected.** Method-name collisions (`.tick()`,
  `.step()`, `.push()`) link callers to every same-named method in the
  workspace. Trait-object and closure dispatch resolve the same loose way.
- **False negatives on `live` are rare**, because every ambiguity resolves
  *toward* reachability. An unresolved qualifier is the one deliberate
  exception: `Vec::new` falls back to free functions only, never to methods,
  because letting external types reach every in-tree `Type::new` wired up whole
  modules that nothing constructs.

Dispatch shapes the graph cannot model used to be a third bullet, and a
systematic one: trait default methods and winit `ApplicationHandler` callbacks
each hid a whole tree of live code. Both are modelled now - the first as a
method of its trait, the second as a root family - and
[`live-audit-triage.md`](live-audit-triage.md#analysis-defects-this-triage-found)
keeps the positive controls that pinned them. The lesson generalises: a
dispatch edge that leaves the tree and comes back is invisible until something
names it, so a new external-callback trait needs adding to
`EXTERNAL_DISPATCH_TRAITS` before the audit can be believed about the code
under it.

So `--not-live` is the trustworthy direction. An address it reports is one that
no plausible in-graph edge - not even a wrong one - could reach. Read `live` as
an upper bound on what runs, and `not-live` as a hard floor on what does not.

Two things the axis structurally cannot see, both of which make a *live* verdict
weaker than it looks:

- **Runtime gates.** A function called every frame behind a flag that is never
  set is statically reachable and behaviourally dead. Static reachability cannot
  distinguish it from a live one.
- **Partial ports.** Reachability is a property of the entry symbol, not of its
  body. A reachable function that implements two of its source's five branches
  still reports live.

### The audit

`--live-audit` writes `target/port-catalog/live-audit.md`, comparing the
reachability verdict against the `NOT WIRED:` disclosures written in the source.
Three sections, in the order they want acting on:

| Section | Meaning |
|---|---|
| Tagged `NOT WIRED` but analysed live | The tag and the analysis disagree. Needs a human - see the four causes below. |
| Undisclosed inert ports | Unreachable, no tag. Either a wiring gap or a missing disclosure - the disclosure gap, and the reason this mode exists. |
| Disclosed inert ports | Unreachable, and the source says so. The declared wiring worklist, working as intended. |

A row in the first section has one of four causes, and only the first is a
stale tag:

1. **The port got wired** and nobody removed the tag.
2. **A method-name collision**, usually intra-crate: a `.tick()` call somewhere
   in the crate resolves to the inert type's `tick` because receiver types are
   not inferred.
3. **A runtime gate.** The tag claims more than the axis measures - the
   function *is* called every frame, behind a flag production never sets.
4. **Anchor granularity.** A `//! PORT:` block claims the file while the
   `NOT WIRED:` note next to it disclaims one specific function; one wired
   function elsewhere in the file reports the whole block live.

Causes 2-4 are properties of the analysis or the tag's granularity, not defects
in the tree. Read the section as a queue of questions, not a defect list.

Causes 2 and 4 are still usually closable, and in source rather than in the tool:
a colliding symbol can be renamed and a coarse anchor can be moved onto the item
it claims. `--live-audit` reads a receiver-gated second graph precisely so
cause 2 does not fire on unambiguous names, but the gate cannot see a name `std`
also defines, nor a free function, nor a crate-root re-export.
[`stale-not-wired-triage.md`](stale-not-wired-triage.md#the-fix-each-mechanism-takes)
tabulates which fix each mechanism takes.

Comparison is per **anchor**, not per address. A formula ported into both
`engine-vm` and `engine-core` has two anchors and can legitimately be wired in
one and not the other; rolling up to the address first hides that.

## Tag drift checker

`scripts/ci/check-port-tags.py` walks `crates/engine-*/src/**.rs` and warns
when a `FUN_<addr>` citation lacks a matching `// PORT:` or `// REF:` tag
*in the same file*. The goal is to catch the "I ported X but forgot the
tag" pattern so the catalog stays in sync with what the engine actually
implements.

Default mode is `--staged` - only lines being added in the staging area
are checked, which is what the pre-commit hook runs. `--scan-all` audits
every line of every engine-crate file (full historical sweep). `--strict`
turns warnings into a nonzero exit for CI.

```bash
python3 scripts/ci/check-port-tags.py                  # default = --staged
python3 scripts/ci/check-port-tags.py --scan-all       # full audit
python3 scripts/ci/check-port-tags.py --strict         # exit 1 on warning
python3 scripts/ci/check-port-tags.py --addr 80019b28  # drill-down
python3 scripts/ci/check-port-tags.py --backfill-refs  # one-shot grandfather pass
```

**Scope rule:** only files that already carry a `// PORT:` tag are checked.
Pure-docs files (no port tag anywhere) are treated as reference-only and
skipped - they exist to describe retail behaviour without claiming ports,
and requiring REF tags inside them would be churn for no signal.

**Backfill workflow:** `--backfill-refs` rewrites in place. For each
port-bearing file with untagged citations, it inserts a `//! REF: ...`
block after the last `//!` line of the leading module-doc comment (or at
the top of the file if there's no leading block). Re-run whenever new
ports or citations land to keep the REF set fresh.

**Pre-commit integration:** `scripts/git-hooks/pre-commit` runs
`check-port-tags.py --staged --quiet` after `cargo clippy`. The hook is
**warn-only** - drift output prints but never blocks the commit, so the
checker doesn't gate unrelated PRs. CI can tighten by switching to
`--strict`.

## Usage

```bash
python3 scripts/ci/port-catalog.py                       # global catalog -> target/port-catalog/
python3 scripts/ci/port-catalog.py --missing-ports       # dumped + documented, not ported (excludes ignore-list)
python3 scripts/ci/port-catalog.py --missing-ports --include-ignored   # include ignore-list entries
python3 scripts/ci/port-catalog.py --missing-dumps       # cited but not dumped
python3 scripts/ci/port-catalog.py --ported-only         # show only ported addresses
python3 scripts/ci/port-catalog.py --ignored-only        # show only ignore-list entries
python3 scripts/ci/port-catalog.py --addr 801dd35c       # drill-down on one address
python3 scripts/ci/port-catalog.py --md                  # markdown to stdout
python3 scripts/ci/port-catalog.py --list-features       # list features in features.toml
python3 scripts/ci/port-catalog.py --feature title-screen   # BFS from a feature's roots
python3 scripts/ci/port-catalog.py --dashboard           # open-work rollup -> open-work.md
python3 scripts/ci/port-catalog.py --live                # add the reachability column
python3 scripts/ci/port-catalog.py --not-live            # ported but unreachable from any host root
python3 scripts/ci/port-catalog.py --live-audit          # reachability vs `NOT WIRED:` disclosures
python3 scripts/ci/port-catalog.py --check --live        # ratchet every baselined figure
python3 scripts/ci/port-catalog.py --check --allow-uncompared   # fast pass, skips the disclosure gap
python3 scripts/ci/port-catalog.py --live --update-baseline
```

Output is written to `target/port-catalog/` (gitignored):

- `catalog.csv` / `catalog.md` - every tracked address, machine-readable + markdown.
- `<feature>.csv` / `<feature>.md` - per-feature subset when `--feature` is used.
- `open-work.md` - single-page dashboard combining per-feature port % + top-N missing-ports per feature + ignore-list summary (see "Open-work dashboard" below).
- `live-audit.md` - reachability verdicts checked against the source's own `NOT WIRED:` disclosures, when `--live-audit` is used.

## The ratchet

The catalog's figures reach the site's landing page through
`scripts/ci/progress-metrics.json`, so for as long as the tool had no failure
mode - no `--check`, no baseline, and a place in neither the hook nor a
workflow - a wave could widen a worklist or drop a `// PORT:` tag, move a
published number, and report nothing. `--check` compares against
`scripts/ci/port-catalog-baseline.json` and is a hard pre-commit gate; the same
step runs in CI, where it reports `SKIPPED` because the `dumped` column reads
the gitignored Ghidra corpus. The hook is therefore the only place any of these
figures is ever compared.

| Figure | Direction |
|---|---|
| `worklist/port` - dumped + documented, not ported, not ignored | may not grow |
| `worklist/dump` - cited but not dumped | may not grow |
| `worklist/ported_not_documented` | may not grow |
| `worklist/ported_not_dumped` | may not grow |
| `live/disclosure_gap` - inert anchors carrying no `NOT WIRED:` tag | may not grow |
| `totals/ported` | may not shrink |

**What is deliberately not in it.** This tool builds two call graphs, and the
receiver-gated one exists solely for the stale-`NOT WIRED:` test (see
["Where the reachability pass over-reports"](#where-the-reachability-pass-over-reports)).
Nothing read off that graph is baselined, because its numbers move with graph
resolution rather than with the work - and sharpening the shared permissive
graph has been tried and reverted more than once. Everything ratcheted above is
a property of the tags, the docs and the dump corpus.

`live/disclosure_gap` is the one entry taken from the permissive graph, and it
is sound in that direction precisely because that graph *over*-reports
reachability: a port it still calls inert really is inert, so an inert anchor
with no disclosure is a lower bound.

### A figure the default path never computes is not ratcheted

`live/disclosure_gap` needs `--live`. It was baselined at 0, the hook ran
`--check` without `--live`, the run printed `NOT COMPARED THIS RUN` and exited
0 - so the figure drifted to 1 and stayed there with every gate green. Printing
a line is not a comparison. Three things follow, and the third is the one that
would have caught it:

- **`--check` fails on an uncompared figure.** A caller that cannot afford the
  slow pass says `--allow-uncompared` at the call site, which makes "the slow
  pass did not run" a visible decision rather than a default.
- **The hook spends the pass when it can matter.** `--live` costs about 20s
  against 3s for the default run. The disclosure gap is a property of the Rust
  call graph and the `NOT WIRED:` tags, both under `crates/`, so the hook runs
  the full compare when the commit stages `crates/` and passes
  `--allow-uncompared` otherwise. The CI step never passes it - it is inert
  today for want of the dump corpus, and must be the complete invocation if
  that ever changes.
- **`--update-baseline` no longer drops what it did not compute.** A snapshot
  carries only the figures its run produced, so writing it verbatim deleted the
  whole `live` block on any run without `--live` - one command, ratchet gone,
  nothing said. Uncomputed figures are now carried forward from the existing
  baseline and named in the output.

A regression report is a prompt to open the per-row pages - `--missing-ports`,
`--live-audit`, and the triage pages under
[`live-audit-triage.md`](live-audit-triage.md) and
[`stale-not-wired-triage.md`](stale-not-wired-triage.md). **Validate a surprise
against rows, never against the count**: three separate waves have found the
count moving for a reason inside the measurement rather than inside the tree.

## Features (BFS from roots)

A *feature* in this tool is a named set of seed Ghidra function addresses
(`roots`) plus an optional list of `stop_at` boundaries. Running
`--feature <name>` filters the catalog to the addresses reachable from those
roots via the citation graph (one edge per "this dump cites that address").

Features live in `scripts/ci/features.toml`:

```toml
[title-screen]
description = "Title overlay tick + boot UI"
roots = ["801dd35c"]
# Optional boundaries kept in the result but not recursed past.
stop_at = ["801de840", "801e295c"]
# Optional BFS depth cap.
max_depth = 2
```

The citation graph only has edges between *dumped* functions - undumped
helpers have no outgoing edges, so the BFS frontier widens as more dumps
land. This is intentional: it lets you start tight (small feature with few
dumps) and progressively widen as you dig in.

Use feature views to:

- Find unported helpers in scope of a specific feature (filter by
  `--feature X --missing-ports`).
- Confirm a port is reachable from the feature root.
- Spot shared-infrastructure spillover that wants a `stop_at` entry.

## Ignore list

`scripts/ci/port-catalog-ignore.toml` lists addresses that the catalog should
treat as out-of-scope for engine porting - statically-linked PsyQ kernel /
runtime / SDK code. The clean-room port maps these clusters to native
equivalents (Rust stdlib, wgpu, cpal) rather than reimplementing the
PSX wrappers, so they shouldn't pollute the port worklist.

```toml
[bios]
"80056678" = "EnterCriticalSection (syscall(0), a0=1)"
"80056688" = "ExitCriticalSection (syscall(0), a0=2)"

[libgte]
"8005ba1c" = "GTE sqrt / normalise (mtc2 0xF000 / mfc2 0xF800)"

[libsnd]
"80062340" = "SsSeqOpen (slot-bitmap walk + load)"
```

Categories are organisational (one TOML table per cluster - `bios` / `libc` /
`libgte` / `libgs` / `libcd` / `libapi` / `libsnd` / `libspu` / `libetc`); the
tool treats every entry the same way. Provenance for each entry lives in
[`docs/reference/functions.md`](../reference/functions.md) and the audio /
save-screen subsystem docs.

One section is not PsyQ infrastructure and asserts something else entirely.
`unreferenced` holds **retail-unreachable entry points**: real routines that
nothing on the disc reaches, in any reference form, so a port of one could only
be inert. What was scanned and why the row is preferred over code is on
[`address-reference-scan.md`](address-reference-scan.md); how it differs from
the other two claim kinds a row can make is in
[`worklist-classification.md`](worklist-classification.md#the-three-kinds-of-ignore-claim).

**Unreferenced is not by itself a reason to exclude.** Being unreachable says
the port cannot be *wired*; it says nothing about whether the routine is worth
reproducing. `unreferenced_transport_and_runtime` excludes four such routines
on what they *are* - drive transport and PsyQ runtime-lib tiers the port
replaces wholesale - while game-mode handlers and a camera preset from the same
sweep stay on the worklist, unreferenced and all. A row moves to the ignore
list on its subject matter, never on its reference count.

### Why this worklist is not zero, and should not be

The port worklist is denominated in addresses this project cites, so it can only
see code something already pointed at. [`disc-coverage.md`](disc-coverage.md)'s
denominator is the game's own bytes, and it surfaces routines **no reference of
any form reaches on the disc** - which means Ghidra built no function record,
nothing cited them, and no citation-denominated worklist could ever list them.

Documenting one moves it into `dumped + documented, not ported` and the worklist
rises. That is the two measurements composing correctly: the byte denominator
finds the work, the citation denominator tracks it. A worklist that stays at
zero while the byte sweep is still finding code is reporting on its own
denominator, not on the disc. Never hold the number down by declining to
document a function, and never park one in the ignore list to hide it.

Default behaviour:

- `--missing-ports` excludes ignored entries. The summary line breaks the
  count down (`of which ignored / remaining port worklist`).
- `--include-ignored` opts back in for completeness checks.
- `--ignored-only` lists the ignore-list itself (useful for auditing).

Adding an entry: copy the address into the appropriate category table with a
one-line reason that names the PsyQ function and (where known) the BIOS
vector. Keep the reason factual - it shows up in catalog drill-down output.
Provenance citations belong in `docs/reference/functions.md`, not in the TOML
reason field.

### Settling a row whose blocker is "no caller"

A worklist row often stalls on "what consumes this?", and the answer decides
whether the row is work at all. Two of the ways this engine reaches code are
invisible to a call-graph sweep - a function-pointer table or actor-template
word, and a `lui`+`addiu` pair Ghidra's reference manager does not resolve - so
"no `jal` targets it" is not yet a finding. Run the row's address through
[`address-reference-scan.md`](address-reference-scan.md) before either porting
or shelving it. It has three useful outcomes:

- **A table or template word.** The consumer is whatever spawns or dispatches
  through that record; the row is real work, and the doc gains a provenance
  citation.
- **Nothing, anywhere.** The address is linked but unreached, and porting it
  can only add an inert row. Document the negative
  ([`battle.md` § Unreferenced SCUS entry points](../reference/functions/battle.md#unreferenced-scus-entry-points)
  is the worked example) and file the address under the ignore list's
  `unreferenced` section, rather than closing the row with code.
- **Branch sites but no call sites.** The address is an intra-function label,
  not an entry - a `classify-worklist.py` `INTERIOR` row that the scan
  confirms from the bytes.

## Open-work dashboard

`--dashboard` emits `target/port-catalog/open-work.md`, a single regenerable
page that answers "what's left to port, in what scope" at a glance. The
dashboard combines four signals:

1. **Global counts** - dumped / documented / ported / ignored / remaining port
   worklist.
2. **Per-feature status table** - for each feature in
   [`scripts/ci/features.toml`](../../scripts/ci/features.toml): reachable, ported,
   port %, missing (port worklist within the feature, ignore-list excluded),
   ignored.
3. **Per-feature top-N missing-ports** - the highest-citation-count helpers
   reachable from each feature's roots that don't yet carry a `// PORT:` tag.
   Sorted high-leverage first, so a feature's blockers surface immediately.
   Cap is `--dashboard-top N` (default 10).
4. **Ignore-list summary** - count per category (bios / libc / libgte / libgs /
   libcd / libapi / libsnd / libspu / libetc).
5. **Provenance gaps** - addresses with a `// PORT:` tag but missing a dump or
   doc citation (shown only when nonzero).

The page is gitignored output (lives under `target/`). Re-run after landing a
batch of ports to see which helpers are now top-of-list. The question-level
companion - open *hunts* rather than per-function status - is
[`docs/reference/open-rev-eng-threads.md`](../reference/open-rev-eng-threads.md).

## What the columns surface

The point of the table is to make the cross-cuts cheap to read:

- **`dumped + documented + not ported, not ignored`** → port worklist. The
  function is understood (we have a Ghidra dump and at least one doc
  citation), not yet implemented in the engine, and not statically-linked
  PsyQ infra. Sort by citation count to find high-leverage helpers first.
- **`cited but not dumped`** → dump worklist. Some other dump references this
  address but no dump exists for it yet. Add to `ghidra/scripts/dump_funcs.py`
  `TARGETS`.
- **`ported but not documented`** → provenance gap. A `// PORT:` tag was added
  without any doc mentioning the source function. Either backfill the doc or
  remove the tag if the attribution was wrong.
- **`ported but not dumped`** → provenance gap. Same shape, opposite axis.

## Caveats

- **Citation graph is dump-local.** The "cited" signal comes from grepping
  dump files - so an undumped helper has no outgoing edges. The frontier of
  reachable functions widens only as dumps land.
- **`functions.md` is curated, but `documented` is broader.** Any doc page
  that mentions `FUN_<addr>` or `0x<addr>` counts. The catalog won't tell you
  which docs are authoritative - that's still a judgement call per topic.
- **One `// PORT:` tag does not guarantee semantic equivalence.** The tag is a
  provenance link, not a correctness proof. Tests + retail-comparison still
  do that job - and `--live` answers only whether the code is *reached*, not
  whether it is right.
- **A `#[cfg(test)]` helper is still a caller node.** Test functions are
  excluded from a module anchor's *scope* but not from the call graph's
  *callers*, so a helper with a common name (`step`, `tables`, `new`) collects
  every same-named edge in the tree and marks its own module live - which
  reads as a stale `NOT WIRED:` tag on a module nothing calls. Give test
  helpers distinctive names; a row in the stale-tag section whose only path
  runs through a test helper is this, not a wiring gap.
- **The ignore-list is curated, not exhaustive.** Newly-dumped PsyQ helpers
  don't auto-classify - `--missing-ports` will surface them until they're
  explicitly added to `port-catalog-ignore.toml`. Treat unfamiliar 16-byte
  thunks in `0x8005xxxx` / `0x8006xxxx` as likely ignore candidates rather
  than ports.

## See also

- [`live-audit-triage.md`](live-audit-triage.md) - per-anchor verdicts for the
  `engine-core` and `engine-vm` rows of the audit's undisclosed-inert section,
  plus the analysis defects that triage turned up.

- [`docs/tooling/ghidra.md`](ghidra.md) - produces the `ghidra/scripts/funcs/` dumps that drive the "dumped" column.
- [`docs/reference/functions.md`](../reference/functions.md) - the curated entry-point directory the "documented" signal draws from.

- [`stale-not-wired-triage.md`](stale-not-wired-triage.md) - per-row verdicts
  for the audit's *tagged `NOT WIRED` but analysed live* section, and the three
  call-graph mechanisms that put rows there.

## Where the reachability pass over-reports

The graph resolves calls by name, never by receiver type, and every ambiguity
resolves toward reachability - that is what makes `--not-live` a floor rather
than a guess. The cost lands on the audit's first section, in three shapes:

- an ambiguous `.name(` or `name(` linking to every in-tree definition of that
  name, which makes any port whose entry point is called `new`, `tick`, `add`,
  `len` or `default` read live regardless of wiring;
- the bare-identifier edge, which links a function *value* to a free function of
  that name and cannot tell it from a struct **field** of the same name;
- a `//! PORT:` module anchor, whose scope is the whole file - so one reachable
  routine reports every tagged address in that file live, including the ones the
  module doc marks `NOT WIRED` by name.

**The first two are corrected, in a second graph.** `--live-audit` builds
`build_rust_graph(strict=True)` alongside the permissive one and reads *only*
the stale-`NOT WIRED` test off it; `live`, `--not-live` and `--live-only` are
unchanged and still come from the permissive graph. Sharpening the shared graph
instead would trade the hard floor away for a fix to the opposite error, which
is why it is two graphs and not one.

The third shape - anchor granularity - is open, and is now most of what the
audit's first section reports. A module anchor cannot distinguish the tagged
routine from a live `Default` impl in the same file without the `PORT:` tag
carrying item-level information.

[`stale-not-wired-triage.md`](stale-not-wired-triage.md) carries the worked
examples, the per-row verdicts, and why the receiver gate must fire only on
*ambiguous* names.

## The runtime denominator (`replay-port-coverage.py`)

Everything above is computed by the engine about the engine: a static graph
walked from the host roots, answering *could this be reached*. It cannot
answer *was it reached*, and because the shared graph is deliberately
permissive, `live` is an upper bound - the count includes ports reachable only
through a path no player takes.

`scripts/ci/replay-port-coverage.py` supplies the missing denominator. It joins
`cargo llvm-cov` output for a replay test against the catalog's
address → `(file, line)` anchors, resolving each anchor to a function the same
way [`collect_port_anchors`](#anchors) does (a tag inside a body belongs to the
enclosing function; a tag above an item to the next one; a `//! PORT:` module
tag to the whole file). It reports three sets:

| set | meaning |
|---|---|
| **inert-entered** | the static graph says no host root reaches it; the run executed it anyway. The graph is wrong or the tag is on the wrong symbol - each row is a finding. |
| **disclosed-entered** | an anchor carrying a `NOT WIRED:` disclosure that a **passing** oracle executed. Highest priority: an oracle traversing stub code can certify behaviour nothing implements. |
| **live-unentered** | statically reachable, never reached. Not a defect - the wiring worklist ordered by what a playthrough actually needs. |

### The denominator is a union of ladders, not one binary

`--json` is repeatable, and joining only one test binary is the trap here. The
repo drives three pad-only ladders, each its own test target and each at full
score: `critical_path_replay` (the world spine), `menu_replay` (pause menu +
save UI), `minigame_replay` (the five minigame doors). Menus and minigames are
also the two largest clusters in `live-unentered`, so a join over the spine
alone reports precisely the subsystems the other two ladders walk as
never-entered - and a worklist ordered off it is ordered against a measurement
that excluded its own top rows by construction.

Produce one export per ladder, then join them. **The union is the default** -
a bare invocation globs `target/cov-*.json`, so the short command is the honest
one and the single-binary join is now the thing you have to ask for:

```bash
for t in critical_path_replay menu_replay minigame_replay v0_1_playthrough; do
  cargo llvm-cov --release -p legaia-engine-shell \
      --test "$t" --json --output-path "target/cov-$t.json"
done
scripts/ci/replay-port-coverage.py
```

Separate exports rather than one multi-`--test` run, because the report's
per-ladder table then says what each contributed and how much of it no other
ladder reached - which is what makes "drop a ladder from the join" a visible
cost rather than a quieter number.

The script also carries the canonical ladder list and **names any member whose
export is absent**, on stdout and in a `Partial union` note in the report. A
union over three of the four is not a conservative version of the number, it is
a number about three ladders, and without the note it reads identically to the
full one.

### Why this stays a manual step

Each export is an instrumented release build plus a full disc-gated ladder run,
so a complete union is minutes of work and needs the disc - it cannot be a
pre-commit gate, and a gate that silently degrades to a partial union would
reintroduce exactly the understated denominator this section exists to name.
`--fail-on-disclosed` is the gateable part: it asks whether a passing oracle
executed disclosed-stub code, which is a defect at any coverage level and needs
no complete union to mean something.

The join needs no source edits - the `// PORT:` tags already carry the
address→symbol mapping, and coverage supplies execution counts against the same
`(file, line)` coordinates.

Three properties to keep in mind when reading the output. The union is over
**separate sessions**, so it measures "some pad-driven ladder entered this"
against "none did", not one continuous playthrough. **`live-unentered` is
scoped to how far those ladders get**, so it is a worklist and never a defect
count; a rung the ladder cannot clear silently widens it. And
**`--fail-on-disclosed` is the only gateable set** - the other two move with
replay coverage, so ratcheting them would punish extending the run.

Requires `cargo-llvm-cov` and the `llvm-tools-preview` component; the script
skips (exit 0) when the JSON is absent, so a CI run without the coverage
toolchain is a pass.
