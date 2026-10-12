# Turn order, double attacks and kicks

Four player-visible oddities that come out of the battle turn-order and attack-string code, plus
one from the dance minigame's input gate: the Spikefish Flute cancelling enemy turns (the "Koru
flute glitch"), the War God Icon sometimes giving one attack string where two are expected, Side
Kick's first hit missing floating enemies even with War God Boots, and a level-3 dance special
swallowing the next one.

Every explanation here is read from the USA executable (`SCUS_942.54`) and its overlays, and from
the data on the disc. None of it is an emulator capture: where a section says what happens in
play, that is a prediction from the code, and each section ends with the debugger check that
would confirm it. Addresses are runtime virtual addresses for the USA build.

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

In an ordinary fight the same item simply ends the battle: the party walks off and the escape
teardown runs.

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

**Koru.** His arm of the monster AI `FUN_801E9FD4` is at `0x801EB52C`:

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
changes.

**Keys are rolled every round.** The round-top arm calls the seeder `FUN_801DA780`
unconditionally (`0x801D0ED8`): every seated fighter gets `SPD + rand % (SPD/2 + 1) + 1` plus a
wound bonus, halved under Slow. That is why a fast party "sometimes" fails to skip a boss: the
flute user sometimes rolls first, and a flute used first skips nobody.

### What is re-verified and what is not

Read directly from the instructions or the disc data for this page:

- the scripted-fight test, the whole flute tail, the round-over compare and the round counter's
  single writer;
- Koru's script, its jump table, the countdown arithmetic and its gate on monster id `0xB6`;
- the picker running the AI before the round-over test, and the unconditional re-seed;
- Koru's record: no live magic id, entries 5 and 6 with hit frame 200, cost 200, clip lengths 64
  and 30.

Carried from an earlier static trace and **not** re-read here:

- the per-boss consequences in the table under [Scope](#scope), including the claim that evolved
  Cort's "charged" flag (`0x801C8FE4`, written at `0x801EBA84` / `0x801EBAE8`) flips on the
  discarded pick, which would erase a charged Final Crisis;
- first-form Cort's forced key of 30000 on round 0 (the store is visible at
  `0x801DAAE4..0x801DAAF0`, but the fight it belongs to was not cross-checked);
- the list of fights that carry the scripted flag.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F6A00` | PROT 0925 | Spikefish cast module body | disassembly |
| `0x801F73E8` | PROT 0925 | Reads `ctx[+0x287]`, the scripted-fight byte | disassembly |
| `0x801F78F8` | PROT 0925 | Ordinary fight: stores action state `0x65` (run wait) | disassembly |
| `0x801F79B8..0x801F7A88` | PROT 0925 | The tail: cursor bump per living seat, item refund, `-1`, party keys zeroed | disassembly |
| `0x800421D4` | SCUS | Give-item routine the refund calls | disassembly |
| `ctx + 0x1A` | RAM | Turn cursor: turns taken this round | disassembly |
| `ctx + 0x25` | RAM | Fighters dropped from the round without acting | disassembly |
| `ctx + 0x28A` | RAM | Round counter | disassembly |
| actor `+0x16C` | RAM | Initiative key, `0` = spent | disassembly |
| `0x801E679C..0x801E67E4` | PROT 0898 | End-of-action gate: cursor `+1`, round-over compare at `0x801E67C8` | disassembly |
| `0x801E6800..0x801E6810` | PROT 0898 | Round counter `+1` | disassembly |
| `0x801DA780` | PROT 0898 | Initiative seeder, called from `0x801D0ED8` every round | disassembly |
| `0x801DABA4` | PROT 0898 | Next-fighter picker; calls the monster AI at `0x801DAF48` | disassembly |
| `0x801E5D58` | PROT 0898 | State `0x50` calls the picker, ahead of the `0x5A` test | disassembly |
| `0x801EB52C` | PROT 0898 | Koru's AI arm; `sltiu v0,v1,5` at `0x801EB540` | disassembly |
| `0x801CF49C` | PROT 0898 | Koru's five-entry jump table | disc data |
| `0x801D0F94..0x801D0FA4` | PROT 0898 | Countdown `4 - counter` into `0x801F6958` | disassembly |
| `0x801EC468` | PROT 0898 | Melee kernel's hit-frame gate | disassembly |
| monster `0xB6` entries 5, 6 | PROT 0867 | Hit frame `0xC8`, cost `0xC8`, clips 64 / 30 frames | disc data |
| `0x801C8FE4` | RAM | Evolved Cort's "charged" flag (carried from the earlier trace, not re-read) | inference |

### Scope

Any fight with `ctx[+0x287]` set. The skip is not Koru-specific; what differs per boss is what a
lost or discarded turn costs.

The table below is carried from the earlier trace and is **not re-verified**: treat each row as
a prediction until its AI arm is read.

| Boss | Move | What a flute skip does (predicted) |
|---|---|---|
| Koru | Dead End Crisis | Gone for good; the script runs off its end and the strip shows 255 |
| Cort, first form | Mystic Shield | Cannot be skipped (forced first turn on the opening round) |
| Cort, evolved | Ultra Charge into Final Crisis | Erases a charged Final Crisis, or arms one with no visible charge |
| Zeto, Xain | Two-step specials keyed on a flag the moves themselves write | Second step delayed one turn |
| Gi, Che, Lu | Every-third-round specials | Lost until three rounds later |
| Rogue | Element change on even rounds, elemental attack on odd | Element stays, or the attack is lost |
| Van | Once-per-battle Jugger Power | Can be spent without happening |
| Others with a per-round roll | - | Lose that round's roll |

Only Koru's script has an end to run past, which is why only he shows the 255 countdown and the
harmless attacks afterwards.

### Verifying it

Break on `0x801F79F0` and `0x801E67C8`. Watch the byte at `ctx + 0x1A`, the byte at
`ctx + 0x28A`, the byte at `0x801F6958` and the halfword `+0x16C` of all four fighters. With the
flute used second or third on the round where the counter reads 4, the three party keys drop to
zero together, the cursor jumps by `L - 1`, and the counter goes from 4 to 5 while Koru's key is
still non-zero. Repeat with the flute used first: Koru acts.

### Open questions

- A member who is alive but cannot act (petrified, or carrying the captured bit `+0x16E & 4`)
  still counts in the flute's `+1` loop, which tests HP only. Whether such a member is also
  removed from the round bound is not traced; it could move the overshoot by one.
- The per-boss table above.
- The Japanese release is reported to behave the same; only the USA code is read here.

Related: [battle action state machine](../../subsystems/battle-action.md#the-turn-cursor-ctx0x1a),
[cast modules](../../subsystems/cast-module.md),
[the Koru timed-fight strip](../../subsystems/minigame-muscle-dome.md#the-koru-timed-fight-strip).

## The War God Icon sometimes gives only one attack string

**Status:** partially explained
**Reported as:** the accessory should make the wearer attack twice in a row, but it randomly fails to (community report).

There is no dice roll. The second pass of the attack string is granted at the end of the first
pass only if a one-byte pass counter reads zero. A doubled attack leaves that counter at 2 or
more, and the game clears it in exactly one place: at the end of the short "step up to the
target" phase of an attack.

If the wearer's next attack starts **already within reach of its target**, the game skips the
step-up phase and goes straight to the strikes. The counter is never cleared, the end-of-string
check finds it non-zero, and the second pass is refused. The single pass that does run deals its
damage normally.

Fighters are not walked back to their starting spots after an action, so "already within reach"
gets more likely as a fight goes on: attacking the monster you are standing beside, or one that
has just walked up and hit you. Anything that sends another attacker through the step-up phase
in between clears the counter again, which is why the failure looks random.

The condition is pinned in the code. How often real fights reach it depends on where everyone is
standing and is not measured, which is why the status is partial.

### Mechanism

All of this is in `FUN_801E295C` in PROT entry 0898. `s5 = ctx + 0x11`, so `5(s5)` is the pass
counter `ctx[+0x16]`, `4(s5)` the strike cursor `ctx[+0x15]` and `2(s5)` the acting seat
`ctx[+0x13]`.

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

A monster with a walk clip reaches the strike loop through states `0x15..0x18` and does not
clear the counter either (`0x801E3548..0x801E3558`).

A linear disassembly of the whole overlay finds three stores to that byte and no others:
`0x801E35B4` (clear), `0x801E37BC` (per-strike bump) and `0x801E3A40` (grant). The damage kernel
only reads it, at `0x801EE10C` and `0x801EE968`.

So the second pass is refused exactly when the wearer's attack enters the strike loop without
passing the `0x19` exit, and no attack has passed that exit since the last doubled attack.

| Clears the counter | Does not |
|---|---|
| A party attacker who needed the short step | A party attacker already in reach |
| A monster with no walk clip (it uses `0x19` too) | A monster that walks in (`0x15..0x18`) |
| | Magic, items, Spirit |

A second route to the same refusal is the counterattack. The swap at
`0x801E3664..0x801E36DC` makes the counterer the acting seat inside the strike loop and rebuilds
its queue (`jal 0x801eed1c` at `0x801E369C`), so a wearer's counter inherits whatever the byte
holds: doubled if zero, single if stale.

When the pass is refused the damage is not lost. The kernel withholds the HP write only while
the counter is below 2 (`0x801EE0F8..0x801EE120`); a stale counter is 2 or more, so the single
pass applies its total as an ordinary attack does.

No turn-order slot limit, speed comparison, combatant count or random draw appears between
`0x801E39B4` and `0x801E3A64`.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `ctx + 0x16` | RAM | Attack x2 pass counter | disassembly |
| `0x80084140 + (id - 1) * 0x414 + 0x6BC` | RAM | Character record `+0xF4`; bit `0x2000` is the icon | disassembly |
| `0x801E39BC..0x801E3A64` | PROT 0898 | The grant and its guard chain; the zero test is `0x801E3A18` | disassembly |
| `0x801E3768..0x801E37BC` | PROT 0898 | Per-strike bump | disassembly |
| `0x801E35B4` | PROT 0898 | The one clear, on the state `0x19` exit | disassembly |
| `0x801E31F4..0x801E3210` | PROT 0898 | State `0x14`: in reach goes straight to `0x1E` | disassembly |
| `0x801E3548..0x801E3558` | PROT 0898 | State `0x18` into `0x1E`, no clear | disassembly |
| `0x801E3664..0x801E36DC` | PROT 0898 | Counterattack swap | disassembly |
| `0x801EE0F8..0x801EE120`, `0x801EE954..0x801EE97C` | PROT 0898 | Damage kernel's carry test, `counter < 2` | disassembly |
| `0x8004E2F0` | SCUS | Range metric, `0` = in reach | repo doc |
| `0x801CEDA4`, `0x801CEDA8` | PROT 0898 | Jump-table slots of states `0x18` and `0x19` | disc data |

### Scope

Every wearer, plain attack strings and Arts alike: both run through the same strike loop.
Nothing in it is specific to a character or an enemy.

A side observation from the same code: the per-strike bump has no party-seat guard. For a
monster attacker it indexes the three-byte party roster at `0x8007BD10` with seat 3..6, reading
past its end, and tests an unrelated character record's bit. It can only increment a counter
that is already non-zero, so it does not affect the grant.

### Verifying it

Watch the byte at `ctx + 0x16`. Break on `0x801E3A18`: a non-zero read there is a refused second
pass. Break on `0x801E320C` with `ctx[+0x13] < 3` to catch the in-reach bypass.

Predicted reproduction: one wearer alone against one enemy that walks in to attack. The first
attack doubles. The next one does not, unless a knock-back has put the target out of reach
again. Writing zero to `ctx + 0x16` before the second attack restores the double.

### Open questions

- How often a party attacker starts in reach. It needs captures of the range metric's return at
  `0x801E31F4` across ordinary fights.
- That battle setup zeroes the byte is inferred from the first doubled attack of a fight
  working, not read.
- Whether some reports are a second pass that ran but whiffed is not examined.

Related: [the action queue](../../subsystems/battle-action-queue.md#the-war-god-icons-per-stage-bump),
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

**Status:** partially explained
**Reported as:** in the dance minigame, the level-3 special animation "eats" the next special if two are done in a row (community report).

A dancer's button presses are only judged while its body is playing the idle or the basic dance
loop. Every triggered move, whether a combo finisher, the Triangle "groovy move" or the flinch
after a wrong press, is a one-shot animation, and until it has played to its last frame the
judge is not called at all. A press made during it is not stored, not counted as a miss, and
for Triangle does not use up one of the three per song. It vanishes.

The level-3 animations are the longest. The level-3 Triangle move lasts 6.83 beats, while the
next scoring slot closes at most 4.75 beats after the press. A second Triangle on that slot is
always lost.

The lock and its length are read from the code and the animation data. Which pair of moves the
report means by "two specials in a row" is not certain, so the table under Mechanism covers
each pairing. That uncertainty is why the status is partial.

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
reaches `frames * 16 - 1` (`0x800206E4..0x8002072C`). The driver is called at the end of the
tick and the flag is tested at the start, so the release is seen one game frame late.

The beat clock advances by the same frame delta times ten, with a beat of `0x119` units and an
acceptance window of the first `0xD2`. Both clocks scale together, so the figures below hold at
any frame rate. One beat is 28.1 delta units.

Which animation plays: Triangle returns move index `lane * 2 + 0x10` (`0x801D1C9C`), using the
level the dancer had **before** the press promotes it. A completed combo returns
`lane * 4 + 4` for Square (`0x801D1F38`) or `lane * 4 + 6` for Circle (`0x801D2058`).

Noa's move clips, from her descriptor at `0x801D4E1C` (all rate 8) and the dance hall's animation
bundle in PROT entry 1229 (all blend-gated with divisor 4, so a step of 4):

| Move | Clip id | Frames | Lock, delta units | Lock, beats |
|---|---|---|---|---|
| wrong-press flinch | 7 / 8 | 30 | 120 | 4.27 |
| level-1 combo | 10 / 9 | 15 / 20 | 60 / 80 | 2.14 / 2.85 |
| level-2 combo | 12 / 11 | 26 | 104 | 3.70 |
| level-3 combo | 14 / 13 | 30 | 120 | 4.27 |
| level-1 Triangle | 15 | 18 | 72 | 2.56 |
| level-2 Triangle | 16 | 34 | 136 | 4.84 |
| level-3 Triangle | 17 | 48 | 192 | 6.83 |

Triangle pays its real reward only on a combo slot, a beat with `beat & 3 == 3`, inside the
window (`0x801D1CA8..0x801D1CCC`). Combo slots are four beats apart. A press made `p` beats into
one slot (`p` up to 0.747) can be followed by a press on the next slot no later than `4.747 - p`
beats afterwards.

| First move | Lock, beats (with the late frame) | Next-slot press |
|---|---|---|
| level-1 Triangle | 2.56 (2.67) | accepted |
| level-2 Triangle | 4.84 (4.95) | the whole next slot is inside the lock |
| level-3 Triangle | 6.83 (6.94) | the whole next slot is inside the lock; the slot after opens 7.25 beats on at the earliest |
| level-3 combo finisher | 4.27 (4.38) | accepted only late: at least 4.38 beats after the finishing press |
| wrong-press flinch | 4.27 (4.38) | as for a level-3 finisher |

The level-3 chart row (`0x801D509C + 0x40`) places one three-note combo per eight beats, closing
on beats 3, 11, 19 and 27, so two combo finishers in a row are eight beats apart and never
collide.

Because the judge is not called, a Triangle dropped this way does not reduce the stock: the
decrement at `0x801D1C98` is never reached. A Triangle pressed after the lock ends but outside a
combo-slot window does spend one, for only `(level) * 3` points (`0x801D1D98..0x801D1DCC`),
which is the other way a second special can feel swallowed.

**An unused buffer.** On clip end the tick checks a per-dancer "queued move" flag at
`0x801D55CC[i]` and, if it is set, binds the move named by `0x801D560C[i]` instead of returning
to the loop (`0x801D14DC..0x801D157C`). The award routine writes the move number on every
completed combo, but the flag itself is only ever written zero (`0x801D1ED4`, `0x801D2008`); the
image holds three references to it and none stores a non-zero value. The follow-up can never
fire. It reads as an unfinished "chain the next move" path, which is an inference.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801D1358` | PROT 0980 | Per-dancer tick | disassembly |
| `0x801D168C..0x801D16B4` | PROT 0980 | The judging gate | disassembly |
| `0x801D14C8` | PROT 0980 | Clip end-flag test | disassembly |
| `0x801D1AF4` | PROT 0980 | Judge / award routine | disassembly |
| `0x801D1C9C` | PROT 0980 | Triangle's clip pick, `lane * 2 + 0x10` | disassembly |
| `0x801D1CA8..0x801D1CCC` | PROT 0980 | Combo-slot and window test for Triangle | disassembly |
| `0x801D1C98` | PROT 0980 | Triangle stock decrement | disassembly |
| `0x801D534C` | RAM | Triangle stock, one word per dancer | disassembly |
| `0x801D55CC`, `0x801D560C` | RAM | Queued-move flag (never set) and move number | disassembly |
| `0x801D4E1C` | PROT 0980 | Dancer descriptor table, `0x80` bytes each; move pairs from `+0x28` | disc data |
| `0x801D509C` | PROT 0980 | Step chart, three rows of `0x20` beats | disc data |
| `0x800204F8` | SCUS | Clip driver | disassembly |
| `0x1F800393` | RAM | Frame-delta byte both clocks scale by | disassembly |
| clip records 7..17 | PROT 1229 | Frame counts above | disc data |
| beat `0x119`, window `0xD2`, hall delta 3 | - | Beat clock constants | repo doc |

### Scope

The lock applies to every dancer and every move; the computer-controlled rivals go through the
same gate. Level 1 never blocks the next slot. Level 2's Triangle does by this arithmetic, with
about a frame and a half to spare, so it may be felt as inconsistent rather than certain.

### Verifying it

Break on `0x801D16B4` for dancer 0 and log the frame number. After a level-3 Triangle the next
hit comes 65 game frames later at the documented delta of 3. Or watch the bound clip id at actor
`+0x5C`: presses made while it reads `0x11` never reach `0x801D1AF4`, and the stock word at
`0x801D534C` does not change.

### Open questions

- Which pairing the report means.
- The hall's actual frame delta in play; it affects only the one-frame and rounding terms.
- That the pad word the judge reads holds newly pressed buttons only is taken from the repo
  docs.

Related: [the dance minigame](../../subsystems/minigame-dance.md).
