# The retail-oracle method

A reimplementation of a console game has one judge: the original running on
its own hardware, or an emulator faithful enough to stand in for it. This page
describes how this project turns that judge into a measurement that runs on
every change, in terms that do not depend on the game, the console or the
engine. Every step names the page where this repository does it, as a worked
example; none of the method needs anything those pages are specific to.

The shape is short to state. Capture moments of the original as emulator save
states. For each one, read what the original was doing out of the state's own
memory, put the reimplementation into the same situation, and score what it
does against what the original did, channel by channel. Commit the scores as a
baseline that may only rise. Around that loop sit the practices that keep it
honest: an evidence grade on every claim, a register of disproved readings, a
coverage denominator taken from the original's data rather than from the work
done so far, and runs that replay bit for bit.

The parts that read emulator states and disc images are reusable code;
[reusing for other PS1 games](reusing-for-other-ps1-games.md) says which crates
carry and runs them on a made-up game in its worked example.

## Capture retail states

A save state is the unit of evidence. It holds the whole machine at one
instant: main RAM, the GPU's video memory and display registers, the sound
chip's RAM and voice state, the scratchpad. Everything a channel later scores
is read out of it offline, so the emulator does not run during a measurement,
and a state taken once serves every later change.

Four rules make a state corpus trustworthy.

**Name each state by its bytes.** A state file that an emulator slot can
overwrite is not evidence; a copy named by the hash of its contents is. This
project keeps immutable copies under a fingerprint name and a manifest that
maps a human label to the fingerprint, the scene it shows, how it was made and
anything done to it
([save-state library](pcsx-redux-automation.md#save-state-library-immutable-backups),
the manifest schema in `legaia_ram_image::scenarios`).

**Record what was done to the machine.** A state made after a debugger poke,
or on a patched disc, is not the original's behaviour. The manifest names the
poke and the patch, and the scoring leaves such states out of the headline.
Two traps make this subtle: a state replays the memory of the disc that booted
it, so loading it onto a different image keeps the old executable resident;
and an emulator may apply a patch file sitting beside the image without saying
so ([patched-disc taint](pcsx-redux-automation.md#patched-disc-taint)). Read
the resident executable out of the state and compare it with the clean one
rather than trusting the file name.

**Capture by script, not by hand, where you can.** A closed-loop probe that
loads a state, drives the pad, waits for a condition and saves a new state
produces captures that can be re-shot when an emulator or a build changes
([the probe pattern](pcsx-redux-automation.md#the-probe-pattern)). A state a
probe derives from another inherits everything the source carried, pokes
included.

**Choose what to capture from a worklist.** A corpus grows where capturing is
easy, which is not where the reimplementation is weakest. The worklist comes
from [taking the denominator from the disc](#take-the-denominator-from-the-disc):
list everything the game's data defines (scenes, enemies, spells), mark what
the corpus already shows, and capture the rest in order of how much depends on
it ([capture coverage](capture-coverage.md)).

## Seed the engine from retail RAM

A captured state is only an oracle if the reimplementation can be put into the
same situation. Seeding is a reading of the original's memory: the scene it is
in, the player's position, the party, the story flags, the camera, the state
of the fight. The reimplementation then enters that situation through its own
code paths and ticks until it settles.

Three choices decide whether the comparison measures the engine or the
seeding.

**Enter through the engine's own load path.** Writing the original's memory
into the engine's structures field by field tests the copy, not the engine.
This project lifts the state's save block, loads it the way a memory-card load
would, enters the scene and only then places the player where the original
stood ([the seeding model](retail-compare.md#the-seeding-model)). The order is
the original's: the game copies the save over its live state before the scene
initialises, so an entry script sees the saved story.

**Seed what is history, and let the engine compute what is not.** Some values
are the result of everything that happened before the capture, which no fresh
entry can reproduce: the camera region the player last walked through, a
random-number stream several hundred draws in, a script paused mid-way. Those
are read from the state and installed. Everything else is left for the engine
to compute, because that is what the channel is there to test. Each installed
value is an admission that the engine is not being measured there, so it gets
named in the report.

**Keep an unseeded state in the report.** A state the seeding cannot reach is
a finding about the instrument. It stays in the output with its reason, so the
corpus never silently shrinks to the states that happen to work.

## Define scored channels

A channel is one observable, read from both sides and scored in `0..1`. Scores
with an explicit tolerance and falloff (exact within a few units, linear to
zero at a stated distance) let a near miss count for something and make the
number stable under small, irrelevant noise
([channels](retail-compare.md#channels)).

Two families of channel do different work.

**State channels** compare values read from memory: scene, mode, position,
camera, music track, flags, the combatants of a fight. They are cheap,
headless and precise, and a channel that fails names the subsystem to look at.
Each needs the address it is read from in the original documented with its
evidence ([retail observables](retail-compare.md#retail-observables)).

**The image channel** compares the frame the original was displaying, cropped
from video memory by its own display registers, against the engine's frame.
It is the channel a player sees, and it catches what no state channel names: a
missing effect, a wrong palette, a mesh drawn that the original culled. Two
rules make it a measurement rather than an impression. Render the engine's
side the way the console did, at its native resolution with its rasterisation
rules, so the comparison is not scored against an upscale
([the native target](../subsystems/renderer-port.md#the-native-target),
[the image channel](retail-compare.md#the-image-channel)). And score with a
block metric that tolerates dithering (the fraction of small blocks within a
per-channel tolerance), not a mean pixel error that one bright object
dominates.

A single frame cannot judge something that unfolds over hundreds of frames. A
time-axis oracle records both sides from the same starting state, pairs frames
on the game's own state machine clock instead of wall time, and scores each
pair ([cast-sequence oracle](cast-sequence-oracle.md#the-pairing-rule)). The
pairing rule is the design problem: the original shows a frame some frames
after it built it, and runs slow frames that the engine does not.

Read the worst states first. A report sorted worst first, with both sides'
values and the side-by-side image, is the product; the mean is the headline.
For each bad state, decide first whether the divergence is the instrument's
(a seeding gap) or the engine's, and keep a catalogue of the divergence shapes
that recur ([divergence shapes](retail-compare.md#divergence-shapes)).

## Ratchet the baseline with a loosening ledger

Commit the per-state, per-channel scores as a baseline. A check run fails when
any score falls below its baselined value, when a channel that was measured is
no longer measured, or when a state that seeded stops seeding. A rise is
folded in by an explicit, reviewed bless
([the ratchet](retail-compare.md#the-ratchet)). The baseline holds scores and
classes only, never pixels or memory, so it carries none of the original's
data.

Two things defeat a ratchet without tripping it.

**The baseline is edited.** A ratchet compares the measurement with a text
file, and the text file can be changed in the same commit. A second gate reads
every baseline at the base revision and in the working tree, and fails on any
value that moved the wrong way unless a ledger records that exact step with a
reason. Ledger entries match one transition and are never removed, so the
ledger is the history of what the ratchets gave back
([a baseline may not loosen silently](ratchet-integrity.md#a-baseline-may-not-loosen-silently)).

**The measurement never runs.** Oracles need data a continuous-integration
runner does not have, so they skip when it is missing, and a skip reports as
a pass. Count skips by reason on the machine that has the data, and fail a run
in which a test skipped for want of an input that machine is supposed to hold
([a skipped test is not a passed test](ratchet-integrity.md#a-skipped-test-is-not-a-passed-test)).

## Grade every claim by its evidence

Every answered question about the original carries a grade naming what its
evidence rests on, and the grade is the weakest load-bearing piece:

- **disassembly**: instructions at stated addresses, branch and store
  sequences;
- **capture**: a save state, a probe log, a measured oracle;
- **decompiled-C**: a decompiler's rendering, a function signature it
  inferred, a label someone typed into the database;
- **inference**: reasoning from surrounding facts or from absence.

The third grade is not a verdict of wrong; it marks a claim nobody has checked
against the instructions, which makes it a re-audit worklist. It deserves its
own bucket because decompilers fail in specific, repeatable ways: arguments
left in registers vanish from a call, stores are reordered or dropped, labels
inside a large function print as calls, and a module imported at the wrong
base prints every address shifted by a constant while reading plausibly. This
project's catalogue of the ways it has been misled is
[decompiler artifacts](ghidra.md#decompiler-artifacts-that-have-produced-false-claims);
the grades and their definitions are
[the evidence column](../reference/re-settled-threads.md#the-evidence-column).

Port from the instructions, and treat the decompiler's output as a pointer to
where to read. A "no writers found" claim deserves the same bar as a positive
one: a search for one encoding of an address misses the others, and a search
over decompiled files misses every function that was never decompiled
([address reference scan](address-reference-scan.md)).

## Keep a falsified-hypotheses register

Every format without documentation invites the same wrong readings, and each
newcomer finds them plausible for the same reasons. Keep the disproved ones,
with three parts each: the tempting reading and why it tempts, the decisive
instruction, capture or count that disproves it, and what is true instead.
Entries are terminal; new evidence that reopens one moves it back to the open
questions rather than editing the verdict, so the trail stays
([do not re-walk](../reference/re-do-not-re-walk.md)).

The register pays off most on the readings that cost the most to disprove: a
data block that looks like geometry, an opcode whose name came from its least
typical caller, a global named after the most interesting of its readers
rather than the primary one.

## Take the denominator from the disc

Progress counted over the work already done always looks good: the share of
the functions someone documented that are also ported says nothing about the
functions nobody has opened. Count instead against what the original contains.

- **Code**: for each executable image (the main executable and every overlay),
  the bytes covered by analysed functions, with the uncovered runs listed
  largest first and classified by shape (data, padding, code nobody has cut
  into functions) ([disc coverage](disc-coverage.md)).
- **Data**: for each archive entry, the bytes some parser actually consumes,
  and the shape of the rest, so a format nobody has walked shows up as a large
  residue ([byte accounting](byte-accounting.md)).
- **Captures**: for each scene, enemy, spell and formation the data defines,
  whether some corpus state shows it ([capture coverage](capture-coverage.md)).

The same discipline applies to negative results. "Nothing calls this" from a
search over decompiled functions only covers the functions that were
decompiled; a search over the raw bytes of every image, in every encoding an
address can take, is what turns "no caller found" into "no reference exists".

## Make runs replayable

An oracle failure is only actionable if it can be reproduced. Make the engine
deterministic under a recorded input stream: a small text file of pad
transitions per frame, plus the seed, replays to a bit-identical trace of
engine state. A test that runs a stream twice and compares the traces, and
two companions showing that a different stream and a different seed each
change the trace, keep determinism from regressing without any data from the
original ([determinism replay](determinism-replay.md#determinism-gate)).

With that in place, every bug report becomes a replay file, every soak-test
finding reduces to the inputs that matter, and a fixed finding stays as a
regression ([soak harness](soak-harness.md)). A replay that starts from a
captured state ties the engine's trace back to the original's, which is what
the time-axis oracles stand on.

## Where to start

The pieces have an order, because each makes the next one cheaper:

1. A state reader for your emulator, and a manifest that names states by
   their bytes. Nothing else works without it.
2. One state channel (scene and position) over a handful of states, with the
   report sorted worst first. Its first findings are usually seeding gaps.
3. The ratchet, its loosening ledger and the skip tally, before the corpus is
   large enough for a silent regression to hide in.
4. The image channel at native resolution.
5. The evidence grades and the falsified register, which cost nothing to
   start and only grow in value.
6. The disc-derived denominators, once there is enough work for a count over
   done work to start flattering.
7. Determinism replay, as soon as the engine has input.

## See also

- [Retail comparison corpus](retail-compare.md) - this project's corpus
  oracle, end to end.
- [Cast-sequence oracle](cast-sequence-oracle.md) - the time-axis sibling.
- [Ratchet integrity](ratchet-integrity.md) - the loosening ledger and the
  skip tally.
- [Reusing for other PS1 games](reusing-for-other-ps1-games.md) - the crates
  that read emulator states and disc images, and a worked example on a game
  built in code.
