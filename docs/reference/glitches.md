# Glitches and oversights

Known glitches and oversights of the North American release, each traced as far as the disc's own code and data allow. The list of cases is a community compilation; every entry states its cause, the addresses it rests on, what else the same cause affects and how to check it in a debugger.

Every page is static analysis of `SCUS_942.54` and the PROT overlays. No case is confirmed by a runtime capture, so each outcome is a prediction from the code until someone watches it happen; the *Verifying it* section of each entry names the breakpoint or watch that settles it. Each claim carries an evidence label: `disassembly` (the instructions were read), `disc data` (the value was decoded from the game's records), `repo doc` (taken from an existing page and not re-read), `inference`, or `community report`.

A status of *explained* means the cause is located in the code and accounts for the report. *Partially explained* means the mechanism is located but a quantity or a trigger is not. *Not confirmed* means no cause was found that produces the report as described.

## Off-centre attack names

[`glitches/name-centering.md`](glitches/name-centering.md) - Battle captions drawn at a position worked out for a different string.

| Case | Status | Cause |
|---|---|---|
| "Ultimate Rave" is drawn off-centre | explained | The caption's X is the literal 100 in the cast module instead of 160 minus half the measured width, so the 78 px string sits 21 px left. |
| Other captions with a fixed or shifted position | explained | Three more captions use a typed-in X instead of a measured one, and ten summon spell names are centred on 166 or 172 rather than 160. |
| Super Art and Miracle finisher names are off-centre | explained | The banner is centred for the fixed name "Vulture Blade"; only arts with a row in the arts-name table get re-centred, and finishers have none. |

## Turn order, double attacks and kicks

[`glitches/turn-order-and-arts.md`](glitches/turn-order-and-arts.md) - The turn cursor, the War God Icon's pass counter, kick heights and the dance input lock.

| Case | Status | Cause |
|---|---|---|
| Spikefish Flute cancels enemy turns (the Koru flute glitch) | explained | The flute's turn-skip adds one to the round's turn count per living party member instead of per member still waiting, so the round ends early. |
| War God Icon sometimes gives only one attack string | partially explained | The second pass needs a counter to be zero, and that counter is only cleared when an attacker has to step up to its target. |
| Side Kick still misses floating enemies with War God Boots | explained | Reach is a property of each hit's power byte; the boots swap in a different low-kick record, but Side Kick is an art with its own fixed bytes. |
| Level-3 dance special swallows the next special | partially explained | Presses are only judged while the idle or dance loop plays; the level-3 Triangle move runs 6.83 beats, past the next scoring slot. |

## Status rolls, jewels and boss immunities

[`glitches/status-and-damage.md`](glitches/status-and-damage.md) - Per-frame status rolls, the physical kernel that skips jewels, and what makes a boss a boss.

| Case | Status | Cause |
|---|---|---|
| Tough Love and Love You roll their status every frame | partially explained | A hand-written per-frame routine flips a 50% coin every frame of the animation after the first strike, with nothing to stop it repeating. |
| Seven boss attacks ignore the defensive jewels | explained | These attacks use a physical (ATK-based) damage wrapper that tells the shared finisher to skip its jewel block; it reads as intended. |
| USA bosses cannot have ATK, INT or DEF lowered | explained | The debuff only lands if the boss's base stat still equals its disc record, and the USA difficulty boost changes the base before the fight starts. |
| Every Berserker can be killed outright | explained | Instant-death summons treat a monster as a boss by a texture-size byte in its record, and that byte is 0 on all three Berserkers. |

## Lapis and Mystic Shield Cort

[`glitches/lapis-and-cort.md`](glitches/lapis-and-cort.md) - An AI gate on current MP, a formation row with no boss byte, and a shield that removes a flinch.

| Case | Status | Cause |
|---|---|---|
| Lapis Wave checks current MP | explained | Lapis only casts the wave at members with current MP above zero, and fleeing saves current MP only, so a returning party at 0 of full MP is never targeted. |
| Lapis can be escaped from | explained | "No escape" is one byte at the start of a fight's formation row, and the row for Lapis has it at zero. |
| Mushura and Theeder against Mystic Shield | explained | Both summons rely on the target's flinch animation to stop repeat hits, and Mystic Shield removes Cort's flinch, so they hit on every tick of contact. |

## Ra-Seru levels, Orb and Jette's Clone

[`glitches/summons-and-jette.md`](glitches/summons-and-jette.md) - A shared level stash, a summon's unchecked spawns, and a coin flip over who is the real boss.

| Case | Status | Cause |
|---|---|---|
| Meta, Terra and Ozma inherit Palma's level | explained | The three Ra-Seru summons are inserted from stash slot 0, the slot that stores Palma's level when the Earth Talisman comes off. |
| Orb's animation can glitch | not confirmed | Not established: the spell's cast module has five state-dependent spots, and none is shown to produce a visible fault. |
| Jette's Clone refills his MP and moves him | explained | Clone builds a new monster from Jette's record, copies only current HP onto it, then a coin flip picks which of the two becomes the 1 HP decoy. |

## Items, stats and equipment

[`glitches/items-and-equipment.md`](glitches/items-and-equipment.md) - Clamps, a take-item path with no recompute, the Point Card hang and a missable reward.

| Case | Status | Cause |
|---|---|---|
| Stat boost overflow | not confirmed | Not found for the permanent items: each adds, then clamps. One-battle boosts nearby do saturate or wrap and are the candidates. |
| Story equipment removal skips the re-check | explained | The script's take-item instruction empties the equipment slot but never calls the routine that turns slots into bonuses. |
| Point Card softlock | explained | The target cursor can step onto an ally, and the Point Card's damage skips the HP-counter bookkeeping the end of the action waits on. |
| Coin vendor sells past 99 (Japanese release) | partially explained | The bag silently trims a stack to 99 and the purchase never checks that its item fitted; only a check before the confirm box prevents the loss. |
| War God Icon can be missed for good | explained | The item and its 'already awarded' flag come from two pieces of code that disagree about what clearing the course means. |

## Incense, doors and the Door of Light

[`glitches/field-sequence-breaks.md`](glitches/field-sequence-breaks.md) - A one-frame movement lock, a timer outside the save, and a region box that re-opens a warp.

| Case | Status | Cause |
|---|---|---|
| Incense trigger bypass | explained | The Incense expiry sets the movement lock mid-frame; the player still steps onto the trigger tile, and a locked crossing is recorded without running. |
| Incense carry-over into a new game | explained | The Incense window is one number in the program's own memory, outside the block that is saved, loaded or rebuilt for a new game. |
| Juggernaut's Room door | partially explained | While the guards stand there the door has no lock of its own; two guard bodies are the whole gate. How the trick passes them is not traced. |
| Noa's sigil and the Door of Light in Nivora | explained | The crest stands inside a nine-by-nine map rectangle marked Door of Light allowed, listed ahead of the tunnel rectangles that forbid it. |

## WORK ERR, missing encounters and script slips

[`glitches/field-oddities.md`](glitches/field-oddities.md) - A debug counter that reaches the screen, silent encounter regions and inconsistent names.

| Case | Status | Cause |
|---|---|---|
| WORK ERR on Mt. Rikuroa | partially explained | The scene asked for more actors than the fixed 143-slot pool holds; the number shown is how many requests have failed. |
| No encounters in Jette's Absolute Fortress | partially explained | Story-gated encounter tables: one flag switches the Fortress off after Jette, and much of the interior is zero-rate even before that. |
| Gaza 2 softlock | explained | A walking monster is carried only by its Move animation; a leftover flinch flag swaps it for idle and the wait for range never ends. |
| Dialogue inconsistencies | partially explained | The English script renders one thing two ways; in the bath scene a choice box also speaks for a character who is not the hero. |

## Related pages

- [`battle-action-exit-gates.md`](../subsystems/battle-action-exit-gates.md) - the two retail battle softlock classes in full.
- [`re-settled-threads.md`](re-settled-threads.md) and [`re-do-not-re-walk.md`](re-do-not-re-walk.md) - answered and falsified reverse-engineering questions.
- [`randomizer-internals.md`](../tooling/randomizer-internals.md) - the patcher hooks that repair some of these, including the art-name centring fix.
