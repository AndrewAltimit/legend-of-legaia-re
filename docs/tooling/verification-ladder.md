# The verification ladder

Nothing of the original executes in the port, so a claim that the port plays
like retail is only as good as the check behind it
([why](../porting-by-reimplementation.md#fidelity-is-measured-not-inherited)).
No single check covers the whole claim. The project stacks six, each answering
one question and each blind to something the next one sees. Read from the
bottom, every rung assumes the ones below it.

| Rung | Question | Retail's side | Gate |
|---|---|---|---|
| 1. Format docs | Is the data read right? | The disc's bytes, traced to the routine that reads them | `disc-coverage.py`, `byte-account-coverage.py` |
| 2. Port provenance | Does a `// PORT:` address name the routine the Rust implements? | The executable's and overlays' bytes at the tagged address | `check-port-provenance.py --check` |
| 3. Retail kernel differential | Does the Rust compute what the bytes compute? | The routine itself, run off the disc in an R3000 interpreter | `differential-coverage.py --check` |
| 4. Retail-compare stills | Does the engine stand where retail stood, at one instant? | A save state's RAM and VRAM | `retail-compare-baseline.json` ratchet |
| 5. Films | Does it unfold the same over time? | An emulator recording, one row a vsync | Report; before and after on one command |
| 6. Full-game ladder | Does the whole game play, New Game to credits, on pad input? | Retail saves at each story milestone | `full_game_baseline.toml` ratchet |

## Contents

- [Rung 1: format docs](#rung-1-format-docs)
- [Rung 2: port provenance](#rung-2-port-provenance)
- [Rung 3: the retail kernel differential](#rung-3-the-retail-kernel-differential)
- [Rung 4: retail-compare stills](#rung-4-retail-compare-stills)
- [Rung 5: films](#rung-5-films)
- [Rung 6: the full-game ladder](#rung-6-the-full-game-ladder)
- [Why the differential is the hinge](#why-the-differential-is-the-hinge)
- [What a gate does and does not mean](#what-a-gate-does-and-does-not-mean)

## Rung 1: format docs

Every format the engine reads has a byte-level page whose layout is traced to
the routine that reads it, with a confidence level (Confirmed, Inferred,
Unknown), and a parser that the extraction pipeline and the patcher share
([format overview](../formats/overview.md)). The denominators come from the
disc: [disc coverage](disc-coverage.md) counts code bytes inside dumped
functions and entries with a known format class, and
[byte accounting](byte-accounting.md) counts the bytes a parser actually
consumes. The patcher's disc-gated oracles re-decode a patched copy, so a
writer is held to its reader.

**Blind to:** what the code does with the data. A correctly parsed table can
still be indexed, clamped or summed differently.

## Rung 2: port provenance

A port is a Rust function tagged `// PORT: FUN_<addr>`. The
[port catalog](port-catalog.md) derives what is ported and what is reachable
from those tags, and [port provenance](port-provenance.md) asks whether the
address names the routine the code implements: it ranks suspicious pairings by
signals read from the disassembly, the docs and the disc images (a routine that
shares no data address, callee or call edge with the rest of its module; one
given a defining description on two unrelated pages; a tag citing evidence its
routine's dump does not carry; a tag on an address the repo elsewhere says
starts no routine). The ratchet
`check-port-provenance.py --check` fails a commit that adds an unwaived
finding; each waiver says what the bytes showed.

**Blind to:** the arithmetic. A tag can name exactly the right routine while
the Rust under it computes something else, and every signal here passes.

## Rung 3: the retail kernel differential

[The retail kernel differential](retail-kernel-differential.md) runs the
retail routine off the user's disc in `crates/retail-exec`, a user-mode R3000
interpreter with a GTE written from the hardware documentation, beside the
Rust kernel that ports it. Both sides take the same generated inputs (caps and
empty cases included) and one scripted `rand()` stream, and the whole regions
they write are compared rather than chosen fields. Each test opens with a
`differential!` declaration naming the routines it runs; the run records every
entry it executes with the bytes there, and fails the test unless each
declared routine was entered and its bytes are the named image's.

`differential-coverage.py --check` gates the declarations: a test with no
declaration fails, and the covered count, the routine count and the declared
test count may only rise. The share of `// PORT:`-tagged addresses a
differential runs is on the site's landing page, refreshed by
`update-progress-metrics.py`.

**Blind to:** when and with what the engine calls the kernel. A routine that
matches here can still be wired a frame late, fed the wrong record, or never
called; and a routine no test runs is not checked at all
(`port-catalog.py --no-differential --live` lists the live ones).

## Rung 4: retail-compare stills

The [retail comparison corpus](retail-compare.md) seats the engine where each
retail save state stood, through the engine's own load path, and scores the
frame and the observables the state's RAM holds: scene, camera, player,
placements, music, flags, party, enemies. Per-state, per-channel scores may
only rise, and a fall goes through the
[loosening ledger](ratchet-integrity.md#a-baseline-may-not-loosen-silently).
[Capture coverage](capture-coverage.md) takes its denominator from the disc,
so the corpus is widened where no state yet looks.

**Blind to:** time. A state is one instant; a cast's order, length and timing
and a walk's path between two states are invisible to it.

## Rung 5: films

The [cast-sequence oracle](cast-sequence-oracle.md) records one cast on both
sides and pairs frames on the game's own clock (the action state, then the
cast module's phase), and the [walk film](walk-film.md) does the same for a
field walk under a fixed pad schedule. Recordings carry retail pixels and stay
out of the repository, so a film has no committed baseline: a change reports
its before and after on the same command.

**Blind to:** everything outside the recorded stretch, and residuals smaller
than two retail recordings of one cast differ from each other.

## Rung 6: the full-game ladder

The [full-game ladder](full-game-ladder.md) plays New Game to credits with no
human, segment by segment from retail saves at each story milestone and as one
continuous run, on pad input. Its tiers are ratcheted, and a seed set repeats
the run on other random streams.

**Blind to:** fidelity. The ladder proves the game can be finished in the
port; it compares nothing with retail, which is what the rungs below it do.

## Why the differential is the hinge

The lower two rungs are about naming: what the bytes are, and which routine a
Rust function claims to be. The upper three are about behaviour as a player
sees it, sampled where a capture happened to stand. The differential is the
first rung that runs retail's own code, and the only one that reaches inputs no
capture holds. It also settles the rung below it: a differential-run address
is reported as verified by the provenance checker
([a differential is evidence](port-provenance.md#a-differential-is-evidence)),
because the declaration's run proves which bytes executed.

Its findings show the gap it closes. Ports that read right against their
dumps, carried a correct tag and passed their own unit tests still computed
something else: a level-up applier that drew none of retail's `rand()` calls,
an arctan table rounded where retail's truncates, a stat aggregator whose
grant scan had no port behind its heading, a status tick that read the
tracker where retail reads the word
([what it has found](retail-kernel-differential.md#what-it-has-found)).

## What a gate does and does not mean

Each gate fails on its own and is committed as a ratchet that may only improve
([ratchet integrity](ratchet-integrity.md)), and a green gate means only what
that gate measures. A disc-gated test that skips for want of its data is not a
pass, and an oracle whose retail side is the engine's own model cannot
disagree with it. The method, written engine-neutral for another project, is
[the retail-oracle method](retail-oracle-method.md); the seven porting steps
these gates sit under are in
[porting by reimplementation](../porting-by-reimplementation.md).
