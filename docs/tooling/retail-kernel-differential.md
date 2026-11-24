# The retail kernel differential

`port-catalog.py` asks whether a `// PORT:` routine has a tag and a caller, and
[`check-port-provenance.py`](port-provenance.md) asks whether the address names
the right routine. Neither asks whether the Rust **computes what the bytes
compute**. The retail-compare corpus does ask, but only for what a save state
happens to show.

The differential asks directly. `crates/retail-exec` is a user-mode R3000
interpreter. Its disc-gated tests load `SCUS_942.54` and any overlay off the
user's disc, seed RAM with generated inputs, call the routine, and compare what
it wrote with what the Rust kernel produces. There is no decompiled C and no
hand reading between the two sides. No Sony bytes are committed; without
`LEGAIA_DISC_BIN` the tests skip. Where it sits among the project's other
checks, and what it cannot see, is [the verification ladder](verification-ladder.md).

```bash
cargo test -p psx-retail-exec --profile dev-fast --test integration
```

## What it can run

Integer instructions, `hi` / `lo`, unaligned loads and stores, RAM and the
scratchpad. `Cpu::hardware_zero_div` runs a `div` by zero as the chip does, for a
routine with no `break` guard, where that result is what ships. COP0 carries
`mfc0` / `mtc0` as register moves and `rfe` as the status-stack pop; any other
COP0 operation, `syscall` or an undefined GTE command stops with
`Stop::Unsupported`. A call can be
replaced with a `Cpu::hook`, which is how `rand()` is scripted so both sides
draw one stream. `Cpu::calls` records the calls made, which is how a test proves
it reached the arm it claims.

### The GTE

`psx_retail_exec::gte` is the coprocessor 2: both register files with their
read and write quirks (halfword sign and zero extension, the `SXYP` push,
`IRGB` / `ORGB`, `LZCS` / `LZCR`, `H` reading back sign-extended), every
command, the 44-bit `MAC1..3` accumulator with its per-step overflow flags,
the `FLAG` saturation bits with bit 31 over `30..23` and `18..13`, the UNR
divider and the MVMVA far-colour bug. It is written from the hardware
documentation (nocash's psx-spx), not from the engine's GTE, so a defect the
two shared could not cancel in a differential. A test seeds and reads it as
`Cpu::gte`; `Cpu::gte_ops` counts the commands a call ran.

Its own evidence is twofold. Unit tests pin spec values worked by hand
(the divider's table and quotients, projection, saturation, the MVMVA bug).
`gte_capture` replays the GTE register files in the PCSX-Redux state library:
where a state's outputs are an `RTPS` / `RTPT` of the vertices still in
`V0..V2`, or an `NCLIP` / `AVSZ` of its FIFOs, re-running the command from the
state's own inputs lands on the captured outputs, projection and divide
included. That is the emulator's GTE, not silicon, so it checks the
interpreter against a second implementation of the same documentation.

`gte_engine` runs the engine's GTE against it with no disc. The live
perspective divide `gte_divide` matches over every `SZ`, and the light-source
rows' `field_light::nccs` matches `NCCS` on random matrices, colours and
normals. The register-file `render_kernels::gte::Gte` matches on `NCLIP`,
`AVSZ3` / `AVSZ4`, `SQR`, `OP` and `MVMVA`; its `RTPS` / `RTPT` keep the
struct's own q19.12 scale and its colour ops do not follow the hardware's MAC,
IR and FIFO steps, which the test reports. Only the `gte-replay` tool runs
that struct.

### Leaves reached by a jump

A dispatcher's table-driven leaf (`FUN_80043390`'s handler banks, entered by
`jr`) is never a `Cpu::call` entry. `Cpu::watched` names such entries: the run
notes each landing on one in the ledger, so a test that drives the dispatcher
can declare the leaf it reaches and the declaration is checked like any other.
`Cpu::probe` observes any instruction without replacing it: the probe sees the
registers, memory and GTE as they stand before it runs, which is how a test reads
what a routine hands the projector (`shatter_draw` reads the vertices at the
shatter renderer's `RTPT`).

## How to add a routine

1. Read the disassembly and find what it reads and writes. Seed those bytes with
   the shapes a game reaches, including the caps and the empty cases.
2. Compare whole regions rather than chosen fields. A write the port does not
   know about is exactly what this is for, and a field-by-field test cannot see
   one.
3. Assert the run reached the interesting arms, so a seed change cannot leave
   the test passing on nothing.
4. Open the test with its declaration, `differential!(scus(VA))` or
   `differential!(prot(ENTRY, VA))`, naming every routine it compares
   ([below](#declaring-what-a-test-runs)). A test with no declaration fails
   the coverage gate.
5. When the test needs a `World` step that is not public, expose it beside the
   arm it drives: a `#[cfg(feature = "test-drivers")] impl World { pub fn ... }`
   block in that arm's own module, not a new method on the shared
   `world::arm_drivers` / `world::field_test_drivers` / `world::test_drivers`
   traits. Those traits are closed to new methods, so tests for different arms
   never edit the same file.

## Declaring what a test runs

Every `#[test]` in `crates/retail-exec/tests/` opens with one of two macros
from `tests/common.rs`:

```rust
#[test]
fn the_passive_sweep_matches_retail() {
    differential!(scus(0x8004_CE2C));
    // ...
}

#[test]
fn the_port_arctan_table_is_the_retail_one() {
    not_a_differential!("compares the port's arctan table with the SCUS table at 0x8006F4C8; runs no routine");
    // ...
}
```

`scus(VA)` names a routine of `SCUS_942.54`, `prot(ENTRY, VA)` one of PROT entry
`ENTRY` (extraction index); several go in one call. Each argument is an integer
literal or a `const` of the test's file (or of `common.rs`), so the declaration
can be read without compiling anything. A `const` in the test's own body shadows
the file's, as in Rust; a name the file defines with two values and the test does
not define itself fails the gate, since the scan cannot tell which one it means.

An address an engine crate exports (`legaia_engine_cast::seru_magic_ticks::THEEDER_TICK`)
is not an argument: write the literal. `--check` names any argument a `use`
brings in, even where the file also defines a `const` of that name, since Rust
resolves the import and the scan would read the file's value; and any `const`
whose value is not a literal (an engine const, a cast, a sum). The scan does
not follow imports into other crates by design: that means compiling, or
re-parsing the engine's const expressions, and a declaration is meant to be
read as typed. The run's guard still checks the literal against the bytes.

**The run checks the declaration.** `Cpu::call` notes every entry it is asked
to run in a thread-local ledger (`psx_retail_exec::ledger`), with the bytes at
that entry. The macro plants a guard whose `Drop` runs when the test ends
without a panic and with the disc present, and fails the test unless each
declared routine was entered on the test's thread and the bytes entered are
that image's - the `SCUS_942.54` bytes at the VA, or a run of the named PROT
entry. A declaration naming the wrong address, or the right VA in the wrong
overlay, fails the test that carries it. A test may enter more than it declares
(a setup call); it may not declare what it did not run.

**The count needs no disc.** `scripts/ci/differential-coverage.py` reads the
declarations and the `// PORT:` tags and reports how many tagged addresses a
differential runs:

```bash
python3 scripts/ci/differential-coverage.py              # covered / ported, per routine
python3 scripts/ci/differential-coverage.py --uncovered  # tagged addresses no test runs
python3 scripts/ci/differential-coverage.py --check      # the gate
```

`--check` fails on a test with no declaration, a test file `integration.rs`
does not mount, an argument it cannot resolve, or a figure below
`scripts/ci/differential-coverage-baseline.json` (`ported_covered`, `routines`,
`tests_declared`; each may only rise). It runs in the pre-commit hook and in
the Repo Gates job. A fall is a loosening, and goes through the
[ledger](ratchet-integrity.md#a-baseline-may-not-loosen-silently). The report
also lists declared routines that carry no `PORT:` tag, which is either a
missing tag or a helper the test runs beside the routine it compares.

The same declarations feed two other tools. The [port catalog](port-catalog.md)
carries a `differential` column, a dashboard figure and a `--no-differential`
filter (the ported routines no test runs, with `--live` the live ones). The
[provenance checker](port-provenance.md#a-differential-is-evidence) counts a
differential-run address as verified.

**Writing the declaration for an existing test.** A test written before the
macros, or one whose author wants the declaration derived rather than typed,
can take it from a run:

```bash
python3 scripts/ci/differential-coverage.py --record           # the suite, with the ledger on
python3 scripts/ci/differential-coverage.py --propose          # ledger vs declarations
python3 scripts/ci/differential-coverage.py --propose --apply  # write the missing ones
```

`--record` runs the suite with `PSX_RETAIL_EXEC_LEDGER` set to
`target/differential-coverage/ledger.tsv`, one line per test and routine it
entered (a libtest thread is named after its test). `--propose` names each
routine's image by finding its entry bytes in `extracted/` - `SCUS_942.54` at
the VA first, then the PROT entries the test file loads, then every PROT entry
- and prints the line each undeclared test should open with; `--apply` inserts
it. The ledger holds the executable's bytes, so it stays under `target/`. The
proposal declares everything the test entered, setup calls included; trim a
routine that is only there to seed the one compared.

**What the credit means.** It is per address. A passing differential proves
which routine ran and that the Rust kernel the test calls computes what it
wrote; it does not prove that every `PORT:` site carrying the address sits on
that kernel, nor that every arm was reached (step 3 above is what does that,
and [branch coverage](#branch-coverage) measures it).

## Branch coverage

A credit says a routine ran, not how much of it. Branch coverage measures the
rest. Built with the `branch-cov` feature and run with
`PSX_RETAIL_EXEC_BRANCHES` set, the interpreter
(`crates/retail-exec/src/branch_cov.rs`) walks each routine the ledger records
statically from its entry, over memory as the call found it, to enumerate its
conditional branches, then notes the direction each one takes while the suite
runs:

```bash
python3 scripts/ci/branch-coverage.py --record        # the suite, recording on (disc)
python3 scripts/ci/branch-coverage.py                 # summary over the PORT-tagged routines
python3 scripts/ci/branch-coverage.py --worst 30      # lowest-covered routines
python3 scripts/ci/branch-coverage.py --addr 801D0748 # one routine, branch by branch
python3 scripts/ci/branch-coverage.py --worksheet 801D0748  # with tag ranges and scope rows
python3 scripts/ci/branch-coverage.py --check-scope   # the scope file's rows
```

Each branch is **both** (taken and fell through), **one** (only one way) or
**unreached**, and direction coverage is `(2*both + one) / (2*branches)`. The
walk follows fall-through and both arms of a branch, and does not follow calls,
so a routine's branches are its own. A `j` stays in the routine when it lands
inside the extent already reached, or further on with no other routine's frame
setup (`addiu sp,sp,-N`) between - the compiler rotates loops to test at the
bottom and jumps forward to the test; a `j` behind the entry or past a frame
setup is a tail call. A `jr` through a register other than `ra` is a jump
table the walk cannot resolve. The compiler keeps tables in `.rdata` and the
case arms inside the routine ahead of its epilogue, so a routine holding one
takes every conditional branch from its entry to its epilogue (the first
`jr ra` whose delay slot reaches the furthest address reached), stopping early
at another routine's frame setup when the `jr` was a tail call through a
function pointer. Against the SCUS routines with a Ghidra dump, the counts
agree except where the dump itself is short of the routine.

Not every branch is a test gap. The `scripts/ci/branch-coverage-scope/*.toml`
files take arms out of scope, one row per range with a verdict and a
disassembly-grounded reason: `unported` (no port implements the arm, by decision), `unreachable` (no
game state reaches it) or `cannot-drive` (the interpreter cannot run it). The
report gives coverage over all branches and over the **ported arms**, the
branches no row covers; what stays below 100% there is the worklist, arms a
port claims and no test exercises. `--worksheet` lists a routine's branches
beside the address ranges its `PORT:` tags cite and the rows that cover them.
A slot-B VA holds different routines in different images, so a row at a
VA-aliased entry also gives `image`, the image hash `--worksheet` prints;
without it a row applies to every image at its entry. The rows are split by
area across the directory's files so that branches adding rows in parallel
touch different files; `--check-scope` flags a range two rows both claim.

Driving the retail side down an arm is half of it. A branch counts once the
retail routine takes it; the test still has to compare that arm's output with
the Rust that ports it, or the arm is exercised and not checked. The table holds addresses and hashes, no
instruction bytes, and stays under `target/branch-coverage/`. It is a
measurement, not a gate.

## What it has found

The first routine in it, the stat aggregator `FUN_80042558`, showed its grant
and revoke scans had no port at all, behind a module heading that tagged them
ported. Nothing in the engine put a Talisman's summon or a final-form Ra-Seru's
summon on a spell list ([the mechanism](../reference/glitches/summons-and-jette.md)).
It also showed the record's live stat window was only rebuilt for HP and MP, so
an accessory's percent boost never reached the status screen's ATK.

The battle seed `FUN_80053CB8` showed the port clamping after the flat equipment
bytes where retail clamps before them. The level-up applier `FUN_801E9504` showed
the stat jitter was never enabled on any host, so every level-up took the curve's
exact mean and drew none of retail's eight `rand()` calls a level, and that the
six battle stats had no cap on the way up.

The melee resolver `FUN_801EC3E4`, run whole against `World::land_melee_hit`,
showed that a clip's `+0x7A` selector inflicted nothing on a normal swing (Venom,
Toxic, Rot and Curse were only rolled on enemy specials) and that a Stoned
defender still took damage when its Stone was the raw status word a cast writes.
It also showed the Mystic Shield stopped no plain swing and halved instead, the
block breaker's quarter damage and the drain clip had no port, the shared
finisher's no-damage floor had been wired onto a routine that has none, the Spirit
percent wraps at a byte in this routine alone, and a block the roll grants spends
the approach terms even when the verdict is then overturned. Seated with the
clip's power run, the strike cursor, the stream and its entry table, it showed
the blocked hit's copy of the apply look-ahead skipping itself in action states
`0x6E..=0x77`, where the port landed the total early.

Its second round, with the Seru absorb staged, the `0xB3` cell, the vibration
row, Terra on seat 2 and monsters striking each other, showed a monster
attacker's hit computing the apply look-ahead that both copies skip for any
attacker past seat 2, the grunt sounding on a hit that does not land the total
or on a broken block's pose (retail's grunt sits behind the apply arm and
reads the pick ahead of the pose override), and the Mystic Shield's rumble
written only for a party attacker.

Its third round compares what the hit leaves on the seats: the impact tint
`+0x04`, its blend `+0x0C` and selector `+0x21F`, the attacker's cue cursor
`+0x1F6`, and the defender's facing off seeded ground and body pairs, with the
bearing `FUN_80019B28` run live. It showed the Mystic Shield's tail turning the
defender onto the attacker on a hit that lands: every arm of the routine falls
into `0x801EEB60`, whose test is the shield word and the attacker's clip alone,
so a block breaker that overturns the shield still turns the struck monster.
The port turned it on the blocked path only. The same tail stores a root speed
of `-4` into the first entry of monster record 0's clip table (`0x801EEBA4..
0x801EEBB4`), the idle of the monster on seat 3 whoever was struck, which the
round compares too.

The anim node tick `FUN_80047430`, driven through World seams with the range
poll given, showed two arms with no port: the Seru-absorb puff behind a
natural end (effect `0x14` at a downed seat committed to idle while a Seru is
staged, and cue `0x1B` once an action), and the walk gates on
`DAT_8007BD71`, which stop the hit and effect-script walks once the battle-end
signal is up while the cue walk runs on. A cast-sequence film across a `rikuroa`
kill that absorbs then showed the port puffing one clip early: the arm reads
`+0x1D9`, which is the reaction channel's entry while a reaction plays, so the
fallen monster puffs when its death entry ends and leaves it on idle, sixty
vsyncs after the knockdown's end, where the port had tested the action
channel alone ([the film](cast-sequence-oracle.md#the-seru-absorb-puff)).
The same film, run four hundred vsyncs past the end-of-action gate that raises
the end signal, shows no ring cue on either side after it and the win pose's
camera agreeing once the pose's `rand()` pick is seated, so on this fight the
walk gates have nothing observable to stop. No film yet shows a hit or effect
walk the gate cuts.

The item applier `FUN_800402F4`, run on a monster's casts, showed a class-1 group
heal on the monster side walks the four monster seats `3..7` and stops short of
the summon seat, which the port's walk included. No arm of it pushes a damage
number, so a use that restores nothing shows no `0`.

The spoils arm of `FUN_8004E568` showed the port paying EXP and gold for every
seat in the formation, where retail pays for a seat that died and was not
captured: a Seru the Ra-Seru took pays nothing, and neither does a monster that
ran. The level-ups draw `rand()` ahead of the drop walk, and the port drew them in
the other order, so a level-up changed which item dropped. A Stoned member takes no
EXP and is not counted in the split, a fallen member's Items Up still counts because
the EXP loop has stood them up at 1 HP before the scan, and a level 99 member keeps
earning up to the record's cap.

The monster flee roll `FUN_801EC0DC` read the fleeing monster's INT back off its
record under the scripted fight's profile even in a random encounter.

The round opening showed four more. A fallen actor draws its initiative roll, and
the port skipped it, so a fallen member shifted every later roll. The written turn
order of Tetsu's spar and of Cort's final form had no port. The formation roll's
spread divide has no guard: level sides divide by zero, the chip leaves the raw
draw as the spread, and the port neither drew it nor added it, so an evenly matched
fight had no ambush and no pre-emptive strike. The status tick read the typed status
list where retail reads the word, so a poison a cast wrote never drained; poison
wore off on a timer retail does not have; and Life Grail and Magic Grail restored
nothing in battle.

The action validator's kernel matches over every arm. Its battle host read the raw
word alone, so a member poisoned by a hit had nothing a cure could target.

The sound driver runs through [its own harness](../subsystems/audio.md#the-keying-differential),
because a track takes minutes of vsyncs and the SPU has to feed `ENVX` back: libsnd's `FUN_80062F98`
over a state's RAM, its registers applied to the port's `Spu`. It found the note-off keyed on channel
where `FUN_8006688C` keys on program, a pan or volume event missing voices already released, the
sequence volume re-reading a moved pan, a pause leaving tails sounding, and the allocation scan reading
live voices where retail reads the flush's own record, which had voice numbers apart in every score.

The bag's accessor family, run as a stream of mixed operations, showed normalize
`FUN_800423E0` to be one pass of two cursors rather than a whole compaction: one call
can pass a hole by, and the slot order after it differed from the port's. The walk
step's wall probes `FUN_801CFE4C` match, except that a negative coordinate wraps
through the grid's masks where the port called it a wall.

The monster picker `FUN_801E9FD4` found a walk that matches nothing drawing a physical
target and a full swing budget the retail seat never gets; a zero-gauge monster with
no base gauge never arming its swings; the interrupt arm of monsters `0x28..0x2A`,
`0x7F`, `0x85` casting at members who had already acted or were Stoned, with the wrong
denominator on `0x29`; monster `0xB6` casting past its five round cases; Tetsu's
post-amble missing whole; and a pass that failed on its target forgetting the cast it
had stored. The generator's spell ids stop at the table's end, and a second test holds
every shipped record to that bound.

The encounter roll `FUN_801D9E1C` found the anti-repeat latch cleared on every scene
entry, where retail's one global stands across them; the counter, the rate shifts, the
story-group walk and the region's battle setup all match. The death arm of
`FUN_8004AD80` and the MP-cost leaf `FUN_80035394` match. The death arm's table test
leaves out row `0xBF`, which holds the neighbouring table's `[9, 0]`: retail would add
item `0` there and the port refuses it, and a second test holds that no shipped monster
reaches such a row.

The applier's battle path `FUN_800402F4`, over every item the battle menu offers,
found Medicine and Antidote reading the typed status list where retail masks the word;
MP items restoring a fallen member, where the arm gates on HP; the MP readout `+0x178`
unwritten; a Phoenix restoring a flat quarter where retail restores `40% + rand %
(max / 8)`, drawing `rand()`, clearing the status word and the gauge boost and
restoring the live gauge; and Fury Boost landing on a fallen member. The whole arm ran
at the commit: the action SM calls it at state `0x3F`'s timer (`0x801E4134`), so a heal
showed on the bar before the user moved, and any `rand()` it drew came ahead of the
band's. The Point Card was not offered at all, on a catalog note that no item reaches
its arm: it does, by id, through state `0x3E`. Selectors 9 and 10 match.

The Arts entry `FUN_801D0748` state `0x50`, driven with a pad script, found a pool that
cannot pay the first command leaving for the review screen with an empty string, where
retail stays in entry. The round sweep `FUN_801D88CC` stops at seven slots, so a fifth
monster seat keeps its spent gauge and its stream; the port reset it. The magic-level
check `FUN_801E70BC` matches.

The walk step `FUN_801D01B0`, with its helpers, found the port reading the held mask
where everything after the slide resolver reads the resolved one, so a skid along a wall
neither turned the player nor took the diagonal's quarter off. The cut also has a camera
mode gate the port lacked, and the terrain slow tile had no port at all.

The queue builder `FUN_801EED1C`, with its learn check and the Super applier, over the real
art banks, found the Spirit price charged after the walk instead of gating it: an art the
gauge could not pay still ran, for nothing beyond an empty gauge. A learn verdict of "not
known" dropped nothing; the Miracle row landed only on a whole-string match, not on the
walk's run, and left the later groups reading arrows it should have covered; and a limb
rotted after the entry kept its swings.

The Spirit block of the action state machine's cleanup arm, `0x801E5D60..0x801E5E8C`, run on
its own off the disc against the Done band, holds the port to the arm that pays an art's
price: the accumulator comes off the gauge ahead of the action's own `+8` or `+0x20`, the two
AP Boost passives and the cap, with the gauge left alone until then.

The state machine itself, run from its entry one pass per state against
`battle_action::step` over a host that logs the element calls, the cast-effect driver
and the cues (`action_sm`), found the end-of-action sweep clearing only the ghost
bits, where retail also clears every slot's impact selector and, on a living actor or
a party seat, the render flag and the capture byte; the Done band recomputing the
turn order with nobody standing; the strike loop's terminator skipped on a pass that
held a swing in flight; the Attack x2 refill demoting starters past the sixteenth
entry; the cast band reading the sentinel for the byte two behind a cursor under 2,
where retail reads the category or the target; and the summon route leaving before
the stream cursor steps, and opening for a monster caster. The opening states, the
Spirit band, the Seru band, the strike return and the wipe scans with the victory
re-pick match.

The anim commit `FUN_8004AD80`, from its entry down the monster death arm, found the
defeat cue silent in a scripted fight whose formation holds a second monster, where
retail's gate opens on the formation's second cell. Its art install, the
SpecialStarter's voices and the shout call match, and so do the actor pass
`FUN_8004CE2C`'s clip-tag arms, Jette and Dohati hooks and status CLUT uploads, the
frame driver `FUN_80046A20`'s exit pass and rumble decay, and the presentation tick
`FUN_80050120`'s scene ramps.

The World-only arms run through `world::arm_drivers`: the commit's reach snap, the
Mystic Shield arm, the frame driver's backdrop spin and the fade's sink match. The
Jette count, which clears the formation's second cell once Jette and his clones are
down, had no port, and the fade's lone-fight latch was skipped behind a staged Seru
absorb or a capture. Running the rest of the state machine found the War God Icon's
pair counter never reset at the walk's arrival, so only a battle's first pair replayed;
state `0x40` leaving a pass early, without its hold on a reacting target, its category
clear or the Fury Boost bar; the spell plate never sent out at `0x3E`; and the Point
Card's frame left whole.

The shared cast kernel `FUN_801DD0AC`, its scale and finisher, and the two wrappers
`FUN_801DD4B0` / `FUN_801DD6B4`, on both branches, found no cast waking a sleeper or
freeing a Numbed seat, where the finisher strips `0x0F80` as the melee resolver does;
a monster's hit on a monster taking the defender's element off the party table;
spell XP and the Seru debuff landing in a special battle, where the tail returns
first; spell XP banked off a Stoned victim whose hit the finisher had zeroed; and a
module's own roll ignoring both seats' Venom and Toxic, drawing a third `rand()` on
the bypass wrapper, which draws two, and handing a Stoned victim the whole hit.
A third test calls each capture-class special the way its module's damage site does:
it reads the site's `jal` (which wrapper) and its `addiu a0, zero, imm` (which power)
off the module image, calls that, and holds `World::enemy_move_predamage` on the move
id to it, half the victims in the Spirit stance. It pins the routing from a move id to
its module, baked power and wrapper, which the wrapper tests take as given.

The Auto assembler `FUN_801F0450`, run whole over the real art banks, found the tail's
Spirit gate drawing `% 7` where the bytes draw `% 14`, and the pool dropping every
command past the sixteenth entry, where the scratch has room for all thirty-two. The
delegated arm's redirect `FUN_801DB124` takes no seat: it reads `ctx[+0x274]`, so the
roll it follows stands dead or not, and its draws and its rewrite land on the turn
picker's last pick. The spend loop can spin for ever in retail; the test holds the port
to ending the spend there.

Running the builder after it found the round playing Auto's arrows raw, where the seed
tokenizes them as it does an entered string, and the learn check `FUN_801EFBFC` teaching
on every use, where a seat on the Auto flag draws `rand() & 0x1FF` on each art and learns
only on a zero. The head of the same builder found the AI companion drawing twenty
`rand()`s a turn, reading the leader's status word off the typed list only, and giving up
on a row after sixteen target redraws, where retail's redraw has no bound. Her HP and MP
pinned at 200 and the string the spar's third round forces on the lead had no port.

The slot machine's kernels in PROT 0975 found a resolved win ending every reach and hot
mode, where a reach stands until its jackpot lands and hot mode ends on a one-in-150 LCG
draw; the winning line taken by best payout, where retail keeps the highest symbol id; a
stop always reaching its landing row, where the budget can leave the reel short of it; and
the per-frame LCG draw missing, so a stop's landing never depended on when it was pressed.

The seed's retarget `FUN_801E7320` found a confused monster in a party of two misreading
its own band's first seat as a party seat, the gate reading the typed list where retail
masks `0x380`, and a confused cast always crossing sides: in retail a group cast crosses
one time in three and comes to nothing the other two.

The field VM `FUN_801DE840`, one instruction at a time over every distinct instruction in
the disc's scene scripts with its calls stubbed, finds the operand widths of the port's
`step` right everywhere it can compare them, and one wrong exit: `4C 80` left the parent's
PC on the child records, where retail walks past them. A call that lands on an empty
function pointer is why `Cpu::stub_outside` counts toward the step budget.

The slot machine's state machine `FUN_801CF0D8`, whole sessions frame by frame from its state `0` on a generated pad script (`slot_machine`), found the port betting on Cross alone, where state `1` takes Square, Cross or Circle (`andi 0xe0`), and with no entry fade: retail holds state `1` off the pad and off the coin gate for thirty-two frames while `DAT_801D3794` runs down.

The routine's discarded `rand()` at its head had no port, so a feature roll never depended on when the bet was placed; state `0`'s own frame draws the LCG and steps the frame counter, and without it every landing sat one draw early and the tally paid on the other frame parity. The frame that dropped state `4` back to idle read the submenu edge and the coin gate a frame early.

Baka Fighter's combat tick `FUN_801D3F44` and round SM `FUN_801D3468`, run whole rounds at a time in the pool walk's order, found the opponent committing on a cooldown of its own, with the player free to read its clip and counter it, where retail's opponent throws only in answer to the player's commit and blind to it. The "cooldowns" those arms store are the rumble levels and pace nothing. Exchanges ended at the booking, so an attack's second strike never landed, a struck fighter's pad was never masked, and the hit streak never cleared when a reaction clip ended; it climbed across whole rounds, and every later hit paid the streak's bonus. The player could throw during the round banners, the push-back and the camera's follow had no port, and neither did the rumble.

The dance floor's handler `FUN_801D1358` with its award routine, run over whole songs for every dancer, found the port holding each dancer to one press a beat, where only the latch and the move clip hold one, and rivals scored about a fifth low for it (a hands-off qualifier's two rivals `245` / `208` against `295` / `253`). It also found a Circle miss counting one, the feed's schedule running on through move clips and refusing to move with no stock, free play's rivals never riding the lead's gauge, the free-play Disco King's schedule cut off at the table's end, and his loop-named moves locking him.

The dance state machine `FUN_801CF470`, whole songs from the clocks' reset to the results with the dancers ticked behind it (`dance_floor`), matches the port's clock and found what the hosts did around it. The door entry opened every mode on the short song, where the song-end test reads the mode alone (`0x801D00D4..0x801D00FC`) and only the how-to is short, so a contest ended fifteen seconds early.

The hosts ran a game frame every vsync on a delta of one, where the hall runs one every third vsync on a delta of three: the judge read the phase three times as finely, and a rival's feed took three turns for retail's one, so rivals closed on different totals. The finale started two game frames early, the frame that reached the song limit running state `0xB`'s spawn and state `0xC`'s first add at once, and the wipe's pause flag `DAT_801D5130` had no port, so rivals went on scoring to the results where retail stops judging at `0x349`. The song each mode streams was picked by length; it is the mode's (`0x801CFAC0..0x801CFB80`), free play rotating three by a counter in the save.

The field VM's state arms close with the host answering what clear RAM answers: `4C 53` holds on the staged mode switch `FUN_801D65D8` where the port stepped past it on one frame, and `4C EA` runs once and advances where the port re-ran it at its own PC. Every other arm the first sweep could not compare agrees on both polarities of its test. Its write-up above is the sweep's stub world; a test seeded with no actor and no item holds the skip paths.

The hooked fish `FUN_801D4004` with the gauge's reel arms matches over whole fights at every species, rod and frame step.

The fishing line's clip `FUN_801D56E4` matches `clip_segment_2d` over random segments against the line's own rectangle and random ones (`fishing_line_clip`), the rectangle read off the scratchpad halfwords `0x1F800388..0x1F80038E`.

The Seru side-effect stager `FUN_801F3D3C` found the port rolling the scripted bypass on the first enemy the cast targeted, where the bytes read the formation's first record whoever is targeted and whether that seat stands, and comparing a group cast's first target, where the bytes walk to the first seat with HP left and stage with no compare when there is none.

Reading the shop's quantity steppers for a differential turned up the word they step on: the window kernel's cursor auto-repeat `FUN_80035C24` had no port, so no list in the game repeated on a held direction, and buying ninety-nine of anything took ninety-eight presses or nine and eight. The steppers' own arithmetic reads the same as the port's. The inn's restore op `4C 82` writes exactly the two HP and MP words the port writes.

The Best Equipment row `FUN_801CF88C` / `FUN_801CF760` found the port taking the last copy of a piece by zeroing its count, where the take helper zeroes the id and leaves a hole, and walking the bag in id order, where a tie goes to the first slot the walk reaches. The impact arm of `FUN_801E09F8` found an enemy special's Rot leaving the Auto strings alone, where a limb newly rotted empties both, as the melee arm does. The action overlay's pool scans and recentres match.

The fishing cadence recogniser `FUN_801D3DB4` found a different walk from the port's: retail passes over one slot that does not match between two steps, spends the template's window as a budget over visited slots, matches within nine frames rather than ten, and on a match leaves the last-button latch unwritten. The waiting arm of `FUN_801D26CC` (`0x801D3198..0x801D382C`) found the recogniser running every frame in the port, where it is called only from inside the band body, so its ring stands still through a held band; the strike draw is taken every frame, held or not; and the countdown runs on below zero at a short line. The power meter of `FUN_801CF3BC` state `0x14` steps `0x20` per frame-step from zero, and the hosts ran it at four times that on a figure the code called unpinned.

The Muscle Dome's arena re-entry `FUN_801CEA6C`, the leg's rows `FUN_801D1184` and the settlement `FUN_801D0F60` match the port's `DomeContest` over whole contests. Reading them for the differential found what the contest does around that arithmetic: the arena copies the `0x2000`-byte SC block out on first entry and settlement copies it back, keeping only the lead's `+0xE8` / `+0xEC`. The port kept the legs' HP, items and gold, and an Expert or Master fighter's stripped gear never came back. It also set the prize latch `0x6CB` itself, which the arena only reads and the lobby's winner's scene sets after testing it.

The dialog pager `FUN_801D84D0`, call by call over generated pages at every frame step and window size, confirms the row window's pacing, scrolls and taps, and found a page turn on `0x48` typing the next box on the second call. Retail's state `9` puts the row count back to three, state `0xA` collapses the box over `0x1000`, and `0x801F273C` keeps it gone for `0x18` frame-steps, about two thirds of a second at the field's step. A press on a box's opening call does nothing there, where the port took it as a skip.

The lure's flight and in-water move in `FUN_801D26CC`, with the tail's record, replaced what the port had reconstructed: the record is the lure's distance from its spawn point, and line comes in by the press, not by holding. An inn's `4C 82` and `3A` match.

Baka Fighter's damage kernel `FUN_801D3B18`, called on its own over every attack type and streak (`baka_leaves`), found the port zeroing a special's damage, where the kernel runs the same arithmetic for it: a power of `0` leaves the streak term `(streak - 1) * 0x40`, taken off a struck seat with HP left. No duel reaches it, since the special is thrown at a seat already at `0`.

Baka Fighter's chrome timelines, each called as its caller calls it with the widget emitter, the announcer and the tint hooked (`baka_chrome`): the round banner `FUN_801D5C7C` and the all-clear announcer `FUN_801D21FC` match, and the intro title card `FUN_801D59D4` found the subtitle's four-frame flash carried as white where the routine pushes the colour word `0xFF`, red alone. The sprite-actor draw callback `FUN_801D67F0` found the port rounding the fade level up before its shift, where the level is read unsigned and takes a plain `>> 4`, a brightness one step high on most frames. The impact pair `FUN_801D4DF8` and the all-clear fighter's tick `FUN_801D6F18` match.

With the clip starter's hook raising the CD-XA word `_DAT_8007BC20` as the real starter does, the announcer found the port starting both of its opening lines on one frame, so the second replaced the first and channel `0x0A` never sounded; the cabinet's own run (`baka_cabinet`) found the beat `0xFB` moving on with a line still playing, where retail holds at full black.

The slot machine's payline pass `FUN_801D3380` and the Muscle Dome's contest-start restore `FUN_801D0ED8` and leg staging `FUN_801D1510` match.

Baka Fighter's round score `FUN_801D2A28` matches, and tracing what feeds it found the channel unwired: no host installed the two bonus tables, the match bonus row was never seeded, and the running total never reached the cabinet, so neither secret opponent could ever appear. Retail also scores only the player's round win, off the player's HP.

The same search for setters with no live caller found the bag's window selector `FUN_8004313C` unwired: the live game never left the full window, so a lone character saw the whole array.

The bearing `FUN_80019B28`, run over the `i16` plane against `bearing_12bit_approx`, disagreed on
about half of all inputs by one unit: the port computed the arctan table at `0x8006F4C8` by rounding
where the retail table truncates, so 1037 of its 2049 entries stood one high. Every battle facing,
approach and separation nudge read through it, and the motion VM's face-target ramp and the field
NPC facings used a rounded `f32` `atan2` with the same error. The one table and the one integer
bearing now sit in `legaia_engine_system::trig`, and the separation pair `FUN_80050BB8` matches
`push_apart` over it. Reading the bearing's callers found the move VM's sub-op `0x3A` measuring from
`+X` (`atan2(dz, dx)`), which mirrored every angle it wrote.

The take-item fallback `FUN_800430AC` walks the present-party list at `0x80084598`; the port walked
the whole roster, so a script could take an accessory off a member who was not in the party. The
shout selector `FUN_8004C140` matches over every roster id and action constant.

The Use list's applicability probe `FUN_8003043C`, run with the validator live beneath it over every
field-usable item, found the port answering it from the catalog's effect where retail asks the validator.
An MP restore lit for a fallen member, a stat seed for a member already at the stat's cap, and an arts
book for a party without its reader (classes `0x0B..0x0D` match one roster id); the Doors never read
system flags `5` / `6`. The Items screen's confirm used the same stand-in, so it could use a row the list
had greyed. The probe now runs the ported validator over the roster records, and the party heal's walk
reads a present id of `3` as the dead quad: out of battle the validator's pointer arrays stop at three,
so that read is stale stack in retail. The spell twin `FUN_8003053C`, which greys a Magic row and refuses
its cast, matches over the same seeding.

The floor sampler `FUN_80019278`, with the kind-2 lookup `FUN_801D5630` live beneath it, matches on every
tile of the grid, ramps and stairs included. Off it the port called the floor `0`, where retail halves the
sub-cell toward zero (a position in `-64..0` stands on tile `0`), wraps the object cell through its masks
and reads the corner block on into the descriptor table or the object map.

The Arrange kernel `FUN_801D64A8` matches the port's sort over every window. Reading its caller for the
differential found the port sorting only the screen's rows: the bag never moved, so the next Items screen,
the battle window and the save all kept the old order.

The menu's stat block, the seeder `FUN_801CF5D0` and the summer `FUN_801CF650` the Status page, the Equip
compare columns and the shop preview draw, matches `menu_stat_block` over the disc's equipment table.

The whole turn picker `FUN_801DABA4`, with the monster picker, the redirect and the Counterattack roll live
beneath it, found four defects. The picker's Numb tail had no port: a Numbed monster cast or struck where
retail's turn arms nothing. A walk that matched nothing stored category `3` in the port, so the redirect
re-rolled and the Counterattack coin was flipped where retail's stale `0` does neither, and the roll never
read the stored category at all. The side scans counted a seat down only on a Stone the status tracker held,
so a cast-written Stone left a petrified side's monster deciding a turn. The sweep, the tie pick and the
item refund match. The Confirm-path sale `ShopSession::try_sell` priced per unit off the shop's stock; it
now pays the stepper's `(item-table price * qty) >> 1`.

The round end's waker `FUN_801F45A4` read only the raw half of the port's status word, so a Numb the
status tracker held drew no `rand()` and never woke; it also wore off on a three-round count retail does
not have. The waker now reads and clears both halves, and Numb lasts until it or a hit's strip clears it.

The partition-2 record dispatcher `FUN_8003BDE0` matches over every such record of every scene MAN on the
disc: the record it locates, its C1-any and C2-all story-flag gates, and the script entry it installs.

Comparing the monster picker's stored category itself, rather than reading a stored `0` as a strike, found two
more. A pass whose later cast entry fails its MP test keeps what an earlier entry of the same walk stored
(`0x801EA51C` sits ahead of the `+0x1DE` store at `0x801EA528`); the port dropped it, so the retry's empty walk
left the seat without the cast retail plays. And monsters `0x97` / `0x98` under a `0x380` status clear a stored
cast to `0` and keep the ring, or keep any other category and skip the ring (`0x801EAD58..0x801EAD80`); the
port ran neither.

The round tick run over six rounds with an ailment the port's status tracker holds, beside the same bits in
retail's word, found Sleep (`0x800`) and the `0x380` group wearing off after three rounds in the port, where
retail's tick never clears them: like Numb they last until a hit's `0x0F80` strip, a cure or the battle's end.
The port's hit strip also left a tracked Confuse standing; it now clears the whole family.

The camera-register zone ramp's handler `FUN_80037018`, read from its disassembly, matches
`zone_ramp_tick` over every gate, store width, the retire arm and the zero-span trap.

The party walk of the per-frame actor pass `FUN_8004CE2C` (`0x8004CE6C..0x8004D018`), which the battle
template's tick `FUN_8004DA00` tail-calls on every battle frame, had no port. Retail's melee status arms,
an enemy special's Venom and Toxic and a cast's raw bits are written whatever the target wears, and this
walk takes off each bit the wearer's guards cover, fallen or not; so in the port a Poison, Curse or Numb
Guard stopped nothing that reached the word that way, and Master Guard only stopped Rot. Maximum AP, which
the same walk uses to hold a living wearer's gauge at 100, did nothing. `World::tick_battle_passive_sweep`
now runs the walk at the head of each battle frame and matches it over every guard combination, with the
word split between the port's raw half and its tracker. The infliction-time guard tests the port keeps
(Glare, the Rot arms) are retail's own: PROT 0940 tests Stone and Master Guard before it writes the bit
(`0x801F7054..0x801F7070`), and the Rot arms read both guards ahead of their draw.

The motion VM `FUN_8003774C`, run leg by leg frame by frame over all six ops, found the tile walk `0x47`
taking one step of the whole frame's distance where the arm takes the frame step's count of steps, each
re-reading the deltas: a frame that closed the dominant axis stopped there instead of starting the
diagonal, and an approach mode's second axis waited a frame. It also ended a leg on the frame that landed
on the target, where retail ends it on the next frame's first iteration, so every scripted walk and the
script parked on it resumed a frame early. The compass walks, the compass ramp, the hold and the
face-target ramp match.

The tile board's walk machine `FUN_801EF2B0` reads the cell under the player flat, `row * width + col`
with no column bound, in its arrival, its pad-octant read and its exit fade's event-tile test. A board
under five columns seats the player off its east edge, where the port's bounded lookup found nothing:
it skipped the arrival and faded the event tile retail keeps. Water Hazard's arm 3
(`FUN_801F7298` in PROT 0956) holds after its countdown until every seat its target code names has
settled, a live seat idle and a dead one on its down clip or faded out; the port's director passed
on the countdown alone, so the band could end while a hit seat still played its reaction.

Holding the Stone and Curse arms to the module images found that no cast module calls the applier
`FUN_800402F4`. Stone Circle (PROT 0941) writes no status at all and is a damage cast, where the port
rolled Stone on every party seat and drew a `rand()` for each; Curse and Curse All write `ori 0x1000`
with no roll and no guard test, where the port rolled for them. Glare repeats the class-9 roll inline,
guard test first, as the port had it.

The ambient motion VM `FUN_80038158`, run tick by tick over generated streams of every op that stays
off the GPU, the model loader, the floor sampler and the ramp pool, with the two overlay probes and
`rand()` answered alike on both sides, found two defects. A restamp of the requested move from the
default-move record left the pair `+0x88` / `+0x5C` an earlier `0x02` had written, where retail stores
the record's byte to both, so the height pass and the ambient lifetime read a stale word. And the
directional steps probed the player even with the VM's own flag bit `0` set, where the arm skips the
call (`0x80038484..0x80038490`). The wander, the home step, the facing ramp, the touch-woken wait and
the flag, cue and bit ops match.

The battle frame's gauge loop `FUN_80046A20` (`0x80046A80..0x80046D28`), against the rows both hosts sync
each battle frame, found the HUD reading the tracker's statuses where the loop and the status element
`FUN_8002C2E4` read the whole `+0x16E` word: a Curse a cast wrote, an enemy special's Venom or a rotted limb
greyed no gauge and drew no element. The rows now carry `World::raw_status_word`. The range metric
`FUN_8004E2F0` matches `range_metric` over every seat pairing, reach and size class.

Seating the port's half of each `+0x16E` word the way a game reaches it, the tracked kinds in the tracker
and the rest in the raw half (`common::seat_status`), turned the monster picker, the turn picker and the Auto
delegation differentials red: a Confuse the tracker held was invisible to the picker's own `0x380` arms and to
the Counterattack roll, and a Stone or Numb it held did not veto the Auto fill (`0x801F04FC`). The same split
reader sat in the seed's retarget gate (`0x801E2C8C`), the victory re-pick's `0x404` test and the win pose's
status tier (`0x8004E828`). All of them now read `World::raw_status_word` or the host's `status_word`. Reading
the seed's gate whole showed the engine running the retarget `FUN_801E7320` twice for a raw Confuse, once where
it arms the monster's turn and again at the seed; retail calls it once (`0x801E2CA0`), so the seed hook no
longer repeats it. The cast modules' seat view had the same split: a module's Stone test and its cure masks
(Vera's `+0x16E` mask) saw the raw half alone, so a cure left a Venom a hit had dealt. The view now carries
the whole word, and a tracked ailment whose bits a module clears leaves the tracker.

The shop's quantity steppers, buy `FUN_801DB7F4` and sell `FUN_801DBD94`, match the port's sessions and the
runtime's purse and bag writes over generated pad scripts, bounds and stacks. One arm differs by choice: at 99
held retail opens the buy stepper with a bound of `0`, buys the one copy it starts on and takes the gold for a
copy the add then trims, where the port's runtime refuses the buy. In play the list refuses a row built at 99 before
the stepper opens ([shop.md](../subsystems/shop.md#screen-composition)), so the arm is latent.

The field interact probe `FUN_801CF9F4`, run over crowds of actors around the probe point, confirms the port's
box and finds the pick differ: every hit overwrites the caller's `+0x98` link, so the last actor in list order
is talked to, where the port takes the nearest. Switching to slot order changed the casino's `koin1` door
`P1[56]` into a talk with `P1[63]`, so the port keeps the nearest until a capture reads that scene's spawn
order.

The text-width measurer `FUN_80035F04`, with its expander, matches `Font::measure` over strings with author
escapes, colour codes, accent cells and newlines at both glyph pads, and disagrees with `Font::layout` on most of
them: the layout draws `0x5E X` and `0xFF X` as glyphs and gives `0xCE` its sprite's advance or none. The balloon
spawner's and the world-map labels' widths read the layout and now read the measure. The battle-intro name
banner `FUN_801D9D3C` matches `enemy_menu_rows` and `layout_enemy_menu_rows` over the measure. On four labels that
cannot sit side by side inside `6..0x13A`, retail's relax-and-clamp loop never settles and spins for ever, where the
port stops at its bound.

The SFX cue ring, run frame by frame - the enqueue trio `FUN_80035B50` / `FUN_80035BAC` /
`FUN_80035BD0`, the aging loop of `FUN_8001698C` (`0x80016AF4..0x80016B58`) and the drainer
`FUN_80016B6C` up to its loop's exit, with the libsnd key calls logged - against `SfxScheduler`
and the `SfxBank` voice choice, found one defect: a sustained runtime row with no voices keyed
nothing in the port, where the drainer releases the held run before it tests the count
(`0x80016DC4..0x80016E00`). The ring, the one-shot cursor, the sustained run and its latch match.
The drainer goes on past the ring into the frame's vsync pacing, which rewrites the frame step,
so the test stops it at `0x80016EA4`.

A spell cast from the pause Magic screen, the single-target and group commits of `FUN_801D9594` /
`FUN_801D9280` against the Magic sub-session and `apply_spell_outcome`, found the port healing a
catalog placeholder (`MP * 8`, `MP * 6`) where the applier `FUN_800402F4` caps the heal at its
level-scaled amount, so 122 of 125 casts wrote different records. The port also charged MP for a
cast on a member already at full HP, which retail's confirm refuses through the validator.

The field walk-regen tick `FUN_801D0B90` (PROT 0897) matches `tick_walk_regen` over the present party's
records: the step counter, the HP / MP / AP Walk bumps with their clamps (a current above its cap snaps
down), a fallen member's bump, and the Incense window's wear-off edge with its event pointer and flag.

The cast arms whose host draws `rand()` for them, each run through its arm off the module image, found four
draw-order defects. PROT 0941's Steal (`FUN_801F730C`) opens a party victim's draw with a `rand()` gate
(`0x801F779C`) the port skipped, and a monster robbing a monster raises a line (`"`" + name + the module's
`0x801F83E0` text) the port left blank. PROT 0940's split (`FUN_801F78B8`) throws its coin only for `0xAE`,
where the host drew for `0x50` too; it copies the clone's HP off the caster before the coin (`0x801F8090`), so
a weakened caster left both halves on 1 HP in the port, and it zeroes the weakened caster's MP. PROT 0964's
Element Change (`FUN_801F88EC`) re-draws only while the draw repeats the last, where the host pre-drew 33. PROT
0956's Water Hazard (`FUN_801F7298`) draws each seat's damage and then its Venom before the next seat, where the
host rolled the whole row's damage first. The attack ring's builder `FUN_801D8A88` and step `FUN_801D8D00`
match.

A second pass over the same hosts found three more. Water Hazard's monster leg sweeps the whole monster row
whatever single seat the code names, where the port struck the one seat and rolled neither damage nor Venom on
that leg. PROT 0915's Mushura chase trades its target with `rand() % 4 + 3` redrawn on a repeat, over all four
monster seats, where the port drew once from the other living seats. PROT 0907's Nighto skips the resist throw
for an immune target (`0x801F6C20`), where the host drew it anyway. PROT 0922's crystal deal and PROT 0955's
Void Accessories and PROT 0957's Death Game deals match (the first by test, the other two by their disassembly).

PROT 0934's Ozma, arm `0x18` run pass by pass against `World::summon_stager_tick` with `FUN_801DD0AC`
live beneath both sides (`ozma_ladder`), found the arm drawing its spray's six `rand()`s at its head
where the port seated the spray after the module's tick, so every ladder hit rolled off different draws;
and each ladder roll going without the finisher's spell XP and spirit, where every `FUN_801DD0AC` call
runs `FUN_801DDB30`. The same bare roll sat in Mule's wave, and the fold threw away what the pre-hits had
banked: it took `module_hit_xp` and then overwrote it with the inline grant. Kiss of Death's arm 4
(`kiss_of_death`) matches `kiss_of_death_tick` on both legs, the refund and a Stoned victim's skip.

Mule's arm 16 against the same seat (`mule_wave`) found the wave walking every monster seat to `ctx[+1]` and
testing HP inside the loop (`0x801F7DEC`), so a fallen seat takes the skip path, losing its staged clip
(`0x801F80A0`) and riding the lift; the port walked the living row only. Its draws and rolls match. A sweep of
the slot-B images (`summon_roll_sites`) finds every `jal FUN_801DD0AC` in `0903..0966`, thirty-two in the
images' own code and two in inherited tails, and holds each to the port routine that rolls it through the
finisher. The encounter counter's scene-entry top-up (`0x8003AC78..0x8003ACB4`, `encounter_top_up`) matches.


The walk-on dispatcher `FUN_801D1EC4`, idle half, over every field scene's real `.MAP` and fallback window
with the grid-mark refresh `FUN_80017BEC` run first (`walk_on`), found the map init's fallback stamp
(`FUN_8003AEB0`, `0x8003AFA8..0x8003B018`) with no port: it ORs `0x400` into every `.PCH` kind-1 tile, 2317
cells over the disc that the `.MAP` and the refresh leave clear. The sub-area window sweep reads that bit as
"owned by the init sweep", though no spawnable object's anchor is among those cells. The port's dispatch had
skipped the `cell & 0x600` filter in its place, which only held while those cells went unstamped; with both
stamps in, the filter stops no table tile on the disc. The kind-1 arm spawns on any non-zero gate (`0x8003BDF0`), where the port
spawned on gate `1` only, and resets the clip base on every hit, gate `0` included. The crossing and re-poll
arms now run as `walk_on_crossing` / `walk_on_repoll` and match over every trigger tile in both lock states.

The scene-init object sweep `FUN_8003A55C` (`object_spawn`), over the same scenes with the kind-1 lookup live
beneath it, matches `parse_map_objects` joined with the lookup on all 2291 binds: which objects spawn, the seat,
the floor rung, the angles and the record. It also holds the port's own bind list
(`object_script_bind_objects`: seat, height, angles, record) and the object contexts `World::seed_object_channels`
spawns from it over the scene's real MAN against the sweep, in order: one context per bound object, script id
`+0x50`, 855 of them on a record an earlier object binds, and the five `edbalden` objects whose record lies past
the MAN's count on the default record (see [one context per object](../subsystems/field-locomotion.md#one-context-per-object)).
The region scan `FUN_80017FBC` under the attribute refresh `FUN_800180EC`, and the camera zone query
`FUN_801DBA20` over the box the refresh latched (`region_scan`), match `refresh_region_attributes` and
`zone_query` on every field scene's region table, at tiles on and around every record edge, off the grid and in
world-map mode.

The battle cue router `FUN_8004FE5C` (`sfx_cue_route`) matches `route_sfx_cue` over the ring, the dedupe word,
both tinted legs' category byte and the CD-XA voice leg. The animation cue walker `FUN_800508DC`
(`anim_cue_walk`) matches `walk_anim_cues`, and its tally showed what the host dropped: each character cry the
walk reaches bumps the seat's record `+0x98` before the mute and CD gates (`0x800509BC..0x800509EC`), the word the
records page prints as Knockouts. Nothing in the port wrote it, so the page read `0` for ever.
The death arm's credit `0x8004B184..0x8004B29C` (`defeat_credit`) had no port either: a monster's death bumps
the record `+0x9C` of the party seat holding the action, once, behind the arm's own `+0x1DC` bit 3. That word is
the records page's Monsters Defeated, not the magic-rank counter it was once read as.
The combo readout's fold `0x801E88D4..0x801E89D8` (`combo_records`) completes the records page: while the cluster
is up and a party seat holds an Attack, the member's Max Damage and Max Hits words keep the larger of themselves
and the string total `DAT_8007BD14` and the push count `ctx[+0x273]`. Neither had a port, nor did the push count
and the total the action seed zeroes (`0x801E2C3C..0x801E2C4C`, with the cast readout cursor `ctx[+0x262]` the port
also carried on across actions).
The escape timer's HUD actor `FUN_801D2EBC` (`escape_timer`), run over whole countdowns with random clock deltas,
frame steps and busy frames, matches `EscapeTimer::tick` and `EscapeTimerHud::step`: the counter, the flags it
fires, the phase machine and its kill bit, the digits and the ink.
The casino coin counter `FUN_801F0ADC` (`coin_counter`), driven frame by frame with generated pad words over
every purse and bank shape, matches `coin_exchange` on the cells, the cursor, the hold, the ceiling, the sale and
the stings, and found the two flag calls the port had no arm for: opening raises system flag `8` and clears the
flag the opening script names (`FUN_8003CE9C(_DAT_8007B450 + 2)`, `0x801F0B74..0x801F0B8C`), and a sale raises
that flag (`0x801F1000..0x801F1010`), so the script that opened the counter could never tell a sale from a walk-off.
The follow camera's per-frame kernels (`camera_follow`), the ease `FUN_801DB510` with the composer
`FUN_801DAB90` beneath it and the focus edge clamp `FUN_801DAA50` after it, match `compose`, `ease_step` and
`clamp_focus` over generated parameter blocks of every mode and sweep nibble, attribute boxes, floors, live
poses, the hold and force-ease bits and the script focus override. A mode-1 / mode-2 block over a box with no
width, or a sweep over one with no depth, reaches the composer's `break` guards; the port reads that divide as
`0`.
The overlay colour tween `FUN_801DDC20` (`colour_tween`) found the port taking the ramp's progress as a word,
where the bytes take `clock - delay` as a halfword (`0x801DDCC4..0x801DDCD8`): a negative delay with the clock past
`0x7FFF` lerped from a progress `0x10000` too large. The field VM's screen-effect arm seats delay `0`, so no shipped
script reaches it. The SCUS fade actor's tick `FUN_80025000` over its step `FUN_80020C14` matches `tick_fade_ramp`
at every phase. Loaded from a template through `FUN_80020B00` and played out, a fade shows on every vsync what
`FadeState` shows, including some 40000 vsyncs past its ramp, where retail's halfword duration count wraps positive
and the port's stays at its end: the channels sit on the target either way. A zero duration differed. The loader's
`div` (`0x80020B84`) has no `break` guard, so the R3000's quotient ships, `-1` for a non-negative difference and `1`
for a negative one, and the first tick clamps onto the target; the port had divided by one, and a frame step above
one then overflowed the channel. No shipped template has a zero duration.
The two scans of a caster's learned-spell list in the battle overlay (`spell_list_scans`), the killing blow's
already-known Seru test `FUN_801E91E8` and the summon return's "No effect." pass `FUN_801F3C34`, match over lists of
every length and every gate once the first compares as the bytes do: `id - 0x80` is a word (`0x801E92B0`), so an id
below `0x80` goes negative and matches no token, where the port's byte subtract wrapped it onto a token of `0x80`
or more. A Seru id is small and a list holds spell ids, so no game reaches it.
The field VM's context stores (`field_ctx`), every distinct instruction on the disc run on a context of generated
bytes with `step` beside it, found three: op `0x4B` writing its keyframe count over `+0x6D` (the face-rotation byte)
where the arm stores `+0x6C` (`0x801E08AC`); op `0x23` keeping the half-tile bit in its two tile bytes `+0x8C` /
`+0x8D`, which the arm masks with `0x7F`; and op `0x3E`'s minigame warp clearing `0x80000` on the context running
the script, where the arm clears it on the player object (`0x801E07BC..0x801E07D8`), so a door's warp left the
player's walk lock up. A field the port leaves to its host is counted rather than compared, since the host there is
inert; what the test holds is every store `step` makes itself.
The seat `4C 51` is the op the port hands its host whole, so a second pass steps it on the real `World`, as a live
placement channel's op, and compares the tile on the context and on the drawn seat, the party-bank bit, `+0x62`,
`+0x5C` and the stored heading. Every seat matched but the hide box: the host returned early on tile `(127, 127)`, a
guard left from when the op started a walk, where the arm writes it like any tile (`0x801E1880`, `0x801E1898`) and
the cull hides the actor. `town01`'s blackboard tour parks the villagers round the map that way and re-seats them
with `CC <id> C3`, which the talk runner then ran on the talker's own context; both now land on the placement. The
player arm keeps the guard, unmeasured against the follow camera.
The separation driver `FUN_80051078`, run over the actor table against `World::tick_battle_separation`
(`separation`), found two. Its gate is each seat's colour word `+0x04` (`0x800510B4..0x800510DC`), where the
port read HP: a fallen member lying on the stage and a monster still in its defeat fade push and are pushed, and
a body a cast has hidden is not. And it walks retail's seven seats, party `0..=2` and monsters from `3`, so a
fifth monster at seat `7` is never separated; the port walked its own compacted slots and took it in behind a
smaller party.
The action SM's anim-rate restore `FUN_801E93C8` (`anim_rate_restore`), run over every party size and formation
with the acting seat on a plain or an art clip, or a monster whose staged record's `+0x87` is up or down, matches
the gate and the `+0x21C` latch clear, and found the same seat walk: it restores retail's seven seats, so a fifth
monster at seat `7` keeps its rate, where the port restored its own compacted slots. The arts slow-motion arms of
`FUN_8004AD80` that the restore undoes walk the same seven (`0x8004B740`, `0x8004BB74`), and the port slowed every
actor; both now walk the table's seats. The monster arm's record table is `0x801C9348`, ten words ahead of the actor
table.
The action SM's HP-bar settle check `FUN_801E7250` (`hp_bar_settle`), the capture takedown `FUN_801E7824`
(`capture_takedown`) and the side-band transfer sequencer `FUN_801F17F8` (`stream_slot`, its libcd calls logged
against the steps `StreamSlotSm` hands its host) match over every formation, target code and stage.
The cast camera script `FUN_801DC0A0` (`spell_cam`), run case by case with the tween builder hooked and the group
aim `FUN_801DCEAC` live, against `spell_cam_inputs` and `spell_cam_case`, found the input builder sizing a slot
target with the party radius: retail reads the target's node `+0x58` (`0x801DC35C`, `0x801DC4B8`), so a monster or
Terra framed at the wrong depth on cases `2` and `4`. It also built the group from the monsters not hidden, where
the aim counts a monster by its colour word `+0x04` (`0x801DCF54`): one faded out after its death left the
centroid, and one frozen in the ice stayed in it. The nineteen cases' shots and hand-offs match. A group with no
drawn member, or a slot code on cases `9` / `0xB` naming a seat not drawn, stops on the aim's `break`.
The auto-command preseed `FUN_801DA34C` and its write-back `FUN_801DA59C` (`auto_command`), over every seat, roster
order and gauge pair, and the effect-placement clamp `FUN_801DF570` (`approach_clamp`) match.
Two more host-handed ops are stepped on the real `World` against where it keeps their writes. The tint `4C 81`
lands in `actor_tints` in all three forms (the running placement, `CC <id> 81` on another, `CC F8 81` on the
player) as retail leaves `+0x74` / `+0x78`; the tile warp `0x23` lands its tile, world pair and walk byte on the
context it names. What the first pass still counts as left to the host is the floor sample and the cull a warp
calls, and the cross-context ops whose target state the port keeps off the context: the rotate `B8` and arc `C3`
legs' halt bit and ramps, `4C C3`'s re-seat on the placement, and the player-aimed `3E` / `4C DA` / `4C E3` stores.
The per-art attack camera `FUN_801D71B8` (`attack_camera`), every character, art id, selector and animation cursor,
matches `attack_camera_framing`, and found the art `0x1A` arms' writes to the target's additive fade `+0x226`
(`0x801D7BCC..0x801D7C2C`, cleared at `0x801D7408` / `0x801D7768` / `0x801D7C4C`) with no port: the target of a
Super Art's opening window never dimmed under it, and the tint pass's `+0x226` arm read `0` for ever.
The framing cases of `FUN_801D5854` against `framing_case_pose` and the pure poses (`battle_framing`) found case 7's
midpoint truncating where the bytes floor (`0x801D662C`) and its pull-in missing the staged id `0x11`; case 8 off Close
skipping its target arms even in a scripted fight once the results word `_DAT_8007BD2C` is up (`0x801D69CC..0x801D69EC`),
Normal not walking the live yaw on by `4 * step / 3` at the battle-over signal (`0x801D6978..0x801D69A4`), and the
scripted tail off Close (`0x801D6E84..0x801D6EDC`, live pitch and height over a fixed depth `0x600`) unported; and the
far framing's box, whose midpoint floors, built over fallen seats where the walk gates on live HP (`0x801D7000`). Cases
0, 1 and 3 match, and so do the prologue's three counters over every case and frame step (`AttackCamCtx::advance`,
the byte wrap past step `0x20` included), case 2's fixed shot, cases 4 and 5 arming nothing, and the redirect of a
seat past the table to case 9 behind the flag-word scrub. That redirect hands case 9 a selector no arm names, so the
far framing's walk then runs between whatever `s0` / `s8` its caller left; no caller passes such a seat. The HUD glide stepper `FUN_801D9BBC` (`sprite_glide`), the ledge-hop probe `FUN_801D1878` (`ledge_hop`), the
typewriter count `FUN_80036044` (`glyph_count`) and the cast dispatchers `FUN_801F1ED4` / `FUN_801F2160` (`cast_dispatch`) match.
The per-frame anim-node cursor `FUN_80047430` (`anim_cursor`) over every rate, frame step, Slowed-idle gate and loop
window found the port's step keeping a fraction retail drops: the cursor is in sixteenths of a keyframe, so a Slowed idle
at rate `2` on a rate-`1` clip does not advance (`sra v1,t1,0x2` at `0x8004772C`). The HP and MP readout ramps of the same
pass, the bystander sweep of `FUN_8004E13C` and the tint machine's arms (`tint_sm`), the pose decoder `FUN_8004998C`'s
blend, centroid and body pair (`pose_decoder`), a held pad among seated moving and static actors (`FUN_801CFC40`,
`walk_step`), the hop arc's tick `FUN_801D5C08` over the Bezier `FUN_801E45BC` (`hop_arc`) and the action effect stepper
`FUN_801DEA50` (`effect_script`) match.

The GTE opened the camera and projection kernels. The camera-relative rotation `FUN_8001CF50` showed the port
building its axis factors from rounded `f32` trigonometry where retail's `FUN_800461A4` / `FUN_8004629C` /
`FUN_8004638C` read the truncated sine LUT and post-multiply through MVMVA: the rounded factor misses the LUT on
three angles in four, and the composed rotation was off retail on three cases in four, by up to four units of
4.12. `camera_view_rotation` now quantises the hosts' radians back to the 12-bit angle and builds from the LUT
(`GteMat3::rot_x_psx` and its siblings), and matches. The register-file `Gte` read `H`, `DQA`, `ZSF3` and `ZSF4`
back as raw words where `cfc2` sign-extends the halfword, clamped `MAC0` on `NCLIP` / `AVSZ` where the register
keeps the low 32 bits, and raised `FLAG` bit 31 for an `IR3` saturation, which the hardware leaves out of it.
`MulMatrix` `FUN_8005B3A8`, the Euler builder `FUN_80026988` (both its ports), the overworld curvature table
`FUN_800271A8` (scene-load, rebuilt and flat), the drop shadow's grid `FUN_800460AC` and packets `FUN_8001C394`,
and the billboard projector `FUN_800195A8` match. One arm of the shadow differs by choice: a near actor whose blob
sorts within ten slots of the front links below the ordering table's base in retail, where the port floors the
slot at `0`; field depths keep the blob far from it.

The cast roll's scale stage `FUN_801DD864`, run on its own (`scale_stage`) against the kernels its port splits it
into over the affinity tables PROT 0898 carries, matches on every seat pairing, both weaken bits, the Spirit stance
and the summon body's magic-level arm, the unlearned spell's slot `0x20` included. Its products wrap at the word
(`mult` / `mflo` at `0x801DD954` and the weaken's `sll` / `addu`), where the port saturated; no roll the game builds
is large enough to tell them apart. The group aim `FUN_801DCEAC` (`group_aim`) reads its extent back as a halfword
before the `0x400` floor (`0x801DD084..0x801DD08C`), so a spread past `0x7FFF` takes the floor; the port floored the
word. The body colour pass `FUN_8004A908` (`actor_tint`), its view transform hooked, scales each depth-cue channel
with a signed `mult` over whole words and a `divu` (`0x8004AAC8`); the port truncated both operands to 16 bits, so
a negative radius or a depth past `0xFFFF` sixteenths coloured differently. All three are out of a staged fight's
reach, and the ports now compute what the bytes compute.

The slot-B stagers, the routine a module's parts reach through move-VM op `0x20`, run arm by arm off their
images (`stager_spawns`): every seat call whole, each seated part's buffer, the module image, the fades and the
`efect.dat` pool spawns, with the context, the actor records and the caller held otherwise untouched. Forty-seven
stagers matched but three. PROT 0959's arm 1 reaches arm 5's tail by `j 0x801F87A4` and seats two more records the
port left off; PROT 0919's and PROT 0941's arm 1 each spawn into the pool (Spoon's effect `0x14`, Steal's 2D puff),
which no port carried. The stager arms that move state (`stager_states`) found PROT 0906's arm 1 storing the summon
seat's tint word `+0x04 = 0x3FF00000`, which the port dropped, and showed PROT 0966's stager arm 4 opening on
Juggernaut's salmon flash and shake word, which the host never raised for Evil Seru Magic: the body's epilogue
runs that word down every tick, the code PROT 0927's has.

The capture-class cameras (`capture_cameras`) run every body a camera director ports, pass by pass from arm 0 to
its end, with the director fed what the host feeds it: the tween builder call `FUN_801D829C` each pass makes, the
countdown word and every hold are compared. Five arms framed something else. Water Hazard's arm 0 and Mystic
Circle's arm 0 turn the caster onto its target before the shot (a seat's live pair, or the party's centroid for
group code `8`) and frame the new heading; the port framed the stale one. Melt Spray's arm 2 and Chaos Flare's arm 2
turn on the victim's display pair `+0x3C` / `+0x40`, not its live pair. Stone Circle's arm 1 writes no focus, so its
cut is on the origin the prologue zeroed. Dead End Crisis's arm 0 lines the party up off `0x80077628` and stands
seat 3 opposite before it frames, which the port did not carry.

The same walk now compares what three bodies write to their seats. Death Game's picked card (`0x801F922C..0x801F9794`)
runs on seats at full, short and wounded HP against `death_game_outcome` and the arm's seat writes, and agrees.
Melt Spray's debuff, on the pass arm 3's countdown runs out, did not: retail cuts each of the ten halfwords by
`(x + 9) / 10`, where the port cut a fifth and turned a stat of `0` or `1` into `0xFFFF`. Terror Scream's caster
bands are held against a census of the AI picker's jump table, the monster records and every formation on the
disc. The summon cameras run on retail's stage seats as well as the walk's own row, and hold the side-band stage
up on each image's poll arm: Palma's arm 3 framed a lift its first monster does not make once that monster is
down, and the band's poll dropped the TR z push Slippery's arm 1 makes while it waits.

The arms that scale a stat or HP by a constant all run against their kernels now, since a magic-number divide
is the easy one to misread: Fatal Decision's outcome arm on every outcome but the item theft (`fatal_outcomes`),
White Shield, Power Charge, Power Up and the surge on the pass that leaves their arm, and the split hits of Hyper
Lightning and Spore Gas on varied rolls (`chain_ticks`). All agree. Terror Scream's turn steal did not: its arm 3
tests the Item category alone, where the port ran Kiss of Death's idiom, which also needs the initiative key.
Kemaro's arm 17 runs on every formation byte (`kemaro_spear`), so its per-species jump table takes every case.
Fatal Decision's whole body also runs pass by pass against `fatal_decision_tick` (`fatal_outcomes`): every arm's
shot, records, cues, message raises, wheel words, icon offsets and seat stores, the steal against
`steal_party_draw`, and the Numb and Stone refund. It found arm 0 zeroing the wheel tick's remainder `0x801F9014`
beside the angle, and arm 2 showing the victim before it hides the caster, so a cast on the caster's own seat
leaves it hidden.

<a id="the-band-shadow"></a>
**The band shadow.** Each capture cast also runs on an engine `World` seated off the same actor records
(`World::drive_arm_capture_cast_module`), stepped through `World::run_cast_module_code` while the band walks
retail's phase, so every layer that writes a seat (director, tick body, band, impact table) is in the comparison.
Retail's HP as each pass opens and the colour words the harness stages between passes (a defeat fade, a settle)
reach the shadow as they reach retail, and every seat `0..7`'s `+0x04` and `+0x21C` must agree. Seats rest on
`0x20080200`, and on one round in four a party member lies fallen on clip `8` and a monster is faded to `0`. It
found the seven-seat walk the band did not keep, the roster-byte and HP-less walks, Guilty Cross's and Mystic
Circle's hides, Void Accessories' show a pass early, the hit tints of Chaos Breath, Mystic Circle and Element
Strike's Wind loop, and party-row tints that missed a seat the hit felled
([the seat walks](../subsystems/cast-module.md#the-seat-walks)). Two things stay outside it, each named in the
test: the creature seat of PROT 0964 and 0966, which a stubbed `FUN_801F19EC` seats, and Death Game past its spin,
whose pad reaches retail alone. A cast whose band leaves retail's phase walk stops being compared at that pass;
most of those are a band holding an arm one pass longer on a clip settle the harness does not stage.

The shadow also compares each seat's status word `+0x16E` against `World::raw_status_word`. For the rolled bits to
be comparable the shadow draws from retail's `rand()` stream as each pass opens, carries every seat's AGL `+0x168`,
gives Glare its one victim, and runs the band's after-pass seam that folds Glare's roll on the pass its arm 2 counts
out (`World::drive_fold_glare_on_its_arm`). A seat the band's own damage roll leaves on other HP than retail's stubbed
wrapper stops being compared for the cast: PROT 0956's arm 2 marks only a living victim. All of Glare's Stone,
Paralyzing Wave's Numb, Kiss of Death's bits and Curse All's mark agree.

<a id="the-summon-band-shadow"></a>
**The summon band shadow.** The summon walk (`summon_cameras`) runs the same shadow beside each summon image: a
`World` seated off the records, its summon stager armed, runs the band's module half
(`World::drive_summon_module_pass`: the creature feeds, the module's pass, the creature and seat writes) on retail's
phase, and every seated seat's `+0x04`, `+0x21C` and `+0x16E` must agree after each pass. The summon band's shown
word is the engine's resting `0` under a lowered marker, where retail writes `0x20080200`. Every run opens on state
`0x34`'s hide loop (`0x801E4B30..0x801E4B6C`, every party seat and living monster hidden), which the module's first
pass finds; the fallen runs also stand the last monster in Stone, so each module's Stone skip is driven. It found
Gilium's arm `0x11` leaving the row hidden and its row strike an arm early, Slippery's arm 8 framing the victim of a
row the band had hidden and its hit tint relighting dead and Stoned seats, Jedo's kill arms missing the dissolve
state, the defeat fade and the spell XP, Aluru's last arm hiding only a dead victim, Ultimate Rave's row loop (it
tests `actor_table[3 + i]` and stores through `actor_table[i]`), and the row hits of Viguro, Gola Gola, Gilium,
Palma and Mule that lit a Stoned seat ([the seat walks](../subsystems/cast-module.md#the-seat-walks)).

The shadow also compares HP and each monster's staged reaction `+0x1DA`. The damage wrapper `FUN_801DD0AC`
answers each seat's calls with a scripted run of rolls (small, middling, past HP) and the shadow's module rolls
take the same (`summon_roll_override`, behind `test-drivers`), so every pre-hit's quartering, floor, `HP - 1`
clamp and kill is compared; the hit arm's own pass runs the band's fold ([below](#the-fold-on-the-hit-arm)). The walk seeds
the group list `0x801F6978` with the living row, fades a felled monster's word out six passes after the kill, and
gates the settle arms on retail's rule (a dead seat once its word is zero, a living one once its clip is). The
restage byte `+0x1DC` stays outside: the engine carries its commit bit as the reaction channel, which a seat with no
clips does not run. Every cast walks retail's phase to its end, the paced bodies included: the shadow reads
Nighto's verdict off retail's fork and resist words `0x801F8534` / `0x801F853C`, and stands Zenoir's creature on
its victim for arm 8's range poll. It found Palma's and Meta's pre-hits knocking a monster down where retail stages
a flinch, Aluru's arms `0x11..0x13` relighting a felled victim, Nighto's and Zenoir's side-band arm missing its step
over arm 3 when the band word is out, Zenoir's hits tinting a Stoned victim and its splash lighting hidden seats,
and Vera's fade arm stepping to 11 where it stores `0xFF`.

The walk leaves seat 3 off the group list on its fallen runs, on both sides, which drives Mule's undivided roll
for a seat the list does not hold. Theeder's and Gizam's side-band arms, which their tick bodies pace and the walk
does not, run in a test of their own (`the_side_band_arm_steps_over_the_band_word_poll_as_retail_does`): each arm
once with the band word `ctx[+0x6D8]` out and once with it standing, beside a shadow world's module pass. It found
Theeder's arm 1 stepping to 2 where retail stores 3.

<a id="the-fold-on-the-hit-arm"></a>
**The fold on the hit arm.** The shadow arms the cast the Magic submenu arms (the
spell record off the executable, on the targets its shape names) and runs the
stager's strike (`World::drive_summon_strike`) on the pass that leaves each
director's hit arm, the pass retail's module lands the same hit in. That pass
compares HP, the staged reaction, the bar delta `+0x10`, `+0x0C`, `+0x21F` and
each struck seat's facing as well. The scripted wrapper roll answers the fold
too, and reads `0` for a Stoned defender on both sides: the wrapper returns
`[sp+0x18] - [sp+0x1C]` (`0x801DD4A4`) and its finisher sets the first to the
second for a defender in Stone (`0x801DE2DC..0x801DE2F8`). PROT 0927 arrives at
its arm 26 with the fold spent, since its stager sweeps (the part calls the walk
does not make) spend it. It found the fold staging a flinch on a Stoned monster
for the eight summons whose hit loop skips Stone whole
(`cast_module_camera::STONE_SKIPPING_HITS`), no turn on the struck row for
Slippery (from the camera focus), Gilium and Mule (from the origin), Meta (from
`(0, 0x200)`), Zenoir's splash (from the creature) and Aluru's victim (onto the
creature), Iota's arm `0xA` re-seating a Stoned monster, and Ozma's ladder
quartering the roll of a seat off the group list, which it quarters only for a
listed seat on a whole-row cast (`0x801F8E6C..0x801F8E90`). PROT 0915's arm `0xC`
runs its reach poll live (`jal 0x8004E2F0` at `0x801F7844`, the battle outcome
byte `0x8007BD71` seeded `0xFF` so the poll does not answer out of reach), with
the creature stood on each monster seat in turn, since the walk plays none of the
root motion that walks it; its chase hits agree.

The shadow also compares each seated seat's ground trio, animation rate `+0x21D` and
render node scale `+0x72`, on dead seats too. It found Terra's arm `0x14` turning and
slowing a felled seat whose fade had run out: retail tests the colour word
(`lw v0,4(v1)` / `beqz`), and the engine's resting shown word is `0` under a lowered
marker, so the port now reads a seat as drawn on its word, or, alive, on its marker. The
shadow runs the retail mode (`WorldToggles::pin_retail`), which matters on one pass:
PROT 0921's exit (arm `0xB`) restores each seat's trio, facing, body radius `+0x58` and
size byte `+0x1F` (`0x801F7F40..0x801F7F98`) but no scale, so a Stoned seat, which arm 9
shrank and arm `0xA`'s hit loop skips, keeps the shrunken scale; the player default
stands it back at unity ([the switch](../subsystems/gameplay-tweaks.md#stoned-foes-keep-their-size)). The spirit gauge and spell
XP stay out: retail's stubbed wrapper never runs the finisher (`FUN_801DDB30`) that
fills them, and the per-hit call pattern that drives them is what the scripted rolls
already pin through HP.

**The creature's seat 7.** The walk runs the installer `FUN_801F19EC`'s creature
stores when a seat arm calls it (the walk stubbed it, so retail's creature never moved
onto the caster): the caster's ground trio and facing, colour word `-1`, mode word,
target, clip, rate `2`, tint state, impact selector, shatter word and the committed-id
ring, the node aside. The shadow seats its own creature when the host does: on a module
with a seat arm (`ModuleProfile::seat_arm`) once the band's pass reaches it
(`module_seat_arm::seat_due`), with the seat pass's held stores landed behind the
installer's resets, and on every other module before its first pass. A side that seats
on another pass is a mismatch of its own. From the seat on, seat 7's colour word, tint
state, status, HP, staged clip, ground trio, rate and scale are compared on every summon
module, but for four arm ranges (`CREATURE_GAPS` in the test): PROT 0905's arm `0xA`
rise, which lands through the camera nudge on the next tick's seam; PROT 0908's arm 8,
where the walk stands the creature on the victim; and PROT 0922's arm 7 and PROT 0927's
arms 12 to 18, whose rate stores wait on the clip cursor `+0x68` a clip-less seat does
not carry.

It found Palma's arm 3
never lifting the row (`+0x36 -= 2 p` a pass while its sweep word is under
`160 * scalar`, then set down, `0x801F70E8..0x801F7144`, `0x801F6FC0..0x801F6FF8`),
Juggernaut's arms 27 and 28 storing no rate on the live row (`2`, and `0` on arm
27 for a monster on its get-up entry), Ultimate Rave's arms 8 and `0xA` storing
none on the victim, and Jedo's arm 30 slowing the row on every fight where retail
does it only on a scripted one (`bnez` at `0x801F86F4`). Over the creature it found
Palma's defeat fade six arms late (arm `0xB`'s exit at `0x801F7CA4`, and the word
zeroed on arm `0xC`'s at `0x801F7D70`, not arms `0x11` / `0x12`); Jedo's creature seated
in the open where retail hides it on the dim word `0x401004` until arm 6's pass, then
slows it to rate `3` on arm 7 and `2` on arm 10 and hides it on arm 26; the seat-arm
colour words of Iota and Ultimate Rave (`0`, the fade-in), Zenoir (`0x20080200`), Vera
(`0x3FFFFFFF` at blend `0x1000`, rate `8`) and Aluru (`0x3FFFFFFF`, hidden); Horn's and
Terra's seat arms storing no blend word, and Terra's arm 4 its `0x1000`; Vera's arm 9
fade; Nighto's arm 7 staging clip `1` on every pass, not on its exit; Gola Gola's exit
stepping the clip back with the bump; Meta's exit staging idle clip `1`; Puera's wake run
off the phase the pass left on (a whole arm early) and its clip steps doubled with the
director's; Juggernaut and Spikefish seating on arm 2's entry where retail seats on its
pass, behind the stream poll; and Mushura's arms 7 and 8 taking their quarter term every
fourth tick where retail takes it on each battle frame's closing tick.

<a id="the-drive-poll"></a>
**The drive poll.** Each image's poll arm (`FUN_8003F2B8(1)` behind its seek
`FUN_8003EAE4(0, slot)`, read off the bytes by `battle_cast_cue::module_drive_arms`)
answers busy for its first three passes, and the shadow world stands its drive in
flight for them, so the host's hold (`World::module_drive_holds`) runs against
retail's busy return. Every module holds as retail does. Arm 2 of PROT 0920
to 0924 drains its own countdown ahead of the poll (PROT 0923's `bgtz a1` at
`0x801F6CAC`), so the poll's first call comes once that countdown is out, and
its busy return needs a seek that outlasts the countdown. The walk answers the
poll busy on its first calls, not its first passes, so those arms are held past
their countdown too. Held there, the host had kept the drain the director's
would-pass reports, which includes the exit's re-arm, and held the arm for a
second countdown once the poll let go; it now keeps the gate's drain alone
(`cast_module_camera::polled_countdown`). Arms 1 and 2 of PROT 0919 to 0924 make
their TR z push behind a `blez` on the module word that branches straight to the poll,
so a held pass whose word is spent pushes nothing (`polled_hold`'s table). A scan of
every summon image's poll sites, each `jal 0x8003F2B8` and each `lbu 0x276`, for a
branch that lands on the poll finds no other camera store a gate skips. The walk raises
the side-band stage again for the first passes the wait arm opens on its word spent, so
the held spent pass runs on every wait arm. The same scan found PROT 0924's arm 1
waiting on the side-band stage (`0x801F6C6C..0x801F6C74`), which the port had left
out.
PROT 0927's arm 2
branches busy to the arm's tail (`0x801F6D8C` to `0x801F6F74`), where the
passing pass falls too, so every pass of the arm hides the party and shows the
live row; the port had stored both on the passing pass only, which left the
monsters hidden for as long as the drive sought.

<a id="host-owned-summon-arms"></a>
**What the host owns, and what the shadow checks of it.** A summon image's arms
split between its director (`cast_module_camera`, compared pass by pass in the
walk above) and the host: the band's stores the director hands over. Branch
coverage of an image credits only the arms the walk drives retail down, so an
arm whose effect is the host's shows as covered on retail's side whether or not
the port's half agrees. The shadow is where the host's half is checked.

| The host's half | Where it runs | What the shadow compares |
|---|---|---|
| The seat-write tables of the walked images | `World::apply_<module>_writes`, `apply_aluru_seat_writes` | `+0x04`, `+0x21C`, `+0x16E`, `+0x14C` on every seated seat; facing on the hit arm's pass |
| PROT 0922's seat writes | `World::apply_puera_writes` | trio, facing, `+0x21C`, `+0x04` of the row and the creature, every arm it carries |
| The pre-hits, Mule's wave, Ozma's ladder | `World::apply_summon_pre_hits`, `mule_wave_hit`, `ozma_hit_ladder` | HP and the staged reaction `+0x1DA` |
| The fold | `World::summon_strike` | HP, `+0x1DA`, `+0x10`, `+0x0C`, `+0x21F`, facing |
| The row's ground trio and animation rate | the seat-write tables | compared, dead seats included |
| The hit tints | `cast_module_impact::MODULE_IMPACTS` | `+0x04` on each seat |
| The side-band and drive polls | `module_code`'s polling hold, `World::module_drive_holds` | the hold, against retail's phase walk |
| Mushura's chase | `World::mushura_chase` | everything above, its reach poll live |
| The creature's placement, walk and clips | `World::apply_module_creature_writes`, `summon_walk_to_victim`, `module_seat_arm` | seat 7's seat pass, word, tint state, staged clip, ground trio, rate and scale on every module; ground and facing synced ahead of each pass |
| Scales of the row | the seat-write tables | the node scale `+0x72` of every seat, the creature's included |
| The part-driven stager calls | PROT 0927's sweeps, `run_juggernaut_stager_calls` | not compared: the walk runs no part |
| The spell XP and spirit gauge | `World::summon_module_roll`, the fold's tail | on the live-wrapper runs (below): every seat's `+0x170` each pass, the caster record's first spell XP slot at the cast's end |

**The live-wrapper runs.** Everywhere else the walk stubs the damage wrapper `FUN_801DD0AC` with scripted rolls,
so its finisher never runs on retail's side. A third run of every body, layout and step on the full row takes the
wrapper live on both sides: retail's kernel and finisher off PROT 0898, and the shadow's `World::summon_module_roll`
and fold, on one spread of seat stats, the caster's record carrying the spell at level 2 (under the level-3 side
effect, whose stager the walk stubs) and one `rand()` stream, re-seated on retail's as the fold opens (the band's
module half draws for the spawns it stages, where this walk's retail runs no part). Each pass compares every seat's
spirit gauge beside its HP, and the cast's end the spell XP the finisher banked. The runs found two defects. The
XP tail's group unit reads the summon seat's target byte `+0x1DD` (`0x801DE51C..0x801DE524`), not the spell's table
shape: Mushura's chase raises it to `9` mid-cast, and the port's single-target unit of `12` tripled its XP. And the
tick bodies that roll inside their tick (Theeder, Zenoir, Swordie, Juggernaut's last hits) wrote their seat row
back whole over the live seats, undoing each roll's spirit fill and status strip; the row now folds back only what
the tick changed (`World::fold_cast_seat_row`).

The images the walk does not run (PROT 0903, 0904, 0906, 0909..0913, 0918,
0919) have ported tick bodies, which their own tick differentials compare; of
their host's half only Theeder's and Gizam's side-band arm runs beside retail
(above), and the tick-body walk below walks both bodies whole.

### The tick-body walk

`cast_theeder`'s walk runs a tick body from arm 0 to its end beside the band's module half
(`World::drive_summon_module_pass`) on retail's phase, like the summon walk, and adds what that walk holds still. Each
layout runs twice more mirrored (the sides swapped, so the creature faces the other half-turn and every sine and
cosine rounding fix-up runs both ways), and every layout runs once on the row as a group and once on one seat with a
monster fallen and one Stoned, the band's timer running into the side-band arm, the creature's clip held off its park,
each struck monster playing its reaction for 250 passes and a kill fading slowly, so every wait on them holds; each image's drive poll answers busy for its arm's first three passes, with the shadow's drive in flight, as in the summon walk. After
each pass the port's phase, the shot it builds (`FUN_801D829C`'s arguments against the run's `camera_shot`), every
seat's colour word, render flag, status word and HP, and each monster's staged reaction and facing must match.

Over PROT 0904 (Theeder) and 0906 (Gizam) it found Theeder's strike word `0x3FF00000` (`lui 0x3ff0` at `0x801F7DA8`,
the full blue flash) stored as `0x03FF0000`, Gizam's Venom bit on every seat its sweep reaches (`0x801F749C`) never
set, since the band feeds that body no roll, and Gizam's struck seats left unturned. Theeder's trail retracts once a pass on both
sides: the World marks each battle frame's first tick (`TheederTrail::set_pass`), so the port leaves arm 13 on
retail's pass, where a fixed count of four ticks a pass held the walk's one-vsync passes four times as long. One place
paces apart on purpose and is walked through: Theeder's arm-12 cone is tested on the port's step-4 ticks, so which pass strikes a seat is not compared, only that each
seat the port strikes retail strikes and that both store the same. Over the summon and Seru bodies whose strikes
turn their seats (PROT 0905, 0908, 0916, 0920, 0921, 0923, 0927, 0930, 0932, 0934) it compares every pass's facing
([the cast-module page's table](../subsystems/cast-module.md#a-strike-turns-the-seat-it-hits)), on runs the summon
walk's fold does not make, and found Zenoir's arm 6 turning neither its creature nor its victim and PROT 0920's arm
10 ending on its countdown where retail waits for the row.

The walk's builder lands each shot on the camera globals at once, as the summon walk's does, and the shadow's
battle camera stands on the globals as each pass opens, so the directors read retail's live yaw, pitch, TR and
focus (`World::feed_seru_cam`). After every pass it compares where the camera is headed: the band's run (its shot,
drift, TR z store and nudge) on the globals the pass opened on, against the globals retail's pass leaves. That
puts the summon directors' camera beside the host's module half, not the director alone, and lets the walk drive
PROT 0917 (Barra, whose arm 1 climbs the TR y global arm 0's shot stored) and PROT 0922 (Puera, whose parts the
walk hands out as it spawns them) through to Done. It found Spoon's arm 8 yaw read off the word after the pass's
drain, where retail reads it as the pass opens (`0x801F84E0`, the drain at `0x801F8520`), and Gilium's arm `0x16`
pull made only while its word ran, where retail pulls on every pass ahead of the word's test
(`0x801F8A74..0x801F8AA0`), through the row wait. PROT 0917 runs on two more layouts, its victim on a seat
the rolls leave standing and the row off the floor, with the side-band stage raised again once arm 1's TR y climb
is through and the creature's word held up on arm `0xF`'s first spent passes. It found Barra's arm `0xF` ending
the cast before the creature had faded (retail waits on its word, `0x801F816C..0x801F8174`) and arm `0xC`'s
stores zeroing the focus, which retail leaves on arm `0xB`'s cut, twice the victim's drop. Every body also runs
on a solo party: `ctx+0 = 1`, the roster bytes `0x8007BD10` of seats 1 and 2 cleared (the group aim
`FUN_801DCEAC` reads a party seat live on its byte), and the shadow seated on one party slot with the monsters
straight after it, its target codes and scripted rolls mapped between the two seat spaces. That drives the loops
over the party after the lead (`ctx+0 - 1`, PROT 0919's `0x801F8408`, PROT 0930's `0x801F7C18`). Beside the walk,
PROT 0925's arm 8 runs once on a scripted fight, which takes the round tail in place of the flee
(`0x801F73F0`), over party rows with a member down and item turns with and without a turn left, against
`spikefish_round_tail`. Three parts stay out (`camera_gaps` in the test): PROT 0928's
roll, PROT 0925's arm 8 exit and PROT 0908's arm 8 (the creature stood on the victim). PROT 0934's shake TR y is
compared: arms 5 to 9 read the pass word `0x801FA408` for its low bit and bump it once a pass
(`0x801F6FAC..0x801F701C`), so the shake flips on every battle frame at any step, where the port had flipped it
every four vsyncs and agreed with retail at step 4 alone.

### Both walks at frame steps above 1

A console's battle CD timing runs battle frames of 2 vsyncs and more, and the engine's own clock runs Zenoir's arms
8 to 12 at step 4 in a PCSX-paced cast, so both walks run every body at frame steps 1, 2, 3 and 4: retail's step byte `0x1F800393`, the director's `frame_step` and the shadow's battle frame clock. The
shadow's pass is the host's (`World::drive_summon_module_frame`, `World::module_pass` over `delta` one-vsync ticks,
stopping on the tick an arm hands over). The summon walk runs its director the same way, feeding the live yaw,
pitch, TR and the creature's cursor between ticks, and lands the owed ticks' drift and nudge after a handing tick
as the host does (`World::drift_rest_of_pass`). At step 1 every tick is a pass, so the step-1 runs are unchanged.

What only shows above step 1 are places where a port written in one-vsync ticks does per tick what retail does once
a pass. The walks found these, each fixed from the disassembly:

- A gate that tests its word once, ahead of its drain (`if (word > 0) { word -= drain; <push>; busy }`), takes
  its branch for the whole pass on the word the pass opened on. Run per tick, a later tick of the pass found the
  word spent and pushed nothing, or left the arm a pass early. The host latches that word
  (`ModuleCamState::latch_pass_word`) and those gates read it (`pass_word`): Slippery's arms 1 to 3 and last arm,
  Iota's drained arms, Gilium's `counted_if_positive`, Puera's drifts, Spoon's and Ultimate Rave's pushes, and the
  poll hold's spent test. Barra's arm 1 climb tests its TR y the same way (`pass_tr_y`).
- A shot aimed on the word ahead of the pass's drain aims on that same word on every tick: Ultimate Rave's arm 4
  sweep, Barra's arm `0xF` ease, Puera's arm 12 and Spoon's arm 8.
- A store retail makes by the whole `scalar * delta` in one pass is owed for the rest of the pass by the tick that
  hands over: Mule's arm 3 and 4 anchor and count, Zenoir's arm 9, `0xA`, `0xB` and `0xC` lift, Jedo's arm 5 and
  6 creature advance, Iota's arm 1 drain and arm 9 sink and shrink, and Gilium's arm 1 drain.
- A poll that holds an arm about to pass kept that tick's rest-of-pass drain (`polled_countdown`), so Slippery's
  side-band arm 1 drained `3 p` a pass where retail drains `2 p`.
- Mule's arm 16 wave struck on every tick, two damage rolls a pass at step 2. It now walks the front, lifts and
  rolls the row and strikes once, on the tick that ends the pass.
- Iota's arm 9 pulls each plane coordinate once a pass, a positive one down a step and then a negative one up a
  step, so a coordinate under one step stands still where the per-tick pull walked it to zero.

A second round traced every arm still apart above step 1:

- A gate that tests its word against the pass's whole step, or a model word stepped by it, decides on the pass's
  opening tick: Puera's arm 8 cursor (`slt` on `scalar * delta` at `0x801F714C`, `Gate::RampDown`).
- Stores the handing tick still owed for its pass: Gola Gola's hit-arm yaw (the shot reads the global after the
  pass's whole drift), Zenoir's arm 7 word ahead of its tween, Mushura's arms 6 to 8 creature run, Puera's arm 4
  slide and arm 12 sink, Terra's arms `0x13` / `0x14` creature walk and row turn, Juggernaut's arm 19 walk, Barra's
  arm `0xD` ramp and word and arm `0xE` flight, Ozma's arms 5 and 8 shake rise.
- Moves inside a word's `blez` take the branch the pass's opening word picks: Puera's arm 4 slide (`0x801F6E40`,
  which carries the creature past `0x80` at step 3), Gilium's arm 4 rise (its pass stores `z = 0x400` and scale
  `0x1000`, `0x801F718C` / `0x801F71A4`), and the drained pushes of Gimard, Theeder, Gizam, Gola Gola, Palma, Mule and
  Meta; Barra's arm 6 arc tests the pass's end word (`slti 0xa1`).
- A stored value travels as a drift that the rest of the pass owes nothing more of: Aluru's arm 6 yaw store
  (`set_yaw`), Jedo's arm `0x1E` TR y.
- Order: Mushura's arms 7 and 8 move the creature before the exit's cut reads it (`sh v1,0x38(s4)` at
  `0x801F72EC`, the cut at `0x801F7400`), so the host runs them ahead of the director; arm 9 steps the victim back
  `0xCA` (`0x801F7494`) before it turns the creature onto it (`0x801F7578`).
- Missing stores: Meta's arm 6 exit stores TR y `0x3A0` and TR z `0x720` (`0x801F70BC..0x801F70C4`), where the
  drift happens to land at step 1. Barra's arm `0xF` ease reads its word unclamped (`0x801F8084`).

The step-4 runs found Puera's arm `0xB` sink and arm `0x13` placement short of retail's: arm `0xB` sinks the
creature by the pass's whole `2 * scalar * delta` ahead of its gate (`0x801F7330..0x801F734C`), so the tick that
leaves owes the rest of the pass's sink, and arm `0x13` places it off the word its pass has drained, unclamped
(`0x801F81C4`), so a pass that overshoots zero carries it past the end. They also found Puera's arm `0x18` row
wait letting go on the port's cap (`SETTLE_TICK_LIMIT`) before a slow fade ended: the cap counts battle frames, not
vsyncs, so a slower frame step cuts no wait short.

`STEP_GAPS` is empty. Aluru's arm `0x14` and Barra's arm `0xF` wait on a dead victim's colour word alone
(`lw v0,4(s3)` / `bnez` at `0x801F8870` in PROT 0916 and `0x801F81A4` in PROT 0917; a live victim on its clip
`+0x1D9` reading idle), a word that eases out only under the defeat fade the commit's monster-death arm arms at the
knockdown's end (`0x8004B66C`). The port had also let both arms go on the down clip, which above step 1 came in
ahead of the fade's end; the arms now wait out the fade as retail does. A dead body standing lit outside the fade
is put into it by the battle fade pass (`DEAD_IDLE_FADE_PASSES`), so the wait always lets go. With the step-3
runs, Mule's backdrop follow on a negative pitch (`0x801F8B80`) is driven both ways. `mule_wave` runs its 600
rounds at steps 1, 2 and 3, the wave's damage, spirit and XP compared live at each, and so does `ozma_ladder` over
PROT 0934's arm `0x18` hit ladder. The summon tick bodies' phase walk (`chain_ticks`) runs at steps 1 to 4, and
the Spoon and Vera heal arms at a random step. The chain-body walk over the capture bodies stays at step 1.

**The spawned records.** The capture and chain walks log every `FUN_80021B04` / `FUN_80050ED4` call a pass makes,
as record, point, angle trio and scale, and every literal-anchored record a director seats (its charge records and
per-pass spawns) must match one of them. The chain walk had run without the SCUS sine and cosine table pointers
(`0x8007B81C` / `0x8007B7F8`), so every placement off a heading read zeros and its rounding fix-ups never ran. With
them it found PROT 0964's far records on the wrong trios: they take the arm's own focus and angle trios
`sp+0x20` / `sp+0x10`, which hold the cut's yaw `0x800 - facing` on a cutting pass and the prologue's zeros on
`k = 4`'s, where the director gave them the caster's facing. It also found Chaos Breath's cloud a unit off: the arm
takes a sixth of the whole product through the `0x2AAAAAAB` multiply-high and corrects the sign after the shift.
The chain walk hands each director the module's countdown word as retail's pass opens on it, so an arm whose
records follow its pass's whole drain (PROT 0947's trail) is compared too; PROT 0947's trail agrees. The summon walk
makes the same comparison over each director's anchored spawns, resolved on the seats the host resolves them on,
and runs half its casts with the Battles vibration row `0x800845C8` off: every module's rumble store, the Puera
landing and Ozma's spray agree that nothing is written then.

The capture-camera walk also starts every fifth round on a framing depth below zero: state `0x6F` drains
`ctx[+0x6D0]` by `16 * step` a vsync with no floor while it waits on the module's seek, so a negative depth reaches
a module (Death Game's `/ 4` fix-up at `0x801F7B2C` takes it).

That line-up and the `0xFF` arm's restore now run off the module image against their port (`dead_end_crisis`): all
eight records whole, the module's saved words and the context. The `cort_evolved_final_crisis_mid_cast` capture
reads the stood seats and the saved poses. PROT 0927's stager arm 7, which jitters its caller's `+0x84` and rotation
`+0x28` off three `rand()`s (`stager_states`), had no port either: every Dark Eclipse ray stood on one angle.

The phase-chain tick bodies (`chain_ticks`) run from arm 0 to their Done off their images, pass by pass, against the
port's pass: the chain runner, the body's camera director on every pass, the band's caster pre-pass, the party-row
rate and the fold's hit rate. The whole context and all eight records are compared, but for the bytes another owner
writes (the fold's HP side and reaction, presentation, poses, the cue byte, PROT 0963's lighting) and the rate-rule
gaps the test names. Five arms differed. Dead End Crisis's arm 5 holds the party at rate `2` (`0x801F7524`) where the
director froze it, which held the engine's Done band for some 1800 ticks after Final Crisis. Genocidal Cannon's
sweep slows each struck seat and holds the party row at `2`, which the port carried neither of. Megaton Press freezes a
victim that plays its `+0x1F2` clip under lead roster `1`. Hyper Lightning's arm 7 leaves on the pass its fourth hit
counts, a pass before the director did. A replayed caster's stage-0 store, its return to idle, belongs to the chain
runner, not the wind-up pre-pass. The summon tick bodies' phase walks, PROT 0918, 0922, 0924, 0925, 0927, 0949 and
0958, match but for Water Crystals, whose arm 5 is terminal; the port ended a pass later.

The rest of the battle side's frameless kernels match on their first run: the Seru learn `FUN_801E92DC` over whole
records and the magic-level line `FUN_801F452C` (`seru_learn`); the swing camera's height `FUN_801F0348`
(`camera_height`); the popup ring push `FUN_801F44A0`; the cursor step `FUN_801D32BC` and the placement copies
`FUN_801D57E8` / `FUN_801D5718` / `FUN_801D5778` (`cursor_leaves`); the Super tail replace `FUN_801EF9E4` over every
character's resident rows and the learned-art check `FUN_801EFBFC` with its gate and draw (`super_learn`); the
monster install `FUN_80054CB0`, both boost profiles and the reaction map (`monster_install`); the MES escape append
`FUN_8003CB54`, the event-list probe `FUN_80050E00` and the side-band arm `FUN_80055B4C` (`battle_small_leaves`); the
dead-target redirect `FUN_801DB124` with its draw count (`target_redirect`); the arts flash ramp `FUN_801E2524` and
its emitter `FUN_801E2650`, packet by packet (`flash_ramp`); the cast-start cue `FUN_801F3990` and the Seru reward
line `FUN_801DBA90` (`cast_cue`); the party attach swap `FUN_8004CCD4` (`attach_swap`); the command ring's four
chip marks `FUN_801DBC30` / `FUN_801DBD04` / `FUN_801DBDDC` / `FUN_801DBEC4` (`chip_marks`); the arts list pager
`FUN_801D3748` (`arts_pager`); the applier's cue-group expander `FUN_801E22C8` (`cue_group`); the target banner
`FUN_801E6D84` (`target_banner`); the bolt ribbon `FUN_801CFA48` with the overlay generator `FUN_801D0290`
(`ribbon`); the near-camera ghost pass `FUN_8004DC68` with the bearing live beneath it (`camera_ghost`); and the clip
crossing-vertex interpolator `FUN_80029724` (`clip_vertex`).
The CLUT-cell HSV writer `FUN_80019D50` (`clut_cell`) found the texel repack ORing the three channels where
the bytes add them (`0x80019FC8..0x80019FE0`). Op `0x44` takes a signed white-blend velocity and the mode-3
integrator has no floor, so the white amount can go below zero; a channel then goes negative and its sign
bits carry into the next field rather than being masked. The RGB / HSV pair under it (`hsv`) matches.

The field and menu leaves under the VMs match on their first run: the system flag bank and the operand
loaders the field VM calls (`field_leaves`), the handler finder and retire sweep, the text packet length,
the actor node pool (`actor_node_pool`), the MAN record resolver `FUN_8003C8F0` over every scene MAN
(`man_record`), the default-move readers (`default_move`), the pad's camera ring remap and wall-slide
resolver called on their own (`pad_resolve`), the walkability probe and the kind-2 elevation lookup over every
field `.MAP` (`tile_lookups`), the camera height ease, the floor-ladder oscillator and the eased move
(`field_timers`), their spawners and the colour tween's (`field_spawners`), the camera-zone record loader and
its re-query (`zone_record`, `zone_requery`), the reflection controller, the Ra-Seru mesh swap, the morph
envelope and the clip cursor (`morph_envelope`, `clip_cursor`), the property-ramp pool (`ramp_pool`), the arc
hop's two spawners and its phase watcher (`hop_spawn`, `hop_session`), the fog emitter, spawner and trig LUT
install (`ambient_emitter`, `fog_spawn`), the world map's entry fade, the L1 map display's enter, tick,
stepper and clamp in PROT 0981 (`map_display`, `map_display_tick`), and the menu overlay's category check and
preview mode (`menu_leaves`). Three stores have no port and no reader the port runs: the post-box byte
classification `FUN_80038050` (`talk_dispatch`) stores a `4C FE xx` operand to the pager's box-gone count,
an arm no shipped script reaches; and the fog spawner writes halfword `1` at record `+0x12`. The ramp
ticker's order is below.

The sound driver's per-frame tier runs in the interpreter too: libsnd's `SsSeqCalc` `FUN_80062F98` with its tempo
slide, both volume slides, the track end and the chained restart, stop, start, the event decoder and the delta-time
pump (`seq_calc`); the voice chooser `FUN_80066B00` (`voice_choose`); the key-on volume chain `FUN_80067550` on the
sequencer and cue paths (`key_on_volume`); the key-on and bend pitch `FUN_80066D8C` / `FUN_80066E50` / `FUN_80067A1C`
(`audio_pitch`); and the SEQ slot release `FUN_8001FF58`. Two volume-slide arms differed: the slide's cached pair is
read back through the set `FUN_80067E9C`, which clamps each side to `0x7F`, where the port cached the unclamped
commit, and the ascending fast arm's ceiling test is a 32-bit sum where the port wrapped a halfword. The pitch arms
match on every shift the disc's VABs hold (`0..=0x7F`); a larger one would read past the 192-entry table.

The LZS decoder `FUN_8001A55C` matches over every container section on the disc and both packers. The SCUS root
`FUN_8005AF0C` under the fishing lure's record counted a negative input's leading zeros, where the GTE's `LZCR` counts
its leading ones. The field animator's sampler `FUN_8001BE80` and angle lerp `FUN_8001D088` over real clip banks, the
`"ME"` reader `FUN_8002B28C` and codec `FUN_8002A9CC` over every slot of `summon.dat` / `readef.DAT`, the VDF morph
`FUN_8001C604` / `FUN_8005B038`, the action-tag lookup `FUN_80050E2C`, the record's spell-list helpers, the MAN
section-0 carve `FUN_8003A110` and the motion-script installer `FUN_8003A9D4` over every scene, the slot machine's LCG,
landing search and marquee blits, the fishing prize gate and reel decoder, the coin counter's entry panel, the
publisher-logo quads, and the mode table's per-frame and INIT handlers match. The CARD body's readout takes the
scratchpad byte `0x1F800391`, which two notes gave as `0x1F800393`, and the dance HUD's displayed-note helper carried
the CPU feed's tag without the feed's schedule.
The field-to-battle transition (PROT 0979) runs off the disc packet for packet. The particle fields
`FUN_801CFDA0` / `FUN_801D0370` showed every per-frame integration term rounded toward zero in the
port, where retail multiplies by the frame step and shifts with `sra`, which floors; only the
spin-up's z adds `7` to a negative product first (`0x801D0628..0x801D0638`). The tile shatter
`FUN_801D0D24` / `FUN_801D0E54`, with the SCUS dispatcher `FUN_80043390` and its leaves live, showed
every flying tile drawn at its face's full grey: the emitter hands the dispatcher the record's
progress, a non-zero one selects the depth-cue bank, and slot 17 is then `FUN_80044DC8`, which drops a
quad wholly off the screen (`FUN_80044798`) and fades the colour toward black by `IR0 = progress`.
The swirl's early submit `FUN_801D1A20` rides the same bank through its flag word's top byte, which
also links each quad at its largest corner `SZ`; and under the double-sided `NCLIP` mask an edge-on
quad still drops, which the port's double-sided arm had skipped. The tick `FUN_801D1888` matches. The
late swirl, which hands the band to the shatter renderer `FUN_80029888`, is not carried faithfully and
the test leaves it out.

The procedural mesh builder `FUN_80028158` matches `effect_default_arm::build` byte for byte over
every shape and texture mode; shapes 1 and 3 on the planes that start at `z` extrapolate vertex 6
through two pads nothing wrote, so the test seeds the pads zero. The field frame blender
`FUN_8001BE80` with the angle lerp `FUN_8001D088`, the camera-relative glide `FUN_80021248` /
`FUN_8002149C` from spawn to retirement, the field-entry camera reset `FUN_80025C24`, the `LZCS`
square root `FUN_8005B0B8` and the `GPF` morph blend `FUN_8005B038` match.

The field camera build `FUN_800172C0` and the single-point `RTPS` `FUN_8003D368` match
`render_kernels::gte::FieldGteView`, an integer port of the matrix the build loads. The kernels that
stand for one of retail's own `RTPS` calls had projected through the hosts' `f32` view instead: on
screen it missed retail's `SXY` on nearly every point, by a pixel from the divide and the integer
matrix, and by hundreds where a point sits within `H / 2` of the eye and the divide saturates. The
framebuffer grab's `rtps_point` and the drop shadow's grid now project through `FieldGteView`.
The fog particle pass joins them (`fog_render`): the walk `FUN_8003F348`, the per-particle step
`FUN_8003F3FC` and the half-sheet emitter `FUN_8003F86C`, run whole and step by step over random
cameras the matrix build made, pools, windows, tints and both arms of the overworld bit, record for
record and packet for packet with each packet's ordering-table bucket. Through the `f32` view the
port's corners matched retail's on fewer than one half in twenty; a map01 capture's 104 sheets went from 6
to 103 exact. The `f32` path also dropped a particle at or behind the eye where retail draws the
saturated corners, and its overworld `NCLIP` bent all three corners where retail bends only the one it
writes back (`SXY2`).

The battle routines too large to run whole in one pass run arm by arm, each against the kernel its port keeps for
that arm. The Triangle arts-list driver `FUN_801D3444` (`arts_list_driver`) carries a caption latch `0x801F4E08`
the port did not have: the closed list's `View Hyper Arts list` is latched in the entry and stays up while a list
closed just before the entry ended slides out, which the port now draws. The battle-result build `FUN_801D84C0`
(`party_panel_build`) publishes the context's `+0x1F9` buffer into two placement records, `0x59` and `0x65`, where
the port's list named one. The Final Heal sweep `FUN_801E6968` (`final_heal`) keeps the ability bit on a member
whose bit is set with no Lost Grail equipped, where the port cleared it; the engine still sweeps the whole party
after every step where retail sweeps the acting actor's target at the action's cleanup. The after-image walk
`FUN_80049348` (`afterimage_walk`) divides `8` by the anim rate with no guard, so a frozen actor's quotient is all
ones and the walk draws nothing, where the port drew a deep pair; and a monster at rate `1` samples depths `16` and
`32`, where the port clamped to `15`. The multi-cast readout `FUN_801E805C` (`value_readout`) draws a five-digit
row's narrow cells fourteen rows tall, not twelve, and its teardown head closes element `0x50` and drops the flag
when the string total is live, which the port's fragments did not compose. The damage finisher `FUN_801DDB30`
(`finisher`), called on its own with the two rolls, matches on the value, the Stone nullify, the strip, the rumble
and the summon XP; its Spirit fill stores each add as the halfword `+0x170` and caps the halfword, so a percent past
`0xFFFF` - a big hit on a tiny max HP - wraps under the cap, where the port saturated. The per-hit numeral
`FUN_801DF6B8` (`popup_numeral`) matches, its near arm included: under an OT depth of `0x80` the projection's
square is reseated `32` px from its near corner (`popup_near_square`), and the test answers depths on both sides.

Matched on their first run: the side-band tick `FUN_80056208` (`battle_sideband`); the battle floor's grid
`FUN_801D02C0`, both passes and every packet, the GTE answered by the interpreter's (`ground_grid`); the per-actor
draw `FUN_80048A08`, per object and the shadow (`actor_draw`); and the arts-entry build `FUN_801D388C` case `9`, the
four chip records (`arts_entry_build`).

The move projectile's slots in `FUN_801E09F8` (`homing_flight`), run whole pass after pass over every shipped
move-power record and mutations of it at every frame step, match `HomingSlots::step_pass` and the streak word's
`MoveFxStreak::tick_counter_by` on every slot's phase, child, position, heading and counter, the census, the spawns,
the drags, the tints, the shivers and the hits. They found two arms with no port. A flying slot calls the streak ribbon
once per `0xFF` byte of the `+0x12` list on every pass (`0x801E1284..0x801E12D8`), so the four records whose flight
list is four `0xFF` drew no bolts between the caster and the target. And the two lists do not read a byte alike: the
landing list's `0x64` is a screen flash (`FUN_80024E80(0x801C9070, 2)`, `0x801E14E0..0x801E1534`) and `0x65..=0x7F`
spawns nothing, where the port handed both to the prototype table; the flight list has no such test.
The menu side runs through a window-VM harness: the interpreter `FUN_801D6628` with its SCUS window-list helpers
live over a bump heap (`window_vm`), every program the overlay scan recovers chained at random and generated ones
over every opcode, the whole list compared node by node after each program. The port's window model created every
window with style `0` where the creator copies the descriptor's class word, never computed the park edge it slides in
from, and closed a closing window again and a sliding one from frame `0`, where retail leaves the first alone and
reverses the second's progress. With the helpers live, the sub-screens that wait on them run frame by frame
(`save_subscreens`): the terminal screen `FUN_801DD12C`, the Yes/No screens `FUN_801D6D38` / `FUN_801D8A58`, the
notice `FUN_801DD1B8` and the release wait `FUN_801DD26C`, with the list kernel `FUN_801D688C` live beneath them. The
save graph's Yes/No screens never seeded their cursor. The pause menu's own ready check, against the same
`FUN_801D6D38`, stepped on Left and Right and stopped at the ends, and moved before it confirmed; retail runs the list
kernel over two rows, wrapping, on Up and Down, with confirm and cancel ahead of any move.
The window walker's motion pass `FUN_80031AE4` runs in the same harness frame by frame, programs landing between passes
at frame deltas of `1` to `3`, the latch `_DAT_8007BB80` it leaves compared with every node: it matched the port's
`MenuWidgetState::step_motion` as written, and the menu's window-slide input lock
([field-menu.md](../subsystems/field-menu.md#the-window-slide-input-lock)) stands on it.

The casino prize counter `FUN_801DC1CC` (`prize_exchange`), over every block of the disc's table, found the port
committing a redeem on the Yes press: retail stores state `3` there and commits on the next frame, which reads no pad,
so the jingle lands a frame after a silent Yes. The menu's decimal writers `FUN_80034B78` / `FUN_80034E4C`
(`menu_numerals`) decide each of nine places alone, and six copies in the port right-aligned a string instead: a value
wider than its field prints its overflow's non-zero digits from the pen in retail, where the port stacked them on the
first cell, and the zero-padded writer cut the value to its field. One kernel, `decimal_field_cells`, now stands for
both. The HP and MP ink tiers `FUN_800349EC` / `FUN_80035EA8` match both hosts' readers, and the field's text
balloon, its spawner `FUN_8003C764` and handler `FUN_801DA7F0` (`text_balloon`), matches tick by tick.
The property-ramp ticker `FUN_80036D80` walks a list, not the slots: the installer `FUN_8003C5F0` links each
new slot at the tail through `+0x1A` / `+0x1C` (`0x8003C664..0x8003C688`) whichever slot its first-free scan
found. Two live ramps on one word therefore end on the newer one's value, where the port, walking slot order,
let an older ramp in a higher slot overwrite it. `RampScheduler` keeps the install order, and `ramp_pool` now
shares destinations. The motion VM's player box `FUN_801CF8AC` and its wander fan `FUN_801D5A68` match
`AmbientPlayerProbe` (`ambient_probe`) on the class arm every placement takes. The party-table rebuild
`FUN_801DE190` matches `rebuild_battle_party_table` (`party_table`), the kernel the battle entry's companion seat
now reads; retail would also append Terra beside a full party or beside Terra, which the engine's seat refuses.

The move VM `FUN_80023070`, run over short random programs on a record of random bytes (`move_vm`), found the
loop's exit wrong: an opcode past `0x46` does not end the frame. The bound check branches to the epilogue
`0x80024150`, which adds the width the previous opcode left in `$a2` and loops; op `0x0B`'s jump-table word is
the same epilogue. The port ended the frame on the first and spun on the second. `move_vm::FrameStep` carries
the width across a frame, for `run_until_break` and the ambient part tick alike. Op `0x46` stores three words,
`+0xB8 = 1` and the sign-extended operand at `+0xBC` and `+0xC0` (`0x80024134..0x8002414C`), where the port
wrote two halfwords. Every other opcode matches over the whole record, op `0x3C` / `0x3D`'s pose block, op
`0x1D`'s global and op `0x21`'s deformation row.

The overlay escape `FUN_801D362C`, every sub-op with a host answering each global from a copy of the RAM the
arm touches (`move_ext`), found sub-ops `0x11`, `0x25` and `0x26` moving the 8-byte position vector
`+0x14..+0x1B` with `lwl` / `lwr`: a slot's fourth halfword is the vector's pad `+0x1A`, where the port saved and
restored the `+0x2A` mirror. The Bezier `FUN_801E45BC` splits each weight into whole and fraction in 32-bit words
over an `lhu` parameter; the port's widened form agreed only inside `0..=0x1000` and now transcribes the split.
Sub-op `0x29` negates its operand as a word (`negu` at `0x801D4418`), so `-0x8000` ramps toward `+0x8000`; the
port negated a halfword. The arm's destination is rung `op[2]` of the scene floor-height ladder at `0x1F80035C`, and its non-zero-duration arm installs a property ramp (`FUN_8003C5F0`, `0x801D43F0..0x801D4428`); the engine's world host had stored the end value in a sixteen-entry array nothing read, so the ladder neither moved nor ramped. It now writes `FieldTerrain::floor_height_lut` and ramps through a `RampScheduler` the field frame ticks. The curve's point is copied out as a whole SVECTOR whose pad is stack the arm never
wrote, so `+0x1A` after `0x0E` / `0x12` is not compared. Sub-op `0x3C` ramps toward the operand's `lh`, where the
port's fade request carries a byte; no larger operand is generated.

The battle effect pool runs whole (`effect_vm`): the init and pack fixup `FUN_801DE914` over the real
`efect.dat`, the spawn API `FUN_801DFDF0` over every effect id with the ids `4` / `0x13` side call, and
the walker `FUN_801E0080` frame by frame at frame steps `0..=3`, every master and child slot compared
and every pass-2 packet held to `child_billboards`. The spawn's randomized offsets are written into the
shared script buffer, so a second spawn of an effect re-deals the legs of an earlier master still
releasing children; the port kept them per master. Both entries also stand still once the battle-end
byte leaves `0xFF`, which the engine's spawn, walk and draw did not test.

The PROT 0900 screen-effect widgets run with their spawn and control APIs live (`screen_fx`): the shared
interpolator `FUN_801DE4C8`, the mask, the letterbox, the image panel and the scripted sprite, every
record field and packet frame by frame. The panel handler draws from its targets, so a `dur = 0` move
shows at once where the port kept drawing the old latch; and the sprite's `0x40` byte is tested once on
entry, so after a passed wait the next record dispatches whatever its first byte is, where the port
parked on a non-`0x40` byte.

The entity tick `FUN_801DA51C` (`world_map_entity`), over every state with its callees answered by
recording hooks, found the Activating state blocking the wrong actor: it sets and the transition clears bit
`0x80000` on the **player** (`_DAT_8007C364[+0x10]`, `0x801DA5C0..0x801DA5E0`, `0x801DA730..0x801DA74C`),
where `step` wrote the entity's own word; and the encounter roll receives the BGM poll `FUN_800243F0`'s
return (`0x801DA548`), which `FUN_801D9E1C` requires non-zero (`0x801DA16C`), where `step` passed `0`.

The field actor cull `FUN_801D79E8` (`actor_cull`) and the window rebuild `FUN_801D7B50` over every field scene's
`.MAP` (`window_rebuild`) match on their first run.

The battle weapon trail runs whole: the band emitter `FUN_800485BC` with the billboard projector
`FUN_800195A8` live beneath it (`weapon_trail`), its sweep driver `FUN_80048310` and the trigger
`FUN_8005112C`. The emitter hands the projector a box of half-size `1` and points all four of its
out-pointers at one slot, so the point it keeps is the last corner, a view unit right and down of the
control point, projected through the matrix the battle frame loads. The port projected through the
hosts' `f32` battle view instead, a pixel off on about one point in twenty;
`FieldGteView::from_battle_pose` now builds the integer matrix and `project_trail_point` stands for
the projector call, and the packets and their ordering-table links match.

The op-`0x45` camera mover's tick `FUN_801DC0BC` (`camera_mover`) has stores for nine of its ten pairs:
no block reads the focus Y pair `+0x1C` / `+0x1E`, so `0x8008911C` holds through a glide, where the port
walked it to the beat's target. The tick's tail re-rolls the shake jitter about the mover's own pose
(`FUN_801D9D30`'s arithmetic inline), which the port skipped while a glide ran. The arming
`FUN_801DD310` copies the live globals' low halves into the pair block and stores the duration as a
halfword the tick reads signed; the port armed on whole words. The tween builder `FUN_801D829C` and the
shake `FUN_801D9D30` (`camera_tween`) match.

The field-to-battle transition's seeders (`intro_seeders`), run with the production `IntroEnv`, showed
its trig tables swapped: `_DAT_8007B7F8` is the cosine table and `_DAT_8007B81C` the sine, so every
particle of `FUN_801CFBB4` / `FUN_801D0164` and every tile of `FUN_801D081C` tumbled about the wrong
axis and the swirl `FUN_801D1564` built its rings transposed. The heading was a floating-point
`atan2` where retail reads the bearing `FUN_80019B28`, and the radial seeder's root a float `sqrt`
where retail runs `FUN_8005AF0C`. The four seeders now match given the same draws, and the draws are
retail's own: BIOS `rand()` over the world stream, lent in where the transition's init found it.

The battle monster loader `FUN_80055468` (`monster_load`), over every monster at every slot, uploads its
CLUT row as a `256 x 1` rectangle from the pool's start, so the row's last sixteen entries are the 4bpp
page's first halfwords; the port wrote only the fifteen palettes. The page, the relocated primitives
and the upload gate on the acting seat's queue byte match. The party battle mesh's splice
`FUN_800536BC`, post-pass `FUN_80053898` and texture relocation `FUN_80053A28` (`battle_char_splice`)
match over the four player files and random loadouts, the TMD registrar `FUN_800268DC`
(`tmd_register`) over every TMD the raw PROT entries carry, the sprite arm `FUN_8002A5A4`
(`sprite_arm`) over every mode word, and the overworld sky band `FUN_801F73E4` (`world_map_sky`) over
random overworld cameras.

The visible-tile crop's decoration pass `FUN_801F7088` (`view_window`), run whole over random maps,
focus words, windows and region boxes, draws exactly the cells `view_cells` and
`ViewCells::decoration_visible` pass, hands its ground emitter the clamped window and restores the
scratchpad bytes after it.

The PsyQ `St` ring (`st_ring`), `StSetRing`, `StSetStream` with its window installer, the demux
`FUN_8005F024` on its memory-source path, `StGetNext` and `StFreeRing`, run as one stream of operations
over generated movies, matches `StRing` on every slot status, the cursors (`StRing::cursors`), the trace
codes and the frames read. The demux compares `chunks - 1` as a word, so a header claiming zero chunks
leaves its frame filling, where the port completed it.

The menu overlay's sub-screens, run frame by frame against `SaveScreenMachine`
(`menu_subscreens`), show the shop quantity spinner `FUN_801DBC5C` re-arming its
list result to `1` on the frame it settles, so that frame never routes, where the
machine routed on whatever the word held; and the mode select's Buy row clearing
only the staged item, quantity and list result, where the machine cleared the
staged slot too. The sprite-descriptor dispatcher `FUN_800198E0` (`title_prims`)
closes every non-`0x11` upload by recording the texpage's CLUT row in the table
at `0x8007BEC0`, which its port did not do, and stores its strip width as a
halfword, so a `w * h` past `0xFFFF` wraps where the port saturated. The slot add
`FUN_80042FE8` (`save_leaves`) refuses a slot at or past the window's end, which
the port did not test; the first-live-monster scan `FUN_801DB8B4` falls through
to the fifth monster's seat when the first four are down, where the port returned
the band end. The save composer `FUN_801E1934` (`save_compose`) stages exactly the
block `legaia-save`'s title, identity and summary writers compose from the same
live state.

The menu overlay's window painters (`menu_painters`), run with every leaf draw logged under the ink
word `_DAT_8007B454`, found the ink the port had dropped as a draw-order word: the Yes / No labels of
windows 46 and 5 stage ink `5` (teal) and the port drew them white. Window 34's passive bound falls
through to the item's own description at `0x40` and above, where `description_source` said none. With
the options popup open, every row but the cursor's draws in ink `0`, where the port scaled each row's
own colour; the popup's cursor is the hand sprite, where the port printed `>`. The Magic info window
prints a spell's level through the template `$<digit>`, and `$` is the font's one-glyph "Lv"
ligature, which the port spelled out. The field party HUD `FUN_801D0D38` (`field_hud`) stages the MP
law for the MP pair, so a fallen member's MP keeps its own tier there, where the port painted it red.
The save screen's grid dispatcher `FUN_801E06C0` (`save_grid`) forms each cell's slide base from the
previous cell's slot, which only the parked row and the slide frames show.

The fishing HUD's sprite path (`fishing_hud`) and the Muscle Dome hub's (`dome_hud`), run into a
packet buffer and ordering table and held to the port's primitives packet by packet, match: the shared
emitters, the digit fields, the banners, the gauge bars and the hub's ROUND banner and course card. So
do the art-name banner placement `FUN_8004C650` (`art_banner`), the counters, prompts, tabs and shop
panels of the menu overlay, the save-flow confirms, and the save screen's box guard and caption.

The STR overlay (PROT 0970) runs from the frame bytes to the MDEC word list. Its Iki converter
`FUN_801D0378` with the table LZSS `FUN_801D0604` (`iki_decode`) matches `psx_mdec::iki_mdec_codes`
word for word over sampled frames of all six movies, header copy and `0xFE00` padding included;
`MdecDecoder::decode_frame` now runs through that list and the MDEC stage `decode_mdec_codes` as
retail splits the work, pixel-identical to the fused decoder it replaced. The STRv2 decoder
`FUN_801D070C` (`strv2_decode`), one-shot and resumed in chunks, and its table unpacker
`FUN_801F1A00` match on generated streams. The play loop's context init, frame poll and slice callback,
the stream open and frame pump over the nine retail dispatch slots on their own movies, the MDEC output
word, the slice-callback install and the four MDEC DMA sync routines (`str_player`, `mdec_sync`) match.

Among the system and render leaves, the boot SPU-base seeder, the staged character pair, the prompt
line counter, the DRAWENV seed, the fade ramp step, the rumble actuator kernel and the scene-name sync
(`system_leaves`), the gouraud colour spread and the display X mirror (`render_leaves`), the battle
intro's style pick and `POLY_GT4` builder (`intro_style`), the CLUT-walk stepper on the actor walker's
case `0xB` (`clut_walk`), the placement spawner's model-bank select (`model_select`), the field-load
entry step (`field_load_entry`), the sustained-cue teardown (in `sfx_drain`) and the battle XA
seek-ahead (`battle_voice`) all match.

The placement spawner also runs whole (`model_select`): every placement of every scene in spawn
order, with the real field VM pre-running each `0x24` / `0x25` prologue, on a fresh flag bank and on
a random one. Its seats, headings, retire bits and stopping pcs match the World's own pre-run
(`World::pre_run_field_channel_prologues`). The rig has to stand up what the scene init stands up
around the loop: the actor list the context resolver `FUN_8003C83C` walks, holding the placements
already spawned, and the spawn bracket `*(_DAT_801C6EA4) + 8` (`0x8003B73C`), without which every
cross-context op on a halted actor stalls the pre-run. The spawn itself runs for real, off the node
pool `FUN_800203EC` seeds: ops that free and append nodes relink the actor lists, and a stand-in
list broke the resolver's walk.

Generated scenes, whose prologues aim cross-context ops at earlier placements, later ones and ids
nothing carries, found the World running an op with no answering context on the poker's own actor
(a move or turn landing on the wrong placement) and running an op aimed at a placement not spawned
yet. Retail's resolver answers zero for both, the dispatcher hands back the pc it was given and the
pre-run ends there; the World's channel stepper now does the same.

The monster install `FUN_80054CB0` (`monster_install`) also holds the seat's short name line
(`ctx + 0x292`) to `battle_name_line` over generated names: the value window's row plates print that
line, where the port had printed the plaque's name with its twin letter and badge. The save screen's
header tab (`save_slide`, mode `1`) and the zone census (`camera_follow::the_zone_divisions_census`)
match; the census finds the composer's zero-span divisions only on four scenes' single-tile regions,
and those tiles lie off the walkable floor, so retail's trap never fires.

PROT 0898's streak ribbon `FUN_801E1D98` (`streak_ribbon`), its corner builder answering with generated
bottom quads, found the chain loop a `do`-`while`: the first segment above the bottom quad is drawn
whatever the baseline, where the port tested first, so a bottom billboard at or above the screen top
grew no segment and drew four `rand()` short. Its single-quad sibling `FUN_801E1AB0`
(`afterimage_quad`), the colour wash `FUN_801F2410` (`colour_wash`), Swordie's split panels and slash
streak (`swordie`) and Deadly Rain `FUN_801F7B88` (`deadly_rain`, frame after frame on the real GTE:
records, splashes, streaks and their ordering-table entries) match.

The battle-stage stream walker `FUN_8001FE70` (`container_walkers`) copies its
lead chunk whole and type-tests only the headers after it, so a lead chunk of
type `0x02` does not end the walk, where the port's type-`0x02`-terminated walk
stopped on it.

The pause menu's remaining painters, on the same rig (PROT 0899). The Status page's Condition panel
`FUN_801D33D8` (`status_page`) found the parenthesised HP and MP printing the live maxima, where the
bytes read the record-side pair `+0x11C` / `+0x11E`: a member wearing an HP or MP percent accessory
showed the boosted figure twice. The Equip screen's main window `FUN_801D21C0` (`equip_main`) found
the slot browse's item panel with no port: with the hand on a filled slot row the renderer moves its
own window record by `(+0x94, +0xC)`, runs the shared item panel on the worn id and frames two shade
boxes, so the port showed nothing about a worn piece until its candidate list was open. The Best
Equipment change arrow is the command-arrow cell under ink `0`, where the port drew an ASCII pair.
The target panel `FUN_801D0520` (`target_panel`) found three: the Life / Magic Water preview draws
a `MAX` tag (icons `0x64` / `0x65`) ahead of the HP / MP label, which sits twenty pixels further
right than the port had it alone; a stat preview's left value is the menu block's word, equipment
included, where the model handed over the bare record stat; and the plain HP row's ink follows the
status word. The Equip compare panel's HP / MP rows never drew their icon pairs on either host. The
top level's three windows (`pause_root`: command list, money and play-time box, party panel) match.

The naming screen (PROT 0897, `name_entry`), its machine `FUN_801F03F0` stepped call by call over
generated pad words with the renderer `FUN_801E6B34` drawn after every call, found the port's cursor
a different walk. A horizontal step wraps inside its own row, where the port ran on into the next;
a vertical step onto the control row keeps its column, where the port snapped to the button's first
cell and so came back up in another column. The arms are exclusive and ordered (cancel, Start alone,
confirm, d-pad): Start jumps to Select and confirms there, which the port had no arm for, and the
confirm prompt has no cancel arm, where the port's cancel left it. The call after the Select press
reads no pad. Select passes a name of blanks, the name is kept untrimmed, the screen queues four
cues the port never played, the prompt is the named character's own line, and the caret needs room
for a tenth byte. Up from columns 5 or 11 of the control row never leaves retail's loop; no walk
reaches those cells.

The Magic screen's caster picker reads no Start: the spell screens' differential now presses it, and
the port's Start-cancel arm is gone. The save UI's dimmed backdrop `FUN_801E02A4` (`save_backdrop`)
links two sprites of `192 x 192` and `128 x 255`; the port drew both at the art's full height. The
stat seeder `FUN_801CF5D0` and the developer menu's flag-number arm `0x801EA51C` match.

The field overlay's op-`0x49` hub family (`hub_actors`), the dispatcher `FUN_801F159C` with the six
panel machines it indexes and the seven window painters, every draw and call logged, found the
dispatcher's release arm reading the board flag `_DAT_8007B450` after the handler ran
(`0x801F1678`), where the port read the frame's snapshot: a handler that zeroes the cell and
finds the completion gate already down takes the busy-bit arm, not the flag store. The panel-script
interpreter `FUN_801E9B3C` and the shared list cursor `FUN_801E9DC8` (`panel_script`), and the
brightness flash, sub-list, records slide and fill fade actors (`panel_actors`), match. The flash's
phase 0 also clears a byte at `0x801F2AA8` that nothing in the field overlay or the executable
reads.

The geometry colour grade behind `4C E6` (`colour_grade`), the object walk `FUN_801D5E20` and the
resident-table walk `FUN_801D8280` over TMDs off the disc, replaced a port that took a flat list
of colour words: a record's colours are its leading words, and the stride add the port's notes
called an over-run is the group's footer slot. The count table at `0x801F26F0` covers selectors
`0xC..=0x13`.

Three spawners return to the worklist's covered side (`timer_spawns`): the shutter `FUN_801DE754`,
the screen frame `FUN_801DE7BC` and the countdown HUD's install / remove `FUN_801D596C`, each a
whole-record compare against the allocator's answer.

The player tick `FUN_801D1344`, run whole with its callees logged (`player_tick`), holds the
pad-hold drain, the walk gate and [the order of the frame's legs](../subsystems/field-locomotion.md#the-player-ticks-legs).
The World ran the walk-regen tick after the step on every field frame; the tick runs it ahead of
the pad controller and only past the walk gate, and tests the system lock `0x1F800394 & 0x400`
there, which the step did not. The scene-transition actor `FUN_80021934` (`scene_transition`)
matches over whole transitions, both stream arms and the name-buffer rotation.

The flag-window picker `FUN_801EF014` behind `49 04`, the Uru Mais lift's floor list, with the list
cursor live beneath it, and its painter `FUN_801E6984` (`flag_window`) match the port's tick, row
loop and legend pens over the disc's two descriptors and generated ones. The submode open
`FUN_801D9C3C` seeds twelve words of the context block one by one, `0x801F3530` and `0x801F3534`
among them after the tail clear, where the port listed ten; the scene actor spawn `FUN_801DE478`
matches.

Feeding the naming screen's d-pad the auto-repeat word it reads (`_DAT_8007BB84`) showed the word
had never reached a host right. `InputState::set_pad`, the entry every host takes, hands the pump
the raw pad word, and the cursor auto-repeat `FUN_80035C24` masked it with the packed layout's chord
(`0xF00F`: the d-pad and the shoulders once byte-swapped). So a held direction never repeated in any
menu, and a held face button, Start or Select re-fired as a direction: a Cross held for a third of a
second walked a list down. The mask entry now runs the repeat on the packed form.

The top-level command list and the Equip main window draw each hand in the mode the cursor word
names: `FUN_8002B994` mode `0` (static) under bit `0x1000`, mode `1` (the idle bob) otherwise, and
under bit `0x2000` a hand on every row in mode `4`, which bobs off the vsync counter. The port drew
one static hand; `pause_root` and `equip_main` now hold every hand's row and mode. The Status
screen's Skills page (`status_page`) matches: the Goods rows that grant a passive under `0x40`,
their pictograms, the three strings a row and their inks, and the "no skills" line.

Battle init `FUN_80055B6C` and the setup `FUN_800513F0`, each run whole with its loaders stubbed
(`battle_init`, `battle_setup`), found two things the port's battle entry did not do. The
[species-order rebuild](../subsystems/battle.md#the-species-order-rebuild) had no port: a formation
of two species draws one `rand()` and, on an odd draw, opens with the other species in front, where
the port seated every formation as authored and drew nothing. And the setup draws one `rand()` a
seated body for its [opening clip phase](../subsystems/battle.md#the-opening-clip-phase), where the
port started every idle on frame `0`, so like monsters idled in step. Both move the battle's
`rand()` stream from its first draw. The seat tables, both family addends, the scripted-fight byte,
the special-word arms, the battle tally, the rumble seed and both entry fades match.

The MAN installer `FUN_8003AEB0` (`man_install`), run whole over a generated MAN with the scene
control block reset and the map-install tail live beneath it, matches the port's pieces in the order
it runs them: the entry flag, the save permission, the back-from-battle flag stores with the
game-over hand-off and the escape clock's penalty, and the place-name banner with the flag it
consumes. One write has no port: system flag `0x16` is cleared on every scene load and raised only
under the boot latch `DAT_8007B71C`, so it reads "the first scene since boot". A card save taken in
such a scene holds it set, and no scene script on the disc reads it; the test holds retail to that
rule and leaves the bit out of the comparison.

The same run holds the card-load seat (`0x8003B764..0x8003B798`) to `SceneHost::arm_resume_seat`
over the position `read_retail_field_position` reads off the save block: the two position words
land in the entry operand and the arrival facing is zeroed. The one difference is the engine's own:
a save whose snapshot reads `(0, 0)`, never taken, gets the scene's seat, where retail seats the
party on the grid's corner.

The pad pump `FUN_8001822C` (`pad_pump`), as one stream of calls over digital and analog reports,
missing pads, the stick fold-ins on both sides of their dead zone, opposed directions and every
frame step, matches on the held, changed and pressed words, the 32-vsync word and the repeat pulse.
The frame-begin pass `FUN_8001698C` (`frame_begin`) matches on the SFX ring's aging, the audio
level's apply and the debug frame skip. The battle cue dispatcher `FUN_8004FCC8` (`cue_dispatch`)
matches over every id: the voice arm's two gates, clip slot, channel and read span, and the ring
arm's value, de-dup and cursor wrap; the port declines a voice id whose span is zero, where retail
starts a clip of nothing. The per-actor draw tick `FUN_800480D8` (`actor_draw_tick`), the party cast
trigger `FUN_801DBF9C` (`spell_trigger`), the side-band applier `FUN_801F12D0`
(`sideband_applier`, pass by pass from a turn's seed to the pass that drops its stage byte), the
action SM `FUN_801E295C` entered at its own entry on the Done band (`spirit_cleanup`), the map
install tail `FUN_8003ADAC` with the fade spawner `FUN_80024E80` and the positioned spawn
`FUN_80024C88` (`map_install`), the status element `FUN_8002C2E4` (`status_icon`) and the scene and
sound leaves in `scus_core_leaves` and `field_scus_hosts` match. The script target resolver
`FUN_8003C83C` is ported beside them as a kernel.

The asset dispatcher `FUN_8001F05C` with its registrars live beneath it (`asset_dispatch`), over
generated chunks of every type byte in both storage modes, installs what `decode` returns and
registers the members `parse_pack` and `vdf_entry` find; the flag types and the unknown ones touch
nothing. Run under the two walkers over every scene block, it registers each scene's models in
`SceneModelBank`'s order. The VDF record table at `0x80083E58` runs on across installs: `edbalden`
stages a bundle slot and a stream chunk, and a reader's buffer is the last one.

The streaming-chunk installer `FUN_8001E54C` (`chunk_install`) found the port stopping the walk at a
chunk shorter than a word, where retail reads the next word as the next header, and saturating the
type-3 budget arithmetic, which is plain wrapping subtraction. No disc stream carries either shape.

The CD plumbing runs with the drive stood in for (`cd_plumbing`): the libcd calls answer "done" and
the VSync a blocking wait spins on delivers the sectors the kick computed. The stream-file shims
and the cursor seek match `StreamFileHost` on the cursor, the deferred-op park and the bytes read.
The DMA primitives found the offline host reading from the last lookup's start, where the kick
reads from the cursor a completed wait advances (`0x8003E030..0x8003E050`), so a second read
repeated the first; the lookup not draining a read in flight; and the blocking wait answering `0`
where the bytes answer `2`. Only tests build that host. The stream load queue `FUN_8003DDA0`, the
overlay loaders `FUN_8003EBE4` / `FUN_8003EC70` and the read sync `FUN_8005EA84` match.

A large routine whose port is a slice of it runs whole with `Cpu::stub_outside` and its few live
callees hooked. The field initialiser `FUN_801D6704` (`field_init`) matches `field_spawn`,
`field_bgm_plan` and `field_prim_buffer_bytes` over every entry mode, and showed the two-part
one-shot `_DAT_8007B9B8` gating the BGM wait barrier as well as the read. Its environment words
vary too: sound-flag bit 2, the field and battle BGM sources, the descriptor walker's status (pack
read, scene objects, MAN decode argument), the field SFX bank latch and the read time the `0x7E`
floor absorbs, against `World::release_field_init_voices`, `SfxBankResidency::field_init` and
`FIELD_INIT_FLOOR_VSYNCS`. A MAN decode that takes the party-wipe arm (game mode `0x16`) skips
the BGM poll and keeps CARD INIT with both sources closed; the engine takes a wipe off the battle
instead (`take_party_wipe`), and its hold queues no field track. The boot init
`FUN_8001D424` matches the initmap sanitiser, the duel arena initialiser `FUN_801CF00C` its record
of seeds (`duel_init`), the battle loader `FUN_800520F0` the stage overlay it names, the mode-entry
prologue `FUN_80016230` its verdicts (`mode_transition`), and the part tick `FUN_80021DF4` the
mode-4 VRAM scroll, with StoreImage / MoveImage / LoadImage applied to a stand-in VRAM.

The actor render bind `FUN_80020F88` (`actor_bind`) found a failed node allocation leaving the node
bit `0x800` set in the port, where the failure returns ahead of the store. The contact broad phase
`FUN_801CF754` (`contact_candidates`), the list walk's emitter release `FUN_8002519C`, the pool
flush `FUN_80050E74`, the rect-copy enqueue `FUN_800468A4`, the on-screen test `FUN_8005126C`, the
MoveImage wrapper `FUN_80058490`, the record copy `FUN_800557B8` and the St frame latch
`FUN_8005ECD4` match.

The Throw Out confirm window `FUN_801D1B20` (`items_panels`) showed the port drawing the bag count
and the window's first prompt line after the item name, where retail seats both at
`WX + 8 + name[0] * 12` - the name's first byte read as a glyph count - which on this disc is
hundreds of pixels right of the screen ([the window](../subsystems/field-menu.md)). The port keeps
its visible layout on purpose and carries retail's seat as the kernel the differential drives. The shop's root
window `FUN_801D4868` (`shop_panels`) showed the composed picker's hand two pixels above the pen
line retail opens it on. The save info panel `FUN_801E08D8` (`save_info`) matches once its numeral
fields walk the retail cell rule and its `Time ` label keeps the literal's trailing space.

The text expander `FUN_80036514` (`text_width`) showed the port folding retail's two passes into
one: a `0xC8..=0xCD` lead is copied alone and its operand read as the next token, and the
measurer's colour drop is the routine's own flag. The line renderer `FUN_80036888` and its two
wrappers (`text_writer`) match glyph for glyph, and settle where a label's `0x0E` line pitch comes
from: the renderer's own `0x7C` step, not the wrapper's `0x0D`. The cursor sprite `FUN_8002B994`
(`cursor_sprite`) matches over frame and bob; a still (mode `0`) call draws frame zero.

The memory-card layer (`card_bu`, `card_flow`) runs with the BIOS calls hooked and scripted. It
showed the session reset `FUN_801E0598` leaving the transfer phase and the open descriptor alone,
which the port cleared; the I/O machine `FUN_801E3294`, the poll fold `FUN_801E16E0`, the write /
format machine `FUN_801E13B8`, the directory classifier `FUN_801E1208` and the frame ticker
`FUN_801E1114` match. The developer menu's adjuster `FUN_801E9F64` (`dev_menu_adjust`) showed the
12-bit ring arm testing its two pad bits independently, and the row-action dispatcher
`FUN_801EA9B0` storing a phase on every one of its arms, not only the out-of-range one.

The dance lesson `FUN_801D0750` (`dance_tutorial`), run whole over the hall's data window, showed the
port's script was a caption walker and not the mode's controller. Its two free-dance steps wait on
`900` units of the frames-danced word `DAT_801D5150`, which the hall's state `10` raises; the port
waited on `900` points of score, which both steps clear on every frame. The lesson also holds the
song timer, raises the load gate that turns presses away while the Disco King talks, restocks the
triangles and sends the hall back to its count-in, none of which the port did. The triangle window
`DAT_801D5144` is stepped by the lesson's practice step alone; the port decayed it every frame of
every mode, and the whole-song differential had been patching retail's word to agree. The same
window now reads retail's own word in `dance_floor`, with the miss window and the frames danced.

The beat track `FUN_801D2524` (`dance_hud`) opens its draw area through column `x + 0x50` (the
`E4` corner is the last column drawn) where the port cropped one column short, and lights its second
note while the human's closed chain flashes (`DAT_801D558C`), which the port had no writer for. The
HUD driver `FUN_801D231C`, the count-in banner `FUN_801D2D98`, the hit sting `FUN_801D3D78`, the
floor spawner `FUN_801D0190` (`dance_spawn`), the sprite-part emit `FUN_801D387C`, the face stamp
`FUN_801D03C4` (`dance_parts`) and the teardown `FUN_801D414C` match.

The contest hub `FUN_801CF870` (`muscle_hub`) matches `FirstVisitHub::tick` frame for frame through
a first visit, and shows a retail fault the port does not carry: arm `0x15` steps the state once
for its hold running out and once more for a skip press on that same frame, which leaves the hub on
an arm it does not have ([the hub's arms](../subsystems/minigame-muscle-dome.md)).

The fishing tackle screen `FUN_801D0F5C` (`fishing_tackle`) draws an equipped lure's count in the
row's highlight ink, since the numeral draw reads the ink word the name left; the port drew it
plain. The bring-up `FUN_801CF070`, the help panel `FUN_801D72A0`, the centred caption
`FUN_801D73B8` (`minigame_panels`), the sky strip `FUN_801D24EC` (`fishing_sky`), the Baka Fighter
help panel `FUN_801D6CBC`, the cameo animator `FUN_801D6310` (`baka_cameo`) and the back-read loader
tick `FUN_801F6B24` (`panel_backread`) match.

The Status page's Magic list (`status_page`) showed the port drawing every row white while a spell
is latched for an exchange; retail writes the other rows in the grey ink (`0x801D42B4`). Both list
pages' empty-list line sat `0x13` px low: retail places it off the window record, not the page
body. The Moves page, the shop's buy / sell list hand (on its row's pen line, as the list kernel
seats it at `0x80032F84`), the sell panel `FUN_801D5AE8` and the game-over letter stager
`FUN_801CE844` (its GPU and CD calls stubbed) match. The dev menu's list picker `FUN_801ECA08`
showed the port running the row-action dispatcher on cancel; retail runs it on confirm and parks a
cancel in phase `2`. The command arrow `FUN_8003C310` and the pictogram emitter `FUN_8002C488`
are held to the cells the menu atlas bakes.


The lure tick `FUN_801D26CC` run whole from its entry, cast by cast through its `+0x22` states with the rod actor `FUN_801D1C5C`
ahead of it and the hooked fish, the cadence recogniser and the record live beneath it (`fishing_lure_tick`), against
`PondSession::tick` on one pad script, found three. The fish did not tick on its strike frame, where the hook arm's word opens
the tick's gate the same pass, so every fight ran a frame behind retail's `rand()` stream. A hooked frame still takes the
band segment's strike draw and throws it away, and the port drew none. The rod's bend clamp is the bound less twice the fish's
surge, where the port stored the bare bound on a note that the surge word stayed zero. A cast that lands inside `0x136` is also
an empty line on its landing frame, where the port waited a frame in the water.

The positional emitter's release `FUN_800250D4` (`sfx_emitter_release`) walks the row's own count and skips a negative first
voice. The session's emitter ops clamped the count to one and filtered per voice, so a runtime-bank row of count `0` keyed and
released a voice retail never touches.

The dance hall's floor pass `FUN_801D2A10` found the step-marker lookup keyed on
the wrong cell: retail asks the kind-1 tile table about the neighbour cell the
tile record's `+6` / `+7` delta points at, and the port asked about the spawn
cell. The shipped hall's marker tiles carry zero deltas, so its ten markers are
unchanged. The same differential split the floor's other pass
`FUN_801D3A2C` / `FUN_801D6BBC` off it: that walk runs the columns outer over end
coordinates, tests the neighbour's block bit and raises the actor's `+0x10` bit,
so it has its own kernel.

The player-file streamer's equipment selector (`FUN_80052770` case 4) found the
port keeping the first entry that matched an equipped id, where retail stores on
every match and so keeps the last; retail also takes the section default when the
stored size is still zero. No shipped player file repeats an id inside a section.

The world-map text box `FUN_801EE90C` found a negative phase skipping the
text-actor tick: the bound test sends an out-of-range phase to the epilogue, not
past it. The horizon emitter `FUN_801D7EA0`, compared packet by packet, matches; its
multiplies now wrap as `mflo` does, so a large staged scale cannot overflow a debug
build.

The BGM swap poller `FUN_800243F0`, run from a changed track word through its first three stages to the payload
read (`bgm_resolver`), found the port's id-to-entry map two entries off from `2069` on. Retail loads
`_DAT_8007BC64 + id - 2000`, one linear run from extraction `988`; the port had routed an id through the
curated sound-test row's entry, a map that steps two entries past `1055`. So `jouine`'s `2071` played
extraction `1061` where a retail state shows `1059` loaded, and `town0e`'s `2070` played `1060` where retail
plays `1058`. The battle bank's tenth copy keeps its bytes under its linear id `2075`.
The fishing segment projector `FUN_801D5C2C`, the slot reel renderer `FUN_801D0FA8`'s faces and shades, the
warp fade repack `FUN_801D58F0`, the fishing pre-hook readout `FUN_801D2050` and Baka Fighter's pack walk
`FUN_801D4C50` match.

The slot overlay's per-frame marquee `FUN_801CFFF0` found the round pips lit from the left in the port, where
retail fills them from the right (one round owed lights the pip at column `0x40`), and a negative payout figure
dropping its units digit, which retail places unconditionally. Its init `FUN_801CEC94` expands the message bank
as `parse_messages` reads it, each texel the nibble times four. The dance overlay's entry `FUN_801CEF54` stages
what `dance_scene_entry` records.

The publisher-logo sequencer `FUN_801CEFD4`, run frame by frame from its state `0`, shows 35 black frames
before SCEA, 17 between SCEA and Contrail, two before PROKION and 18 after it. The port's session skipped
every settle, so its pass ran 504 frames where retail's runs 575, Contrail cut in as SCEA faded, and the title
opened early. The world-map controller `FUN_801E76D4` steps the focus Z word on Up / Down and X on Right /
Left; the port had the banks the other way round and panned the stored, negated words as world units, so the
top-view debug camera moved across the axis retail moves along.

The fishing result actor's draw `FUN_801D67BC`, run to its part loop, found the scale step keeping the low
halfword of each product, where the port grew past `i16`; no hook strength reaches a scale that wraps. Baka
Fighter's developer dump `FUN_801D553C` prints every action record's eight leading words and each live
sub-keyframe, where the port printed two columns; the port now prints the routine's numbers line for line.
The ground emitter's cell loop `FUN_801F6D48`, the stage placement `FUN_801D6D60`, the tile board's install
state of `FUN_801EF2B0`, the fishing session machine's power meter entered at `FUN_801CF3BC` and the Hyper
Crush and Hyper Lightning stagers match.

The party battle-mesh assembler `FUN_80052FA0`, run whole for three seats from the loader's stream table, matches the
port's merged TMD, swing entries and texture uploads. Baka Fighter's cabinet `FUN_801CF388`, run over whole sessions
from boot through the secret rungs and the all-clear, found a won secret rung never clearing its override (the ladder
served the same secret opponent again with the stage frozen), the round index carried across matches (so a secret
rung's first-round intro never played), the tally state leaving on its first frame where retail holds two seconds after
the drain, the pause menu opening mid-exchange where the combat ticks gate it on the player standing idle, and the scene
suppress, two menu latches and the shutout tally a frame or a sense out. The anim commit `FUN_8004AD80`, entered at its
head, matches the cue-cursor seed and the tag ladder.

The Equip screen's candidate step `FUN_801D9C14`, entered, armed and run one frame per candidate row with the stat
aggregator live beneath it, found three defects. Window 25's trial block was summed off the record's live window as it
stood, where retail writes the candidate into the record and runs the aggregator first, so an accessory's HP, MP or
stat boost never showed as a rise on the Goods rows. The Remove row staged `0` where retail stages `-1`, so on an
accessory row the panel did not fall back to the worn piece's stat rows. And taking the last of a stack through an
equip left an `(id, 0)` stack in the bag where retail's by-slot consume empties the slot. The same run shows a retail
oversight the port does not reproduce: the trial's aggregator lowers a current HP or MP above the trial maximum and the
restore never raises it back, so hovering a candidate that lowers a maximum clips the character's current value. The
shop's party column `FUN_801D4C28`, run with its summer and the aggregator, matches the port's members, notes and
arrows, and the commit log's launch leaf `FUN_801DB7B0` matches the record the glide stepper then walks.

The Items screen's apply screens `FUN_801D8308` and `FUN_801D7FF8`, run from the confirm through the applier, the
aggregator, the bag's by-slot consume, the re-probe and the exhaustion timer against the field menu's Items session
on a `World`, showed the port's target confirm testing only that the member was alive. Retail runs the action
validator on the picked member, so the port spent a healing item on a member at full HP and an MP item on a full
member, accepted any member for a Hyper-Art book, and refused a book's owner while that owner was down. The records,
the bag, the cue and where the screen lands afterwards (the target rows again, the Use list or the command window)
match.

The summon installer `FUN_801F19EC`, run over every player cast's creature record off `summon.dat`, matches the record
layout the port reads: the name, TMD and pool words become pointers off the record, each part word a pointer off the
record or, for the seven big summons, off `*0x8007B85C + 0x44000` (where the previous slot's part pool lands, the pool
the port reads those parts from), and each part's links resolve off the record's table. The seat's resets match
`World::seat_summon_actor`, with the module's own seat-arm stores taken as the arm's.


The name-based scene-change packet `FUN_8001FD44`, run with its resolver `FUN_8001D7F8` over the define table the loader
builds, shows the buffer at `0x80084558` holding the destination's own label: on the shipped arm (`_DAT_8007B8C2 != 0`)
the packet copies the active buffer into it after the resolver has overwritten that buffer with the staged name. Every
block on the library cards carries the active label there, where the port wrote the scene the player had left. The pool
allocator `FUN_80020DE0` showed `Actor::init_allocated` leaving the clip rate `+0x6A = 0x10`, the neutral colour
`+0x74 = 0x808080` and the `+0x10` bit the allocator ORs onto every descriptor at zero; it also shows that retail does
not clear the record whole (`+0x16`, `+0x50`, the move PC `+0x70` and the tail past `+0x8F` keep the last occupant's
bytes). `main`'s mode loop `FUN_80015E90`, the object-table stamp `FUN_80024D78`, the three-actor talk spawner
`FUN_801D2D38`, the TMD renderer's corner loads in `FUN_8002735C` over every TMD on the disc, the FMV play loop
`FUN_801CF098`, the FMV master dispatch `FUN_801CEA3C` and the message-bar raise through `FUN_801D8DE8` match.

The touch post `FUN_801D5B5C` showed an encounter roll hold the port did not have: `_DAT_8007B604`, which the system
entity's idle state spends one game tick at a time in place of the region reader, so for eight ticks after a talk or a
prop touch (four after a record spawn, `FUN_8003BDE0`) no step is sampled, counted or rolled. The dialog SM's state-2
arm in `FUN_80039B7C`, over every text lead in the disc's records, showed the walk past shown text running on over zero
padding and other yield bytes, where the runner resumed at the page's dispatch byte. The fishing line actor
`FUN_801D4948` showed the catch celebration firing its stages off a stage word: the stage cue on the arm's second frame
rather than with the bursts, each stage once at any frame step, and the hand-back to the tracking sub-state.

Baka Fighter's developer keyframe editor `FUN_801D4FC8`, run one tick at a time over its globals, both actors and an
action table, showed the port's model of it wrong in three places: it installed or removed a keyframe whenever the frame
cursor moved, where retail does so only when the edit flag changes across the dev menu `FUN_8002B984`, which ships as an
empty stub, so never; it wrapped the action cursor at `0x11`, where that wrap is the fighter cursor's, tested unsigned so
a cursor stepped below zero lands on `0`; and it reset the frame cursor past `count - 1`, where retail resets at
`count - 1`. No shipped play reaches the editor. The run-to-next-text helper `FUN_8003CF7C`, the master frame driver
`FUN_80016444`'s pass plan, the `READ INIT` body `FUN_801CE9C0` (whose CLUT-row load at `(0, 500)` sets two of its
sixteen entries and takes the rest from the routine's uninitialised stack) and the animated-mesh renderer `FUN_8001B964`
posing a sprite-arm node's parts match.

The front end's tick `FUN_801DD35C`, run over every sub-mode with the card driver's words dealt at random, showed the
port's model of its card screens wrong in most handlers. The card check `0x15` had its two arms swapped (the any-button
return is the timeout prompt's, not the probe's); the main menu `0x14` stepped the title's row counter where retail steps
its own (`state[-0xe9c]`), stored the pick in the wrong word and ignored the two panel fades that hold its input; the
grid `0x0B` kept a Load's confirm on the grid and buzzed only on a valid cell; `AttractIdle` wrapped the epilogue's
cursor against a row count of `-1`; the format operation behind `0x12` had no phases, and its exit fired on phase `0x0D`
where retail leaves from `0x0C`. The epilogue returned early on any second argument, which is how the pause menu's Load
and Save (`(1, 2)` / `(1, 1)` from `0x801DAEB4` / `0x801DAF84`) call the routine, so their result re-routes, cancels and
cursors never ran; and the card words were read-only, so a staged operation never reached the epilogue's poll. A Continue
whose save has story flag `0x1A` set raises the debug-mode word `0x8007B98C` (`0x801DFB14`); the engine never does.

The list kernel `FUN_80032A44` tests its four directions as an else-if chain, so a frame holding two moves the cursor
once; the port applied every pressed bit in turn. The dev menu's camera row (`FUN_801E9F64`, `0x801EA1A4`) snaps the
camera on every frame it holds follow on, where the port snapped only on the flip. The window emitter `FUN_8002C69C`
draws nothing for a zero-by-zero centre rect. The window content builder `FUN_80030628`'s shop buy, Goods and landmark
lists, the typewriter count's substitution arms in `FUN_80036044` and the dev row drawer's readouts in `FUN_801EAD98`
match.

The command flow `FUN_801D0748`, run a pass at a time over generated pad words for each prompt, the attack and magic
target cursors, the armed round and the glide gate, showed the port testing the arms of a surface in an order of its
own. A refused ring arm fell through to a diagonal's second arm, so a cursed member's Right-and-Down committed Spirit.
A direction on the lit arm stopped the cross-button entry's highlight walk where retail passes on to the next, and a
Cross on the flee confirm did nothing under that entry. Cancelling the attack target cursor went back to the ring
where retail returns to the `Auto | Command` prompt. Every target cursor ignored the Up or Down that commits under the
directional entry, and the magic window's party cursor skipped downed members where retail stops on every seat. The
monster picker skipped the recent-target ring for monsters `0x97` / `0x98` under a `0x380` status and aimed Koru's
mode-3 cast at an engine seat where retail names the first monster seat. The spoils arm showed the Crimson Book's EXP
Boost unported: retail credits the member's share twice.

The window content builder's armament lists and spell lists, run against the equip session's candidate rows and both
Magic screens, match: the armament walk and its stat sort are now one kernel the equip session builds through. The window
walk `FUN_80031D00`'s number records drop a figure's interior zeros above its field when the figure is wider than the
field; the port's right-aligned layout agrees on every figure that fits. The Status pager in `FUN_801DA2A0` turns Left and
then Right on one frame, so pressing both lands where it started; the port took Left alone. Retail reopens Status on the
page it last showed, and the port opens it on the first. The list kernel's confirm cue overwrites the cue ring's last slot
where its cancel queues a new one.

Branch coverage of the field side found its own share. The field VM `FUN_801DE840`, run over the disc's scripts and a
synthetic operand corpus under seeded story flags, pad, region mask and halted or player contexts, showed `4C 4E` /
`4C 4F` halting where the nibble-4 head has already stepped six bytes, every `4C Fx` advancing where only `4C FF` does,
local-flag bits of 16 or more wrapping onto the low bits where retail's 32-bit shift stored with `sh` drops them, op
`3D` clearing the leader byte when it empties the list, `4C DC` matching item id `0` on an empty bag slot, and
`36 FF FF` running on where it waits out a field XA one-shot's drive word `_DAT_8007BC20`; and `4C C9` running on
where it holds while a BGM swap is in flight. Its operand reader `FUN_8003CE9C` sign-extends, so `4A` and the
7-byte `4E` forms read their operands signed, which the port read unsigned (no shipped operand reaches `0x8000`). The
camera mover `FUN_801DC0BC` keeps its progress in the halfword `+0x9C`, so a sum past `0x7FFF` goes negative and runs
the curve from there; the port clamped a wider integer and arrived. The ambient motion VM `FUN_80038158` resets the
actor's default-move record and wander state on a variant swap, which the port kept, and op `0x0D` compares unsigned.
The pager `FUN_801D84D0` zeroes the pad word while the automatic press counts, caps the picker slide's frame step at
4, and reads a resting picker's first Up or Down; the script runner `FUN_80039B7C` ends its pass on a stalled
`A1 <id>`, whose masked byte reads `0x21`. The part tick `FUN_80021DF4` leaves straight from the sound emitter's arm
on the tick it takes its stop request, with no move-VM run and no clamps. The BGM poller's stage walk `FUN_800243F0`,
run call by call against `BgmSwapPoller` at one call a game frame, matches.

The list kernel `FUN_80032A44`'s header and page triangles, run on random node rects, showed the battle item
and magic lists drawing both one pixel low and the next-page triangle one pixel left: their seats were read off a
capture rather than the live node `(174, 36, 131, 120)` the kernel draws from. A walk of PROT 0963's Genocidal Cannon
at a party row with a fallen and a stoned seat showed its sweep's rate store sitting past the skip label, so every
party seat takes rate `2`, not only the struck ones.
