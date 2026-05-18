# Port catalog

A per-function status catalog that unifies three independent signals across the
decompilation and engine-port tracks:

| Column | Source of truth |
|---|---|
| **dumped** | A Ghidra decompiler dump exists under `ghidra/scripts/funcs/` (gitignored — regenerable from the Ghidra project). |
| **documented** | The address is cited from at least one file under `docs/` (`FUN_<addr>` or `0x<addr>`, case-insensitive). |
| **ported** | A Rust source under `crates/` carries a `// PORT: FUN_<addr>` tag for that address. |

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
python3 scripts/port-catalog.py --missing-ports       # dumped + documented, not ported
python3 scripts/port-catalog.py --missing-dumps       # cited but not dumped
python3 scripts/port-catalog.py --ported-only         # show only ported addresses
python3 scripts/port-catalog.py --addr 801dd35c       # drill-down on one address
python3 scripts/port-catalog.py --md                  # markdown to stdout
```

Output is written to `target/port-catalog/` (gitignored):

- `catalog.csv` — every tracked address, machine-readable.
- `catalog.md` — the same rows as a markdown table.

## What the columns surface

The point of the table is to make the cross-cuts cheap to read:

- **`dumped + documented + not ported`** → port worklist. The function is
  understood (we have a Ghidra dump and at least one doc citation) and not yet
  implemented in the engine. Sort by citation count to find high-leverage
  helpers first.
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
