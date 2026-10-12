# Turn order, double attacks and kicks

Four player-visible oddities that come out of the battle turn-order and attack-string code, plus
one from the dance minigame's input gate: the Spikefish Flute cancelling enemy turns (the "Koru
flute glitch"), the War God Icon sometimes giving one attack string where two are expected, Side
Kick's first hit missing floating enemies even with War God Boots, and a level-3 dance special
swallowing the next one.

Every explanation here is read from the USA executable (`SCUS_942.54`) and its overlays, and from
the data on the disc. Three of the four are also observed in a running game, and each section
says how: the dance lock by scripted pad input with nothing written to memory, the War God Icon
refusal and the flute's early round end on poked states (each section lists what was poked). Side
Kick is read from the code and data only. Addresses are runtime virtual addresses for the USA
build unless a section says otherwise.

Shared vocabulary: `ctx` is the battle context block, reached through the pointer at
`0x8007BD24`. The actor table is eight pointers at `0x801C9370` (party seats 0..2, monsters from
3). Monster record pointers sit at `0x801C9348[slot - 3]`.

## The Spikefish Flute cancels enemy turns (the Koru flute glitch)

**Status:** explained
**Reported as:** used in a boss fight, the flute skips the party's remaining turns, and for every party member who already acted it also cancels an enemy turn, slowest enemy first; against Koru this can stop Dead End Crisis for good and leave him using attacks that deal no damage (community report).

A battle round is not a list of turns. It is a count. One byte counts how many turns have been
taken this round, and the round ends when that count reaches the number of fighters. Who goes
next is decided fresh after every action: the fighter holding the highest unspent initiative
key.

The flute is an escape item. In a fight flagged as scripted it cannot flee, so only the tail of
its code runs, and that tail is meant to throw away the party's remaining turns. It does so by
adding one to the turn count for **every living party member** rather than for every member
still waiting, then zeroing the party's keys. With `p` party members already done, the count
runs `p` too far and the round ends `p` turns early. The only fighters still holding keys are
monsters, and the picker always takes the highest key first, so the turns that fall off the end
are the lowest keys: the slowest enemies.

Koru has no timer. His script picks a move from the battle's round number, and only round
number 4 means Dead End Crisis. The round number goes up at the end of every round whoever
acted. End that round before Koru moves and the number becomes 5, his script has no entry for
it, and the counter does not come back to 4 until the byte wraps. The on-screen countdown is
`4 - round number` stored in one byte, which is why it reads 255. What is left is the generic
monster picker, which builds a plain attack out of two of his body animations whose hit frame
lies past the end of the clip, so they never land.

The boss also "decides" its move one step before the game notices the round is over, and that
decision is thrown away. For most bosses this costs a turn and nothing else. For a boss whose
script remembers something at the moment it decides, the memory still changes: evolved Cort can
lose a charged Final Crisis this way, which was watched happening in his fight.

In an ordinary fight the same item simply ends the battle: the party walks off and the escape
teardown runs.

The Japanese release has the same code, link for link, so the trick is not particular to the
USA disc.

### Mechanism

The flute casts spell `0x97`, whose cast module is `FUN_801F6A00` in PROT entry 0925. Its outcome
arm tests the scripted-fight byte and either stages the escape or skips to the tail:

```text
801f73e8  lbu  v0,0x287(a0)        ; ctx[+0x287], non-zero = scripted fight
801f73f0  bne  v0,zero,0x801f79b8  ; scripted: skip the escape staging
```

The tail (`s4 = ctx + 0x11`, so `0x9(s4)` is the turn cursor `ctx[+0x1A]`):

```text
801f79e0  lhu  v0,0x14c(v0)        ; seat HP
801f79e8  beq  v0,zero,...         ; dead seat: no bump
801f79f0  lbu  v0,0x9(s4)
801f79f8  addiu v0,v0,1
801f79fc  sb   v0,0x9(s4)          ; cursor += 1 per LIVING party seat
801f7a08  lbu  v1,0x1de(a0)        ; queued command == 1 (Item)...
801f7a18  lhu  v0,0x16c(a0)        ; ...and key still unspent
801f7a2c  jal  0x800421d4          ; refund that member's item
801f7a5c  addiu v0,v0,-1
801f7a60  sb   v0,0x9(s4)          ; cursor -= 1
801f7a70  sh   zero,0x16c(v1)      ; seat 0 key = 0   (7a7c, 7a88: seats 1, 2)
```

The end-of-action gate in the battle action state machine `FUN_801E295C` (PROT entry 0898) is the
only round-over test:

```text
801e67a4  addiu v0,v0,1            ; cursor += 1 for the action that just ended
801e67a8  sb   v0,9(s5)
801e67b4  lbu  v1,0(a2)            ; seated party count
801e67b8  lbu  a0,1(a2)            ; seated monster count
801e67bc  lbu  a1,0x14(s5)         ; ctx[+0x25]: fighters dropped without acting
801e67c8  slt  v0,v0,v1            ; cursor < party + monsters - dropped ?
```

True continues with the next fighter; false ends the round, where `ctx[+0x28A]` is incremented
(`0x801E6800..0x801E6810`), its one writer.

Putting the two together, with `P` seated party members, `L` of them alive, `M` monsters and `c`
turns taken before the flute user's own:

```text
after the flute tail   : cursor = c + L - 1
after the action ends  : cursor = c + L
the round continues while cursor < P + M - ctx[+0x25]
```

With everyone alive the turns left are `M - c`. The monsters among the `c` already had theirs,
so the monsters still waiting lose exactly `p` turns, where `p` is the number of party members
who acted before the flute. The picker `FUN_801DABA4` scans for the highest key
(`0x801DAC88..0x801DAD08`, ties broken by a random draw), so the lost turns are the lowest keys.

Three consequences follow directly:

- A flute used as the round's first action overshoots by zero. The rest of the party loses its
  turns and every enemy still acts.
- The boss's position in the order does not matter, only how many party members went first.
- A boss that has already acted this round has nothing left to lose.

**Koru.** The monster AI `FUN_801E9FD4` dispatches on the acting monster's own id through the
jump table at `0x801CF1CC` (index `id - 4`). Koru's arm is at `0x801EB52C`:

```text
801eb538  lbu   v1,0x28a(v0)       ; round counter
801eb540  sltiu v0,v1,5
801eb544  beqz  v0,0x801ebdac      ; 5 or more: nothing scripted
```

Counters 0..4 index the jump table at `0x801CF49C`, whose five arms store spell ids `0xA2`,
`0xA3`, `0xA4`, `0xA5` and `0xA1` (Dead End Crisis). The countdown strip is computed at
`0x801D0F94..0x801D0FA4` as `4 - ctx[+0x28A]` and stored as a byte at `0x801F6958`; counter 5
gives `0xFF`. Nothing writes 255 anywhere: the counter walks off the end of the script.

With no scripted move, the generic branch is all that is left. Koru's record carries no live
magic id, so the picker takes the physical branch and strings together his costed attack
entries. He has two, entries 5 and 6. Both list their hit event at frame 200, on clips 64 and 30
frames long. The melee kernel `FUN_801EC3E4` lands a hit only once the animation reaches the
listed frame (`0x801EC468..0x801EC474`), so these never do. The real specials take their damage
and their effect sprites from Koru's cast module, which a plain attack never runs.

**The boss still decides on a turn it never gets.** After any action, state `0x50` calls the
picker (`0x801E5D58`), and the picker runs a monster's AI at once (`0x801DAF48`). The round-over
test is in the later state `0x5A`. So on a flute round the boss is picked, its AI chooses a
move, and then the round ends and the choice is discarded. For Koru this changes nothing, since
his AI keeps no state. For a boss whose AI writes a flag when it chooses, the flag still
changes; the per-boss table under [Scope](#scope) lists which do.

**Keys are rolled every round.** The round-top arm calls the seeder `FUN_801DA780`
unconditionally (`0x801D0ED8`): every seated fighter gets `SPD + rand % (SPD/2 + 1) + 1` plus a
wound bonus, halved under Slow. That is why a fast party "sometimes" fails to skip a boss: the
flute user sometimes rolls first, and a flute used first skips nobody.

### The AI latches a discarded pick can move

The AI's scratch state is a 32-word block at `0x801C8FE0`, zeroed at battle setup
(`0x8005601C..0x80056030` in `SCUS_942.54`). Every reference to it in PROT entry 0898 is inside
the AI pick; none is in the action state machine. Two of its conventions matter here.

Word 1, `0x801C8FE4`, is a single shared latch. Evolved Cort's arm (`0x801EBA6C`) uses it as his
charge:

```text
801eba74  lw    v0,4(s0)             ; 0x801C8FE4
801eba7c  bnez  v0,0x801ebae8        ; charged: the Final Crisis arm
801eba84  jal   0x80056798           ; rand
801eba8c  andi  v0,v0,3
801eba90  bnez  v0,0x801ebad8        ; 1 in 4
801ebaa0  sltiu v0,v0,0xc8           ; own MP: needs 200
801ebab8  sb    v0,0x1df(s4)         ; queue 0xA5 Ultra Charge, target self
801ebad4  sw    v0,4(s0)             ; flag += 1
...
801ebaec  li    v0,0xb4              ; queue Final Crisis, whole party
801ebb14  sw    v0,4(v1)             ; flag -= 1
```

Both stores run inside the pick, and neither of his cast modules (PROT entries 0961 and 0962)
refers to the block. So a pick the flute round discards still moves the flag. A discarded Final
Crisis pick, certain while the flag is 1, drops it to 0 and the charged move is gone. A discarded
Ultra Charge pick, 1 in 4 while the flag is 0 and he has 200 MP, leaves the flag at 1: the next
real turn is a Final Crisis with no charge animation and no Ultra Charge MP spent. `0x801EBA84`
and `0x801EBAE8` are the heads of those two arms, not the stores.

Van Saryu's arm (`0x801EBD30`) uses the same word as a one-shot: flag 0, MP at least 100 and HP
at most a third queues Jugger Power (`0xBA`) and adds one at `0x801EBDA8`. Nothing decrements
it, so a discarded pick spends the one use. Rogue's arm (`0x801EB910`) reads the word as his
current element, but it is written by the Element Change module (PROT entry 0964, `0x801F8A90`),
not by the AI.

Words `seat + 4` (`0x801C8FF0 + seat * 4`) are per-seat two-step latches. Zeto (`0x801EAD84`) and
Xain (`0x801EAFFC`) only read theirs: non-zero queues the second step, zero rolls for the first.
The cast modules write it - PROT entry 0946 sets it at `0x801F6CCC` and clears it at
`0x801F6BEC`, PROT entry 0953 sets it at `0x801F6DD0` and clears it at `0x801F6C90` - so a
discarded pick changes nothing for these two.

**First-form Cort cannot be preceded.** The seeder forces seat 3's key to `0x7530` (30000) when
the first formation id is `0xB4` and the round counter is 0 (`0x801DAAB8..0x801DAAF0`), and his
arm picks Mystic Shield whenever the round counter is 0 (`0x801EB994..0x801EB9B4`). The fight is
formation row 13 of `chitei2` / `deroa`, one monster, id `0xB4`. His later picks are fresh rolls
with no stored state.

### What was observed in play

Captured on a poked state, in the evolved-Cort fight. The start is a save state of the real
fight as reached in play (scene `jouine`, formation head `0xB5`, `ctx[+0x287] = 4`, Vahn, Noa and
Gala), on the retail disc; the state has an unrelated place-name patch resident in memory. What
was poked: party HP held at 9000; party speed written so the seeded order is seat 0, seat 1,
Cort, seat 2; the queued actions of seats 1 and 2 rewritten to the Spikefish cast; and, on the
opening round only, one AI random draw forced so that Cort takes the Ultra Charge branch. The
Final Crisis pick on the following round was not forced.

| Moment | What happened | `0x801C8FE4` | Turn cursor |
|---|---|---|---|
| Opening round, state `0x50` after a party action | Cort's AI arm runs and stores at `0x801EBAD4` | 0 to 1 | 2 |
| Opening round, later | Cort is dispatched and Ultra Charge plays | 1 | 3 |
| Next round, seat 0 attacks | An ordinary turn | 1 | 0 to 1 |
| Next round, seat 1 casts the flute | The tail at `0x801F79E0` runs once per living party seat, three passes, and zeroes the party keys | 1 | 1 to 3 |
| State `0x50` after the flute | Cort's AI arm runs, queues Final Crisis and stores at `0x801EBB14` | 1 to 0 | 3 |
| State `0x5A` | Round over: cursor 4 equals the fighter count, round counter 1 to 2, keys reseeded | 0 | 4 |

Cort still held a non-zero key (184) when the round closed and was never dispatched. That one
round shows the early end, the pick running before the round-over test, and the charge being
erased by a move that never played. In both rounds the AI arm and its store ran in state `0x50`
of the *previous* fighter's action, well before the next dispatch.

Three more runs of the same fight show the other case: with the flute user as the first party
member to act, the cursor goes to 3 of 4, the other two party turns are dropped and Cort takes
his turn.

Read from save states without running them: two boss fights reached in play (evolved Cort and a
Gaza fight) hold `ctx[+0x287] = 4`, and a flute being used against an ordinary Gobu Gobu
holds 0. A state taken mid Ultra Charge holds the charge word at 1, one taken mid Final Crisis
holds 0. States taken during Zeto's Call Wave and Big Wave hold his latch at 1 and 0.

Not observed: Koru's own fight (the 255 countdown and the attacks that never land rest on the
code and his record), the Ultra Charge direction of the discarded pick, and Van Saryu's
one-shot.

### The Japanese and French releases

The Japanese build (`SCPS_100.59`) is a different compile, not the USA code moved: its overlays
load higher (PROT entry 0898 at `0x801D05C8` against `0x801CE818`, the cast modules at
`0x801F9DB0` against `0x801F69D8`) and some context fields sit at other offsets. Every link of
the chain is present with the same logic:

| Link | USA | Japan |
|---|---|---|
| Scripted-fight test in the flute module | `0x801F73E8`, `ctx[+0x287]` | `0x801FA858`, `ctx[+0x1E7]` |
| Tail entry | `0x801F79B8` | `0x801FAE58` |
| `+1` per living party seat | `0x801F79D8..0x801F79FC` | `0x801FAE7C..0x801FAEA0` |
| Item refund | `0x801F7A08..0x801F7A30`, `jal 0x800421D4` | `0x801FAEAC..0x801FAED4`, `jal 0x80043210` |
| `-1` | `0x801F7A54..0x801F7A60` | `0x801FAEF8..0x801FAF04` |
| Party keys zeroed | `0x801F7A70` / `7A7C` / `7A88` | `0x801FAF14` / `AF24` / `AF34` |
| End-of-action step and round-over compare | `0x801E679C..0x801E67E4` | `0x801E96E4..0x801E9730`, compare at `0x801E9714` |
| Round counter and its one writer | `ctx[+0x28A]`, `0x801E6800..0x801E6810` | `ctx[+0x1EA]`, `0x801E9750..0x801E9760` |
| Koru's script and jump table | `0x801EB52C`, `0x801CF49C` | `0x801EEA10`, `0x801D1258` |
| Countdown `4 - counter` | `0x801D0F94..0x801D0FA4` | `0x801D2ED0..0x801D2EE8` |
| State `0x50` calls the picker | `0x801E5D58` | `0x801E8C74` |
| Picker calls the monster AI | `0x801DAF48` | `0x801DE304` |

The Japanese table holds the same five spell ids in the same order, the round-over compare is in
the state `0x5A` arm on both discs, and the AI routine's only caller is the picker on both.
Koru's monster block is byte-identical on the two discs from the end of his name to the end of
the block, so his two attack entries with hit frame 200 are the same data.

The French build (`SCES_019.44`) is the USA code relocated: its flute tail (`0x801F83E0` in its
PROT entry 0925) and round-over step (`0x801E70EC` in its PROT entry 0898) match the USA
instructions one for one.

No regional build was run; what the Japanese game does on screen rests on its code and the
community report.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F6A00` | PROT 0925 | Spikefish cast module body | disassembly |
| `0x801F73E8` | PROT 0925 | Reads `ctx[+0x287]`, the scripted-fight byte | disassembly |
| `0x801F78F8` | PROT 0925 | Ordinary fight: stores action state `0x65` (run wait) | disassembly |
| `0x801F79B8..0x801F7A88` | PROT 0925 | The tail: cursor bump per living seat, item refund, `-1`, party keys zeroed | disassembly, capture |
| `0x800421D4` | SCUS | Give-item routine the refund calls | disassembly |
| `ctx + 0x1A` | RAM | Turn cursor: turns taken this round | disassembly, capture |
| `ctx + 0x25` | RAM | Fighters dropped from the round without acting | disassembly |
| `ctx + 0x287` | RAM | Scripted-fight byte; reads 4 in boss fights reached in play | disassembly, capture |
| `ctx + 0x28A` | RAM | Round counter | disassembly, capture |
| actor `+0x16C` | RAM | Initiative key, `0` = spent | disassembly |
| `0x801E679C..0x801E67E4` | PROT 0898 | End-of-action gate: cursor `+1`, round-over compare at `0x801E67C8` | disassembly, capture |
| `0x801E6800..0x801E6810` | PROT 0898 | Round counter `+1` | disassembly |
| `0x801DA780` | PROT 0898 | Initiative seeder, called from `0x801D0ED8` every round | disassembly |
| `0x801DAAB8..0x801DAAF0` | PROT 0898 | Seeder: formation `0xB4`, round 0, seat-3 key = 30000 | disassembly |
| `0x801DABA4` | PROT 0898 | Next-fighter picker; calls the monster AI at `0x801DAF48` | disassembly |
| `0x801E5D58` | PROT 0898 | State `0x50` calls the picker, ahead of the `0x5A` test | disassembly, capture |
| `0x801CF1CC` | PROT 0898 | Monster AI jump table, index `monster id - 4` | disc data |
| `0x801EB52C` | PROT 0898 | Koru's AI arm; `sltiu v0,v1,5` at `0x801EB540` | disassembly |
| `0x801CF49C` | PROT 0898 | Koru's five-entry jump table | disc data |
| `0x801D0F94..0x801D0FA4` | PROT 0898 | Countdown `4 - counter` into `0x801F6958` | disassembly |
| `0x801EC468` | PROT 0898 | Melee kernel's hit-frame gate | disassembly |
| monster `0xB6` entries 5, 6 | PROT 0867 | Hit frame `0xC8`, cost `0xC8`, clips 64 / 30 frames | disc data |
| `0x801C8FE4` | RAM | Shared one-word AI latch in the per-battle block `0x801C8FE0` (zeroed at `0x8005601C`): evolved Cort's charge, Van Saryu's one-shot, Rogue's element | disassembly, capture |
| `0x801EBAD4` / `0x801EBB14` | PROT 0898 | Evolved Cort: charge flag `+1` on the Ultra Charge pick / `-1` on the Final Crisis pick | disassembly, capture |
| `0x801EBDA8` | PROT 0898 | Van Saryu: one-shot flag `+1` on the Jugger Power pick | disassembly |
| `0x801C8FF0 + seat * 4` | RAM | Two-step latch, set and cleared by the cast module (PROT 0946 `0x801F6CCC` / `0x801F6BEC`, PROT 0953 `0x801F6DD0` / `0x801F6C90`) | disassembly |
| `0x801EB7CC..0x801EB81C` | PROT 0898 | Gi / Che / Lu: special when `round counter % 3 == 2` | disassembly |

### Scope

Any fight with `ctx[+0x287]` set. On the disc that is every fight launched from a formation row
whose first byte is non-zero: the two Piura, Caruban and Golem in Rikuroa, the Viguro pair, Zeto,
all three Songi, all three Berserkers, Van Saryu, Dohati, Xain, the Kemaro pair, the Puera fights
and Zora in the Floating Castle, late Tetsu, both Gaza, Lu / Che / Gi, Koru, Rogue, Jette,
Caruban $2 / $3 and both Cort forms. Lapis, the opening Tetsu, Queen Bee and Gold Face are not
flagged, so there the flute simply flees.

The skip is not Koru-specific; what differs per boss is what a lost or discarded turn costs.
Each row is read from the boss's AI arm. The discarded-pick outcome is observed for evolved
Cort's Final Crisis; the Ultra Charge direction and Van Saryu's row follow from the same
store-inside-the-pick mechanism and are not captured.

| Boss | Move | What a flute skip does |
|---|---|---|
| Koru | Dead End Crisis | Gone for good; the script runs off its end and the strip shows 255 |
| Cort, first form | Mystic Shield | Cannot be skipped: on round 0 his key is forced to 30000, so he always moves before any flute. Later picks are fresh rolls; a skipped turn costs only that turn |
| Cort, evolved | Ultra Charge into Final Crisis | The charge is a flag the AI flips when it picks, not when the move plays. A discarded Final Crisis pick erases the charge; a discarded Ultra Charge pick (1 in 4 with 200 MP) arms a Final Crisis with no charge shown and no MP spent on it |
| Zeto | Call Wave into Big Wave | The latch is written by the move itself, so a discarded pick changes nothing; a skipped Big Wave is chosen again next turn |
| Xain | Bull Charge into Terio Punch | Same latch as Zeto |
| Gi, Che, Lu | Blazing Slash / Megaton Press / Plasma Strike on every round whose counter is 2 mod 3 | Not made up; the next one is three rounds later |
| Van Saryu | Jugger Power (once, at one third HP) | A discarded pick spends the one use; it is never cast |
| Rogue | Element Change on even rounds | A skipped change leaves the old element's attack in use |

Only Koru's script has an end to run past, which is why only he shows the 255 countdown and the
harmless attacks afterwards.

### Verifying it

The Koru half is the part still to be run. Break on `0x801F79F0` and `0x801E67C8`. Watch the
byte at `ctx + 0x1A`, the byte at `ctx + 0x28A`, the byte at `0x801F6958` and the halfword
`+0x16C` of all four fighters. With the flute used second or third on the round where the
counter reads 4, the three party keys drop to zero together, the cursor jumps by `L - 1`, and
the counter goes from 4 to 5 while Koru's key is still non-zero. Repeat with the flute used
first: Koru acts.

For a discarded pick, add a write watch on `0x801C8FE4` and a breakpoint on the action dispatch
at `0x801E2CDC`: the word changes in state `0x50` and no dispatch for seat 3 follows before the
round counter moves.

### Open questions

- A member who is alive but cannot act (petrified, or carrying the captured bit `+0x16E & 4`)
  still counts in the flute's `+1` loop, which tests HP only. Whether such a member is also
  removed from the round bound is not traced; it could move the overshoot by one.
- The Ultra Charge direction of the discarded pick and Van Saryu's one-shot are not captured.
- Of the flagged rows in Rikuroa, which belongs to which of Caruban, the Golem and the Piura is
  not decoded; the monster ids come from the rows themselves.

Related: [battle action state machine](../../subsystems/battle-action.md#the-turn-cursor-ctx0x1a),
[cast modules](../../subsystems/cast-module.md),
[the Koru timed-fight strip](../../subsystems/minigame-muscle-dome.md#the-koru-timed-fight-strip).

## The War God Icon sometimes gives only one attack string

**Status:** explained
**Reported as:** the accessory should make the wearer attack twice in a row, but it randomly fails to; players describe it in solo Muscle Dome Master Course fights ([community report](https://gamefaqs.gamespot.com/boards/197766-legend-of-legaia/75401453)).

There is no dice roll. The second pass of the attack string is granted at the end of the first
pass only if a one-byte pass counter reads zero.

The counter shares its byte with something unrelated: the AP cost of the Low command on the
arts-entry screen. Every time a fighter's command gauge is built for manual entry the byte is
set to that cost, 30 for a single-width command. A doubled attack also leaves it at 2 or more.
The game clears it in exactly one place: at the end of the short "step up to the target" phase
of an attack.

So the marker reads "second pass already used" at the start of every round whose commands were
typed by hand, and it stays that way until some attacker, party or monster, has to close the
distance to a target. If the wearer swings before anyone has done that, and is **already within
reach** of the target, the step-up phase is skipped, nothing has cleared the byte, and the
second pass is refused. The single pass that does run deals its damage normally.

Fighters are not walked back after an action. A monster that has just hit the wearer is left
standing at the very edge of the wearer's reach, and which side of the edge depends on the
knock-back of the move it used and on who the wearer is: Vahn's reach is 43 units shorter than
Noa's and 96 shorter than Gala's. The failing round is: the wearer is the first to attack,
types commands by hand, and targets the monster that hit them last round while it is still
within reach. In a crowded fight the other attackers shove the target away and clear the
counter, so it almost never lines up. In a one-on-one fight, which is where players report it,
it can.

Choosing "Auto" instead of typing commands, attacking a different monster, or letting anyone
else attack first all avoid it.

The refusal was captured in a running game on a poked state: with the byte at 30 from the arts
screen and the wearer nudged inside reach, the string-end check read 32 and went to recovery
with no second pass, three rounds in a row, and zeroing the byte at that instant brought the
second pass back. A refusal with no position nudge, and how often Noa or Gala reach it, are not
measured.

### Mechanism

The grant, the bump and the clear are in `FUN_801E295C` in PROT entry 0898. `s5 = ctx + 0x11`, so
`5(s5)` is the pass counter `ctx[+0x16]`, `4(s5)` the strike cursor `ctx[+0x15]` and `2(s5)` the
acting seat `ctx[+0x13]`.

The grant, reached when the queue byte at the cursor is the `0x00` terminator:

```text
801e39bc  sltiu v0,v0,3            ; acting seat must be a party seat
801e39fc  lw    v0,0x6bc(v0)       ; character record +0xF4
801e3a04  andi  v0,v0,0x2000       ; ability bit 0x0D, the War God Icon
801e3a08  beqz  v0,0x801e3a74      ; no icon: recovery state 0x1F
801e3a10  lbu   v0,5(s5)           ; pass counter
801e3a18  bnez  v0,0x801e3a74      ; NON-ZERO: recovery, no second pass
801e3a38  sb    zero,4(s5)         ; rewind the strike cursor
801e3a40  sb    v0,5(s5)           ; counter += 1
```

followed by the loop `0x801E3A44..0x801E3A64` that rewrites the queue for the replay.

Each staged strike bumps the counter again when it is already non-zero and the record carries
the bit (`0x801E37AC..0x801E37BC`). A doubled attack therefore ends with the counter at one plus
the number of strikes in its second pass: at least 2.

The only clear is on the exit of state `0x19`, the short step, taken when the range metric
`FUN_8004E2F0` returns zero:

```text
801e3594  jal   0x8004e2f0
801e35a0  bnez  v0,...             ; still out of reach: keep waiting
801e35b0  sb    v0,7(v1)           ; action state = 0x1E
801e35b4  sb    zero,5(s5)         ; pass counter = 0
```

The bypass is in state `0x14`, which runs the same range test first:

```text
801e31f4  jal   0x8004e2f0
801e3200  bnez  v0,0x801e3230      ; out of reach: approach (a party member goes to 0x19)
801e320c  addiu v0,zero,0x1e
801e3210  sb    v0,7(v1)           ; in reach: straight to 0x1E, counter untouched
```

**The fourth store.** The byte has one more writer, outside the action state machine. The
arts-entry gauge build, case 9 of `FUN_801D388C` in the same overlay, stores each direction
command's AP cost (the command record's `+0x74`) into four consecutive context bytes:

```text
801d38c8  ...                      ; s4 = ctx + 0x11
801d3b3c  sb    v0,3(a2)           ; a2 = s4 + i, i = 0..3: ctx[+0x14 + i]
801d3bc8  sltiu v0,a3,4            ; Left, High, Low, Right
```

`i = 2`, the Low command, is `ctx[+0x16]`. It runs each time the directional-entry screen comes
up, so with manual entry the byte holds the Low cost, `0x1E` for a single-width command, before
the round's first swing. The "Auto" command path does not build the gauge and leaves the byte
as the last attack left it. The tempting reading of the counter as private to the attack loop
fails here: the three stores in `FUN_801E295C` are not the only ones.

The complete list of stores that cover `ctx[+0x16]`, from a reference sweep of the executable
and every overlay (including the cast modules) that follows the context pointer through both
its `lui`-relative and its `gp`-relative loads:

| Store | Routine | Writes |
|---|---|---|
| `0x801D3B3C` | `FUN_801D388C`, gauge build | the Low command's AP cost, at manual command entry |
| `0x801E35B4` | `FUN_801E295C`, state `0x19` exit | zero |
| `0x801E37BC` | `FUN_801E295C`, strike stage | `+1`, only while already non-zero |
| `0x801E3A40` | `FUN_801E295C`, string end | `+1`, the grant |

The damage kernel only reads it, at `0x801EE10C` and `0x801EE968`. Battle setup zeroes it:
`FUN_80055B6C` in `SCUS_942.54` allocates the battle block, points `0x8007BD24` at
`block + 0x37B4`, and clears `0x1E8D` words from the block's base (`0x80055F14..0x80055F24`),
which covers the whole context. So in a fight entered with "Auto" the first attack always
doubles, and with manual entry the byte is already 30 before the first swing.

**Who clears it.** The exit of state `0x19` is reached by every attacker that has to close the
distance: a party member, and any monster whose record has no walk clip. The other route into
the strike loop, the walk chain `0x15..0x18`, needs an action entry tagged `0x20`
(`jal 0x80050E2C` with `a1 = 0x20` at `0x801E3260`) and does not clear
(`0x801E3548..0x801E3558`). Of the 186 monster records only six carry one: Caruban, Caruban $2,
Caruban $3, Evil Shadow, Shade and Nightmare. Every other monster, bosses included, takes the
`0x19` route and clears the counter whenever its melee starts out of reach.

| Sets the byte non-zero | Clears it | Leaves it as it is |
|---|---|---|
| Manual command entry (the Low cost, 30) | A party attacker who has to step up | A party attacker already in reach |
| A granted second pass (one plus its strikes) | A monster melee that starts out of reach, for all but six monsters | A monster already in reach, and the six that walk in |
| | | Magic, items, Spirit |

So the second pass is refused exactly when the byte is non-zero at the end of the wearer's first
pass: the wearer's attack entered the strike loop without passing the `0x19` exit, and no attack
has passed that exit since the byte was last written non-zero. Being in reach alone is not
enough. A wearer who starts in reach with a clean byte still doubles.

**Reach.** `FUN_8004E2F0` puts a party attacker in reach of a monster when
`reach_bias[character] + distance < size * 16` (`0x8004E344..0x8004E36C`,
`0x8004E504..0x8004E534`), where `size` is the monster record's byte `+0x1F` and `reach_bias` is
the table at `0x80078878`: `+43` for Vahn, `0` for Noa, `-53` for Gala. Vahn has to stand 43
units closer than Noa; Gala may stand 53 further away. Coordinates are re-centred on the target
at every action and nobody is walked home, so both sides stay where the last knock-back left
them.

**The wearer's counterattack.** The swap at `0x801E3664..0x801E36DC` makes the counterer the
acting seat inside the strike loop and rebuilds its queue (`jal 0x801eed1c` at `0x801E369C`), so
a wearer's counter inherits whatever the byte holds: doubled after a monster that came through
`0x19`, single after one of the six walkers or a monster that started in reach. Monsters
themselves never counter: the latch `0x801F6970` is armed only for a monster's physical pick on
a party seat whose record has the Counterattack bit `0x8000` (`0x801DAF74..0x801DB050`).

**Nothing else cuts the second string.**

- The target cannot die between the passes. While the counter is below 2 the damage kernel
  withholds the HP write (`0x801EE0F8..0x801EE120`, twin at `0x801EE954..0x801EE97C`), so the
  first pass's total is committed only when the second ends.
- When the pass is refused the counter is 2 or more, so the single pass applies its total as an
  ordinary attack does.
- No AP test, turn-order slot limit, speed comparison, combatant count or random draw appears
  between `0x801E39B4` and `0x801E3A64`.
- Magic, items and Spirit never double: only the strike loop grants.

One edge: the per-strike bump is a one-byte increment, so a byte left stale for long enough
wraps through zero and the next string end is granted again.

### What was observed in play

Two fights were run on the retail disc: three-on-one against Gaza and one-on-one against a Gobu
Gobu. No available save has the Icon equipped, so in every run the ability bit was poked onto
Vahn (`0x800847FC |= 0x2000`). The Gaza state has an unrelated arts-AP patch resident in the
battle overlay; the Gobu Gobu state is retail apart from four data bytes.

| What | How it was reached | What was seen |
|---|---|---|
| A normal double | Ability bit poked, nothing else | Range non-zero, state `0x19`, clear at `0x801E35B4`, strikes with the byte at 0, `0x801E3A18` reads 0, grant, second pass bumps it to 6, next read is 6 and goes to recovery. No HP is written until the second pass ends |
| In reach with a clean byte | Ability bit poked; Vahn's position not touched | The Gobu Gobu steps up to hit Vahn (clearing the 30 left by the gauge) and stays beside him. Vahn's attack reads range 0, takes `0x801E320C`, never passes `0x801E35B4`, and `0x801E3A18` reads 0: granted |
| The refusal | Captured on a poked state: ability bit; HP 9000 on both sides; Vahn's maximum HP raised each round so that he acts first; Vahn moved 49 to 78 units toward the target at the range test | `0x801E320C` taken, two strikes bump the byte from 30 to 32, `0x801E3A18` reads `0x20`, recovery, no second pass. One string, damage 84 to 88 committed normally. Three rounds running |
| The control | The same run, with the byte also poked to 0 at `0x801E3A10` | Grant taken, second pass runs, 178 committed instead of 88. The next round, with no such poke, is refused again |
| Every store to the byte | A write breakpoint over `ctx + 0x14..0x17` through both fights, about sixty attacks, a monster's magic cast, manual and Auto entry | Only the four stores in the table above write `ctx + 0x16`. No summon, cast-module action or item use ran under the watch; for those the list rests on the reference sweep |
| Battle setup | A save taken at the first prompt of the Gaza fight | `ctx + 0x14..0x17` read `00 00 00 00` |
| The gauge store | Manual entry, both fights | Entering directional entry takes `ctx + 0x15` and `ctx + 0x16` to 30 every time; under "Auto" the byte keeps its old value |

**How close an unassisted wearer gets.** With the position left alone, Vahn attacking the Gobu
Gobu that had just hit him read range results of `0x150` to `0x181` against a threshold of
`0x150` in eleven rounds, 0 to 49 units outside reach, and was inside it in one. In ten of those
eleven the margin is smaller than his own `+43` bias, so Noa or Gala standing in the same spot
would have been in reach; that is an inference from Vahn's margins, not a capture with them as
the wearer. Against Gaza with three attackers every start was far out of reach (range results
of 800 to 2260). A monster can start in reach as well and skip the clear, which was seen twice.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `ctx + 0x16` | RAM | Attack x2 pass counter, and the Low command's AP cost on the arts-entry screen | disassembly, capture |
| `0x80084140 + (id - 1) * 0x414 + 0x6BC` | RAM | Character record `+0xF4`; bit `0x2000` is the icon | disassembly |
| `0x801E39BC..0x801E3A64` | PROT 0898 | The grant and its guard chain; the zero test is `0x801E3A18` | disassembly, capture |
| `0x801E3768..0x801E37BC` | PROT 0898 | Per-strike bump | disassembly, capture |
| `0x801E35B4` | PROT 0898 | The one clear, on the state `0x19` exit | disassembly, capture |
| `0x801D3B3C` | PROT 0898 | Gauge build in `FUN_801D388C`: per-command AP cost into `ctx[+0x14 + i]`; `i = 2` is the counter | disassembly, capture |
| `0x801E31F4..0x801E3210` | PROT 0898 | State `0x14`: in reach goes straight to `0x1E` | disassembly, capture |
| `0x801E3260` | PROT 0898 | Looks up the tag-`0x20` walk entry that selects the walk chain | disassembly |
| `0x801E3548..0x801E3558` | PROT 0898 | State `0x18` into `0x1E`, no clear | disassembly |
| `0x801E3664..0x801E36DC` | PROT 0898 | Counterattack swap | disassembly |
| `0x801DAF74..0x801DB050` | PROT 0898 | Counter latch `0x801F6970`: armed only against a party seat with the Counterattack bit | disassembly |
| `0x801EE0F8..0x801EE120`, `0x801EE954..0x801EE97C` | PROT 0898 | Damage kernel's carry test, `counter < 2` | disassembly, capture |
| `0x8004E2F0` | SCUS | Range metric, `0` = in reach; party-on-monster test at `0x8004E344..0x8004E36C` | disassembly, capture |
| `0x80078878` | SCUS | Reach bias per character: `+43`, `0`, `-53` | disc data |
| monster record `+0x1F` | PROT 0867 | Size byte; reach threshold is `size * 16` | disc data |
| `0x80055F14..0x80055F24` | SCUS | Battle init `FUN_80055B6C` zeroes the battle block, context included | disassembly |
| `0x801DA850..0x801DA8C4` | PROT 0898 | Seeder's wound bonus for a party seat, from `max HP - HP` | disassembly |
| `0x801CEDA4`, `0x801CEDA8` | PROT 0898 | Jump-table slots of states `0x18` and `0x19` | disc data |

### Scope

Every wearer, plain attack strings and Arts alike: both run through the same strike loop. The
code is the same for all three characters, but the reach bias makes the in-reach start rare for
Vahn and likelier for Noa and Gala, and a fight with several attackers on either side almost
always has someone step up first. One-on-one fights with manual command entry and a wearer
faster than the opponent are where it shows: the solo Muscle Dome courses.

A side observation from the same code: the per-strike bump has no party-seat guard. For a
monster attacker it indexes the three-byte party roster at `0x8007BD10` with seat 3..6, reading
past its end, and tests an unrelated character record's bit. It can only increment a counter
that is already non-zero, so it does not affect the grant.

### Verifying it

Watch the byte at `ctx + 0x16`. Break on `0x801E3A18`: a non-zero read there is a refused second
pass. Break on `0x801E320C` with `ctx[+0x13] < 3` to catch the in-reach bypass, and on
`0x801E31FC` to log the range result. Writing zero to `ctx + 0x16` at `0x801E3A10` restores the
double.

The run that would close the remaining gap: a memory-card save with the Icon on Noa or Gala in a
one-on-one Muscle Dome fight, manual command entry, the wearer faster than the opponent, logging
`0x801E31FC` and `0x801E3A18` over ten rounds. The prediction is range 0 and a non-zero read on
most rounds that follow the opponent's melee.

### Open questions

- A refusal with nothing nudged: the captured one needed the wearer moved 49 to 78 units.
- How often a Noa or Gala wearer starts in reach. The estimate above comes from Vahn's margins.
- The wearer's own counterattack and the one-byte wrap are read from the code, not captured.

Related: [the action queue](../../subsystems/battle-action-queue.md#the-war-god-icons-per-stage-bump),
[the arts command gauge](../../subsystems/arts-command-gauge.md),
[accessory passives](../../formats/accessory-passive-table.md).

## Side Kick's first hit misses floating enemies even with War God Boots

**Status:** explained
**Reported as:** War God Boots let Gala's low kick hit floating enemies, but Side Kick still misses its first hit against them (community report).

Whether a hit can reach a floating enemy is decided by the hit's own **power byte**, not by
anything on the attacker. Low-type hits come in two encodings: one that cannot reach a floater
and one that always connects.

The boots do not grant an ability. Every pair of shoes ships its own Low and High kick, and the
War God Boots' Low kick simply carries the always-connects byte where Gala's other shoes carry
the restricted one.

Side Kick is not a shoe's kick. It is an art, stored in Gala's art bank with its own fixed
bytes, and its first hit is the restricted kind. Changing boots never touches it. It is also the
only art hit in the game encoded that way.

### Mechanism

The test sits at the head of the melee kernel `FUN_801EC3E4` in PROT entry 0898 and applies to
party attackers only:

```text
801ec488  sltiu v0,a3,3            ; attacker is a party seat
801ec494  lbu   a0,0(a1)           ; this hit's power byte
801ec49c  sltiu a1,a0,0x16         ; 0x16 and up always connects
801ec4c0  lbu   v1,0x1e(v0)        ; target monster record +0x1E
801ec4c8  bne   v1,2,...           ; class 2 (floating):
801ec4cc  sltiu v0,a0,0x11         ;   byte 0x11..0x15 misses
801ec504  bne   v1,3,...           ; class 3 (short):
801ec53c  sltiu v0,v0,0x11         ;   byte below 0x11 misses
801ec554  sb    v0,0x263(v1)       ; miss: ctx[+0x263] = 1, no roll, no damage
```

The power byte space, with the multiplier table at `0x801F64EC`:

| Byte | Kind | Reaches |
|---|---|---|
| `0x0C..0x10` | high, restricted | everything except class 3 |
| `0x11..0x15` | low, restricted | everything except class 2 |
| `0x16..0x1A` | high | everything |
| `0x1B..0x1F` | low | everything |

Where the byte comes from differs by command:

- **Direction commands** play swing records spliced in from the equipped items by `FUN_80052FA0`
  (copy helper `FUN_800557B8`). The footwear section's first record becomes the Low kick (action
  slot `0xE`) and its second the High kick (slot `0xF`). The power run is the record's first
  bytes.
- **Arts** play a record from the character's art bank (stride `0xD0`, power run at record
  `+0x24`), selected by action constant as record `constant - 0x10`. Equipment has no part in
  it.

Gala's footwear in his battle file (PROT entry 0865):

| Footwear | Low kick | High kick |
|---|---|---|
| none, Power Shoes (`0x66`), Fighting Boots (`0x67`) | `0x13` restricted | `0x0E` restricted |
| War God Boots (`0x68`) | `0x1D` free | `0x18` free |
| Ra-Seru Thongs (`0x69`) | `0x13` restricted | `0x18` free |

Side Kick is Gala's action constant `0x23`. Its record carries the power run `12 17` with hit
frames 7 and 16. `0x12` is low-restricted, so the first hit misses a class-2 target; `0x17` is
free, so the second connects.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801EC488..0x801EC554` | PROT 0898 | The limb-versus-height miss test | disassembly |
| `0x801F64EC` | PROT 0898 | Power multiplier table `0C 12 14 16 1C` | disc data |
| monster record `+0x1E` | PROT 0867 | Height class: `2` floating, `3` short | disc data |
| `ctx + 0x263` | RAM | Miss strobe | disassembly |
| `0x80052FA0`, `0x800557B8` | SCUS | Battle-file assembler and swing-record copy | repo doc |
| footwear section, word `+0x04` / `+0x08` | PROT 0863..0865 | Offsets of the Low / High swing records | disc data |
| art bank, record `(0x23 - 0x10)`, `+0x24` | PROT 0865 | Side Kick's power run `12 17` | disc data |
| Side Kick = constant `0x23` | - | Learned-art slot 8 for Gala | repo doc |

### Scope

**Arts.** Across the three characters' art banks every other non-zero power byte lies in
`0x16..0x1F`. The `0x12` in Gala's constant `0x23` is the single height-restricted art hit.

**Floating monsters** (record `+0x1E == 2`): Evil Fly, Death Wings, Demon Fly, Drake Ghost,
Guardian, Ghost Knight, the three Theeder, the three Vera, Pump Bat, Devil Pump, Boogie Pump, the
three Nighto, Hornet, Killer Bee, Queen Bee, Spikefish, Mad Bird, Wailing Bird, the three Viguro,
Mage, Sorcerer, Thermo, the three Gola Gola, Amethyst, Garnet, Topaz, the four Evil Bat records,
the three Slippery, the three Puera, the three Gilium, Lapis, Rogue and Zora.

**Short monsters** (`+0x1E == 3`, which high-restricted hits miss): Mushroom, Mushrin, Mush Mush,
Frog, Gel Frog, King Frog.

**The other characters' shoes** follow the same pattern:

| Character | Low kick reaches floaters with | Low kick restricted with | High kick restricted with |
|---|---|---|---|
| Vahn | Triumph Boots, Ra-Seru Boots | none, Warrior, Ironman, Master, Expert, Hero Boots | Ironman, Master, Expert, Hero Boots |
| Noa | nothing | every shoe, Ra-Seru Shoes included | none, Electric, Tempest Shoes, Steel Boots |
| Gala | War God Boots | none, Power Shoes, Fighting Boots, Ra-Seru Thongs | none, Power Shoes, Fighting Boots |

Weapon and Ra-Seru swing records (the Left and Right commands) are in the free ranges for all
three characters.

### Verifying it

Fight an Evil Fly with Gala in War God Boots and break on `0x801EC494`. A plain Low reads
`a0 = 0x1D` and connects. Side Kick reads `0x12` on its first hit event, branches to
`0x801EC548` and sets `ctx + 0x263`; its second reads `0x17` and connects. Changing the art
record's first power byte from `0x12` to `0x1C` (the same multiplier, free range) makes the first
hit land.

### Open questions

Whether `0x12` is deliberate, a low sweep meant to pass under fliers, or an authoring slip cannot
be told from the data.

Related: [damage formulas](../../subsystems/battle-formulas.md#the-limb-vs-height-miss),
[art data](../../formats/art-data.md), [player battle files](../../formats/battle-data-pack.md).

## The level-3 dance special swallows the next special

**Status:** explained
**Reported as:** in the dance minigame, the level-3 special animation "eats" the next special if two level-3 specials are done in a row (community report). The special is the Triangle move, the in-game lesson's "groovy move". An early fan guide gives the same warning from the other side: a Triangle move used at level 3 lasts so long that the player misses the next move.

A dancer's button presses are only judged while its body is playing the idle or the basic dance
loop. Every triggered move, whether a combo finisher, the Triangle "groovy move" or the flinch
after a wrong press, is a one-shot animation, and until it has played to its last frame the
judge is not called at all. A press made during it is not stored, not counted as a miss, and
for Triangle does not use up one of the three per song. It vanishes.

The level-3 animations are the longest. The level-3 Triangle move lasts 6.83 beats. The next
scoring slot closes at most 4.75 beats after the press, so a second Triangle there is always
lost. The next level-3 combo starts 6 beats after the press, so its first note is lost too and
that combo cannot be completed. A Triangle on the last note of that combo, 8 beats on, does
register.

Holding the button does not help: the game only sees a button on the frame it goes down.

All of this was watched in the running game, driven by scripted pad input with nothing written
to memory: presses made during the move reached the pad word but never the judge, the Triangle
count did not change, and the lock lasted exactly as long as the animation data says.

### Mechanism

The gate is in the per-dancer tick `FUN_801D1358` in PROT entry 0980:

```text
801d168c  lw   v0,0x10(s1)         ; this dancer's idle clip id
801d1690  lh   v1,0x5c(s0)         ; the clip bound right now
801d1698  beq  v1,v0,0x801d16b4    ; idle: judge
801d16a0  lw   v0,0x18(s1)         ; dance-loop clip id
801d16ac  bne  v1,v0,0x801d1750    ; anything else: skip the judge
801d16b4  jal  0x801d1af4          ; judge / award routine
```

The release is the clip driver's end flag, `+0x62 & 0x100`, tested at `0x801D14C8`; when it is
set the dance loop is bound again and the gate passes in the same call.

The clip driver `FUN_800204F8` (SCUS) steps the cursor by `(rate * 2 + div - 1) / div` times the
frame-delta byte `0x1F800393` (`0x800205C8..0x80020690`) and raises the end flag when the cursor
reaches `frames * 16 - 1` (`0x800206E4..0x8002072C`). The driver call at the end of the tick
that binds the clip is already its first step, so the gate is shut for exactly
`ceil((frames * 16 - 1) / (step * delta))` game frames after the judged one. There is no extra
frame for the release: reading the driver-at-the-end, test-at-the-start order as "one frame
late" double-counts the step taken on the binding frame.

The beat clock advances by the same frame delta times ten, with a beat of `0x119` units and an
acceptance window of the first `0xD2`. Both clocks scale together. The hall runs at a frame
delta of 3: the dance init writes the frame-step floor `0x8007B9D8 = 3` (`0x801CF068..0x801CF070`
in PROT entry 0980), and the frame driver `FUN_80016B6C` rebuilds `0x1F800393` every frame from
the worst recent frame time and raises it to that floor (`0x80017108..0x80017198`). So the clip
cursor moves 12 per game frame, clips whose end is not a multiple of 12 round up to the next
frame, and one beat is 28.1 delta units or 9.37 game frames.

Which animation plays: Triangle returns move index `lane * 2 + 0x10` (`0x801D1C9C`), using the
level the dancer had **before** the press promotes it. A completed combo returns
`lane * 4 + 4` for Square (`0x801D1F38`) or `lane * 4 + 6` for Circle (`0x801D2058`). Triangle
pressed together with Square or Circle is judged as Triangle alone: its arm sets the hit latch
`0x801D548C` (`0x801D1E18`) and both direction arms skip on a non-zero latch (`0x801D1E5C`,
`0x801D1F9C`). "With the last command of a combination" therefore means "instead of it".

Noa's move clips, from her descriptor at `0x801D4E1C` (all rate 8) and the dance hall's animation
bundle in PROT entry 1229 (all blend-gated with divisor 4, so a step of 4):

| Move | Clip id | Frames | Lock, game frames | Lock, delta units | Lock, beats |
|---|---|---|---|---|---|
| wrong-press flinch | 7 / 8 | 30 | 40 | 120 | 4.27 |
| level-1 combo | 10 / 9 | 15 / 20 | 20 / 27 | 60 / 81 | 2.14 / 2.88 |
| level-2 combo | 12 / 11 | 26 | 35 | 105 | 3.74 |
| level-3 combo | 14 / 13 | 30 | 40 | 120 | 4.27 |
| level-1 Triangle | 15 | 18 | 24 | 72 | 2.56 |
| level-2 Triangle | 16 | 34 | 46 | 138 | 4.91 |
| level-3 Triangle | 17 | 48 | 64 | 192 | 6.83 |

Triangle pays its real reward only on a combo slot, a beat with `beat & 3 == 3`, inside the
window (`0x801D1CA8..0x801D1CCC`). Combo slots are four beats apart. A press made `p` beats into
one slot (`p` up to 0.747) can be followed by a press on the next slot no later than `4.747 - p`
beats afterwards.

| First move | Lock, beats | What it costs afterwards |
|---|---|---|
| level-1 Triangle | 2.56 | nothing: a press on the next slot is judged |
| level-2 Triangle | 4.91 | the whole next slot: the gate reopens 1380 phase units after the press and the slot's window has closed by 1333 |
| level-3 Triangle | 6.83 | the whole next slot, and the first note of the next level-3 combo, 6 beats on. A press 8 beats on is judged |
| level-3 combo finisher | 4.27 | a next-slot press is judged only if it comes at least 4.27 beats (40 game frames) after the finishing press |
| wrong-press flinch | 4.27 | as for a level-3 finisher |

The level-3 chart row (`0x801D509C + 0x40`) places one three-note combo per eight beats, closing
on beats 3, 11, 19 and 27, so two combo finishers in a row are eight beats apart and never
collide. Measured from a Triangle pressed on one closing beat, the next combo's notes fall 6, 7
and 8 beats later. The first is inside the lock; the other two are judged as plain hits, and a
chain missing its first note does not close. Pressing Triangle on the third instead is judged
and pays in full, because Triangle replaces the closing press.

Because the judge is not called, a Triangle dropped this way does not reduce the stock: the
decrement at `0x801D1C98` is never reached. A Triangle pressed after the lock ends but outside a
combo-slot window does spend one, for only `(level) * 3` points (`0x801D1D98..0x801D1DCC`),
which is the other way a second special can feel swallowed.

**An unused buffer.** On clip end the tick checks a per-dancer "queued move" flag at
`0x801D55CC[i]` and, if it is set, binds the move named by `0x801D560C[i]` instead of returning
to the loop (`0x801D14DC..0x801D157C`). The award routine writes the move number on every
completed combo, but the flag itself is only ever written zero (`0x801D1ED4`, `0x801D2008`). No
image on the disc that can be loaded with the hall refers to it except the read at `0x801D14D8`
and those two stores, and a write watch on the flag words of three dancers saw only the two zero
stores. The follow-up can never fire. It reads as an unfinished "chain the next move" path,
which is an inference.

### What was observed in play

The qualifier (Noa and two rivals) was run in the retail hall. All input went through the
emulator's pad; level 3 was reached by pressing the chart's own notes; nothing was written to
memory. The run starts from a save state taken on the way into the hall, which then loads the
dance overlay from the retail disc. That state has an unrelated patch resident in the
executable: four code sites differ from the disc, none of them in the frame driver or the clip
driver, and the overlay's code is byte-identical to PROT entry 0980.

Triangle on consecutive combo slots at level 3 (bit `0x0010` of the new-press pad word
`0x8007B874`):

| Game frame | Beat | Bound clip | Judge called | Stock | Score |
|---|---|---|---|---|---|
| 543 | 51 | loop to 17 | yes | 3 to 2 | 180 to 255 |
| 581 | 55 | 17 | no, though the press is in the pad word | 2 | 255 |
| 607 | 57 | loop again | yes | 2 | 255 |
| 618 | 59 | loop to 17 | yes | 2 to 1 | 255 to 330 |
| 656 | 63 | 17 | no | 1 | 330 |
| 693 | 67 | loop to 17 | yes | 1 to 0 | 330 to 405 |

The gate stayed shut for exactly 64 game frames after each judged level-3 Triangle. The other
runs:

- **From the start of a song**, Triangle on every combo slot: the level-1 press (beat 3) reopens
  the gate 24 frames later and the next press is judged; the level-2 press (beat 7) holds it for
  46 frames and the press on beat 11 is not judged and not spent; the press on beat 15 is judged
  at level 3 and holds it for 64.
- **Triangle, then the next combo played normally**: the note on beat 57 is pressed with clip 17
  still bound and is not judged; beats 58 and 59 are judged as plain hits, the chain does not
  close and the score does not move. The combo after that closes normally.
- **Triangle, then Triangle on the last note of the next combo**: the note on beat 57 is dropped,
  the note on 58 is judged, and the Triangle on beat 59 is judged, takes the stock from 2 to 1
  and pays in full.
- **Holding the dropped Triangle** for 80 game frames, through the end of the lock: the pad word
  shows it on one frame only and nothing fires when the gate reopens.

The frame delta read 3 on every play frame, with three vertical blanks between game frames. The
measured locks for the combo clips are 20, 27, 35 and 40 game frames, as in the table above.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801D1358` | PROT 0980 | Per-dancer tick | disassembly |
| `0x801D168C..0x801D16B4` | PROT 0980 | The judging gate | disassembly, capture |
| `0x801D14C8` | PROT 0980 | Clip end-flag test | disassembly |
| `0x801D1AF4` | PROT 0980 | Judge / award routine | disassembly |
| `0x801D1C9C` | PROT 0980 | Triangle's clip pick, `lane * 2 + 0x10` | disassembly |
| `0x801D1CA8..0x801D1CCC` | PROT 0980 | Combo-slot and window test for Triangle | disassembly |
| `0x801D1C98` | PROT 0980 | Triangle stock decrement | disassembly |
| `0x801D1E5C`, `0x801D1F9C` | PROT 0980 | Direction arms skip when the hit latch `0x801D548C` is set, so Triangle pre-empts them | disassembly |
| `0x801D534C` | RAM | Triangle stock, one word per dancer | disassembly, capture |
| `0x801D55CC`, `0x801D560C` | RAM | Queued-move flag (never set) and move number | disassembly, capture |
| `0x801D4E1C` | PROT 0980 | Dancer descriptor table, `0x80` bytes each; move pairs from `+0x28` | disc data |
| `0x801D509C` | PROT 0980 | Step chart, three rows of `0x20` beats | disc data |
| `0x801CF070` | PROT 0980 | Dance init writes the frame-step floor, 3 | disassembly |
| `0x8007B9D8` | RAM | Frame-step floor | disassembly, capture |
| `0x80016B6C` | SCUS | Frame driver that rebuilds `0x1F800393` and applies the floor | disassembly |
| `0x800204F8` | SCUS | Clip driver | disassembly |
| `0x1F800393` | RAM | Frame-delta byte both clocks scale by; 3 in the hall | disassembly, capture |
| `0x8007B874` | RAM | Pad word of newly pressed buttons; Triangle is `0x0010` | capture |
| clip records 7..17 | PROT 1229 | Frame counts above | disc data |
| beat `0x119`, window `0xD2` | - | Beat clock constants | repo doc |

### Scope

The lock applies to every dancer and every move; the computer-controlled rivals go through the
same gate. Level 1 never blocks the next slot. Level 2's Triangle blocks the next slot as well,
every time, but does not reach the next level-3 combo, whose first note is 6 beats on. A wrong
press costs the same 4.27 beats as a level-3 finisher.

The capture ran the qualifier only. The gate is the same code in every mode of the minigame.

### Verifying it

Break on `0x801D16B4` for dancer 0 and log the frame number. After a level-3 Triangle the next
hit comes 64 game frames later at the hall's delta of 3. Or watch the bound clip id at actor
`+0x5C`: presses made while it reads `0x11` show in the pad word at `0x8007B874` but never reach
`0x801D1AF4`, and the stock word at `0x801D534C` does not change.

Related: [the dance minigame](../../subsystems/minigame-dance.md).
