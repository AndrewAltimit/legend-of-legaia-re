# Runtime reach triage

[`replay-port-coverage.py`](port-catalog.md) joins `cargo llvm-cov` output for
the pad-only replay ladders against the port catalog's `// PORT:` anchors and
reports three sets. Two of them are defect lists and are normally empty. The
third - *live but never entered* - is neither empty nor a defect list, and it is
the one this page is for. (A fourth bucket, *not observable (const)*, holds the
item anchors - see the note under the buckets below.)

Its size is a property of the ladder set, not of the port, and the ladder set is
what has moved. When the union was one headless binary the static live count was
several times what a run executed; over the full canonical union it is now a
**minority** of live anchors that no run enters. Two things follow, and the
second is the one that changes how the page reads: a row here is now much more
likely to be a real gate than a missing fixture, and the page's own residue is
concentrated rather than spread - see
[the cast-module band](#the-cast-module-band-the-largest-cluster-on-this-page-and-the-one-with-no-rows).

This page is the per-row verdict for that third set, so the question "is this a
gap in the port, a gap in the ladders, or neither" is answered once per address
instead of re-derived. It is a snapshot of a worklist: a row leaves the page
when a ladder reaches it or the wiring lands. What outlives the rows is the
bucket definitions plus the structural facts below about what a pad-only ladder
can and cannot execute at all.

## The three figures, and the denominator they belong to

The report opens with three counts over the canonical union: the `// PORT:`
anchors the static graph calls **live**, how many of those some run
**entered**, and how many **no run entered** - the third being the set this
page verdicts. Over the current union they are **810 live / 705 entered / 51
never entered, across 62 ladders**, with both defect lists empty.

The ladder count belongs in the same breath as the other three, because none
of them is a property of the port: every ladder that lands moves all three,
and a figure quoted without its denominator reads like a measurement of the
engine. That is also why this is the *only* count the page carries - the
per-bucket totals below stay off it for reasons that are about rot and about
concurrent editing, and are spelled out where the buckets are.

Two more buckets sit beside the three and are neither entered nor
never-entered: the item anchors the report files *not observable (const)*,
and the addresses [no binary in the union carries a record
for](#a-row-can-also-be-neither-entered-nor-never-entered). The second is much
the larger of the two and moves with the ladder set rather than with the port,
so an address list joined against the never-entered set alone will read those
as converted.

One caveat travels with the current figure and should travel with the next
one: a member of the union was **red** when it was taken (`v0_1_playthrough`
exits non-zero, contributing whatever it ran before it failed). A union taken
while a member fails is a different number rather than a smaller one - see
[the partial-union note](#a-ladder-that-fails-and-a-ladder-nobody-exported-are-the-same-line)
- and re-deriving with a bare `replay-port-coverage.py` is cheaper than
trusting this line.

## Buckets

- **(a) NO-LADDER** - reachable in real play on at least one host, but no
  existing ladder drives that content. The fix is a replay fixture, not a wire.
- **(b) GATED** - reachable only behind a story flag, scene or game state the
  ladders do not reach. The gate is named per row.
- **(c) HOST-DEAD** - the static graph finds a caller, and nothing on any of the
  three hosts reaches it in play. The valuable bucket: these are `live` in the
  permissive graph, so `--live-audit`'s *undisclosed inert ports* section cannot
  see them.
- **(d) NOT-PLAYTHROUGH** - not playthrough-shaped. A preservation parser whose
  host is a CLI subcommand is *wired* (a CLI subcommand is a host root, per
  [`stale-not-wired-triage.md`](stale-not-wired-triage.md)); "no engine consumer"
  is a different claim and is written as one.

**A const anchor is never a row in these buckets' tables.** A `// PORT:` tag
above a `const` / `static` / `type` alias anchors to an item with no lines to
execute, so the report resolves it through attributable references - the
catalog's strict item rule, one definition shared by both scripts
(`port-catalog.py: item_reference_patterns` / `item_reference_hit`): the item
reads *executed* when an executed non-test `fn` names it bare from the item's
own file or module-/type-qualified from anywhere, and otherwise files under
**not observable (const)** - deliberately neither "entered" nor "never
entered", because no line of coverage can ever convert such a row. Type
anchors (struct / enum / trait) resolve through the executed methods of the
type's own `impl` blocks, with the same no-`impl`-in-file fallback the
liveness pass uses. A const row in the report is therefore always a statement
about its referencing functions, and a disclosed `NOT WIRED` const is accused
only when executed code really references it.

Liveness is an upper bound, so bucket (c) is never assigned off the static
verdict. Each (c) row below rests on a strict caller scan of the workspace with
comment lines dropped and `#[cfg(test)]` bodies excluded - "the only caller is a
unit test" is the finding, and it is not visible to a scan that counts doc
comments as references.

### A `--release` export cannot tell "never called" from "inlined"

`-C instrument-coverage` emits one counter per function. An optimised build
inlines the small ones and leaves the out-of-line record at zero, and nothing
downstream can distinguish that zero from a function no run entered. Measured
on one ladder over the same disc, default profile against `--release`:
`advance_slice` 40 executions against 0, `slice_word_count` 39 against 0, three
more at 1-3 against 0. Two of those five were on the never-entered worklist for
no other reason.

This lands on the two bucket kinds very differently, and blanket-caveating the
page would lose the distinction:

- an **(a)** or **(b)** row whose only evidence is "no run entered it" is a
  hypothesis when it was measured off a `--release` export - the routine may
  have run and been inlined out of its own record;
- a **(c)** row is unaffected. Its evidence is the source-side caller scan
  above, which no compiler profile participates in; the coverage number only
  ever corroborated it.

So: re-measure an (a)/(b) row on the default profile before spending a fixture
on it, and read a (c) row as it stands.

### A `-p`-scoped export reports one crate, however many it ran

`cargo llvm-cov -p <pkg> --test <name> --json` scopes the **report** to that
package's sources, not only the build. Measured on the native minigame ladder
over one set of profiles: the scoped export carries 42 files, every one under
`crates/engine-shell/`, and reporting the same profiles with no `-p` carries
652 across fifteen crates. The ladder's whole yield is in the second number -
the dance HUD, the fishing chrome and actors, the Baka number drawers and the
casino counter are `engine-core`, and the draw builders under them are
`engine-ui`.

The failure mode is quiet in exactly the wrong direction: a scoped export shows
the ladder joining the union and changing nothing, which reads identically to a
ladder that did not work. Export in two steps - `--no-report` to run, then a
bare `cargo llvm-cov report --json` over the profiles - whenever the code a
ladder drives lives outside the package the test does.

It also lifts one of the structural exclusions below: `engine-render` is a hard
wgpu link the browser composition ladder cannot carry, and the native window
*is* that link, so a spawned `play-window` run reports executed regions there.

### A ladder that fails and a ladder nobody exported are the same line

`replay-port-coverage.py` opens its report with a `PARTIAL UNION` line naming
every `CANONICAL_LADDERS` member with no `cov-*.json`, and that line cannot
tell the two reasons apart. A member nobody exported and a member whose run
**failed** both leave no export behind, so both arrive in the same list, and
the second reads as an export somebody forgot rather than as a red test.

The remedies are opposite. An unexported ladder is one command away from
joining the union. A failing one silences its whole contribution until the
failure is fixed - every row it was written to convert keeps reading *never
entered*, and the page's verdict for those rows ("a ladder reaches it") is
true of the source and false of the measurement.

So a union taken while a member is red is a different number, not a smaller
one, in the same way the module docstring already says a union over a subset
is. Read the `PARTIAL UNION` line against the export run's own log before
reading the never-entered set: a member that appears there *and* has a failing
run is the one to fix first.

### A tag between two functions is scored by the next function that has regions

`FileCoverage.verdict_at` resolves a `// PORT:` line to the enclosing function
span, and failing that to the next span starting at or after the tag. When the
tag's own symbol is absent from the export's line geometry, "the next span" is
some later function, and the tag inherits *that* function's verdict.

The `mode_init_bare` tags are the worked example. All three sit in a comment
block between items, no exported span contains their lines, and the next span
present starts 70-odd lines further down and executed - so the join reports
three `NOT WIRED`-disclosed anchors as executed, which is the report's own
highest-priority category. No caller exists: the only references to
`mode_init_bare` outside its own definition are `#[cfg(test)]` unit tests in
the same file, which no ladder builds. Scanning every export for the symbol
finds it at count 0 in all of them.

Two properties make it hard to spot. The export carries a record per binary
built into the target dir, so one symbol appears under several crate hashes at
several line geometries at once, and a stale record's span can sit hundreds of
lines from the current source. And the fall-through is silent: an anchor whose
owner genuinely never ran is exactly the anchor whose record carries no live
regions, so the rows most likely to be mis-scored are the ones the category is
meant to find.

The joiner already knows each anchor's symbol - it prints it in the row - and
already has `executed_overlapping` for resolving a symbol's *source* span
against the export. Routing anchors with a known symbol through that path,
instead of through the line fall-through, is what closes this. Until then, read
a "disclosed `NOT WIRED` anchor executed" row as a claim to check rather than a
finding: confirm a caller exists before treating it as a disclosure defect.

### The other way that category misfires: a disclosure about one arm

An anchor's disclosure is read anywhere in its doc block, and the marker is an
**anchor-level** claim - "no host calls this port". A doc block that uses the
same words about one *branch* of a two-branch routine therefore disclaims the
whole routine, and `--fail-on-disclosed` then reports the ladders as having
traversed a stub they did not.

`FUN_801DBF9C`'s port is the worked case and it is not a stale disclosure. The
routine is the party cast trigger, called on every pre-cast expiry, and the
ladders run it; what is unimplemented is the `spell_id < 0x25` arm's per-spell
anim-pair list, because the engine has no parse of the overlay table at
`0x801F4E64` / `0x801F4EDC`. Both halves are true, and only one of them is
about wiring.

So a gap in an arm's **data** is written as prose, never in the disclosure form
- the form is reserved for "nothing calls this". The alternative reading, that
the routine should carry the marker because part of it is unfinished, makes the
report's highest-priority category fire on exactly the routines a ladder
proves are live.

### A row can also be neither entered nor never-entered

The report has a third answer, and reading a page row against the
never-entered set **alone** turns it into the wrong one. An anchor whose file
no binary in the union carries has no coverage record at all: it is outside
both counts, and an address list joined against "never" finds it absent and
reads that as converted. A missing measurement becomes a claim of progress,
which is the one direction an instrument must never fail in.

Measured over this page's own citations: of the addresses it names, a set the
size of a small table is in that bucket, and **every one of them is in
`engine-vm`** - module anchors on `scus_core_helpers.rs`, `overlay_rng.rs`,
`world_map_clut_fade.rs`, and function anchors in `battle_stream_slot.rs`,
`battle_helpers.rs`, `code_lock_actor.rs`. The shape is consistent with
link-time dead-code removal: a function nothing in the linked binary
references is not emitted, so no counter for it exists to be zero.

`replay-port-coverage.py` names that set in the report (*Not observable in any
of these binaries*) rather than only counting it, for the same reason the
host-drift gate names its orphans: a count cannot tell "the ladders reached
it" from "nothing looked". Converting such a row means linking the crate from
a ladder, not wiring anything - and until one does, the row's reach verdict is
**unmeasured**, which is a third word this page needs and did not have.

## What a pad-only ladder structurally cannot execute

The *headless* ladders drive `BootSession`, which constructs no renderer, no
audio device and no draw list; `crates/engine-shell/src/boot.rs` names neither
`engine-ui` nor `engine-render`. Under their union alone, four whole crates
report **zero** executed regions - which is a fact about the harness, not the
port, and it is what kept the largest NO-LADDER cluster on this page invisible
to the reach report.

The union now carries a rendering host: `play_compose_ladder`
(`crates/web-viewer/tests/`) drives the browser play page's `LegaiaRuntime` by
pad and composes the page's whole per-frame read surface, so the draw-list
builders execute under coverage. Per crate, what that converts and what it
structurally cannot:

| crate | files in the coverage data | executed, headless union only | executed, with the composition ladder |
|---|---|---|---|
| `engine-ui` | 42 | 0 | 22 |
| `engine-render` | 28 | 0 | 0 - a hard wgpu link `web-viewer` does not carry; a spawned `play-window` run is that link and does report executed regions (see the scoping note above) |
| `engine-audio` | 20 | 0 | 3 - the page's SFX channel; the mixer output path has no producer in the union (see below) |
| `mdec` | 6 | 0 | 0 - the play page has no STR playback (its FMV arm auto-skips) |

Those two columns are the measurement that motivated the ladders below them, and
they are kept as that: each is a union over one named subset, not over the
canonical set. What the **full** union says about the same four crates is that
the exclusion is gone rather than narrowed - across all of them together, a
handful of anchors are left unentered, and every one is a specific routine
rather than a crate-wide blank. Re-derive it with `replay-port-coverage.py`
rather than from this table, which answers a different question.

Three more structural exclusions matter as much and are easy to misread as port
gaps:

**No `#[test]` can *call* into a `bin/` target - but it can still cover one.**
The call half is a real exclusion: `crates/engine-shell/src/bin/legaia-engine/`
holds the native window's whole composition layer and no integration test links
against it, so nothing there can be invoked directly.

The coverage half of that claim was wrong and is corrected here, because it is
the half that put rows on this page. `LLVM_PROFILE_FILE` is **inherited by
child processes**: a test that spawns `CARGO_BIN_EXE_legaia-engine` gets the
child's own profile written and merged into the same export, measured at 40
executions of a function whose only driver was such a spawn. So a `bin/`-
resident address is reachable by a ladder that *runs the subcommand*, and a
`bin/` row on this page names a fixture that could exist rather than a
structural impossibility. The browser hosts never had either problem: their
composition is in `crates/web-viewer/src`, a library, which is the seam the
composition ladder drives.

The spawn route is practical for the native window - `play-window` takes
`--pad-script` and `--screenshot-tick`, which is exactly that shape of run.
What it could not do was **enter a minigame**, and that had nothing to do with
`bin/` either: the native host opens every minigame from
`WindowEvent::KeyboardInput` (`K` dance, `U` dance how-to, `L` fishing, `O`
casino slots, `M` Muscle Dome, `B` Baka Fighter in
`window/event_handler/keyboard.rs`), as it does the fishing prize exchange
(`P`) and the inline-dialogue option picker, while `--pad-script` writes a
*pad word* and that handler never runs. No pad word names a minigame, so a
pad-only run could not open one however long it ran.

`--key-script` is that missing channel: `TICK:KEY` pairs delivered through the
same keyboard arms a player's keys reach, injected from inside the per-tick
loop. A scripted key that is *also* bound to a pad button becomes this tick's
pad word as well: the key arm sets the bit, its release on the next line
clears it, and the harness's neutral-pad write would then stamp the word to
zero - so a key-only script could arm a window toggle and nothing else. The
two scripts compose - keys open the surface, the pad plays it - and
`w5_native_minigame_ladder` (`crates/engine-shell/tests/`) is the ladder built
on it. It spawns one `play-window` per minigame and asserts on the **captured
frame**: each rung requires the PNG to differ from the same tick of the same
scene with nothing open, because a HUD builder that emits an empty draw list
passes any "did it run" check.

Two gates beyond the disc, both printed rather than inferred: the rungs need a
display (`play-window` needs a real wgpu surface even for its offscreen
readback), and a rung that opened a surface which painted nothing fails on the
frame comparison rather than on the exit status.

**No file move was needed**, and that is the reusable part. Moving the
composition layer out of `bin/` into a library module was the obvious fix for
a *call*-shaped exclusion, and this cluster never had one - it had a missing
input channel. A wide file move is the riskiest change available; the CLI flag
was one argument and one loop.

**The browser minigames page is outside the union entirely.** `minigame_replay`
drives the *engine-shell* minigame path, and `play_compose_ladder` drives the
*play page* - neither is the standalone minigames page, so a port wired only on
that page is never-entered by construction. Its own oracles live in
`crates/web-viewer/tests/`.

**No ladder in the union holds a `BgmDirector`, so the whole BGM route is
unreachable from it.** This is sharper than "the ladders have no audio device",
and it is two separate exclusions stacked. `critical_path_replay`,
`minigame_replay` and `play_compose_ladder` drive `SceneHost` / `LegaiaRuntime`
directly and never call `SceneHost::route_bgm_events` at all; the `BootSession`
ladders, which do reach `boot.rs`'s call site, every one construct their session
with `enable_audio: false`, so `BootSession::bgm` is `None` and the call site is
skipped. Whether a cpal device could be opened never enters into it. Every
`0x35` sub-op arm - the SEQ-byte resolve, the pause/resume pair, the volume
re-apply, the swap-commit - is therefore never-entered by construction, and the
same is true one layer down: nothing in the union attaches a sequencer, so the
`engine-audio` mixing path has no producer either. The session ladders
`crates/engine-core/tests/w1e_scene_bgm_transition_ladder.rs` and
`crates/engine-audio/tests/w1e_audio_session_ladder.rs` supply that producer -
the latter through `legaia_engine_audio::TestAudioSink`, the device-free twin of
the cpal mixing core (see the crate README) - and both export as their own
ladder JSONs. They are session-shaped rather than pad-driven, which the union
should keep visible: they measure "code a mixer-attached frame loop executes",
not "code a player pressing buttons executes".


**An anchor is attributed to its first site, and a data anchor has no site.**
The report emits one row per address carrying `sites[0]`, so the file and symbol
it names may not be the anchor that made the address `live`, and for a
multi-anchor address it is simply the first one the source walk found.
`8002174c` is the worked example: the row names `apply_morph_weights`, which the
liveness pass calls inert, while the address is `live` through the sibling
`MorphWeightEnvelope::tick` in the same file. Separately, a tag on a plain data
`struct` with no `impl` falls back to *module* scope - a file-wide verdict on
both the liveness and the coverage side, answering "does anything in this file
run", not "does this port run". The `mode.rs`, `sound_state.rs` and
`scene_bundle.rs` rows below were that shape until their tags were re-keyed
onto implementing functions. A tag above a `const` is not that shape: it
anchors to the item and resolves through references on both sides (the
const-anchor note under the buckets above), which is what gives `new_game.rs`'s
`GAME_STATE_COLD_RESET` a defined verdict.

**Two ways a tag ends up file-scoped, and only one of them looks like it.**
A `//!` tag is file-scoped by definition and reads that way. A `///` tag on a
data `struct` with no `impl` falls back, and reads like a type anchor. A third
way used to exist and is closed: the collector once stopped its forward walk at
a lookahead bound and did not recognise `pub const` as an item, so a tag above
a long doc block or a value item silently became file-scoped while looking
like a function tag. The walk is unbounded now and a `const` / `static` /
`type` alias is an anchorable item ([`port-catalog.md`](port-catalog.md)), so
a tag anchors to whatever it documents.
`crates/engine-core/src/cutscene_narration.rs` was the worked case for
`80037174` (tag at the foot of the module doc, first following line a
`pub const`); it sits on `pub struct CutsceneNarration`, which has an `impl`,
so the anchor is the type the port actually is.

The same fallback also produces **pseudo-entries** - an address the report
counts as *entered* whose routine never ran, because a module-scope anchor's
entry verdict is "any region in the file executed". Two measured cases, both
surfaced the first time a coverage source contained their files:

- the seven `engine-vm/src/lib.rs` addresses (the actor VM and its SCUS
  helpers) read entered under the composition ladder while the only executed
  function in that file is `Position::new` - the exact one-type import
  [`vm-inventory.md`](../subsystems/vm-inventory.md#ported-but-inert) names.
  The interpreter's coverage record is unexecuted; the HOST-DEAD verdict below
  stands.
- `801d603c` (the casino prize confirm painter) once reported as a
  **disclosed `NOT WIRED` anchor executed** - a red-flag row - because its
  anchor resolution fell back to module scope: the tag sits above a 60-line
  disclosure block, which the collector's then-bounded item lookahead never
  crossed. `choice_panel_draws_for`'s own record is unexecuted with no
  production caller in the workspace, so the disclosure was correct and the
  report row was the fallback. The unbounded walk anchors the tag to the
  function it documents, which dissolves the misreport; the row is kept
  because the *shape* - a thorough disclosure pushing its own anchor off the
  item - is exactly what a lookahead bound produces, and it accused the most
  careful disclosures first.


<!-- BEGIN engine-core -->

## `engine-core`

`engine-core` carries the largest crate share of the never-entered set. Every
address in it has a row below **except the ones a refresh has just added** -
see [the unverdicted rows](#rows-a-refresh-added-and-nobody-has-bucketed-yet),
which is where an address goes the day the export first reports it.

**Per-bucket totals are deliberately not written here.** They are a count of
project state, which this page keeps out on the same grounds as the rest of
`docs/`, and they are the fastest-rotting thing on it: every ladder that lands
moves rows between buckets, and a stale total reads exactly like a fresh one.

They are also the one part of the page that cannot survive concurrent editing.
Row verdicts are independent - two people revising different rows produce a
mergeable diff - but a total is a function of every row at once, so each
revision writes a different number to the same line and no arithmetic over the
diffs recovers the true one.

The totals belong to the instrument. `replay-port-coverage.py` recomputes them
from the coverage exports each run; the per-row verdicts below are what this
page is for, and they stay valid whatever the totals are.

### The escape pair reads as a contradiction, and both halves are true

`801e791c` is filed NO-LADDER below while the `engine-vm` table further down
says the flee roll is "ladder-covered
(`crates/engine-core/tests/battle_flee_ladder.rs`)". Both statements hold, and
the join between them is the denominator: only the test binaries named in
`CANONICAL_LADDERS` in `replay-port-coverage.py` are in the union, and a ladder
that seats the player by building a `World` by hand measures a different thing
however pad-driven the rest of its run is. **`CANONICAL_LADDERS` is the only
statement of membership**; this page deliberately does not restate it, because
a ladder joins the union by being added there and a copy here would go stale
the moment one is.

So "a pad ladder drives it" and "no run in the denominator entered it" are
different claims, and a row can satisfy the first while failing the second. For
such a row the fix is not a new fixture: it is promoting an existing seeded
oracle into the canonical set, or driving the same content from a ladder that
is already in it. Rows of that shape name their existing driver.

### HOST-DEAD, and what an anchor's scope was hiding

Every row here now carries a `NOT WIRED:` disclosure naming its own
prerequisite, and the way they got one is the reusable part.

Each was `live` for a reason that had nothing to do with the routine: its tag
was file-scoped, by one of the three routes above, and a file-wide verdict
answers "does anything in this file run", not "does this port run". Two
consequences, and the second is why the rows sat here:

- the address reads `live`, so `--live-audit`'s *undisclosed inert ports*
  section cannot see it, however dead it is;
- a truthful disclosure on it becomes **unwritable**, because the stale-tag
  test reads the same file-wide verdict and would report the disclosure as a
  false accusation.

The fix was a per-anchor re-key: each `PORT:` tag moved onto the function that
implements that address, with the disclosure on the same item. The data
descriptors the tags used to sit on keep a `REF:` pointing at the new home, so
the address is still findable from the shape it describes. Where no function
existed to key onto - `FUN_80020038`, whose port was a values-only `const` -
the missing half of the routine was written instead: `DrawEnvInit::stores`
carries the three *pair offsets* the values are stored at, which a value-only
descriptor drops.

| group | n | addresses | why |
|---|---|---|---|
| `cd_dma.rs` | 1 | `8003dda0` | **Re-verdicted `REPLACED-BY`** - see below. `ProtCdDmaHost` is constructed only inside `#[cfg(test)]` and the disc-gated `cd_dma_real_prot` test; no crate outside `engine-core` names `cd_dma`, and `overlay_loader`'s only non-test implementor is that same test-only host. Six sibling addresses have left this page - see below. |
| `stream_file.rs` | 2 | `800559ec` `80055ac8` | **Re-verdicted `REPLACED-BY`** - see below. `StreamFileHost` has exactly one production mention - its own `impl` line. Every construction is a unit test or the disc-gated `stream_file_real` oracle. Three sibling addresses have left this page - see below. |
| `mode.rs` | 4 | `80017978` `80025eec` `80025f2c` `80025f74` | Closed: `mode::ModeSeat` wraps `ModeDriver` and `engine-shell`'s `BootSession` owns one, driving it every frame; `World::tick` resolves `runs_master_frame_driver` (and so `per_frame_stage`) on every host. |
| `prize_exchange.rs` | 1 | `801dc1cc` | Closed: `World::try_arm_prize_exchange` (op-`0x49` sub-op 7) stages the session and `MenuRuntime::tick` drives it on both hosts; the koin1 interaction oracle `prize_exchange_disc.rs` reaches it from a real record. |
| `scene_name_sync.rs` | 1 | `8001d7f8` | `sync_scene_name` is called only from that file's tests. Two anchors share the address - the `fn` and a `//! PORT:` module tag - and it is the module tag that carries the liveness verdict, so both need the disclosure. |
| `save_select.rs` | 1 | `801e3294` | Closed: the same flow keeps a `CardIoMachine` for as long as a card screen is up and advances it through `card_frame_tick` each frame, with the poll status taken from the host's own block backend. |

Three of these name routines that are heavily used on the disc, so the gap is a
port that is not reached rather than a port of dead code. A five-form
[address-reference scan](address-reference-scan.md) puts `FUN_8003DE7C` at 127
`jal` sites spanning `SCUS_942.54` and eleven overlay images, `FUN_800558FC` at
four (two in SCUS, two in the battle-action overlay), and `FUN_80025EEC` in
twelve slots of the game-mode table at `0x8007078C` - every other entry, which
is the odd-indexed per-frame modes the port's own tag claims.

#### Ten of these rows stopped being wiring gaps without being wired

`replay-port-coverage.py --page-audit` joins every address this page cites
against the catalog, and reports the ones that are ported but no longer live -
because such a row belongs to `--live-audit`, not to a page about what a
playthrough executes. It found fourteen, and the split is the interesting half:

- **Ten carry `REPLACED-BY:`** - most of the `cd_dma.rs` / `stream_file.rs`
  cluster above, plus the battle party panel's label-actor lifecycle. That
  cluster is now exempt: a synchronous read through `crate::scene::ProtIndex`
  has nothing left to wait for, so the CD-DMA read's 127 `jal` sites are a
  heavily-used *retail* routine whose job the port does by construction, not a
  port nobody calls. The panel row is the same shape one layer up - retail
  registers a retained-mode SCUS text actor, and
  `legaia_engine_ui::battle_hud_draws_for` rebuilds every `TextDraw` from the
  live model each frame, so there is no handle to hold.
- **Four carry `NOT WIRED:`** - the `DRAW_ENV_INIT` values, the field
  load-entry plan, and the battle party panel's label build and cross-out
  mark. These are still the declared wiring worklist.

The distinction is not bookkeeping: a `REPLACED-BY` row is out of the wiring
denominator entirely, so counting it as a gap states work that will never be
done.

All fourteen addresses have since been **removed from this page**, which is
what the audit was asking for and not what it originally got: the first pass
wrote the finding up here and left the citations in place, so the next
`--page-audit` run reported the same fourteen and the section explaining them
was itself the reason they were still cited. A disclosed `NOT WIRED:` row is
`--live-audit`'s and
[`live-audit-triage.md`](live-audit-triage.md)'s; the addresses live in
`target/port-catalog/catalog.csv`, which is where a verdict should be read
from anyway. Re-run `--page-audit` before quoting any row in this section - it
needs no coverage export and answers in seconds.

One rename fell out of the re-key pass and is worth knowing about, because it
is a graph property rather than a style choice. `StreamFileHost::seek` was the
workspace's **only** in-tree definition of that name, and the call graph's
receiver gate deliberately declines to resolve a one-definition name - so every
`File::seek` in every crate linked to the retail seek shim and made it
reachable from a host root in **both** graphs. It is `seek_bytes` now.

Six rows stay permissively `live` after the re-key for the mirror of that
reason - a name with *many* in-tree definitions, where the permissive graph
keeps every edge the gate would drop. They are `read` and `close` in
`stream_file.rs`, and the four handler addresses now keyed to
`mode::per_frame_stage`, reached through a `.tick(` collision on `ModeDriver`.
The receiver-gated graph calls all six inert, which is what the stale-tag test
reads, so their disclosures stand; what is left is an over-count in the
permissive `ported + live` figure, not a contested verdict.

The reading to resist is that a disclosure retires the row. It does the
opposite: it moves the address into `--live-audit`'s *disclosed inert ports*
list, which **is** the declared wiring worklist, and each disclosure names the
one prerequisite that would let a wire be real rather than synthesised. For
`mode` it is a seat for `ModeDriver`, the port of the 28-entry mode table,
which the engine's hosts currently bypass entirely.

`cd_dma` and `stream_file` were read the same way here - "a production owner of
the trait / host type, which means routing the engine's loaders through them
instead of through `ProtIndex` whole-entry reads" - and the tree has since
answered that differently. Most of both clusters now carries `REPLACED-BY:
crate::scene::ProtIndex` rather than a `NOT WIRED:` disclosure: `ProtIndex` is
declared the *replacement* for the sector-DMA and streaming-read paths, not the
thing to route around. A `REPLACED-BY:` row is in neither half of the wiring
ratio ([`port-catalog.md`](port-catalog.md)), so those addresses are not owed a
host and are not reach work. The rows that keep a `NOT WIRED:` disclosure inside
those two files - the CD-DMA entry point, and `read` / `close` in
`stream_file.rs` - still are.

### A row can leave this page without a ladder reaching it

The page's own framing is that "a row leaves when a ladder reaches it or the
wiring lands", and there is a third exit that is neither: the **static verdict
moves**. Bucket (c) exists because a host-dead address still reads `live` in the
permissive graph, so `--live-audit` cannot see it; the moment a re-key, a
disclosure or a `REPLACED-BY:` makes the same address read *inert*, it becomes
`--live-audit`'s row and stops being this page's. Nothing about the runtime
changed - no ladder ran - and the row is still work, just filed where the
instrument that can see it lives.

Checking that exit needs no coverage export at all, which is worth knowing when
one is unaffordable - an export is a full instrumented build per ladder, and
there are dozens of ladders:

```bash
python3 scripts/ci/port-catalog.py --live          # writes target/port-catalog/catalog.csv
python3 scripts/ci/replay-port-coverage.py --page-audit
```

`--page-audit` joins every address this page cites against that CSV and names
the rows whose `live` column is now `0`, with the reason (`REPLACED-BY:` or a
`NOT WIRED:` disclosure). Run it *before* spending an export: a row that has
already taken this exit is not work a ladder can convert. The whole `cd_dma` /
`stream_file` replacement above shows up that way, as do the `sound_state` /
`scene_bundle` pair and the party-panel trio below.

### A third way a row closes: promote the oracle that already drove it

The exits above are "a ladder reaches it", "the wiring lands" and "the static
verdict moves". There is a fourth, it is the cheapest of the four, and the
page's own framing keeps hiding it: **the fixture already exists and is not in
`CANONICAL_LADDERS`.**

`crates/engine-core/tests/w1f2_field_vm_op_arms_disc.rs` is the worked case. It
walks every CDNAME scene's MAN carriers through the field-VM disassembler,
takes each arm's sites at decoded instruction boundaries behind the census
tools' own clean-resync run, and executes the carrying record in a real
`World` - which is exactly the fixture the `4C EA` and `4C 52` rows above
specified, written before they were written up. It was green the whole time
and exported nothing, because membership of the union is the list and not the
test.

So the first thing to do with an (a) row is not to design a fixture: it is to
grep the test corpus for the row's address. A test that names it is either the
fixture (promote it, in the same commit as the row's closure) or an oracle
that measures something else (say which, so the next pass does not re-check).
The cost of the mistake is asymmetric - a fixture written next to an existing
one is wasted work, while a promotion is one line.

Two cautions came with this one. A promoted oracle changes what the union
*means* when it is not pad-driven, so it belongs in the list with the L3
members' disclosure - record-seated, driven through the ordinary engine path -
rather than silently among the pad ladders. And a record-walking oracle counts
**sites**, not disc occurrences: the op-arm oracle reports two `4C EA` sites
against [the census's](#the-op-census-names-both-carriers) one coherent
occurrence, which its own per-arm cap of two makes an *at least*, and a scene
with two MAN carriers presents one record twice. The two counts are not
comparable without settling that, and neither number is wrong on its own terms.

### The mode-driver seat, and why `scene_mode` cannot be the bridge

"The hosts bypass the mode table" reads like architectural drift - the port
reimplements retail's top-level dispatch spine and then runs its own. It is
worth stating precisely what retail's spine is before proposing a seat,
because the disassembly settles two of the three questions the framing raises.

Retail's `main` (`FUN_80015E90`, `0x8001615C..0x8001620C`) is a mode-table
loop and nothing else:

```text
  mode = gp[0x524];  if (mode < 0) exit
  loop:
    handler = *(u32*)(0x8007078C + mode*24 + 0x10);  handler()
    if (gp[0x524] != gp[0x494]) { <mode-change edge> }
    if (gp[0x524] >= 0) goto loop
```

So the mode dispatch is genuinely the outermost level, the per-frame handlers
([`per_frame_stage`]) are the middle one, and the master frame driver
`FUN_80016444` is what a handler calls. The port has the middle level's *shape*
in `per_frame_stage` and the inner level wired to every host as `World::tick`
(tagged `FUN_80016444`, with the split spelled out at its own site). The outer
level is what has no seat.

**The seat is taken on both hosts, and two of the three blockers this page
named were gone before it was.** `engine-shell`'s `BootSession` owns a
`mode::ModeSeat` and drives it once per frame from `tick`; the browser play
page's `LegaiaRuntime` owns one too and reconciles it from its own frame path.
`docs/subsystems/boot.md`
([the port's seat](../subsystems/boot.md#the-ports-seat-at-the-mode-table))
carries the shape. What made it takeable:

- The lossy map is no longer lossy. `ModeDriver::tick`'s first act is
  `world.mode = self.scene_mode()`, and that resolves the `(GameMode, warp
  sub-id)` **pair** through `GameMode::scene_mode_with_warp`, so a running
  fishing / dance / casino / duel session is not dropped into `Title` on the
  frame a host takes the seat. The bridge this page called "a missing join"
  landed as `mode::WARP_SUB_ID_ADDR` plus that method, pinned at both ends
  (writer: the field VM's `0x3E` arm at `0x801E07B0`; reader: mode 24's init at
  `0x80025A14`).
- The direction that stays lossy is the other one, and the seat handles it
  rather than avoiding it: `ModeSeat::adopt_scene_mode` stages the sub-id
  alongside the word whenever the scene mode it is adopting maps to
  `OTHER MODE`.

The Muscle Dome sentence on this page over-read its own evidence and is
corrected here. `dome_leg_ends_on_ko_real` asserts that the **arena overlay's
one game-mode store** writes `BattleInit` - that is a dome *round* handing off
to an ordinary battle, which is the negative result the test exists for. The
dome *hub* is PROT 0977, warp sub-id `5`, entered by mode 24 like the other
four; "not `OtherMode` at all" is true of the round and false of the ladder.

**The mode-change edge is wired**, and it is the half with observable
behaviour. Retail's `0x800161B8..0x80016200` runs a fixed sequence on every
transition: the CD read-wait poll `FUN_8003DE7C`, an overlay wait
`FUN_8003ED04`, the mode-transition routine `FUN_80016230`
(`engine-render::mode_transition`), `FUN_80058104(0)`, the pad-report
re-publish `FUN_8001822C` (`engine-core::input::set_pad_reports`), and clears
of `gp+0x3D8` (the frame-begin-skip flag), `gp+0x538`, `0x8007B938` and
`gp+0x55C`. The seat performs the two pad clears - through
`InputState::clear_edges`, so the button that caused a transition is not
re-delivered as the first input of the mode it opened - and the frame-begin-skip
clear. The pad half is applied only to a transition a host performs
synchronously, because the port publishes the frame's pad *before* the tick
where retail polls it *after* the edge; the boot page carries that ordering
note. The rest are device-layer calls the port replaces.

Two readings of that block are corrected while it is being cited: it holds
**four** clears, not three (`0x8007B938` at `0x800161F4` sits between
`gp+0x538` and `gp+0x55C`), and the `gp+0x564` / `gp+0x494` stores that close it
are *copies of the new mode word*, not clears - `gp+0x494` being the
previous-mode cell the loop's own `bne` compares against.

**Holding a seat is not the same as driving it**, and the difference is
invisible in the word. Both hosts also **enter** two INIT modes by hand -
`MAIN INIT` at field entry and `CARD INIT` on the pause-menu open - through
`ModeSeat::enter`, which resolves the INIT column's plan and hands the word to
the mode's RUN sibling before it returns. So an INIT mode never appears in a
sample taken after the call, on either host, and a host that reached
`MAIN MODE` by letting `adopt_world_mode` follow the world's scene mode walks
the **same chain of words** as one that entered `MAIN INIT` first. What it
does not do is take the extra mode-change edge, which is what
`engine-shell/tests/mode_seat_host_parity.rs` compares: it drives both hosts
over one ladder (field entry, menu open, menu close) and asserts the chains
**and** the edge counts match.

**What is still owed**, now that the seat exists:

- the residency model. `ModeSeat::enter` resolves the INIT column's staging
  plan and performs each mode's own hand-off store, so `mode_init_stage`,
  `other_warp_init_stage` and `mode_init_bare` are walked - but nothing loads
  an image at a base and calls the entry the plan names, which is
  `crate::overlay_loader`'s gap, not the seat's.
- the frame-begin skip's *producer*. `World::clock.frame_begin_skip` now has a
  consumer on a host (the seat clears it on every edge and `ModeDriver::tick`
  reads it), and still no writer outside tests, so the "a skipped frame runs no
  frame-end pass" law remains unexercised end to end.

**One of the laws in that unreached shape is a live gameplay divergence**, and
it is the reason the family is worth more than its four rows. Mode 23 CARD is
what every menu-open capture holds, and `per_frame_stage` records that its body
replaces the master frame driver rather than parameterising it. The
disassembly is unambiguous: `FUN_80025F74` is frame-begin -> `FUN_80017978` ->
frame-end, and `FUN_80017978` (`0x80017978..0x800179BC`, eighteen instructions)
calls the debug chord, the card actor's `+0x0C` handler and the dev HUD - there
is **no `jal 0x80016444` in it**. So retail runs no actor tick pass, no render
pass and no display flip while the pause menu is up.

That law is now in `World::tick`: it resolves
`mode::runs_master_frame_driver` off its own `SceneMode` and suspends the
effect pool, the move VMs, actor physics, the handler-actor pass, the actor
motions, the banners, the narration roller, the text balloon, the register
ramps and the scripted countdown when the answer is no. What is left is a
host-side asymmetry in the *other* direction: neither shipped host ticks the
world at all while its menu owns the frame, so both freeze more than retail
does - retail keeps the CARD handler's frame-begin and frame-end passes
running, and those are where the timed sound release and the cadence resolver
live.

### HOST-DEAD, disclosed

Same verdict, already stated in the source. No further disclosure work; they are
listed so the bucket count is the whole of what no host reaches.

| group | n | addresses | why |
|---|---|---|---|
| `card_bu_io.rs` | 4 | `801e0598` `801e3d68` `801e380c` `801e435c` | **`REPLACED-BY`** `legaia_save::emu::CardView` + `legaia_save::card` - out of the wiring denominator, not owed a host. |
| `cutscene_script_elements.rs` | 2 | `801d5d60` `801d6058` | The seat exists now - `World::tick_cutscene_elements` runs the channel from the frame tick on both hosts - but nothing in production **spawns** an element into the pool, so a replay still enters none of the three `step` bodies. |
| `shop.rs` | 2 | `801db7f4` `801dbd94` | Retail's quantity **steppers** - not a list, and not the engine's list screen. |
| `camera_rel_glide.rs` | 1 | `8002149c` | No producer for the family's 20-halfword spawn record. |
| `card_flow.rs` | 1 | `801e13b8` | **`REPLACED-BY`** `legaia_save`'s synchronous card writer - out of the wiring denominator. |
| `effect_ribbon.rs` | 1 | `801cfa48` | The only production mention of the module is its `pub mod` line. |
| `field_save_screen_actor.rs` | 1 | `80024190` | **`REPLACED-BY`** the save screen as host screen state - out of the wiring denominator, not owed a host. |
| `scene_transition_actor.rs` | 1 | `80021934` | Scenes load as `Scene` resources, not as a streamed raw bundle, so nothing seats the actor. |
| `morph_weight_apply.rs` | 1 | `8002174c` | Both anchors disclosed; `MorphWeightEnvelope` is test-only. |

Two groups have left this table by being **entered**, and neither was wired to
get there. All eight `save_subscreen.rs` bodies run - three from
`SaveScreenFlow`'s own card-rack ticking and five from `w1g_save_subscreen_ladder`,
whose gate was the entry context rather than a pad stream (see
[below](#the-five-sub-screens-were-behind-an-entry-context-no-host-constructs)) -
and `world/field_movement.rs`'s `800467e8` runs from the driven walks. The
wiring claim underneath the first is unchanged: no host opens a shop through
that machine, which is `--live-audit`'s row and not this page's.

#### Per-row wiring verdicts for this table

Five of these rows are the ones a drain pass is most often pointed at, and the
answer for all five is the same shape: a named **prerequisite capability**, not
a missing call. Recorded per row so the decision is not re-derived:

| rows | verdict | the capability that is missing |
|---|---|---|
| `card_bu_io.rs` x4, `card_flow.rs` | already `REPLACED-BY` | nothing - the tree re-verdicted these; a drain pass that re-opens them is counting work that will never be done |
| `shop.rs` `801db7f4` `801dbd94` | resolved | `MenuRuntime::quantity_session` installs the retail stepper when a list stages a stack and takes the pad for the screen; `ShopConfirm` is no longer reached from the shop flow. See [shop.md](../subsystems/shop.md#the-quantity-screen-is-a-stepper) |
| `morph_weight_apply.rs` `8002174c` | keep `NOT WIRED` | a spawn site allocating a morph actor from descriptor `0x8007068C`, so something carries `actor+0x4C` / `actor+0x90` |
| `effect_ribbon.rs` `801cfa48` | keep `NOT WIRED` | a producer emitting actor render-mode-4 primitives; the module has no entry point until one exists |
| `scene_transition_actor.rs` `80021934` | keep `NOT WIRED` | a staged-bundle scene loader - a host that parks a raw `.LZS` bundle where the descriptor walker reads it. The engine's `Scene` resource load is not a replacement for the *sequencing*, which is visible (the fade-out / countdown / MAIN INIT order) |
| `field_save_screen_actor.rs` `80024190` | **`REPLACED-BY`**, taken | see [the verdict below](#the-save-screen-actor-is-a-replacement-not-a-wire) |

#### The five sub-screens were behind an entry context no host constructs

`SaveScreenMachine::new` takes a `SaveEntryContext`, and the context decides
which sub-screen the flow opens on. `SaveScreenFlow` - the only production
constructor in the workspace - passes the **card** context and nothing else, so
four of retail's five contexts have no host at all, and every sub-screen behind
them is unreachable however a pad is driven.

`w1g_save_subscreen_ladder` takes the chain that is a real retail entry:
`SaveEntryContext::ShopEntry` is the op-`0x49` record's own kind byte
(`0x00`, `0x801DC89C`) and opens on the mode select, whose Sell row walks to the
quantity spinner and back and whose Quit row walks to the terminal screen and
exits the flow. Three bodies, by pad, through the dispatcher. The other two
(`tick_pad_release_wait`, `tick_party_picker`) have no predecessor this module
ports, so the ladder writes the screen id - which is what retail's own
transition is, a store - and then drives each body by pad to both of its exits.

Each rung asserts the body's observable product: the screen it moves to, the
exit code it writes, or the effect it asks the host to perform. What the
conversion does **not** establish is the wiring: no host constructs
`ShopEntry`, `PostSave`, `CasinoPrizeCounter` or `DebugParamEditor`, and that
gap is unchanged.

#### The save-screen actor is a replacement, not a wire

`80024190` was the page's closest call, held open by the disclosure's own
alternative ("a deliberate decision to model the swap latency"). It is settled
against bytes now, and the swap-latency option has no consumer: the engine's
own mode-seat entry asserts that CARD INIT stages no overlay request.

The routine is reached only as a function pointer - a five-form sweep of the
disc returns one reference, the handler word of the actor template it is the
`+0x08` of, and no `jal`, `j`, branch or `lui` pair anywhere. Its eleven states
order three things: two overlay loads through `FUN_8003EBE4` plus the five
queue waits that poll them, one slot-B image pick, and one call into the save
UI. `overlay_loader` already carries `FUN_8003EBE4` as a replacement (on-demand
PROT resolution), so eight of the eleven states sequence a mechanism the tree
has already ruled nobody is owed; the engine resolves a PROT entry when it
needs the bytes and has no RAM window to page into, which also means there is
no mid-swap frame for the cover fill to hide.

What remains is the mode bounce (states `0` and `10`) and the UI (state `4`),
and both are live Rust: `BootSession::open_field_menu` writes `SceneMode::Menu`
and enters `GameMode::CardInit` through the mode seat, then
`FieldMenuSubsession::build` opens the Save / Load row on the running field
session and `SaveScreenFlow` runs the card half. Both shipped hosts own that
flow.

The decoded state walk, the cover-fill depth arithmetic and the jump table stay
in the module as the measured spec of the retail beat - a replacement retires
the wiring obligation, not the RE.

`800467e8` is the one to read before proposing a wire. `remap_pad_direction` is
a faithful port of the retail 45-degree camera-relative pad remap, and the tag
declines to route the live pad path through it because the two implementations
agree on every even `rot` - that is, on every camera a retail field scene
installs. Wiring it would be a provable identity, which is worse than the gap.

### NO-LADDER, harness-blind

Wired, and reached in real play on a host the ladder harness cannot execute.
The composition ladder converted the rows whose host is the browser *play
page*; `w5_native_minigame_ladder` converted the rows whose host is the native
window; `w1l4_page_compose_ladder` converted the browser cards page's
directory walk and the dance page's sting rows; `w2b_dialog_picker_ladder` and
`w2b_fmv_handoff_ladder` converted the two native rows whose blocker was scene
content rather than an entry point.

| group | n | addresses | host |
|---|---|---|---|
| `other_game_overlay.rs` + `engine-audio::sfx.rs` | 2 | `801d1288` `80065034` | the Muscle Dome **INTERVAL tally**, with a live audio device. One gate, not two: `arena_voice_cue` resolves a per-lane voice attr off `ScoreTallyRamp::tick` and `key_on_voice_attr` is the only thing that takes it, and the two callers of that pair are the native window's minigame side-channel (a `bin/` target, so only a spawned `play-window` rung reaches it - and `w5_native_minigame_ladder` has no dome rung) and the standalone browser minigames page, which is outside the union. No ladder mentions `ScoreTallyRamp` at all |

One row moved rather than converting. `801d5510` (the shop's buy-quantity
panel) is no longer a *host* gap: the missing native window-35 painter that
its column called for is written, so the kernel is a both-hosts panel and
what enters it is either host driven to the buy-quantity phase. It sits in
[the `engine-ui` table](#engine-ui) with its history, which is worth reading -
the column was wrong twice, and the second correction was a wire.

### Converted by `w5_native_minigame_ladder`

Everything the native window paints for a minigame. Each was dark for the same
reason - `--pad-script` cannot open a surface the keyboard handler owns - and
all of it executes under one ladder now.

| group | n | addresses | what runs it |
|---|---|---|---|
| `dance.rs` (HUD + banner) | 7 | `801d231c` `801d3e28` `801d32f8` `801d2524` `801d2d98` `801d2f38` `801d387c` | `40:K`, then judged face-button presses |
| `fishing_chrome.rs` | 6 | `801d03b0` `801d78c0` `801d74b0` `801d7a5c` `801d70ec` `801d7c30` | `40:L` + a cast; the venue panel needs `P` |
| `fishing_actors.rs` | 4 | `801d2050` `801d765c` `801d2278` `801d4948` | the same run's wander / line / celebration actors |
| `minigame_floor.rs` | 2 | `801d2a10` `801d6028` | the fishing venue's floor solve |
| `baka_fighter.rs` (digit strips) | 3 | `801d6a18` `801d6f44` `801d69e4` | a duel played to a **player win** - a lost match installs no tally and two of the three stay dark |
| `dance_tutorial.rs` | 1 | `801d0750` | `40:U`, the Disco King how-to |
| `slot_machine.rs` | 1 | `801e6f70` | `40:O` - the empty coin bank sends the entry through the exchange counter |
| `fishing.rs` (prize row remainder) | 1 | `801d092c` | a **committed** prize purchase; the panel alone stops one gate short |
| `bin/.../window/field_render.rs` | 1 | `8001ada4` | any spawned `play-window` frame loop |

Two of these are worth reading as a pattern rather than as rows. `801d6a18` /
`801d6f44` and `801d092c` were not blocked by the entry at all - the duel HUD
and the prize panel were both plainly on screen while they stayed dark, because
the drawers sit behind a *won* match and the cap behind an *afforded* row. A
rung that reaches the screen is not a rung that reaches the code on it, and a
screenshot cannot tell the two apart.

### NO-LADDER, content not driven

Wired through `engine-core` and reachable by a headless ladder in principle -
these were the rows a new or deeper fixture would convert. (The composition
ladder converted the battle-HUD row bake, the dialog-atlas bake, a `4C 60`
stamp, a scripted CLUT-cell arm and the effect-script reader; the rows below
are what it did not reach.)

All but one are closed now, and the `reach` column is kept rather than deleted
because what closed each one is the useful part - the note under the table is
about the cells that outlived their own fixtures.

| group | n | addresses | what reaches it |
|---|---|---|---|
| `screen_fx.rs` | 10 | `801de4c8` `801f8d4c` `801f811c` `801f8004` `801f7a9c` `801f88fc` `801f8e6c` `801f849c` `801f8f28` `801f8a34` | Closed: `chapter1_frontier_ladder` enters all ten - a scene whose script spawns an iris mask, letterbox or image panel is exactly what its scene walk drives. |
| `fishing.rs` (session kernels) | 6 | `801d5298` `801d0474` `801d0f5c` `801d26cc` `801d3db4` `801d746c` | Closed: `w1f1_fishing_pond_ladder` casts, matches the reel cadence and lands a catch (the point credit, the band roll, the species spawn, the cadence), and `w1g_fishing_tackle_pick_ladder` adds the two picker screens it could not reach - see the note below on why a second ladder was the only way |
| `muscle_dome.rs` | 4 | `801cf074` `801d1184` `801d1510` `801d9bbc` | Closed, and `801d9bbc` closed **elsewhere**: the export enters it through its second anchor, `engine-vm::battle_value_readout`'s per-handle step, which the battle numerals now run on both hosts. Its `muscle_dome.rs` anchor still has no producer, and an address-level verdict cannot say so - see the anchor note above |
| `baka_fighter*.rs` (tally + intro) | 4 | `801d6710` `801d239c` `801d2a28` `801d59d4` | Closed: `w1b_baka_duel_ladder` plays the duel from its intro card to a player **win** and drains the tally. The door entry is a separate rung and still arms neither |
| `pause_screens.rs` (special Use) | 4 | `801d7e50` `801d8a58` `801d8b90` `801d8d94` | Closed: `w1f1_pause_special_use_ladder` seeds the bag (no `debug_` helper grants `0x88` / `0x89` / `0x8A`) and drives Door of Light's confirm and Door of Wind's destination pick to their commits |
| `other_game_overlay.rs` | 1 | `801d14b0` | Closed by delegation: `baka_fighter::tally_drain_step` (`801d6710`) **is** `other_game_overlay::step_scale`, one routine linked twice, so the duel ladder's tally drain enters the anchor. The arena's own driver is still one call away |
| `battle_tutorial.rs` | 2 | `801f6b70` `801f747c` | Closed: `w1f1_battle_tutorial_ladder` primes the script, walks into a real encounter and drives the box; `training_battle` was never the only route |
| `world_map.rs` | 2 | `800196a4` `801d8258` | Closed by `w1d_world_map_render_ladder` - rung 2 taps L1 on the overworld and runs the fade ramp to its mode-12 hand-off, and the horizon-gate rung arms the emitter through `World::tick`. Read the second with its own caveat (see below) |
| `fog_particles.rs` | 3 | `8003f348` `8003f3fc` `8003f86c` | Closed by `w1h_fog_page_prims`, the one *composing* member of the union - a headless fixture cannot convert these at all, whatever scene it walks; see [the composing-host note](#a-render-pass-row-needs-a-composing-ladder-not-a-deeper-one) |
| `cutscene_narration.rs` | 1 | `80037174` | Closed: `w1a_narration_ladder` drives the opening-prologue subtitle roller |
| `world/narration.rs` | 1 | `8003cf7c` | Closed: the same ladder drives the inline field-VM conversation path, as opposed to the pre-decoded dialog panel every other ladder drives |
| `world/battle/stats.rs` + `battle_formulas/escape.rs` | 1 | `801e791c` | Closed: `battle_flee_ladder` is a canonical member now, and its first rung is an **assured** escape that leaves the battle |
| `fade.rs` | 1 | `80020b00` | Closed with it: `victory.rs`'s `BattleEndCause::Escaped` arm loads `escape_fade_template()` on the teardown that same rung reaches |
| `world/vm_hosts.rs` (op `4C EA`) | 1 | `8003c7ec` | Closed, and by the third exit rather than by a new fixture: `w1f2_field_vm_op_arms_disc` already drove the op from the disc's own bytecode and was simply not in `CANONICAL_LADDERS`. See [the promoted-oracle note](#a-third-way-a-row-closes-promote-the-oracle-that-already-drove-it) |
| `equipment.rs` + `world/vm_hosts.rs` (op `4C 52`) | 1 | `800430ac` | Closed by the same promotion, and it is the sharper of the two: the op's **fallback** leg only runs when the bag misses, so the row was gated twice over, and the oracle drives the same real instruction under both bag states rather than once. See [the promoted-oracle note](#a-third-way-a-row-closes-promote-the-oracle-that-already-drove-it) |
| `publisher_logos.rs` | 1 | `801cefd4` | Closed: `w3c_boot_logos_ladder` (`crates/web-viewer/tests/`) opens the logo phase on the browser play page and steps the sequencer to its end on a neutral pad, then again with Start. It is the only union member that starts **before** the title card, which is why one rung was enough |
| `menu_arrange.rs` | 1 | `801d64a8` | Closed: `w1f2_menu_depth_ladder`'s Items rung picks command-window row 2. It named that step before it reached it - the rung drove Arrange on a bag its own Throw Out leg had emptied, so retail's buzz-on-empty dispatch swallowed the confirm; Arrange runs first now |
| `save_subscreen.rs` (sub-`0x15` list source) | 1 | `801da2a0` | Re-verdicted **(b)**: the reorder page opens off the Status screen's confirm and `ListOrderSession::open` rejects an empty list, so a rung is not the gap - see [the gate table](#gates-behind-the-b-rows) |
| `ui_menu_window_painters.rs` (window 37) | 1 | `801d5944` | Closed: `w1f2_menu_depth_ladder`'s sell rung drives the root picker's Sell row to the quantity screen. It was a pure reach gap on **both** hosts - `sell_quantity_draws_for` has a `play_shop.rs` and a `window/shop_windows.rs` call site, each filtering window id 37 - and the union's other shop rungs all buy |
| `ui_menu/records_screen.rs` | 1 | `801ed710` | Closed: `play_compose_ladder`'s dev-menu rung taps Square for the Records page and asserts it drew, which is the one row selection the cell asked for |

Every row in the table is converted but one, and that one left as a **gate**
rather than as a fixture (`801da2a0`, the reorder page). Most of the rest were
converted by ladders that already existed and were already canonical while the
table went on naming the fixture each one needed. A coverage export over the full canonical union confirms it at the
level the rows are written at - **every address in the table that a headless
ladder can reach at all is entered**, the last two being the fishing menu pair,
where the cell claimed a screen the ladder never opened until one was written
for it. The `fog_particles.rs` row was the last exception and closed last,
through a different *kind* of fixture rather than a deeper one - see the note
below it. That is worth more than the row count, because it is the failure mode
this page is most exposed to: **a row's `reach` cell is a claim with no
instrument behind it.** `--page-audit` checks the address column against the
catalog and says nothing about the prose; nothing checks that a cell still
names work. The cheap guard is to re-read a cell against
`CANONICAL_LADDERS` before quoting it - membership is one grep - and the
expensive one is the coverage export, which is what actually moves a row.

Two of them also close *differently* from the way their cell predicted, and
both distinctions survive the row:

- `801d14b0` is not a second routine. `FUN_801D6710` (the Baka tally drain) and
  `FUN_801D14B0` (the PROT 0977 hub overlay's copy) are twenty-four
  instructions each and agree opcode for opcode, differing only in the
  `lui`/`lw` pair that loads the bypass flag and in the relocated branch
  targets, so the port holds one implementation and the Baka entry delegates
  to it. A ladder that drives either drives the anchor.
- `801d8258`'s horizon params are the *ladder's*, not the disc's. What its
  coverage proves is the chain `World::tick` -> `tick_world_map` -> the arm ->
  the emitter body; whether any shipped scene sets those globals is a separate
  question the ladder deliberately does not answer, and its own test name says
  so.

#### A render-pass row needs a composing ladder, not a deeper one

The fog emitter is the row that separates "no fixture drives this content" from
"no fixture of this *kind* exists", and the two read identically in the
never-entered set.

Its three addresses sit under `World::fog_render_step`, which is a **render
pass** call: the native window makes it in `redraw_passes.rs` and the browser
page in `play_field_fx.rs`, and nothing else in the workspace does. A headless
ladder can walk the scene that raises the gate and still never touch them,
because it composes no frame - so "drive a deeper scene" is the wrong fixture
however far it goes.

The gate itself is not the gap. `_DAT_8007B854` is raised and cleared by the
field-VM `0x4C` nibble-3 pair, and the [op census](field-op-census.md) finds
both arms widely carried - `[4C 30]` and `[4C 31]` each land in dozens of
scenes, `town01` among them, which is the scene both composing ladders sit in.
What the pool still needs is a spawn: `FogPool::spawn` (`FUN_801D629C`) runs
from the cutscene element path, so the frame the row wants is one where the
gate is up *and* the pool is populated *and* a host is drawing.

So the convert is a rung on a composing ladder - `play_compose_ladder` or a
`play-window` spawn - parked in such a frame, and the useful generalisation is
that a row whose only callers are render passes should say so in its cell:
otherwise it reads as content the next headless fixture will pick up, and no
headless fixture ever will.

**The rung existed and was not in the union.** `w1h_fog_page_prims`
(`crates/web-viewer/tests/`) drives the browser play page's per-tick
screen-prim assembly - `tick_frame` -> `tick_field_fog_prims` ->
`World::fog_render_step` - over the first scene whose scene-controller record
raises the gate **unconditionally**, re-derived from the same walk the native
census `w1h_fog_gate_census` uses so the two oracles cannot pick different
scenes. It asserts that the page's screen-prim pass carries at least as many
primitives as the pool drew quads and that the uploaded geometry is non-empty,
which is the composing half; the pool's own spawn comes from the scene's
cutscene element path rather than from the fixture. So the three addresses
convert through a ladder that was written, green and unlisted - the
[fourth exit](#a-third-way-a-row-closes-promote-the-oracle-that-already-drove-it)
again, and on the one row the page had called structurally open.

The row is worth keeping for the part that did **not** change: none of this
made a headless fixture work. `w1h_fog_page_prims` converts the rows because
it composes, and the rule the cell now states - a row whose only callers are
render passes needs a drawing host - is what made "which fixture" answerable
at all. What the page got wrong was the second half of the sentence, that no
such fixture existed: the search for one has to include the ladders that are
green and outside `CANONICAL_LADDERS`, because that set is where a fixture
sits between being written and being counted.

#### The op census names both carriers

`8003c7ec` and `800430ac` are the first rows on this page whose bucket turns on
a question about the **disc's own bytecode** - does any shipped scene carry op
`4C EA` / `4C 52` at a decoded opcode boundary. Nothing here could answer it:
`legaia-engine man-scripts` has a disc-wide walk with the field-VM
disassembler behind `--system-flag-census`, `--motion-flag-census` and
`--op49-window-census`, but each reports one *family* of ops. A byte scan is
not a substitute - that is the false-positive trap the scripted-encounter hunt
already records, where every `0x37` / `0x41` byte in dialog text reads as a hit.

[`field-op-census.md`](field-op-census.md) is the missing instrument, keyed on
an arbitrary opcode and broken out by sub-arm, and it answers both rows:

- **`4C EA`** has **three** coherent occurrences disc-wide, one per kingdom
  world-map bundle (`map01` / `map02` / `map03`); the `map03` one, partition 2
  record 9, is the reference shape - a named scene change, a self-looping
  jump, the op, then a wait and another self-loop. A scripted hand-off that
  never returns. (An earlier census counted one: its walk ended at each
  record's first text segment.)
- **`4C 52`** has **seventy**, across twenty-five scenes: it is the chest
  script's item consume, one `0x1F` line into every record that carries it.
  (The census first counted three - `geremi` and the two `ropeway` variants -
  the only sites a walk that ended at the first text segment could reach.)

Both rows are closed now - by
[promoting the oracle](#a-third-way-a-row-closes-promote-the-oracle-that-already-drove-it)
that was already driving both arms off the disc corpus - and the census is
what made them specifiable: before it, neither row could say whether a carrier
existed at all. The `4C 52` row keeps its second gate as well - the op
only falls through to the unequip leg when the bag misses, so the fixture has
to put the item on a character rather than in the bag.

The general rule the census adds to this page: **a clean count of zero means
no shipped scene reaches that arm through the field VM's own bytecode**, which
moves a row out of `(a)` NO-LADDER and towards `(d)` NOT-PLAYTHROUGH - there is
no fixture to write. A non-zero count names the scene to write one against.

A third row is in that position and is left at `(a)` deliberately. The
field-actor timers ask for a script issuing `0x43 0C` or `0x43 09`, and the
census finds **zero** coherent occurrences of either - but 31 incoherent ones
across six carriers, every one inside a record the walk had already desynced
in. Both sub-ops are sized by the decoder, so the census is not structurally
blind to them; what it cannot rule out is an occurrence hidden behind an
earlier desync in its own record. Reading those 31 is what would settle the
bucket, and until then the row's premise has no evidence behind it and its
fixture cannot be specified.

#### The fishing pair: a precondition is not a screen, and the row was owed both

`w1f1_fishing_pond_ladder` passes the lure and the rod to `PondSession::new`,
so nothing in it moves a cursor, refuses an unowned lure row or walks past an
unowned rod slot - which is why two kernels the row counted stayed at zero
however deep the pond run went. `w1g_fishing_tackle_pick_ladder` takes the
other denominator: it drives both screens by pad over a tackle bag and then
opens the pond on **what they returned**, so venue 0's band-4 preconditions
(Normal lure, third rod) arrive from the picks rather than from constructor
arguments, and the hooked species is the one the picked lure's own spawn row
names.

The row converts and the wiring question does not, which is the distinction
[the Seru-capture note](#gates-behind-the-b-rows) draws: **no host owns either
screen.** The native window and the browser play page enter the pond directly
and take the rod from a dev constant; the standalone minigames page takes rod
and lure as `fishing_pond_start` arguments and models no tackle inventory at
all, so `RodLureSelect`'s owned-count probe has nothing to count. Both kernels
say so in their own doc blocks. Read the converted row as reach, and the host
gap as still owed.

Five rows left this table through the scene-session ladder
(`crates/engine-core/tests/w1e_scene_bgm_transition_ladder.rs`): the four BGM
plumbing addresses (`80019898` `800243f0` `800266e0` `80026520`) and the
scripted CLUT-cell cross-fade arm (`801e4c58`). The BGM four needed a
`BgmDirector` more than they needed a scene - see the structural exclusion
above - and driving the sub-ops the scenes' own MANs carry, in the order a
transition performs them, is what makes the pause/resume pair falsifiable
rather than four independent hook calls.

Driving them surfaced a wiring gap the reach number cannot show, because the
gap is on the far side of the trait. `BgmDirector::reattach_volume` has a
default no-op body, and **neither** rendering host overrides it -
`AudioBgmDirector` implements `pause` / `resume` / `stop` / `unhalt_pause` and
not this one, and the browser runtime's director matches. So sub-op 8 computes
retail's level (`FUN_80019898`'s `(raw << 15) >> 16`) and every host discards
it. The primitive it would drive already exists
(`legaia_engine_audio::Sequencer::set_master_vol`).

**That two-host wire is declined with proof**, and the proof is on the half the
gap reading did not look at: the level. `FUN_80019898`'s source global
`DAT_8007B6EC` has exactly two writers anywhere on the disc - `FUN_8001DCF8`
(`0x8001DEC4`) and `FUN_8001FFA4` (`0x80020004`), the two cold-reset leaves that
clear the `0x80084140` game-state block - and both store `li v0,-0x1`. Nothing
else writes it, so it holds `-1` for a whole retail session, and
`bgm_reattach_volume(-1)` is `-1`. Its only other reader, the load-settle stage
`FUN_800243F0` (`0x80024804`, `sra a0,a0,0x1`), takes the same halving of the
same constant. So sub-op 8 can never change the volume relative to what the
track's own load already applied: a host that implemented `reattach_volume`
would re-apply one compile-time constant, which is unobservable by
construction. Same shape as `remap_pad_direction` below - wiring it would be a
provable identity, which is worse than the gap.

What would make the channel real is the missing *writer*: the engine's
`SceneHost::bgm_volume_raw` likewise has no production writer either, so both
sides of the level are constants today. Port a writer first, then the hook has
something to carry.

Reachable only from a game state the ladders do not seed. The fix is a seeded
save or a longer spine, not a pad stream.

| group | n | addresses | gate |
|---|---|---|---|
| `world/battle/casting.rs` | 2 | `801dd4b0` `801dd6b4` | Closed - `w1c_capture_class_cast_ladder` seeds one disc monster record's magic list and lets the monster AI pick the cast, so the route is `World::tick` -> the action SM -> the fold. See [below](#the-two-capture-wrappers-and-why-only-one-of-them-shows-up-in-a-damage-contrast) |
| `world/battle/capture.rs` + `battle_formulas/victory.rs` | 1 | `801e70bc` | Closed - `seru_cast_magic_xp_ladder` seeds the spell and is a canonical member now. The gate sentence was written while it was not, and the row outlived the ladder |
| `magic_xp.rs` | 1 | `801e92dc` | Closed - `w1g_seru_capture_ladder` casts the capture spell in a live fight, lands the roll on a weakened monster and lets battle teardown reach the record-side commit. The row had been satisfied for the wrong reason first; see [the gates table](#gates-behind-the-b-rows) |

##### The two capture wrappers, and why only one of them shows up in a damage contrast

The in-crate oracle the old row cell named can never be a union member - it is
a `#[cfg(test)]` module, and `CANONICAL_LADDERS` takes `--test <name>` binaries.
It is also a different route: it calls `World::cast_spell_on_slots` directly,
while every seam between a host and the fold (`arm_monster_cast`,
`fold_pending_cast`, `cast_spell_on_slots_prepaid`, `enemy_move_predamage`) is
`pub(in crate::world)`, so the only public way in is `World::tick`.

Driving it that way measured two things the oracle's arithmetic form could not.

**The guard-respecting wrapper is invisible to a damage contrast.** Running the
same fight twice - once with the disc spell table installed so the class byte
routes the hit, once without it so the same hit takes the shared kernel
`FUN_801DD0AC` - separates the resist-bypass wrapper `FUN_801DD6B4` cleanly
(measured 972 HP against 311 on the same seat, on move `0x37`). It does **not**
separate `FUN_801DD4B0`: on the same stat bridge and the same draw stream the
respect wrapper and the shared kernel produce the same number (measured 251
against 251, on move `0x36`), which is what `battle_damage_wrappers`' own note
about "identical arithmetic to the shared kernel's defender roll" amounts to
end to end. So a contrast rung for that arm has to assert the **routing** - the
band module resolves on the routed half and does not on the other - rather than
the magnitude.

**The class byte steers two decisions, not one.** The routed half reaches the
fold 21 frames later than the shared half, because the record's `+0x00` class is
read by `is_capture_class_move` *and* by the action-seed band pick
(`legaia_engine_vm::battle_action`'s `action_seed`, which compares the same byte
against `0x14`). Installing the table moves the whole Magic band, so a contrast
of this shape must not require the two halves to land on the same frame.

#### Four rows converted by seeding the gate

Each of these was reachable in retail and unreachable from a cold-boot pad
stream, and each is now driven by a fixture that writes exactly the one piece
of state the gate *is* and then runs the ordinary engine path. All four are in
`CANONICAL_LADDERS`.

- `field_actor_program.rs` (`801d4a60` `801d5a24`) - the `MAN_LOAD_RESUME`
  flags. `l3_scripted_scene_program_gate` sets system flag `0x17` / `0x0C` and
  loads a scene, which is what the flag means in retail (an opener ran and its
  closer did not), then steps the program the loader seats.
- `world/vm_hosts.rs` (`801d2d38`) - the three-actor talk. Its one shipped
  carrier is a `43 02` in `nilboa`; `l3_gated_field_arms_disc` finds it by
  disassembling the scene corpus and executes that record.
- `world/battle/monster_ai.rs` (`801e7320`) - the confuse-class target
  resolver. `l3_confused_monster_target_gate` lands Confuse on a monster and
  drives the fight, contrasting against an unconfused monster in the same
  battle so "it targeted the party band" cannot pass vacuously.
- `world/field_movement.rs` (`801d2404`) - the ledge hop. No fixture was
  needed: `field_ledge_hop_disc` already walked the player into a real
  `town01` ledge and verified the whole arc, and the row survived only because
  that test was not in the union.

Driving them surfaced three things the addresses alone do not show.

The step function's `NOT WIRED:` disclosure overstates its own blocker: of the
three BGM-gated states it names, `0x02` belongs to program 0 - an *opener*,
which the loader never spawns - and `0x19` gates on the CD-XA counter the same
disclosure says is not a blocker. Program 3 reads no BGM field at all.

`FUN_801D27E0`, retail's talk **controller**, is the six-state leader-cycle
SM: state 0 polls flag `0xD` (`0x801D28C8`) and, once the *script* clears it,
state 5 despawns (`0x801D2D04`). It never writes the party count back and
never clears the lock itself - retail's post-talk membership comes from the
scene script's own party ops. (An earlier reading here had the controller
restoring count and leader at `0x801D2AE4..0x801D2B20`; the disassembly
falsifies that.) The engine ports the flag-poll → despawn edge as
`World::tick_three_actor_talk`, and because the op-`0x43` collapse discards
the party list, it restores the arm-time snapshot the arm captures
(`ThreeActorTalk::saved_party`) - disclosed as engine bookkeeping, since later
script party ops converge on the same membership.

**There is no timer, and the repro that assumed one is gone.** An `#[ignore]`d
test used to tick 2000 frames waiting for the party back, blaming a missing
port. Its carrier's own bytes say otherwise: six instructions after `nilboa`'s
`43 02` the script runs `44 <n>` (SPAWN_RECORD) and then a two-byte
`nop` / `jmp -2` park loop, and the spawned partition-2 record branches on the
**leader** flags `0x10` / `0x11` / `0x12`, installs that leader's destination
banner and tile walls, and parks the same way. Neither clears `0xD`. So the
instruction arms a persistent hub in which the party leader *is* the player's
choice - which is also what makes the leader-cycle swap a reachability
question rather than a curiosity - and the lock drops when the scene sends the
player on. `a_three_actor_talk_ends_when_the_lock_drops_not_on_a_timer` asserts
those two halves and runs live.

**Where confused damage lands is now retail's answer on both sides.** The
resolver rewrites `+0x1DD` onto the caster's own band, and
`World::resolve_attack_target` (`world/battle/loop_driver.rs`) honours an
armed *living* target wherever it points - the opposing-side clamp the port
once applied has no retail counterpart (`FUN_801EC3E4` resolves against
whatever the action SM left in `+0x1DD`), so the side fallback applies only to
an unset or dead target.
`the_retarget_lands_the_damage_on_an_ally_not_on_the_party` - formerly an
`#[ignore]`d repro of the clamp - asserts the ally hit live.

Three older rows of this table converted. The `town01` opening naming prompt
(`801f03f0`) left through the composition ladder, whose opening rung drives
the prompt to its commit instead of booting past it. `battle_status_clut.rs`
(`8004ce2c`) was gated on "a Stone landed on an actor", and no gameplay path
could land one: the impact-selector ladder carries only Venom / Toxic / Rot,
and Stone's applier is `FUN_800402F4`'s class-9 arm, reached only by the
streamed capture-class boss modules with the class as a code literal. The arm
is now ported and wired (`World::apply_enemy_agl_status`, roll kernel
`status_effects::agl_status_inflict_roll`),
`world/tests/battle_stone_gaze.rs` drives cast -> Stone -> `sync_status` ->
CLUT-row grey end to end, and the composition ladder's driven fight lands an
ailment stamp in play. `magic_xp.rs` (`801f452c`) is driven by the pad ladder
`seru_cast_magic_xp_ladder` - round prompt to spell submenu to the threshold
cross and the level-up banner.

### NOT-PLAYTHROUGH

| group | n | addresses | why |
|---|---|---|---|
| `dev_menu.rs` | 2 | `801dbd04` `801db8f4` | the overlay-0897 developer EVENT FLAG editor |
| `new_game.rs` | 1 | `8001ffa4` | `GAME_STATE_COLD_RESET` is a `const`, read in production by `scene/host/lifecycle` and `world/frame_tick` - wired; the reach report resolves an item anchor through its executed references (the const-anchor note above), so the row reports off those readers' own coverage |
| `baka_cabinet.rs` | 1 | `801d553c` | the developer action-table dump retail writes as `ot5stat.txt` |
| `cutscene_script_elements.rs` | 1 | `801d841c` | reached only from the dev world-map panel's fade/flash actor |

The character-parameter editor (`801d6e18`) left this bucket: the play page's
dev-menu opt-in is itself pad-driven library code, so the composition ladder
walks its rows and the "not playthrough-shaped" reading no longer holds for
the browser host.

<!-- END engine-core -->

## Per-crate verdicts

Rows are grouped by module: one address is rarely a distinct decision from its
neighbour in the same file, and a flat table of one-line rows would hide that.
The `reach` column names the ladder that would enter an (a) row, the gate that
blocks a (b) row, or the disclosure state of a (c) row.

### engine-vm

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `actor_alloc.rs` | 3 | (a) | field-actors | `80024c88` `80024d78` `80024dfc` |
| `battle_action.rs` | 1 | - | a `//!` module anchor, not a routine verdict - see [the added rows' note](#two-of-the-added-rows-were-the-anchor-mechanism-not-a-gap) | `801d5854` |
| `battle_action/overlay_rng.rs` | 1 | (c) | disclosed | `801d0290` |
| `battle_burst.rs` | 1 | (c) | disclosed | `801f30c4` |
| `battle_cursor_pose.rs` | 4 | (c) | disclosed, and the strongest case of it on this page: a strict caller scan finds **no** reference to any of the module's four public items anywhere under `crates/`, `#[cfg(test)]` bodies included. Three carry `NOT WIRED` with a named prerequisite each; `801d9ae8` carries `REPLACED-BY` and owes no host - see [below](#the-cursor-pose-module-is-four-leaves-with-no-caller-at-all) | `801d32bc` `801d57e8` `801d5778` `801d9ae8` |
| `battle_helpers.rs` | 1 | (c) | disclosed | `80046870` |
| `battle_stream_slot.rs` | 2 | (c) | disclosed | `80055b4c` `801f17f8` |
| `code_lock_actor.rs` | 1 | (c) | disclosed | `801eed58` |
| `effect_vm/pool.rs` | 1 | (a) | field-actors | `801de914` |
| `field_actor_timers.rs` | 2 | (a) | a scene script issuing the field-VM op that **spawns** the timer - `0x43 0C` for the cinematic wipe (`op43_alloc_scripted_actor`) and `0x43 09` for the three-axis tween (`op43_sub9_tween`). Both `step` bodies already run from the production world tick (`world/frame_tick.rs`), and the in-crate oracle that drives them (`world/tests/field_timer_actors.rs`) can never be a union member. The [op census](#the-op-census-names-both-carriers) finds **no coherent carrier for either op**, so this row is close to `(d)` - see the note under that section | `801dd4c4` `801dd784` |
| `field_party_cursor.rs` | 1 | (c) | disclosed | `801f1278` |
| `field_passive_hud.rs` | 1 | (b) | a party member holding one of the six HUD-badge ability bits. `hud_anchor_offsets` is reached only through `World::passive_hud_points`, itself behind `World::passive_hud_active()`, and a cold-start party holds none of the six - so the badge column never anchors and the offsets never resolve | `801d095c` |
| `scus_battle_helpers.rs` | 2 | (c) | disclosed | `80046978` `80055854` |
| `scus_core_helpers.rs` | 4 | (c) | disclosed. Read with the note below: whether these are measured at all moves with the ladder set, so the verdict rests on the caller scan | `800203ec` `80020424` `80020454` `800204a4` |
| `vram_rect_copy.rs` | 1 | (a) | a scene script issuing op `0x43` sub-`0x12`. `build_packet` is reached from `enqueue`, which `FieldHost::op43_vram_rect_copy` drives on both hosts; the [op census](field-op-census.md) puts a dozen clean carriers across nine scenes, every one of them in the ending band (`edteien`, `edbylon`, `edbalden`, `edretoin`, `edkorout`, `edbubu`, `eddoman`, `edson`, `edstati3`), which no ladder enters | `80057914` |
| `world_map.rs` | 1 | (c) | undisclosed - the atmospheric fog tick, and no host builds one; see [below](#the-composed-overworld-trio-has-no-constructor-on-any-host) | `801e3e00` |
| `world_map_clut_fade.rs` | 1 | (c) | undisclosed, same trio - and the coverage side cannot corroborate it, because no binary in the union carries the file (the report's *not observable* set) | `801e4d8c` |
| `world_map_particle_burst.rs` | 1 | (c) | undisclosed, same trio | `801e5338` |

Seven module rows this table used to carry are closed by ladders that are now
canonical, and the shape of what closed them is the useful part rather than
the count. The `baka_hub_actors.rs` cluster (thirteen addresses, the page's
largest single (a) group) went with `w1b_hub_ladder`; the `lib.rs` actor-VM
seven went with the widget-script resolver and the composition ladder; the
`battle_intro_*` / `battle_target_group` / `pool_ops` render rows went with
`w1c_battle_render_ladder`; `world_map_dim`, `world_map_horizon`,
`world_map_overlay`, `world_map_panel` and `world_map_panel_actors` went with
`w1d_world_map_render_ladder` and the dev-menu ladder; `camera_mover`,
`dev_equip_commit`, `battle_gauge_rearm`, `battle_cast_dispatch` and
`battle_formulas/stat_init` went with the driven fights and the page ladders.

**Whether the `(c)` rows are measured is not a property of the rows.** The
page's [unmeasured bucket](#a-row-can-also-be-neither-entered-nor-never-entered)
named `scus_core_helpers.rs`, `overlay_rng.rs`, `world_map_clut_fade.rs`,
`battle_stream_slot.rs`, `battle_helpers.rs` and `code_lock_actor.rs` as
addresses no binary in the union carried a record for - outside both counts,
so their verdict was *unmeasured* rather than never-entered. A later export
had a record for each of them and the page said so; **the export after that
has none of them again**, every one back in the report's *not observable*
set.

Nothing about those rows changed in between, which is the point. The bucket
is a **link-time** property of the union's binaries - whether some crate a
ladder links references the function at all - so it moves when the ladder set
moves and says nothing about the port. Read against it, "it is measured now"
is not a durable statement, and this paragraph should never have made one.

What does not move is the caller scan, which is why bucket (c) is defined on
it: a source-side scan with `#[cfg(test)]` bodies excluded answers the same
way whatever binaries exist. The verdicts below stand on that, and the
coverage side is corroboration when it happens to be present.

The `disclosed` rows carry a `NOT WIRED` disclosure at their own tag, so they
are inert by the source's own account; they appear here only because the
permissive graph also calls them live. The `lib.rs` row was measured before
the widget-script resolver landed and is superseded - written up under
[The actor VM: a resolved bytecode source](#the-actor-vm-a-resolved-bytecode-source).

Four former "spirit-cast" / "summon-cast" rows left the table when the wiring
landed: the live loop's battle **Item** command now arms the action SM's
category-1 band instead of parking at `EndOfAction`, which puts
`battle_action/spirit.rs` (`801f3990`, the cast-audio cue),
`battle_cue_group.rs` (`801e22c8`, the cue-group expansion) and - through the
SummonFlute reroute - `battle_action/summon.rs` (`801f3c34`, the queued-magic
guard) on the playthrough path of all three hosts; the pad ladder is
`crates/engine-core/tests/battle_item_cast_band.rs`. The three
`battle_cast_dispatch.rs` addresses were re-bucketed on their own module's
account: the two dispatchers are disclosed `NOT WIRED` (they resolve to retail
VAs with no engine channel), and `801dba90` is a retail-dead entry point whose
instruction-identical twin (`FUN_801D8DE8` case `0x59`) is the wired one -
none of that is a "spirit-cast" gate.

Three (b) rows converted by seeding their gate, all now in
`CANONICAL_LADDERS`:

- `escape_timer.rs` (`801d2ebc`). Its gate was misnamed once and the name has
  already misled: `FUN_801D2EBC` is the field-VM `4C D3` scripted countdown,
  not the battle flee (that is the action SM's run band plus the
  `FUN_801E791C` roll, both covered by `battle_flee_ladder`). What no ladder
  reached was a *scene* whose script arms it. `l3_gated_field_arms_disc`
  disassembles the corpus for `4C D3` and drives every carrier it finds
  through `World::tick`: nine sites across `taiku`, `map03` and `chitei2`,
  five of them writing a zero duration (retail's own "leave it disarmed"
  case). The armed ones carry durations `2400`, `21600` and `35999`, so the
  fixture drives short carriers to expiry and long ones far enough to pin the
  readout and the ink band.
- `field_ledge_hop_arc.rs` (`801d2298`), with its `engine-core` sibling
  `801d2404`. No fixture was needed - `field_ledge_hop_disc` already drove a
  real `town01` ledge end to end and was simply outside the union.
- `travel_art_actor.rs` (`801ee094` `801ee328`). Both `PORT:` tags sit on one
  function, and `w1d_world_map_render_ladder` was already reaching it through
  the sub-list picker's row-1 hand-off. `l3_travel_art_visited_gate` covers
  what that ladder cannot: the scan **miss** arm (retail's `"UNFIND MAP
  NUMBER"` park), a multi-record visited table, and both handlers' dwell
  pairs. Two findings came with it, and both are settled. The Rula binding
  has no production installer *by retail's own layout*: the sub-list's state
  3 is the `FUN_801D84B4` return-to-title hand-off (`801ed590.txt`), and both
  travel arts are dev-band handlers reached only through the dev handler-id
  table - so the `TravelArt::Riremito` hand-off in `world/worldmap.rs` is the
  one deliberate synthetic binding, disclosed at the site, and the Rula arm
  is driven through `PanelActorHost::install`. And the visited table holds
  one record per kingdom: `tick_world_map_panels` keys each visit on the
  kingdom the party stands on (`kingdom_index_for_scene_base`, falling back
  to the entry fade's derived index and the active `mapNN` label) - it once
  read `visited.last().map_id` back out of the table it was writing, which
  pinned the whole session to record `0`.
  `each_kingdom_crossed_gets_its_own_visited_record`, formerly the
  `#[ignore]`d repro of that defect, asserts the multi-record behaviour live.

The world-map cluster splits three ways and the split is worth keeping: the
`dev-menu` rows sit behind a host hotkey a pad ladder cannot press, the
`world-map-panel` rows behind the panel-actor screens the spine ladder does not
open, and the plain `world-map` rows behind the overworld render pass.

#### The composed-overworld trio has no constructor on any host

Three rows were filed `(a)` behind one fixture - "a **drawing** host parked on
the overworld" - and it was the wrong bucket for all three. `AtmosphericFogTick`
(`801e3e00`), `ClutBlendFade` (`801e4d8c`) and `ParticleBurst` (`801e5338`) are
each named, outside their own definition and `impl`, only by `#[cfg(test)]`
bodies in their own file. Nothing on any of the three hosts constructs one, so
no overworld frame enters them however it is composed, and a ladder written
against that proposal would have drawn the whole overworld and converted
nothing.

What made the misfiling easy to write is worth more than the correction. The
retail routines *are* render-pass consumers - each is an actor tick the
world-map pass runs - and that reading is about the disc, not about the port.
A reach bucket is a property of the Rust anchor, the same rule
[the split-port note](#the-address-is-wired-and-its-anchor-is-not) records for
`panel_labels`: the question is which host calls *this item*, and for these
three the answer is none. The cell said what would drive the retail actor.

The page already carried the contradiction and nothing joined it up.
`world_map_clut_fade.rs` is named in the paragraph above as one of the
[unmeasured rows](#a-row-can-also-be-neither-entered-nor-never-entered) that
now has a record and reads never entered - which is the coverage half of
exactly this verdict - while its table row still read `(a) world-map`. A
never-entered measurement plus a `(a)` cell naming a fixture is the shape to
re-read: if the fixture were the gap, the measurement would not be there yet.

All three are **undisclosed**, which is the part that is work: each carries a
`PORT:` tag with no `NOT WIRED:` marker, and each stays out of `--live-audit`'s
*undisclosed inert ports* section only because the permissive graph resolves
its `tick` / `new` through some other type's method of the same name. They owe
a disclosure naming their own prerequisite - a world-map render pass that
builds the actor rather than the arithmetic - and until they have one the
address is invisible to both instruments at once.

#### The cursor-pose module is four leaves with no caller at all

Most `(c)` rows on this page have *some* caller - a unit test, a sibling
module, a `pub use` - and the finding is that none of them is a host.
`battle_cursor_pose.rs` is the degenerate case: `step_actor_cursor`,
`element_placement_copy`, `element_placement_copy_remapped` and
`release_widget_pool` are referenced **nowhere** in the workspace outside their
own definitions. That is worth naming rather than filing quietly, for two
reasons.

First, it is consistent with the [link-time
removal](#a-row-can-also-be-neither-entered-nor-never-entered) that puts a
function outside both counts: nothing in a linked binary references these, so
there may be no counter for them to be zero. A row of this shape should be
checked against the report's *not observable* set before it is read as
never-entered, or a missing measurement is read as a gap.

Second, three of the four already carry a `NOT WIRED:` disclosure that names a
concrete prerequisite - the round boundary adopting retail's cursor order for
`801d32bc`, and a mutable screen-element placement array, in a crate a drawer
can see, for the copy pair - and the fourth carries `REPLACED-BY:` because the
port's battle UI rebuilds every widget from world state each frame, so no pool
has a lifetime to end. `801d9ae8` is therefore out of the wiring denominator
and leaves this page the way the [no-ladder
rule](#a-row-can-leave-this-page-without-a-ladder-reaching-it) allows.

### engine-ui

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `battle_trail.rs` | 1 | (b) | the weapon-trail gate, the same one `801e1ab0` names: a move-FX scene whose move-power record carries a non-zero trail texture page (`+0x0b`). `weapon_trail_prims` is called on **both** hosts' battle render passes (`redraw_passes.rs`, `play_battle.rs`), so this is content, not a host gap. Read it with the [module-anchor caveat](#a-tag-between-two-functions-is-scored-by-the-next-function-that-has-regions) - the tag is a `//!` block |  `800485bc` |
| `ui_menu_window_painters.rs` | 2 | (a) | the shop's buy-quantity panel at its own phase (`801d5510`, see below) and the casino prize-exchange confirm (`801d603c`, disclosed inert - its tag anchors to `choice_panel_draws_for` itself, so the executed-disclosure misreport the pseudo-entry note records is resolved and the row reads unexecuted, as the disclosure says) | `801d5510` `801d603c` |
| `gte/math.rs` | 1 | (c) | disclosed | `8004629c` |

The crate used to be the largest one-reason cluster on this page: with no
rendering host in the union, every anchored builder read never-entered at
once. That bulk is gone rather than narrowed. The composition ladder took the
pause-menu screens, field panels, window painters, name entry, dev-menu list,
records screen and the live fishing HUD rows; `w1f1_fishing_banner_ladder`
took the five one-shot banner animators; `w1c_battle_render_ladder` took the
five `battle_intro.rs` styles; `w5_native_minigame_ladder` took the
`other_game_hud.rs` dome rows; `w1f1_pause_special_use_ladder` and
`w1f2_menu_depth_ladder` took the special-use confirms, the item target panel,
the entry-context pair, the spell-level notice and the title/save screens.

`801d5510`'s host column has now been wrong twice, and the second correction
is a wire rather than a re-reading. It first named a fixture that could not
exist: `shop_buy_quantity_panel` had exactly one caller in the workspace -
`web-viewer::play_shop::buy_quantity_panel_draws` - while the native window's
descriptor draw path painted windows 32 / 33 / 34 / 37 and carried no buy-side
block at all, so no native ladder could reach it however it drove the shop.
The missing native window-35 painter that column called for is written, so the
kernel is a both-hosts panel now and the row is a reach gap rather than a host
gap: what enters it is either host driven to the buy-quantity phase. Its
`engine-core` session siblings `801db7f4` / `801dbd94` are the same gap one
layer down and sit in the engine-core tables above.

The gap was player-visible while it lasted, which is the part worth keeping.
Window 35 carries the held count, the unit price and the running total, so a
native-window buyer sized a stack with none of the three numbers the decision
needs on screen - and every gate was green throughout, because a panel only one
host draws is invisible to a builder-reachability check when the kernel it
composes lives in `engine-core` rather than in `engine-ui`. Window 35 is also
the shop's only pens-returning renderer (no `MenuWindowPainter` variant
resolves `FUN_801D5510`), so both hosts filter the id on the descriptor's own
`renderer_va`; that constant is pinned against the disc's table by
`crates/engine-shell/tests/menu_window_dispatch_real.rs`.

One near-miss is worth recording because the grep that finds it is wrong. The
window-25 / window-41 stat-compare chain in `ui_menu_window_painters_large.rs`
reads like an orphan cluster: `compare_panel_draws_for` and
`party_compare_panel_fields` have no caller outside their own file, and their
in-file call sites sit at line numbers past the module's test boundary. They are
not in a test - both are inside `recipient_picker_draws_for`, which both hosts
call from their shop recipient window. Only the *window-25* half
(`equip_compare_panel_fields` and its category chain) is genuinely orphaned, and
that half is disclosed at its tag and waived by the drift gate.

### engine-audio

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `anim_cue.rs` | 1 | (c) | disclosed | `800508dc` |
| `sfx.rs` | 1 | (a) | the Muscle Dome interval tally's key-on, paired with its producer `801d1288` - see [the harness-blind table](#no-ladder-harness-blind) | `80065034` |

The footstep cadence and the SFX delay ring left this table through the
composition ladder - both tick on the browser play page's frame path, which is
exactly the host asymmetry the footstep module doc records against the native
window. The seven `sequencer` / `sfx` / `vab_bind` rows left it through the
audio session ladder: what they needed was a mixer-attached tick, which
`legaia_engine_audio::TestAudioSink` supplies without a device by driving the
same mixing core the cpal callback drives.

The ten `seq_calc` / `seq_events` addresses were filed `(d) differential` - the
SsAPI per-frame calc tier, whose host is the `note-trace` CLI rather than the
audio output path - and the full union **enters every one of them**. The
verdict was about the production route and the measurement is about the
binaries: the audio session ladder links the crate and runs the tier. Read
their tags for what calls them; the reach column no longer has anything to
say, so the rows are gone.

`seq_slots.rs`'s `8001ff58` has also left, and its exit is the useful one to
record because its cell named an **owner** rather than a tick:
`SeqResourceTable` was instantiated nowhere in the workspace. Whatever now
references it, the row is a statement about the tree rather than about the
ladders, so re-read it from `target/port-catalog/catalog.csv` before quoting
the old blocker.

### asset

No row. Every address this table carried is entered over the full union, and
the split is worth keeping because the table's own reading was that the
*render-facing* half of the crate was structurally out of reach: mesh
assembly (`character_pack_real`), the pose decoders and face animator
(`w1c_arts_swing_ladder`), the save-icon sheet (`w1f2_menu_depth_ladder`),
the summon side-band slots (`w1c_battle_render_ladder`) and the boot-overlay
resolver (`boot_overlay_disc`) each found a driver. `8002574c` is the one
that did not convert the way the others did - it is a **const** anchor now
and resolves through references, so it sits in the report's
*not observable (const)* bucket rather than in either count.

`boot_overlay` was also called "the only genuinely tooling-only module here",
and that stands as a statement about its host (`asset boot-overlay`); what
changed is that a ladder spawns the subcommand, and a spawned bin's profile
merges into the export.

### mdec

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `strv2_table.rs` | 1 | (d) | no-input | `801f1a00` |

Seventeen of the eighteen rows this table held were one fact - no pad-only
ladder plays a movie - and `w1a_fmv_ladder` is that ladder: it plays every
retail `fmv_id` through the dispatch slot / `StrPlayer` / `MdecDecoder`
chain and spawns `CARGO_BIN_EXE_mdec` for the hardware half.

`strv2_table` is the exception, and it is (d) on disc evidence rather than on
harness evidence. Retail's play loop does expand this table once per FMV, but
the port never selects the STRv2 arm: only slots 9 and 10 clear the Iki flag and
neither file is on the released disc, so no playthrough of a retail image can
reach it. The `strv2_decode` module states the same prerequisite chain.

### engine-render

| module | n | bucket | reach | addresses |
|---|---|---|---|---|
| `attach_swap.rs` | 1 | (c) | disclosed | `8004ccd4` |
| `lib.rs` | 2 | (a) | menu-render | `80034e4c` `8003c1f8` |

The `lib.rs` rows are module-scope tags whose real bodies are the `engine-ui`
menu-ink and sprite builders the crate re-exports, so they close with the same
draw ladder as the `engine-ui` table above; three of the five have, and the
crate's `afterimage` and `battle_intro` rows went with
`w1c_battle_render_ladder`. `engine-render` itself is still the hard wgpu link
the browser composition ladder cannot carry - what reports executed regions in
it is the spawned `play-window` of `w5_native_minigame_ladder`.

### save

No row. The `card.rs` checksum and the six `retail_inventory.rs` accessors
were bulk-classified `(d) cli` on the module's own statement plus a caller
scan, and `w2c_card_inventory_ladder` runs the whole family over a real
memory card's SC block. The classification stands - nothing on the engine's
frame path constructs a `RetailInventory`, deliberately - and the reach column
has nothing left to say.

### engine-shell

No row. The ocean-animation row (`8001ada4`) was the worked example of the
`bin/` exclusion and is now the worked example of what that exclusion
actually bounds: `w5_native_minigame_ladder` covers it several thousand times
per run, because `advance_ocean_animation` is on the window's per-tick path
and a spawned `play-window` writes its own profile. "Unreachable by call" and
"unreachable by coverage" were never the same claim. `xa_clip.rs`'s
`8003d53c` went with `w1c_arts_swing_ladder`.

### prot

No row. `retail_name_table` (`8001d8fc`) is deliberately the *lossy* CDNAME
reader and must never sit on a resolution path; `cdname_retail_parse_disc`
runs it over the real file, which is its `(d) cli` host doing exactly what
the verdict said it would.

## The actor VM: a resolved bytecode source

`FUN_801D6628` is the actor / sprite VM - the first VM ported and the `Host`
trait shape every later VM port follows. Its interpreter is
`legaia_engine_vm::run`. This section previously graded the seven tagged
addresses "(c): no host reaches it" and pinned the reason precisely: **the
missing prerequisite was a bytecode source, not a call site** - nothing
resolved the VM's programs out of the disc, so any new call site could only
synthesize its operands, and a call proving the interpreter runs on invented
input would have been a fake wire.

The prerequisite is resolved. The programs are **data resident in the menu
overlay itself** (PROT 0899 - a program table in the overlay's data segment,
[`window-script.md`](../formats/window-script.md)), which also dissolves the
"per-scene lookup" framing: the carrier is per-boot overlay data, and what
selects a program is the menu code path, not a field-VM or scene-entry
event. The wired chain: `legaia_asset::widget_script` parses + scans the
programs, `World::install_menu_overlay_tables` (both hosts call it with the
real overlay bytes) resolves them, and `MenuRuntime::tick` feeds them into
`legaia_engine_vm::run` over the `engine-core::menu_widget` window-list host
on the shop picker entry / Sell transition edges - the transitions retail's
`FUN_801DAFD4` drives. Disc-gated pins:
`crates/engine-core/tests/menu_widget_scripts_real.rs`.

What stays true from the old verdict: `World::run_actor_bytecode`
(`crates/engine-core/src/world/effects.rs`), the field-actor host, is still
reached only from `FieldDemoHandler::run` in
`crates/engine-core/src/mode.rs` - a handler that synthesizes its bytecode
and is constructed nowhere outside the `#[cfg(test)]` module in the same
file. That edge is a demo, disclosed at both ends; the production route is
the menu-widget one above. A rerun of the replay reach report is what moves
the seven addresses out of the (c) table rows above, which record the
pre-resolver measurement.

## The op-`0x49` submode screens

The `baka_hub_actors.rs` row above was the page's largest single (a) cluster,
and its blocker was not a missing ladder: the engine had **no mapping from an
op-`0x49` sub-op to a handler slot**, so `slot_for_op49_sub_op` answered
"close tick" for all eleven non-dedicated sub-ops and every screen a script
asked for closed itself on its first frame. The mapping is retail's own
14-byte table at `0x801F33A4`
([`script-vm.md`](../subsystems/script-vm.md#which-screen-a-sub-op-opens-the-table-at-0x801f33a4)),
now ported; `crates/engine-core/tests/w1b_hub_ladder.rs` drives four of the
screens from a field-VM instruction, by pad, through `World::tick`.

Six of the thirteen are still not script-reachable, and the reason is
structural rather than a ladder gap: they are panel painters installed by
handler slots with **no ported body** (`0x20` `FUN_801EE90C`, `0x21`
`FUN_801EED58`, `0x31` `FUN_801ED590`, and one of `FUN_801E9B3C`'s own
descriptor-op handlers), plus `801f1d90`, whose slot `0x13` no immediate in
the field overlay ever stores to `+0x50`. The ladder paints those through the
host-pinned window and says so; porting the three handler slots is what would
make them script-reached.

## Ladder proposals

Ranked by how many of this page's (a) rows each would move. The counts are for
the non-`engine-core` slice only, so a shared ladder moves more than its row
here says.

The top proposal of this table's earlier state is **built**:
`crates/web-viewer/tests/play_compose_ladder.rs` is the draw-composition
ladder, a canonical member of the report's union, and it also swallowed most
of what the *battle render*, *menu render* and *dev menu* proposals covered -
its driven fights build the intro, the ground grid, the party HUD, the
assembled meshes and the attack camera, its menu rungs run the ink and sprite
builders, and its opt-in rung walks the dev list and records page. What each
remaining proposal would still move:

| ladder | rows | what it drives |
|---|---|---|
| FMV | *built* | `w1a_fmv_ladder` plays every retail `fmv_id` and spawns `CARGO_BIN_EXE_mdec`; one `mdec` row is left |
| Baka Fighter hub | *built* | `w1b_hub_ladder` opens the op-`0x49` submode screens; no `baka_hub_actors.rs` row is left |
| audio | *built* | `w1e_audio_session_ladder` attaches the mixer; one `engine-audio` row is left |
| world-map panels | *built* | `w1d_world_map_render_ladder` and the dev-menu ladder between them; the panel-actor cluster is gone |
| boot chain | *built* | `w3c_boot_logos_ladder` opens the publisher-logo phase - the one stage every other member starts after |
| field fog | *built* | `w1h_fog_page_prims` composes the page's screen-prim pass over a scene whose entry script raises the gate; the `fog_particles.rs` trio is the one cluster no *headless* member could have taken |
| composed overworld | *withdrawn* | the three rows it named have no constructor on any host, so a drawing host parked on the overworld enters none of them - see [the trio](#the-composed-overworld-trio-has-no-constructor-on-any-host) |
| field actors | 3 | an effect that spawns a child actor through the allocator (`actor_alloc.rs`), plus the effect pool's own `init` |

Quote that table's *rows* column against a fresh
`replay-port-coverage.py` run rather than as a standing figure: it is the count
of this page's rows a ladder would move, and every ladder that lands changes it.
The `built` rows are kept rather than deleted because a proposal that closed is
the evidence for the next one - each named a denominator no existing ladder had,
and that is what made it worth writing.

A **withdrawn** row is kept for the opposite reason, and it is the cheaper
lesson: a proposal is a claim about the rows it would move, and nothing
checks it. *Composed overworld* named a fixture that would have worked - a
drawing host on the overworld is a real thing to build, and two of the three
addresses it named describe retail actors the overworld pass really does tick
- and it would still have converted no row, because the port has no
constructor for any of them. So a proposal is worth costing the way a row is:
take one address it names, scan for a production caller, and only then count
it. A ladder is the most expensive thing on this page to write, and the
cheapest thing to write *against the wrong bucket*.

The native window's composition is driven by **spawning** it -
`w5_native_minigame_ladder` runs `play-window` per rung and the child's
profile merges into the export (the `bin/` exclusion bounds calls, not
coverage; see above). The standalone browser minigames page and the `cards`
page remain outside the union and keep their harness-blind rows above.

### Battle render, battle target and arts swing are built

Three proposals left this table together, as two files:
`crates/web-viewer/tests/w1c_battle_render_ladder.rs` (the intro styles and
the attack-target ring) and `crates/engine-shell/tests/w1c_arts_swing_ladder.rs`
(the shout bank, the facial animator and the XA-clip census). Each needs its
own `cargo llvm-cov` export joined into the union, **without `--release`**.

Why the styles needed a ladder at all is worth keeping: the four non-default
transition styles are not a beat a player reaches, they are a *data* arm.
`select_intro_style` keys on the formation's first monster id, and the ids
that select the confetti / curtain / swirl belong to formations no scene the
composition ladder enters registers - so the driven fights all took the
default `TileShatter` arm and four ported style bodies never ran once.

Four addresses did **not** move with those ladders. Two named wiring gaps
rather than reach gaps and are wired since; the other two name their own
gates:

| address | why it stayed |
|---|---|
| `801f44a0` | resolved: `engine-core`'s `BattleHud::push_popup` delegates every popup push to `DamagePopupRing::push`, so simultaneous popups are ring-bounded on both battle-HUD hosts |
| the battle party panel's three | `panel_anchors` is production-called (`engine-ui`'s `party_panel_stage_x` reads it for the roster name pens); the rest of `battle_party_panel.rs` (`panel_labels`, the label-actor lifecycle, `cross_out_mark`) stays disclosed `NOT WIRED` - and the address follows the *anchor*, see below |
| `801e1ab0` | content-gated: the streak needs a move-FX scene whose move-power record carries a non-zero trail texture page (`+0x0b`) |

The party-panel row was the sharper finding: `engine-ui` reproduced
`panel_anchors`' constants as its own `party_panel_stage_x` literals, and an
`engine-shell` test pinned the two equal - the gate passed, the numbers
agreed, and the ported kernel was dead. `party_panel_stage_x` now reads
`panel_anchors` directly, and the shell test asserts the production path
returns the kernel's values. `DamagePopupRing` was the same shape one level
down: it models retail's **8-slot wrapping** ring while the live HUD kept an
unbounded `Vec<DamagePopup>`; `BattleHud::push_popup` now delegates every
push to the ring, so retail's "a ninth simultaneous popup overwrites the
first" holds in the port.

Four `PORT` tags moved off module scope in the same pass, onto the routines
they name (`pick_channel`, `build_afterimage_quad`, `LabelState::opened`,
`cross_out_mark`, `panel_labels`). At module scope each would have resolved,
under the anchor fallback above, to an unrelated neighbouring function -
`ArtsShoutBank::new`, `streak_half_width`, `name_field_ptr` - so constructing
a bank would have read as "the arts-voice selector ran".

### The address is wired and its anchor is not

The panel build's address sat in the per-crate table as an `(a)` row reading *wired -
`party_panel_stage_x` reads `panel_anchors` on every battle-HUD frame*. The
sentence is true and the row was wrong, because a bucket is a property of the
**anchor**, not of the retail routine. `FUN_801D84C0` is retail's panel build +
teardown; this workspace splits it in two, and the halves have opposite
verdicts:

- `panel_anchors` - the per-party-size X anchors - is production-called from
  `engine-ui`, and carries **no** `PORT:` tag. It is a section comment.
- `panel_labels` - the two build arms - carries `PORT: FUN_801D84C0` plus a
  `NOT WIRED:` disclosure naming its own prerequisite (`engine-ui` does not
  model the four label buffers at all).

The third address of the trio has since parted company with the other two.
The label-actor lifecycle registers a **retained-mode** SCUS text actor and
stashes its handle;
the port's battle HUD rebuilds every `TextDraw` from the live model each frame
on both hosts, so there is no handle to hold and no host is owed one. It carries
`REPLACED-BY:` and is out of the wiring denominator, while the panel build
and the cross-out mark remain declared gaps.

So every instrument that keys on the address answers for `panel_labels`, and
the catalog reads it inert with a disclosure. Crediting the row as
wired credited the *unported* half. The row is filed `(c) disclosed` with its
two siblings.

The shape generalises, and it is the reason a `reach` cell should never be
written from a prose claim about the retail routine: **when one retail routine
is ported as two Rust items, the address belongs to whichever item wears the
tag.** Check `target/port-catalog/catalog.csv` before writing a verdict; if the
wired half is the one that deserves the address, move the tag rather than the
row.

## The cast-module band, the largest cluster on this page and the one with no rows

`crates/engine-vm/src/cast_module_ticks.rs` is still the largest single-file
cluster in the never-entered set, and it is still the only cluster of any size
not cited anywhere else on this page - the file arrived whole and never went
through the per-row pass the rest of the buckets did.

What has closed is all of it, and the denominator worth quoting is **tagged
bodies**, which is a property of the sources rather than of the ladder set. The
three modules `cast_arm_ticks.rs`, `cast_seru_ticks_a.rs` and
`cast_seru_ticks_b.rs` were the first to reach zero never-entered; the eleven
that outlasted them were all in `cast_module_ticks.rs` - `801f69f8` `801f6a0c`
`801f6a14` `801f6a28` `801f7158` `801f767c` `801f77e8` `801f7fa4` `801f85a8`
`801f86a4` `801f8d64` - and
[`w1c_cast_module_bodies_ladder`](#the-eleventh-hour-row-set-a-source-denominated-ladder-for-cast_module_ticksrs)
drives every one.

Five ladders did that, and each is a different *denominator*, which is the
transferable part:

| ladder | denominated in | why the axis matters |
|---|---|---|
| `w4d_cast_band_ladder` | spell ids | asks whether a band entry's body is reached at all; blind to what runs inside it |
| `w1b_seru_ticks_ladder` / `w1c_seru_ticks_ladder` | phase depth, ids `0x81..=0x8b` | those bodies are `beq` chains fifteen arms deep whose simulation writes live in the late arms |
| `w1d_trampoline_arms_ladder` | `(PROT entry, action id)` pairs | a body is reached only through its module's trampoline, and one cell can hold two |
| `w2c_cast_band_body_ladder` | `// PORT:` addresses scraped from **three** band modules' sources | cannot go stale when a later lane adds a body to one of those three |
| `w1c_cast_module_bodies_ladder` | `// PORT:` addresses scraped from `cast_module_ticks.rs`, by dispatch **seam** | the fourth module, and the three seams its bodies are reached through |

### The eleventh-hour row set: a source-denominated ladder for `cast_module_ticks.rs`

The eleven that outlasted the other three modules did so for two reasons at
once, and neither is a gate a pad stream could open.

The first is a hole in the scrape. `w2c_cast_band_body_ladder` takes its
denominator from the sources and cannot go stale - but only over the three
modules it `include_str!`s, and `cast_module_ticks.rs` is not one of them. A
source-denominated ladder is only unstaleable over the sources it reads.

The second is the id axis. `w4d_cast_band_ladder` seats **one representative
spell id per PROT entry** (`seat.entry(entry).or_insert(id)`), which is exactly
blind to a module holding several choreographies behind different trampoline
arms: PROT 0955 holds six, and only the lowest id's body ever ran. Five of the
eleven are PROT 0955's other five arms; three more are the second arm of PROT
0945 / 0951 / 0952 / 0957.

The remaining two are neither - they are the whole-row **AoE sweep stagers**
(PROT 0927's `801f85a8`, PROT 0966's `801f8d64`), which `run_cast_module_code`
never calls at all. Their seam is `World::run_cast_module_aoe`, the one
`fold_pending_cast` takes for the two modules whose damage lands inside their
own sweep rather than in the generic fold, and a ladder that only drives the
tick seam cannot reach them however many ids it seats.

So the module has **three** dispatch seams and a row that names the wrong one
passes without entering anything. `w1c_cast_module_bodies_ladder` keys each row
on its seam - the trampoline (`(entry, action id)`), the no-trampoline band arm,
and the AoE stager - and a fourth kind for the five **inline stagers**, which
run before the tick match and report no `CastTickStep` at all: PROT 0923 carries
a stager and no tick body, so `tick_ported` stays false for it and "entered" has
to be the state the stager itself writes (`ctx[+0x278] = 3`). Four of the five
get such a probe; PROT 0906's Gizam stager writes only fields the cast-band seam
does not carry back out of its view, so its credit is the unconditional call
site.

All four were written before they were in `CANONICAL_LADDERS`, and until they
were named there the export recipe did not produce a `cov-*.json` for them, so
every row they drive kept reading *never entered* with the ladder green. **A
ladder converts nothing until it is in that list**; when a lane adds one, it
belongs there in the same commit.

Every row was bucket **(b) GATED**, and they all shared one gate. The engine
reaches these bodies through `World::cast_module_for(spell_id)`
(`crates/engine-core/src/world/battle/cast_band.rs`), which resolves a cast's
spell id onto a PROT `0903..=0966` module and only then selects that module's
tick body and sweep arm. So the gate is not "a battle", and it is not "a cast" -
the union already drives both, and the seeded cast ladder enters 110 live
addresses. The gate is **the specific spell or summon id**, one per row.

This is the intro-style shape one layer down: a data arm rather than a beat a
player reaches. A pad fixture cannot convert these rows however long it plays,
because nothing about pad input selects a module; what converts one is a seeded
cast of that id with the band resolved.

The work was one integration ladder that seats a cast per id and steps it, and
it converted rows in proportion to the ids it covered - four such ladders now
exist and the table above says what each measures. The constraint that shaped
them is still worth knowing:
`crates/engine-core/src/world/tests/cast_band.rs` walks the id-to-module
resolution per id and can **never** be a union member, because it is a
`#[cfg(test)]` module rather than a test binary and the list takes
`--test <name>` integration targets.

`screen_fx.rs`'s ten rows had the same shape and are closed: the scene-frontier
ladder enters them, which is the reading the table below predicted - they were
gated on a scene whose script spawns the effect, not on anything a pad does.

## A wire that lands with its ladder never becomes a row

Every row on this page arrived the same way: a port landed, the next refresh
reported it live and unentered, and someone read it a wave later. The cheaper
order is the other one, and it costs a ladder rather than a triage pass - so
when a wave wires a field-VM arm, the arm's disc carriers are what the same
wave measures it against.

`w3b_wave_wires_disc` is that ladder for three arms at once, and it is
`w1f2_field_vm_op_arms_disc`'s shape rather than a new one: sites taken from
the disc corpus at decoded instruction boundaries behind the census tools'
clean-resync run, one per scene, then stepped in a real `World`.

| arm | what the ladder drives | carriers it takes |
|---|---|---|
| `4C 14` actor clone (`FUN_801D835C`) | the seat, then the clip-fade pool kernel (`FUN_801D820C`) to the frame its 12-bit accumulator fills and the retire sweep collects it | `vozz`, `retona`, `urudre3` |
| `4C 86` reflection install (`FUN_801E573C`) | the seat's two endpoints, then one pool pass of the tick (`FUN_801E5154`) mirroring the source's pose across the instruction's own plane | `concnow`, `urudre2`, `conc2` |
| `34 0x` screen-effect tween (`FUN_801DE2B0`) | both selectors recomputed from the sub-op byte, then one pool pass turning the seated tween into a screen push (`FUN_80024EE4`) | `town01`, `town0b`, `town0c` |

The rule each rung is built on is the one this page draws between a fixture
that proves the interpreter runs and one that proves the content exists: each
arm already had a sibling oracle writing its instruction out as a byte array,
and a hand-built instruction can be correct about the decode while no shipped
scene issues it. What the disc walk adds is the carrier; what driving the pool
tick adds is the consumer, because **a seat nothing steps is indistinguishable
from a seat that never ran** - the install alone writes no pose, and the tween
alone emits no push.

Two of the three turn out to have a second driver, and it is the one union
member denominated in scenes: `chapter1_frontier_ladder` walks the chapter-1
closure, which contains the `conc` band and the clone's carriers, so the
clone seat and the reflection pair are entered by a scene walk as well as by
the arm ladder. The pool kernels behind them are not: the clip-fade tick and
the reflection tick's *spawner* register only through the ladder written for
them, because a scene walk that never parks in the beat those instructions sit
in never reaches the frame that steps the actor they seat.

**The screen-effect tween's consumer does not exist.** The `34 0x` arm now
seats a `ScreenTintPush` per frame, and a workspace scan with `#[cfg(test)]`
bodies excluded finds **no reader on either host** - the field pushes are
produced and nothing draws them. So the arm's reach row would close and the
feature would still be invisible to a player, which is the distinction between
this page's question and [`host-drift.md`](host-drift.md)'s. The ladder asserts
the push because that is the port's own seam; what is owed is a renderer that
takes it.

### A shared kernel has no address, so this instrument is silent about it

The same wave replaced five per-frame decisions each play host had been making
locally with one `engine-core` kernel apiece - the camera's snap-beat bank,
the occlusion fade's arming gate and body centre, the save-select phase
layout, the field HUD's projection. None of them appears in this report, ever,
and the reason is structural rather than a gap: they carry no `// PORT:` tag,
because they are not ports of a retail routine with an address. The report
joins coverage against the **catalog's anchors**, so an untagged kernel is
outside its denominator in the same way an untagged helper is.

That is worth stating rather than leaving implicit, because the natural
reading of a silent instrument is that it approves. What answers for these is
[`host-drift.md`](host-drift.md)'s paired-kernel tiers plus a ladder per host:
`crates/web-viewer/tests/w1b_host_parity_ladder.rs` drives the page's own
per-frame read surface over two disc scenes, and the native half is in a
`bin/` target, so what runs it is a spawned `play-window` -
`w5_native_minigame_ladder`'s rungs each open on a field scene with the
occlusion fade at its default, which is the native arm's own gate.

Reading the coverage exports directly for the symbol - which is what one has
to do for an untagged kernel - puts the occlusion pair and the cull-mode
selector on **both** hosts, the save-select phase layout and the page's HUD
projection on the page only (they have no second host to reach), and leaves
two entered by neither: the camera's snap-beat **drain**, which both hosts
call but only inside a frame a scripted shot owns, and the ocean-head
animation fallback, which needs a kingdom bundle with no CLUT-walk table. Both
are ordinary `(a)` content gaps; neither can ever be a row here, because
neither has an address for the report to key on.

## Rows a refresh added, and nobody has bucketed yet

A coverage refresh does two things at once: it converts rows, and it *adds*
them - every port that landed since the last export arrives with a reach
verdict nobody has written. The added ones have no home in the tables above
until someone reads them, and the failure mode to avoid is the silent one:
leaving them out entirely, so the page reads as complete while the instrument
knows otherwise.

So they are listed here, with the only thing the refresh establishes - that no
ladder in the canonical union entered them - and **no bucket**. Assigning one
means reading the port: (a) if a fixture would drive it, (b) if a game gate
stands in front, (c) if nothing on any host reaches it, (d) if it is not
playthrough-shaped. A row leaves this section when it gains that verdict, not
when someone guesses.

| address | crate | anchor |
|---|---|---|
| *(none)* | | |

The set is empty because the four it last held have verdicts - see the table
after the next one, and the correction under it.

The set this section held before them is bucketed, and where each went is the
part worth keeping rather than the fact that it emptied:

| addresses | verdict | where it went |
|---|---|---|
| `8003f348` `8003f3fc` `8003f86c` | (a) | [content not driven](#no-ladder-content-not-driven) - the fog emitter, which only a **composing** ladder can enter |
| `80057914` | (a) | the [`engine-vm` table](#engine-vm), on the op-`0x43` sub-`0x12` carriers the census names |
| `801d095c` | (b) | the [`engine-vm` table](#engine-vm), on a party that holds a HUD-badge ability |
| `8003c7ec` `800430ac` `801cefd4` | (a) | [content not driven](#no-ladder-content-not-driven) - two field-VM op carriers and the boot chain |
| `801d1288` `80065034` | (a) | [harness-blind](#no-ladder-harness-blind), as **one** gate - the producer and its consumer |
| `800485bc` | (b) | the [`engine-ui` table](#engine-ui), on the weapon-trail content gate `801e1ab0` already names |
| `801d32bc` `801d57e8` `801d5778` | (c) | the [`engine-vm` table](#engine-vm), disclosed, with no caller of any kind |
| `801d9ae8` | - | leaves the page: `REPLACED-BY`, no host owed |
| `801dd4c4` `801dd784` | (a) | the [`engine-vm` table](#engine-vm), on the op that spawns each timer |
| `8004fe5c` `801d5854` | measurement | neither is a gap - see below |

A later refresh added four more, and the first reading of them - that each is
wired on a host inside the union and each waits on a **rung nobody wrote**, so
all four are (a) - held for three. Reading the ports rather than the addresses
moves one of them and corrects the *reason* for another two:

| address | verdict | what the reading found |
|---|---|---|
| `801ed710` | (a), **converted** | `play_compose_ladder`'s dev-menu rung already taps Square for the Records page and asserts it drew; the export enters `records_screen_draws_for`. The step the row named existed |
| `801d64a8` | (a), **converted** | the step the row named also existed - and did not work. `w1f2_menu_depth_ladder`'s Arrange rung drove the row and the sort never ran; see below |
| `801da2a0` | **(b)** | not a missing rung. The reorder page opens off the Status screen's confirm, and `ListOrderSession::open` takes its own reject arm on an empty list - so the gate is a record with a spell in it |
| `801d5944` | (a), **converted** | the bucket was right and the step was writable: a shop **sell** driven to its quantity window, now a rung. The host claim attached to the row was wrong - see below |

**`801da2a0` is a gate, not a fixture.** `sub15_list_source` is reached from
`ListOrderSession::open`, which the Status screen's `Cross` arm calls with
`status_spell_rows`' list for the shown character - each living member's own
`spell_list()` off its `0x414`-byte record. A cold-start party carries `count
= 0` there, retail's Seru magic being learned rather than granted, so the
confirm hits the `None` arm and the page never opens. That is
[a gate that closes by seeding the one piece of state it is](#gates-behind-the-b-rows):
one learned spell on one record, which is a write the engine already makes
(`magic_xp::learn_spell_prepend`, the Seru-capture ladder's own route).

**`801d64a8` is the sharper of the two**, because the rung that names it was
canonical, green, and not reaching the code. The export over that ladder's own
binary reports `arrange_bag_slots` and `PauseItemsSession::arrange` at zero
while the rung passed: it drove Throw Out first, discarded a cold-boot bag
small enough to empty, and retail's own "scan the bag, buzz if empty" dispatch
then swallowed the Arrange confirm. The idempotence check the rung scores on
cannot see that, because a sort that never runs leaves the drawn list
identical to itself - the assertion is true of both outcomes it was written to
separate. Arrange now runs first, on the untouched bag, behind an explicit
non-empty precondition.

The generalisable part is the shape rather than the bag: **an invariant
assertion passes vacuously when its subject did not execute.** Idempotence,
determinism, "the total did not change" and "the list is still sorted" are all
of that family, and each is exactly what an unreached kernel produces. A rung
scored on one needs either a precondition that fails when the kernel is
skipped, or a positive effect to assert - and a coverage export is the only
thing here that tells the two apart.

`801d5944` was filed here with a second question attached - that its painter
is called from the browser play page and from no native site, so "no ladder
entered it" and "one host cannot enter it" were both true of the address and
only the first was a reach verdict. **The host half of that is wrong.**
`sell_quantity_draws_for` has a native call site in
`window/shop_windows.rs`, in the same block as the buy-side painter and
filtering the same descriptor id 37, and it has had one since long before the
row was written. What survives is the distinction the row was drawn to make:
a reach bucket answers what a fixture would do, whether the other host owes a
call is [`host-drift.md`](host-drift.md)'s question, and conflating them would
let a drift row be closed by writing a ladder. What does not survive is this
row as its example - the address is an ordinary `(a)`, and a shop **sell**
rung on either host converts it.

The way it got written is the reusable part, because the grep that produces it
looks conclusive. A workspace-wide search for the symbol answers with the
`engine-ui` definition, its own unit tests and the compose oracle first, and
both call sites sit past a `| head` boundary - so a scan that is not counted,
or is read off a truncated pipe, reports the definition side and no callers at
all. The cheap guard is to grep the two host directories by name
(`crates/web-viewer/src`, `crates/engine-shell/src/bin`) rather than the tree,
because a host claim is about those two places and nowhere else.

The pairing was the verdict-shaped part of the set, and it held: `801d1288` is
the Muscle Dome tally's per-lane voice resolve and `80065034` is the audio side
it keys, so they are one row's worth of work and not two.

#### Two of the added rows were the anchor mechanism, not a gap

`8004fe5c` and `801d5854` are both `//!` **module** anchors, and a module block
has no span of its own, so each inherited the verdict of whatever function
followed it - the fall-through this page already documents. Read against the
address instead of against the anchor, both are ordinary production code:

- `8004fe5c`'s `route_sfx_cue` is called from the melee-impact path in
  `world/battle/loop_driver.rs`, behind retail's own `target_anim < 0x10`
  guard, so any driven fight that lands a physical hit runs it.
- `801d5854`'s ported body is `battle_cam_script::apply_death_reframe`, called
  from `drive` - the battle camera's own per-frame script - on the case-8 arm
  when the framed target's HP reaches zero. Its `battle_action.rs` module
  anchor is one of eleven addresses on two `//! PORT:` lines and says nothing
  about this routine at all.

So the work these two name is a **tag** move (onto the routine each address
implements), not a fixture. Until then, read either row against the function
anchor.

## Gates behind the (b) rows

| gate | rows | what has to happen |
|---|---|---|
| a learned spell on one record | `801da2a0` | one party member's `spell_list()` carries a non-zero `count`, so the Status screen's confirm opens the reorder page instead of taking `ListOrderSession::open`'s reject arm. The engine's own route to that state is `magic_xp::learn_spell_prepend` off a capture |

Both rows this table last held before it are gone, and they went different
ways.

**slot-bonus was stale, not open.** Its five `legaia_asset::minigame_slot_scene`
kernels (`801cec94` `801cfff0` `801d069c` `801d0fa8` `801d3230`) are entered -
a coverage export over the full union measures every one of them - and the pair
of ladders that does it, `w1l4_slot_bonus_marquee` (the disc-data half, which
also spawns `asset slot-scene`) and `w1l4_slot_bonus_marquee_ladder` (a played
bonus round), have both been canonical for some time. A gate row outlives its
gate silently, because nothing joins this table against an export; re-read a
gate against `CANONICAL_LADDERS` before quoting it, the same guard the
`reach`-cell note above asks for.

**Seru capture closed with a ladder, and the coverage number could not tell.**

The former spirit-cast (5 rows) and summon-cast (3 rows) gates opened with
the item-band wiring and the summon-spawn ladder; the "battle-escape" gate
was a misnomer for the timed-flags scene countdown. Two more closed with the
ladders above: the **cast-module id** gate (11 rows) and the **capture-class
cast** gate (2 rows).

The Seru-capture row (`801e92dc`, `magic_xp::learn_spell_prepend`) never needed
a fixture to make its *number* move: the coverage side was already satisfied,
because `battle_depth_replay` - a canonical member - calls
`learn_spell_prepend` from its own test body to seat a caster. That executes
the function, so the reach row read converted while the production route
(`World::resolve_captures`, reached from battle teardown) ran in no ladder at
all. A row whose only executor is a ladder's setup code is entered and undriven
at the same time, which is the one shape this page's buckets cannot express.

`w1g_seru_capture_ladder` drives the route: a capture spell cast in a live
fight, the roll landed on a weakened monster, `World::finish_battle` ->
`World::resolve_captures` -> `seru_learning::record_capture` -> the record-side
prepend, asserted on the record rather than on the call. Its two seeds are both
preconditions and neither is the gate's behaviour - the monsters sit at 1 HP
because retail's roll scales with the missing-HP fraction, and the
capture-points total is seeded just under the Seru's learn threshold the way a
resumed save carries it, because one fight banks one capture's worth and the
learn edge is otherwise unreachable from a cold start.

**The instrument cannot see that this happened.** The ladder's own export
reports `0 unique` - every address it enters, some other member enters too -
so the closure is invisible to the number and visible only here. That is the
honest cost of the shape: when a row's executor was setup code, the fixture
that fixes it converts nothing.

Four more gates closed the same way, and the pattern is worth naming: **a
gate closes by seeding the one piece of state it is, not by waiting for a pad
stream to earn it.** The `MAN_LOAD_RESUME` flags, the talk lock, the
confuse-class bitfield, the `4C D3` scene, the ledge and the visited-map
record are all one write each, and every one of them is a write the engine
already makes somewhere. What a fixture must not do is invent the *content* -
three of the six take their bytecode from the disc corpus through the field-VM
disassembler, because a hand-built script proves the interpreter runs and not
that any shipped scene reaches the arm.
