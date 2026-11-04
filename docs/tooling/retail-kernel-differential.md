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
`LEGAIA_DISC_BIN` the tests skip.

```bash
cargo test -p legaia-retail-exec --profile dev-fast --test integration
```

## What it can run

Integer instructions, `hi` / `lo`, unaligned loads and stores, RAM and the
scratchpad. `Cpu::hardware_zero_div` runs a `div` by zero as the chip does, for a
routine with no `break` guard, where that result is what ships. A routine that reaches the GTE or COP0 stops with
`Stop::Unsupported`, so a pure kernel is the natural target. A call can be
replaced with a `Cpu::hook`, which is how `rand()` is scripted so both sides
draw one stream. `Cpu::calls` records the calls made, which is how a test proves
it reached the arm it claims.

## How to add a routine

1. Read the disassembly and find what it reads and writes. Seed those bytes with
   the shapes a game reaches, including the caps and the empty cases.
2. Compare whole regions rather than chosen fields. A write the port does not
   know about is exactly what this is for, and a field-by-field test cannot see
   one.
3. Assert the run reached the interesting arms, so a seed change cannot leave
   the test passing on nothing.

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
the approach terms even when the verdict is then overturned.

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
