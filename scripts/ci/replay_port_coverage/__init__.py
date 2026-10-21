"""Join a replay's runtime coverage against the port catalog.

Every other reachability number in this repo is computed by the engine about
the engine: `port-catalog.py --live` walks a *static* call graph from the host
roots and asks whether a `// PORT:`-tagged Rust symbol is reachable in
principle. That graph is deliberately permissive (see the two-graph split in
`docs/tooling/port-catalog.md`), so "live" is an upper bound - it says
reachable, not reached.

This script supplies the missing denominator: what a **pad-driven run actually
executes**. It consumes `cargo llvm-cov` output for one or more replay ladders
and joins the per-function execution counts against the catalog's
address -> (file, line) anchors, reporting three sets:

  inert-entered   an address the static graph calls NOT reachable from any host
                  root, whose anchor ran anyway. The graph was wrong, or the
                  tag is on the wrong symbol. Each one is a finding.

  disclosed-entered
                  an anchor carrying a `NOT WIRED:` disclosure that ran anyway.
                  These are the dangerous ones: a passing oracle traversed code
                  the source says nothing reaches, so the oracle may be
                  certifying a stub.

  live-unentered  an address the static graph calls live that no run entered.
                  Not a defect - this is the prioritisation list, and it is only
                  meaningful relative to how far the ladders get.

## A const anchor has no lines to execute

Line coverage answers "did this code run", and a `// PORT:` tag above a
`const` / `static` / `type` alias anchors to an item with **no code**. The
naive resolution - read the verdict of the next function in the file - turns
the neighbour's coverage into the const's, which both *accused* honestly
disclosed `NOT WIRED` consts of executing (the sibling ran) and *credited*
never-referenced consts as entered. So item anchors take their executed
verdict from attributable REFERENCES instead, mirroring the catalog's strict
item-liveness rule (`port-catalog.py: item_reference_patterns` /
`item_reference_hit` - one definition, both consumers): the item is executed
iff some executed non-test `fn` names it bare from the item's own file or
qualified (`module::NAME` / `Type::NAME`) from anywhere. If no attributable
referencing function executed, the verdict is **not observable (const)** - a
fourth bucket, deliberately neither "entered" nor "never entered", because a
worklist row that no line of coverage can ever convert is not work in this
report's sense. Type anchors (struct / enum / trait) resolve the same way the
catalog does: through the executed methods of the type's own `impl` blocks,
falling back to the file when the file gives the type no `impl`.

## The denominator is a union of ladders, not one session

`--json` is **repeatable**, and the difference matters more than it looks. The
repo drives several pad-only ladders, each its own test binary and each at full
score: `critical_path_replay` (the world spine), `menu_replay` (the pause menu
and save UI), `minigame_replay` (the five minigame doors), `v0_1_playthrough`
(the cold-boot field anchor), and `play_compose_ladder` (the browser play
page's draw composition - see below). A join over only the
first reports the menu and minigame subsystems as never-entered - and those two
are the largest clusters in the live-unentered list, so a worklist ordered off
a single-binary run is ordered against a measurement that structurally excluded
its own top rows.

## One ladder is a rendering host

The four `engine-shell` ladders drive the headless `BootSession`, which builds
no draw list - so `engine-ui` reported zero executed regions across their whole
union, and the largest NO-LADDER cluster in `docs/tooling/reach-triage.md` was
invisible to this report by construction. `play_compose_ladder`
(`crates/web-play/tests/`) closes that shape: the browser play page's
composition is *library* code, so the ladder feeds pad words per tick and calls
the page's per-frame read surface (`play_overlay_draws_json`, the menu / battle
/ fishing / dev-menu overlays, the screen-prim route, the battle 3D + FX
exports). It is still pad-only - nothing is seated or poked - but its frames
are composed, which is exactly the half of a rendering host the native window
keeps behind a wgpu surface.

So the headline number is the **union** across every `--json` given, and the
per-source table below it reports what each ladder contributed and how much of
that no other ladder reached. A union across separate sessions is not one
continuous playthrough and the report says so; what it does measure honestly is
"code some pad-driven run entered" versus "code no pad-driven run entered".

## The union is the default, and a short join says so

With no `--json`, the script discovers `target/cov-*.json` and joins **all** of
them, so the bare invocation is the union rather than one binary. It also knows
the canonical ladder set ([`CANONICAL_LADDERS`]) and names any member whose
export is missing, in the report and on stdout - because a union over three of
the four is not a smaller version of the number, it is a different number, and
the earlier single-binary default understated the denominator by 1.7x without
saying anything at all.

The cost is why this stays a manual step rather than a pre-commit gate: each
export is an instrumented build plus a full disc-gated ladder run, and the
ladders are minutes each. (Instrumented, NOT `--release` - the recipe below
passes no `--release` anywhere, because the line table a release build emits
does not survive the span join.) `--fail-on-disclosed` is the gateable part, and
it is the part that needs no complete union to be meaningful.

Usage:
    # produce one export per ladder (slow: instrumented build of the engine
    # crates). One export each rather than one multi---test run: the
    # per-ladder table is only possible when the exports are separate, and
    # that table is what makes dropping a ladder a visible cost. The package
    # differs per ladder ([`CANONICAL_LADDERS`] carries it): the engine-shell
    # ladders and the web-play composition ladder are different crates, and
    # `--test` links each crate's LIBRARY - which is why the web-play route
    # can execute composition code the native window runs only behind a wgpu
    # surface.
    # Clean first: the reader collapses duplicate spans keyed on the exact
    # (line_start, line_end), so a stale sibling binary left in `target/` can
    # shadow a fresh executed record with its own zero.
    cargo llvm-cov clean --workspace

    # NO `--release` anywhere - see "a release export loses executed code"
    # below. On the default profile the whole set (`--list-ladders`) is several
    # hours of wall clock, and the shape of that is worth knowing: almost every
    # ladder finishes in minutes, while `full_game_ladder` (about two hours on
    # its own) and `chapter1_frontier_ladder` (about one) are the tail, because
    # they are disc-heavy and an unoptimised build pays for every sector and
    # tick they walk. The optimised build would buy that back and cost
    # executed code, so it is still the wrong trade - budget for the tail
    # instead. Each raw export is tens of MB; on a tight disk, reduce each one
    # to the `filenames` / per-file line span / executed bit the join reads
    # as soon as it is written.
    # `--list-ladders` prints `<test> <package>` for every canonical entry, so
    # the recipe cannot drift from the list the report checks against.
    #
    # TWO STEPS PER LADDER, and the split is load-bearing. `-p <pkg> --json`
    # scopes the *report* to that package's own sources, not just the build -
    # so a one-shot export of `menu_replay` carries 42 files, all under
    # `crates/engine-shell/`, while everything the ladder drives is in
    # `engine-core` / `engine-vm` / `engine-ui` and is silently absent. Build
    # scoped (`-p` picks the ladder), then report UNSCOPED over the profiles.
    #
    # `--profraw-only` between ladders keeps the per-ladder table honest: it
    # drops the profile data so each export is that ladder alone, while leaving
    # the build artifacts in place so this is not 26 rebuilds.
    # `scripts/ci/export-replay-coverage.sh` is this loop as a file.
    scripts/ci/replay-port-coverage.py --list-ladders | while read -r t pkg; do
        cargo llvm-cov clean --profraw-only
        cargo llvm-cov -p "$pkg" --test integration --no-report -- "${t}::"
        cargo llvm-cov report --json --output-path "target/cov-$t.json"
    done

    # then join them - no arguments needed, the union is the default
    scripts/ci/replay-port-coverage.py

## A release export loses executed code, silently

`-C instrument-coverage` counters are emitted per function, but an optimised
build inlines the small ones away and the out-of-line copy's record is left at
**zero** - indistinguishable, in this join, from a function no run called. It is
not a rare shape: on one `fmv_ladder` run, `advance_slice` (40 calls),
`slice_word_count` (39) and `mdec_output_control` / `skip_requested` /
`mdec_control_flags` (1-3 each) all reported `count = 0` under `--release` and
their real counts under the default profile, from the same ladder over the same
disc. Two of them are `mdec` rows this report had listed as *live but never
entered*, which was an artifact of the export rather than a gap in the ladders.

So a `--release` row in the never-entered list is a **hypothesis**, not a
measurement: re-export that ladder without `--release` before reading it as
work. The flag survives above only because the five exports it produced are what
the existing numbers came from; re-taking them is the fix, and it changes the
headline.

Skips (exit 0) when every coverage JSON is absent, so a disc-free or
coverage-toolless CI run is a pass, matching the `LEGAIA_DISC_BIN` convention.
"""
