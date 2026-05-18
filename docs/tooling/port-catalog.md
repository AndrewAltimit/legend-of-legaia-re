# Port catalog

A per-function status catalog that unifies three independent signals across the
decompilation and engine-port tracks, plus a fourth axis for scope-excluded
addresses (statically-linked PsyQ library code that the engine maps to native
equivalents rather than porting line-by-line):

| Column | Source of truth |
|---|---|
| **dumped** | A Ghidra decompiler dump exists under `ghidra/scripts/funcs/` (gitignored — regenerable from the Ghidra project). |
| **documented** | The address is cited from at least one file under `docs/` (`FUN_<addr>` or `0x<addr>`, case-insensitive). |
| **ported** | A Rust source under `crates/` carries a `// PORT: FUN_<addr>` tag for that address. |
| **ignored** | The address is listed in `scripts/port-catalog-ignore.toml` as a non-port-site (BIOS thunk / libc shim / libgte / libgs / libgpu / libcd / libsnd / libspu / libapi / libetc). Excluded from `--missing-ports` by default. |

Tool: [`scripts/port-catalog.py`](../../scripts/port-catalog.py). Reuses helpers
from [`scripts/function-coverage.py`](../../scripts/function-coverage.py) and
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

The tag may appear as plain `//`, doc `//!`, or outer-doc `///` — putting it in
the doc block keeps the provenance co-located with the rustdoc description and
makes it visible in generated docs.

Rules:

- The tag is the only signal trusted for "ported". Plain mentions of
  `FUN_<addr>` in module docs or comments are ignored — they show up in many
  contexts that don't imply a port (cross-refs, "inspired by", "not yet
  ported", etc.) and noisily inflate the column.
- Address must be lowercase hex in the SCUS / overlay code range.
- Match is line-local — put the tag on its own line or as a trailing comment.
- A single Rust file can carry many tags. The catalog records the crate name
  each tag appears in.
- One Ghidra function can be ported into more than one crate (e.g. a
  formula shared between `engine-vm::battle_formulas` and a helper in
  `engine-core`). The catalog lists every crate that tags the address.

When porting a Ghidra function, add the tag once in the Rust function that
*implements* its behaviour. Don't tag every caller of the ported function.

## Usage

```bash
python3 scripts/port-catalog.py                       # global catalog -> target/port-catalog/
python3 scripts/port-catalog.py --missing-ports       # dumped + documented, not ported (excludes ignore-list)
python3 scripts/port-catalog.py --missing-ports --include-ignored   # include ignore-list entries
python3 scripts/port-catalog.py --missing-dumps       # cited but not dumped
python3 scripts/port-catalog.py --ported-only         # show only ported addresses
python3 scripts/port-catalog.py --ignored-only        # show only ignore-list entries
python3 scripts/port-catalog.py --addr 801dd35c       # drill-down on one address
python3 scripts/port-catalog.py --md                  # markdown to stdout
python3 scripts/port-catalog.py --list-features       # list features in features.toml
python3 scripts/port-catalog.py --feature title-screen   # BFS from a feature's roots
```

Output is written to `target/port-catalog/` (gitignored):

- `catalog.csv` / `catalog.md` — every tracked address, machine-readable + markdown.
- `<feature>.csv` / `<feature>.md` — per-feature subset when `--feature` is used.

## Features (BFS from roots)

A *feature* in this tool is a named set of seed Ghidra function addresses
(`roots`) plus an optional list of `stop_at` boundaries. Running
`--feature <name>` filters the catalog to the addresses reachable from those
roots via the citation graph (one edge per "this dump cites that address").

Features live in `scripts/features.toml`:

```toml
[title-screen]
description = "Title overlay tick + boot UI"
roots = ["801dd35c"]
# Optional boundaries kept in the result but not recursed past.
stop_at = ["801de840", "801e295c"]
# Optional BFS depth cap.
max_depth = 2
```

The citation graph only has edges between *dumped* functions — undumped
helpers have no outgoing edges, so the BFS frontier widens as more dumps
land. This is intentional: it lets you start tight (small feature with few
dumps) and progressively widen as you dig in.

Use feature views to:

- Find unported helpers in scope of a specific feature (filter by
  `--feature X --missing-ports`).
- Confirm a port is reachable from the feature root.
- Spot shared-infrastructure spillover that wants a `stop_at` entry.

## Ignore list

`scripts/port-catalog-ignore.toml` lists addresses that the catalog should
treat as out-of-scope for engine porting — statically-linked PsyQ kernel /
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

Categories are organisational (one TOML table per cluster — `bios` / `libc` /
`libgte` / `libgs` / `libcd` / `libapi` / `libsnd` / `libspu` / `libetc`); the
tool treats every entry the same way. Provenance for each entry lives in
[`docs/reference/functions.md`](../reference/functions.md) and the audio /
save-screen subsystem docs.

Default behaviour:

- `--missing-ports` excludes ignored entries. The summary line breaks the
  count down (`of which ignored / remaining port worklist`).
- `--include-ignored` opts back in for completeness checks.
- `--ignored-only` lists the ignore-list itself (useful for auditing).

Adding an entry: copy the address into the appropriate category table with a
one-line reason that names the PsyQ function and (where known) the BIOS
vector. Keep the reason factual — it shows up in catalog drill-down output.
Provenance citations belong in `docs/reference/functions.md`, not in the TOML
reason field.

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
  dump files — so an undumped helper has no outgoing edges. The frontier of
  reachable functions widens only as dumps land.
- **`functions.md` is curated, but `documented` is broader.** Any doc page
  that mentions `FUN_<addr>` or `0x<addr>` counts. The catalog won't tell you
  which docs are authoritative — that's still a judgement call per topic.
- **One `// PORT:` tag does not guarantee semantic equivalence.** The tag is a
  provenance link, not a correctness proof. Tests + retail-comparison still
  do that job.
- **The ignore-list is curated, not exhaustive.** Newly-dumped PsyQ helpers
  don't auto-classify — `--missing-ports` will surface them until they're
  explicitly added to `port-catalog-ignore.toml`. Treat unfamiliar 16-byte
  thunks in `0x8005xxxx` / `0x8006xxxx` as likely ignore candidates rather
  than ports.
