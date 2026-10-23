# Glitches and oversights

Known glitches and oversights of the North American release, each traced as far as the disc's own code and data allow. The list of cases is a community compilation; every entry states its cause, the addresses it rests on, what else the same cause affects and how to check it in a debugger.

Every case starts from the disassembly of `SCUS_942.54` and the PROT overlays. Many are also observed in a running game, and several are compared against the Japanese and French builds. Each claim carries an evidence label: `capture` (observed under an emulator; the entry says whether the state was played, poked or an edited save), `disassembly` (the instructions were read), `disc data` (the value was decoded from the game's records), `repo doc` (taken from an existing page and not re-read), `inference`, or `community report`. Where an outcome is still a prediction from the code, the entry says so and its *Verifying it* section names the breakpoint or watch that settles it.

A status of *explained* means the cause is located in the code and accounts for the report. *Partially explained* means the mechanism is located but a quantity or a trigger is not. *Not confirmed* means no cause was found that produces the report as described. *Intended behaviour* means the code does what its data says and the report describes a design choice.

## Off-centre attack names

[`glitches/name-centering.md`](glitches/name-centering.md) - Battle captions drawn at a position worked out for a different string.

| Case | Status | Cause |
|---|---|---|
| "Ultimate Rave" is drawn off-centre | explained | The caption's X is the literal 100, right for the ten-character Japanese name it was typed in for; the 78 px English string drawn there sits 21 px left. |
| Other captions with a fixed or shifted position | explained | Three more captions keep an X typed in for their Japanese names, and ten summon spell names are centred on 166 or 172 rather than 160. |
| Super Art and Miracle finisher names are off-centre | explained | The English banner is centred for the fixed name "Vulture Blade" where the Japanese code centred each art's own name; finishers have no table row to fix it. |

## Turn order, double attacks and kicks

[`glitches/turn-order-and-arts.md`](glitches/turn-order-and-arts.md) - The turn cursor, the War God Icon's pass counter, kick heights and the dance input lock.

| Case | Status | Cause |
|---|---|---|
| Spikefish Flute cancels enemy turns (the Koru flute glitch) | explained | The flute's turn-skip adds one to the round's turn count per living party member instead of per member still waiting, so the round ends early. |
| War God Icon sometimes gives only one attack string | explained | The 'second pass used' byte doubles as the Low command's AP cost, so typing commands sets it; only an attacker stepping up to a target clears it. |
| Side Kick still misses floating enemies with War God Boots | explained | Reach is a property of each hit's power byte; the boots swap in a different low-kick record, but Side Kick is an art with its own fixed bytes. |
| Level-3 dance special swallows the next special | explained | Presses are only judged while the idle or dance loop plays; the level-3 Triangle move runs 6.83 beats, past the next scoring slot and the next combo's first note. |

## Status rolls, jewels and boss immunities

[`glitches/status-and-damage.md`](glitches/status-and-damage.md) - Per-frame status rolls, the physical kernel that skips jewels, what makes a boss a boss, and Zenoir's shatter.

| Case | Status | Cause |
|---|---|---|
| Tough Love and Love You roll their status every frame | explained | A hand-written routine flips a 50% coin every battle frame from the first hit to the end of the animation: 20 flips for Tough Love and 80 for Love You at the battle's normal pace, fewer when frames drop, never under 10. |
| Seven boss attacks ignore the defensive jewels | explained | These attacks use a physical (ATK-based) damage wrapper that tells the shared finisher to skip its jewel block; it reads as intended. |
| USA bosses cannot have ATK, INT or DEF lowered | explained | The debuff only lands if the boss's base stat still equals its disc record, and the USA difficulty boost changes the base before the fight starts. |
| Every Berserker can be killed outright | explained | Instant-death summons treat a monster as a boss by a texture-size byte in its record, and that byte is 0 on all three Berserkers. |
| Zenoir leaves a lone Golem in pieces | explained | Zenoir shatters the monsters it kills; a lone scripted boss is never faded away, so Zenoir skips bosses by the texture byte plus a hand list of Berserkers, which misses the Golem and the lone Puera. |
| Evil Fly and other boss-byte monsters can be killed outright | explained | The texture byte is only read in scripted fights; eleven monsters that have it, Lapis among them, are never met in one. |

## Lapis and Mystic Shield Cort

[`glitches/lapis-and-cort.md`](glitches/lapis-and-cort.md) - An AI gate on current MP, a formation row with no boss byte, and a shield that removes a flinch.

| Case | Status | Cause |
|---|---|---|
| Lapis Wave checks current MP | explained | Lapis only casts the wave at members with current MP above zero, and fleeing saves current MP only, so a returning party at 0 of full MP is never targeted. |
| Lapis can be escaped from | explained | "No escape" is one byte at the start of a fight's formation row, and the row for Lapis has it at zero. |
| Mushura and Theeder against Mystic Shield | explained | Mystic Shield blanks Cort's flinch, the only thing that stops these sweeps re-hitting: Mushura hits on every contact tick, Theeder twice, which just cancels the halving. |

## Ra-Seru levels, Orb and Jette's Clone

[`glitches/summons-and-jette.md`](glitches/summons-and-jette.md) - A shared level stash, a summon's unchecked spawns, and a coin flip over who is the real boss.

| Case | Status | Cause |
|---|---|---|
| Meta, Terra and Ozma inherit Palma's level | explained | The three Ra-Seru summons are inserted from stash slot 0, the slot that stores Palma's level when the Earth Talisman comes off. |
| Orb's legs sometimes do not unfold | partially explained | A stale "return to idle" flag on the summon creature's actor throws away Orb's queued unfold, which Orb queues only once; what sets the flag in play is not found. |
| Jette's Clone refills his MP and moves him | explained | Clone builds a new monster from Jette's record, copies only current HP onto it, then a coin flip picks which of the two becomes the 1 HP decoy. |

## Items, stats and equipment

[`glitches/items-and-equipment.md`](glitches/items-and-equipment.md) - Clamps, a take-item path with no recompute, the Point Card hang and a missable reward.

| Case | Status | Cause |
|---|---|---|
| Speed Elixirs and void turns | explained | The turn-order number rolled from Speed is 16 bits and the next-fighter picker reads its best-so-far as signed, so 32,768 or more can never win. |
| Story equipment removal skips the re-check | explained | The script's take-item instruction empties the equipment slot but never calls the routine that turns slots into bonuses. |
| Enemy items aimed at an ally (Point Card softlock, Lippian Flute) | explained | The target cursor can step onto an ally. The Point Card's damage skips the HP-counter bookkeeping the end of the action waits on; Lippian's kill runs a fade that stands a party member back up at 0 HP. |
| Coin vendor sells past 99 (Japanese release) | explained | The Japanese prize counter only asks whether the coins cover the price; the bag then adds the item and silently trims the stack back to 99. |
| War God Icon can be missed for good | explained | The item and its 'already awarded' flag come from two pieces of code that disagree about what clearing the course means. |

## Incense, doors and the Door of Light

[`glitches/field-sequence-breaks.md`](glitches/field-sequence-breaks.md) - A one-frame movement lock, a timer outside the save, and a region box that re-opens a warp.

| Case | Status | Cause |
|---|---|---|
| Incense trigger bypass | explained | The Incense expiry sets the movement lock mid-frame; the player still steps onto the trigger tile, and a locked crossing is recorded without running. |
| Incense carry-over ("New Game+") | explained | The Incense window is one number in the program's own memory, outside the block that is saved, loaded or rebuilt for a new game. |
| Juggernaut's Room door | explained | From a strip beside a guard, one interact press lands in his talk box and the door's box at once; he makes it a talk, and the door, later in the actor list, gets it. |
| Noa's sigil and the Door of Light in Nivora | explained | The crest stands inside a nine-by-nine map rectangle marked Door of Light allowed, listed ahead of the tunnel rectangles that forbid it. |

## WORK ERR, missing encounters and script slips

[`glitches/field-oddities.md`](glitches/field-oddities.md) - A debug counter that reaches the screen, silent encounter regions and inconsistent names.

| Case | Status | Cause |
|---|---|---|
| WORK ERR on Mt. Rikuroa | partially explained | The rock and dust burst after the Golem fight fills the fixed 143-slot actor pool; the number is how many requests arrived while it was full. |
| No encounters in Jette's Absolute Fortress | explained | Escaping Zora's Floating Castle with under a minute on the clock leaves its "no battles" flag set for good, and the Fortress's encounter tables still obey it. |
| Rogue's Tower water changes the encounter rate | explained | Not a glitch: green water doubles the random-battle rate and blue water halves it until the next battle ends, when the game clears both flags again. |
| Spikefish Flute escape can freeze the game | explained | The flute's escape skips the end of its attack, so its caption stays up after the battle frees its text; redrawn over field data, the text routine can loop forever. |
| Gaza 2 softlock | explained | A walking monster is carried only by its Move animation; a leftover flinch flag swaps it for idle and the wait for range never ends. |
| Dialogue inconsistencies | explained | The English script renders one thing two ways; in the bath scene it turns a question and answer about the researcher into ones about Vahn. |
| Conkram mirror freeze (emulators) | explained | The scene runs on a nearly full heap; an emulated CPU faster than a console's keeps more effect parts alive, an allocation fails, and the move VM writes through the NULL pointer over the CPU's exception vector. Fixed on disc by `--conkram-freeze-fix`. |

## Related pages

- [`battle-action-exit-gates.md`](../subsystems/battle-action-exit-gates.md) - the two retail battle softlock classes in full.
- [`re-settled-threads.md`](re-settled-threads.md) and [`re-do-not-re-walk.md`](re-do-not-re-walk.md) - answered and falsified reverse-engineering questions.
- [`randomizer-internals.md`](../tooling/randomizer-internals.md) - the patcher hooks that repair some of these, including the art-name centring fix.
