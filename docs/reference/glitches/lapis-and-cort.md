# Lapis and Mystic Shield Cort

Three boss-specific oversights, traced through the battle overlay and the cast / summon modules of
the USA disc (`SCUS_942.54`): why the optional super-boss Lapis stops casting Lapis Wave after the
party leaves and returns, why that fight can be fled, and why two sweep summons land many hits on
Cort while his Mystic Shield is up. Everything here is read from the disassembly and the disc data.
None of it is an emulator capture; where a statement is a prediction it says so.

Shared vocabulary:

- The battle actor table is `0x801C9370[slot]`: party seats `0..2`, enemy seats `3..6`, the summon
  body in slot `7`. The monster record pointers are `0x801C9348[seat - 3]`. The battle context
  pointer is `*0x8007BD24`.
- The battle-action overlay is PROT entry 0898 (load base `0x801CE818`). The cast and summon
  modules are PROT entries 0903..0966, all linked at `0x801F69D8`
  ([cast-module.md](../../subsystems/cast-module.md)).

| Actor offset | Field |
|---|---|
| `+0x14C` / `+0x14E` | HP, current / maximum |
| `+0x150` / `+0x152` | MP, current / maximum |
| `+0x16E` | Status word (bit `0x4` is Stone) |
| `+0x1D9` / `+0x1DA` / `+0x1DC` | Playing clip id / staged clip id / commit flags |
| `+0x1DE` / `+0x1DF` | Action category / action id |
| `+0x1EF..+0x1F2` | Reaction-clip map: flinch, alternate flinch, knockdown, get-up |

One fact joins the first two entries. The Lapis fight is the only late-game boss fight whose
formation row leaves the "scripted fight" byte at zero, and every boss protection in the battle
code (no escape, instant-death resistance, debuff resistance, the boss stat profile) keys on that
one byte.

## Lapis Wave checks current MP

**Status:** explained

**Reported as:** Lapis Wave tests the party's current MP where it was meant to test maximum MP, so
Lapis can be killed outright with magic the party is not supposed to have (community report).

Lapis Wave is not a damage attack and not a conditional kill. It sets both the current and the
maximum MP of every party member to zero for the rest of that battle, which is why MP items do
nothing afterwards.

The test the report describes sits in Lapis's turn logic, not in the attack. Each time Lapis acts
it counts the party members whose **current** MP is above zero, and casts Lapis Wave only when
that count is non-zero. A party at zero current MP is left alone, whatever its maximum is.

Leaving a battle writes each member's current MP back to the character record and never writes the
maximum. A party that has taken one Lapis Wave and then fled therefore stands at zero current MP
with its real maximum intact. On the second visit Lapis sees nobody with MP and never casts the
wave, an MP item now works because the maximum is no longer zero, and a restored caster who acts
before Lapis's next turn can cast.

The cast that ends the fight is Jedo. Jedo's module has two outcomes chosen by the scripted-fight
flag: on a flagged fight it deals rolled damage, on an unflagged one it sets every living enemy's
HP to zero with no roll. The Lapis fight is unflagged ([next entry](#lapis-can-be-escaped-from)),
so the kill is certain.

A gate that read maximum MP would fire on the returning party, zero the maximum again and close the
route. Whether the designers intended that field is an inference; the code is equally consistent
with a gate written for the in-battle case, where both fields are zero after a wave.

### Mechanism

1. **The ambush is hardcoded.** The formation setup `FUN_80051D84` in `SCUS_942.54` compares the
   first formation id against `0xA7` (Lapis) and stores `1` into `ctx[+0x290]`, the back-attack
   code. A back attack skips the party's first command band
   ([battle-round-loop.md](../../subsystems/battle-round-loop.md)), so Lapis acts before any input.

   ```text
   80051fc0  lbu  v1,-0x42f4(v1)     ; first formation id, 0x8007BD0C
   80051fc4  li   v0,0xa7
   80051fc8  bne  v1,v0,0x80052068
   80051fd0  li   v0,0x1
   80051fe0  sb   v0,0x290(v1)       ; ctx[+0x290] = 1
   ```

2. **The gate reads current MP.** The monster AI pick `FUN_801E9FD4` in PROT entry 0898 dispatches
   on the formation id through the jump table at `0x801CF1CC` (index `id - 4`); the word at
   `0x801CF458` sends id `0xA7` to `0x801EBC38`.

   ```text
   801ebc60  lw    v0,0x0(v1)        ; party actor i
   801ebc68  lhu   v0,0x150(v0)      ; current MP
   801ebc70  beq   v0,zero,<next>
   801ebc78  addiu s1,s1,0x1         ; count members with MP
   801ebc90  beq   v0,zero,0x801ebdac  ; nobody: generic pick
   801ebc98  lhu   v0,0x150(s4)      ; Lapis's own MP
   801ebca0  sltiu v0,v0,0xff
   801ebca4  bne   v0,zero,0x801ebdac  ; needs 255
   801ebcac  sb    v0,0x1de(s4)      ; category 2, magic
   801ebcb4  li    v0,0xb5           ; Lapis Wave -> +0x1DF
   ```

   The loop runs over `0 .. ctx[0]` with no liveness test.

3. **The wave zeroes both fields.** `FUN_801F6A04` in PROT entry 0943 is the `0xB5` body. Its third
   arm walks every party seat:

   ```text
   801f6cf4  lhu v0,0x150(v1)
   801f6cfc  sh  v0,0x178(v1)        ; MP readout accumulator = old MP
   801f6d08  sh  zero,0x150(v0)      ; current MP = 0
   801f6d1c  sh  zero,0x152(v1)      ; maximum MP = 0
   ```

   The body calls no damage wrapper.

4. **Only current MP leaves the battle.** The escape routine `FUN_801E791C` in PROT entry 0898
   copies actor `+0x150` to the character record's current MP (`+0x10A`) at
   `0x801E7C78..0x801E7C80` and actor `+0x14C` to current HP (`+0x106`) at
   `0x801E7CD8..0x801E7CE0`. It has no store of `+0x152`. Battle load seeds `+0x150` / `+0x152`
   from the record's `+0x10A` / `+0x108` ([level-up.md](../../subsystems/level-up.md)).

5. **Jedo kills on an unflagged fight.** `FUN_801F6A58` in PROT entry 0931 (spell `0x9D`):

   ```text
   801f87d4  lbu v0,0x287(v1)        ; scripted-fight flag
   801f87dc  bne v0,zero,0x801f8844
   801f8804  lhu v0,0x14c(a0)        ; each enemy seat 3..6
   801f880c  beq v0,zero,<next>
   801f8814  sh  zero,0x14c(a0)      ; HP = 0
   801f8820  sb  a2,0x21c(v0)        ; defeat fade
   ```

   With the flag set the module instead reaches the damage roll at `0x801F8438` behind the test at
   `0x801F833C`.

Nighto (PROT entry 0907, `FUN_801F69E8`) also reaches its kill on this fight, as a roll rather than
a certainty. Its resist test is skipped when the flag is clear (`0x801F6BF0`), the resist roll is
`rand % (0x13 - magic_level) < 9` (`0x801F6C28..0x801F6CAC`), and the kill fork is `rand % 8 == 0`
(`0x801F6B50..0x801F6B7C`, store at `0x801F7E58`). At magic level 9 that is 0.9 x 1/8, about 11%
per cast. Lapis's record carries the resist byte `+0x20 = 1`, so a flagged fight would have forced
the resist.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x80051FC0..0x80051FE0` | `SCUS_942.54`, `FUN_80051D84` | Formation id `0xA7` forces `ctx[+0x290] = 1` (back attack) | disassembly |
| `0x801CF1CC`, word at `0x801CF458` | PROT entry 0898 | AI formation jump table; id `0xA7` arm is `0x801EBC38` | disc data |
| `0x801EBC38..0x801EBCB4` | PROT entry 0898, `FUN_801E9FD4` | Lapis arm: count party `+0x150`, require own MP `>= 0xFF`, queue `0xB5` | disassembly |
| `0x801F6A04` | PROT entry 0943 | Lapis Wave body, five arms, head table at `0x801F69D8` | disassembly |
| `0x801F6CFC`, `0x801F6D08`, `0x801F6D1C` | PROT entry 0943 | Stores to `+0x178`, `+0x150`, `+0x152` per party seat | disassembly |
| `0x801E7C78..0x801E7C80` | PROT entry 0898, `FUN_801E791C` | Escape write-back of current MP to record `+0x10A` | disassembly |
| `0x80084708 + n * 0x414` | RAM | Character record; `+0x108` max MP, `+0x10A` current MP | repo doc |
| `0x801F87D4..0x801F8820` | PROT entry 0931, `FUN_801F6A58` | Jedo: flag clear sets every living enemy's HP to 0 | disassembly |
| `0x801F833C`, `0x801F8438` | PROT entry 0931 | Jedo: flag set takes the rolled-damage path | disassembly |
| `0x801F6BF0`, `0x801F6C28`, `0x801F6B50`, `0x801F7E58` | PROT entry 0907, `FUN_801F69E8` | Nighto resist gate, resist roll, kill roll, kill store | disassembly |
| Spell record `0xB5` | `SCUS_942.54` spell table | Lapis Wave, cost 255, targets the whole party | disc data |
| Monster record 167, `+0x20` | PROT entry 0867 | Resist byte is `1` | disc data |

### Scope

- The escape is not required by the code. Any party that enters with every member at zero current
  MP is never waved. This is a prediction from the gate; the community route is escape and return,
  which works because the wave itself produces the zero.
- One member with a single point of MP re-arms the wave for the whole party. The count has no
  liveness test, so a fallen member's MP counts.
- Lapis needs 255 MP of its own for each cast.
- No other monster arm in the AI pick uses this "who has MP" count.
- That MP items do nothing at zero maximum MP is a community report consistent with the item
  routine's use of `+0x152` ([battle-action-exit-gates.md](../../subsystems/battle-action-exit-gates.md));
  the clamp itself is not re-read here.

### Verifying it

- Break at `0x801EBC68` on Lapis's turn. `v0` is a party actor; read `+0x150` and `+0x152`. On the
  first visit the count is non-zero and execution reaches `0x801EBCB4`. After fleeing and
  returning, `+0x150` reads 0, `+0x152` reads the real maximum, and the branch at `0x801EBC90` is
  taken.
- Write-watch a party actor's `+0x152` through the cast. The predicted writer is `0x801F6D1C`.
- After the escape read the character record at `0x80084708 + n * 0x414`: `+0x108` unchanged,
  `+0x10A` zero.
- Break at `0x801F87DC` while Jedo is cast on Lapis: `ctx[+0x287]` reads 0.

### Open questions

- Whether the gate was meant to read maximum MP.
- Whether the restored caster reliably acts before Lapis (speed 451) was not simulated.

## Lapis can be escaped from

**Status:** explained

**Reported as:** the party can run from Lapis, unlike other bosses (community report).

A fight is closed to escape only when the formation row that launched it has a non-zero first
byte. Nothing else marks a battle or a monster as a boss for the escape roll: not the scripted
battle opcode that starts the fight, and not the monster record.

Lapis's row has zero in that byte. The fight is launched by a scripted opcode like any boss fight,
yet the battle code treats it as an ordinary encounter, so Run, the escape accessories and the
Spikefish Flute all work.

The same byte feeds every other boss protection, so the Lapis fight loses those too. That is what
lets the instant-kill summons of the [previous entry](#lapis-wave-checks-current-mp) land.

### Mechanism

1. **Source.** At battle launch `FUN_801DA51C` in PROT entry 0897 reads byte 0 of the installed
   formation row and raises bit `0x80` of the per-battle flags byte `0x8007BD60` when it is
   non-zero ([encounter.md](../../formats/encounter.md)).

   ```text
   801da5f8  lw  v0,0x94(s1)         ; installed formation row
   801da600  lbu v0,0x0(v0)
   801da608  beq v0,zero,0x801da620
   801da610  lbu v0,-0x42a0(v1)      ; 0x8007BD60
   801da618  ori v0,v0,0x80
   801da61c  sb  v0,-0x42a0(v1)
   ```

2. **Latch.** `FUN_800513F0` in `SCUS_942.54` stores `(byte >> 5) & 4` into `ctx[+0x287]` at
   `0x80051430..0x80051444`.

3. **Consumer.** The escape roll `FUN_801E791C` in PROT entry 0898 fails the flee when the party
   roll is lower or when the flag is set ([battle-formulas.md](../../subsystems/battle-formulas.md)).

   ```text
   801e7afc  sltu v0,s2,s3           ; party roll < enemy roll
   801e7b00  bne  v0,zero,0x801e7b24 ; fail
   801e7b14  lbu  v0,0x287(v0)
   801e7b1c  beq  v0,zero,0x801e7b40 ; flag clear: success path
   ```

4. **Lapis's row.** Scene `deene` (Mt. Dhini) is PROT entry 0383. In its decoded MAN the fight is
   launched by `3E FF 0B` at offset `0x3840` (partition-2 record 18), naming formation row 11. The
   row sits at offset `0x38C3` and reads `00 00 00 01 A7 00 00 00`: a zero header, one monster, id
   `0xA7`.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801DA5F8..0x801DA61C` | PROT entry 0897, `FUN_801DA51C` | Row byte 0 non-zero raises bit `0x80` of `0x8007BD60` | disassembly |
| `0x80051430..0x80051444` | `SCUS_942.54`, `FUN_800513F0` | `ctx[+0x287] = (flags >> 5) & 4` | disassembly |
| `0x801E7AFC..0x801E7B24` | PROT entry 0898, `FUN_801E791C` | Escape roll: flag set forces the failure arm | disassembly |
| MAN offset `0x3840` | PROT entry 0383 (`deene`) | `3E FF 0B`, scripted battle on formation row 11 | disc data |
| MAN offset `0x38C3` | PROT entry 0383 (`deene`) | Row 11: `00 00 00 01 A7 00 00 00` | disc data |

### Scope

Every scripted-battle site on the disc whose formation row has a zero header byte:

| Scene | Opcode | Row | Monsters |
|---|---|---|---|
| `deene` (Mt. Dhini) | `3E FF 0B` | 11 | Lapis |
| `town01` (Rim Elm) | `3E FF 04` | 4 | Tetsu, the opening spar |
| `town0b`, `town0c` (Rim Elm) | `3E FF 03` | 3 | Queen Bee and three Killer Bees |
| `town0b` (Rim Elm) | `3E 00 02` | 2 | One Gimard |
| `stone` (Shadow Gate) | `3E 00 03` | 3 | Gold Face |

Every other scripted site names a flagged row: the Piura pair, Zeto, every Songi, the Viguro pair,
every Caruban, Dohati, every Berserker, the Kemaro pair, Van Saryu, Xain, the late Tetsu, both
Gaza fights, the three Delilas siblings, Koru, Rogue, Jette and both Cort forms. The monster ids
that appear only in unflagged rows and in no random region are 79 (Tetsu), 125 (Gold Face) and 167
(Lapis).

What the clear flag changes for Lapis, consumer by consumer:

| Consumer of `ctx[+0x287]` | Effect on the Lapis fight | Evidence |
|---|---|---|
| Escape roll, `0x801E7B14` | The party can flee | disassembly |
| Jedo kill arm, `0x801F87D4` | Certain kill | disassembly |
| Nighto resist gate, `0x801F6BF0` | The kill and confuse rolls run despite record `+0x20 = 1` | disassembly |
| Zenoir and Aluru resist gates | Same gate as Nighto | repo doc |
| Seru side-effect stager `FUN_801F3D3C` | Stat debuffs land on every hit, with no suppression roll | repo doc |
| Stat boost `FUN_80054CB0` | The random-encounter profile is installed instead of the boss profile | repo doc |
| Spikefish Flute, PROT entry 0925 | The flee arm runs | repo doc |

The stat-boost row is a prediction with a number attached. From the record's ATK 680, UDF 975,
LDF 987, INT 999 the random-encounter profile gives 680 / 1706 / 1727 / 1248, where the boss
profile would give 850 / 1950 / 1974 / 1123.

### Verifying it

- At the start of the fight read `0x8007BD60` (bit `0x80` clear) and `ctx[+0x287]` (0; a flagged
  boss reads 4).
- Read Lapis's actor at `*(0x801C9370 + 0xC)`: `+0x158` (ATK) and `+0x15C` (UDF) tell the two
  stat profiles apart.

### Open questions

- Lapis's installed stat block is predicted, not read from a running fight.

## Mushura and Theeder against Mystic Shield

**Status:** explained

**Reported as:** Mushura and Theeder do excessive damage to Cort while his Mystic Shield is up;
Crazy Driver racks up hits on the shield, the only place in the game where that happens
(community report).

Element, overflow and a special formula play no part. Every summon uses the same damage roll, and
the shield halves every hit on Cort, these two included.

Mushura and Theeder are sweep attacks. On each tick they test every enemy for contact with the
moving body or beam and hit whatever they touch. The only thing that stops them hitting the same
enemy on consecutive ticks is a test that the enemy is on its idle animation: a hit stages the
enemy's flinch, and the flinch is the lockout.

Mystic Shield zeroes Cort's four reaction-animation ids so that he does not flinch behind it. Clip
id zero is the idle clip. A hit therefore stages "idle", the lockout never engages, and the sweep
hits Cort again on every tick it overlaps him.

Mule, a Ra-Seru summon with the same sweep shape, carries an explicit test in the same place that
skips its hit while the shield is up. Mushura and Theeder have no such test. Three modules share
the pattern and one of them is guarded.

### Mechanism

1. **The shield.** The `0xAC` body in `FUN_801F7240`, PROT entry 0940, spawns the shield effect,
   stores its handle in `0x8007BD84`, and blanks the reaction map of the first enemy seat.

   ```text
   801f7678  sw  v0,-0x427c(v1)      ; 0x8007BD84 = effect handle
   801f7690  sb  zero,0x1ef(v0)
   801f769c  sb  zero,0x1f0(v0)
   801f76a8  sb  zero,0x1f1(v0)
   801f76b4  sb  zero,0x1f2(v0)
   ```

2. **The halving.** The damage finisher `FUN_801DDB30` in PROT entry 0898, for an enemy defender
   with the handle non-null, rewrites the attack value as `def + (atk - def) / 2`
   (`0x801DDB98..0x801DDBC8`). It applies to every caller.

3. **The break.** The per-frame actor pass `FUN_8004CE2C` in `SCUS_942.54`, with first formation id
   `0xB4`, restores the map to `3, 2, 4, 5` and clears the handle once the seat's HP is at or below
   half its maximum (`0x8004D534..0x8004D658`). Cort has 50000 HP, so the shield covers the first
   25000.

4. **Mushura's lockout.** `FUN_801F69D8` in PROT entry 0915, the run-over arm, every tick and for
   every enemy seat:

   ```text
   801f7844  jal  0x8004e2f0         ; range metric (slot 7, seat); 0 = touching
   801f7860  lhu  v0,0x14c(v1)       ; alive
   801f7870  lbu  v0,0x1d9(v1)       ; playing clip
   801f7878  bne  v0,zero,<next>     ; the only re-hit lockout
   801f789c  jal  0x801dd0ac         ; damage roll, power 0x12
   801f78f0  srl  s0,s0,0x2          ; a quarter, floor 1
   801f79c4  lbu  v0,0x1ef(v1)       ; flinch id (0x1f0 if zero)
   801f79dc  sb   v0,0x1da(v1)       ; stage it
   801f79e8  sb   v0,0x1dc(v1)       ; commit flags = 5
   ```

5. **Theeder's lockout.** `FUN_801F69D8` in PROT entry 0904, the sweep arm, every tick and for
   every enemy seat:

   ```text
   801f7ce0  lhu  v1,0x14c(a0)       ; alive
   801f7cf0  lbu  v0,0x1d9(a0)       ; playing clip
   801f7cf8  bne  v0,zero,<next>     ; the same lockout
   801f7d0c  addiu v0,v0,-0x30       ; beam bearing vs victim bearing
   801f7d10  sltiu v0,v0,0xfb1       ; inside +-0x30
   801f7d38  jal  0x801dd0ac         ; damage roll, power 0x11, not quartered
   801f7e0c  lbu  v0,0x1ef(v1)       ; flinch id (0x1f0 if zero)
   801f7e24  sb   v0,0x1da(v1)       ; stage it
   ```

6. **Why it never engages.** With the map blanked both modules stage clip `0`. The clip commit
   copies the staged id to the playing id
   ([battle-actor-rendering.md](../../subsystems/battle-actor-rendering.md)), so `+0x1D9` stays `0`
   and the test passes again on the next tick.

7. **Mule's guard.** `FUN_801F69FC` in PROT entry 0929 has the same lockout at `0x801F7E08`, then
   loads `0x8007BD84` at `0x801F7E34` and branches past the roll at `0x801F7E4C` when it is
   non-null.

Across all 64 cast and summon images, a clip-id test that sits directly in front of a damage-roll
call occurs three times: Theeder, Mushura and Mule. Every other read of `+0x1D9` in those images is
a wait for a clip to end. The other summons hit on a fixed step of their own sequence, once per
scripted beat, so a missing flinch changes nothing for them.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F7678` | PROT entry 0940, `FUN_801F7240` | Shield handle stored to `0x8007BD84` | disassembly |
| `0x801F7690..0x801F76B4` | PROT entry 0940 | Seat-3 reaction map `+0x1EF..+0x1F2` zeroed | disassembly |
| `0x801DDB98..0x801DDBC8` | PROT entry 0898, `FUN_801DDB30` | Halve the hit on an enemy while the handle is non-null | disassembly |
| `0x8004D534..0x8004D658` | `SCUS_942.54`, `FUN_8004CE2C` | Break at half HP: map restored to `3, 2, 4, 5`, handle cleared | disassembly |
| `0x801F7870` / `0x801F789C` | PROT entry 0915, `FUN_801F69D8` | Mushura: lockout test / damage roll | disassembly |
| `0x801F78F0` | PROT entry 0915 | Mushura: roll quartered, floor 1 | disassembly |
| `0x801F79C4..0x801F79E8` | PROT entry 0915 | Mushura: flinch staged from `+0x1EF` / `+0x1F0` | disassembly |
| `0x801F7CF0` / `0x801F7D38` | PROT entry 0904, `FUN_801F69D8` | Theeder: lockout test / damage roll | disassembly |
| `0x801F7D0C..0x801F7D14` | PROT entry 0904 | Theeder: cone test, `+-0x30` | disassembly |
| `0x801F7E0C..0x801F7E24` | PROT entry 0904 | Theeder: flinch staged from `+0x1EF` / `+0x1F0` | disassembly |
| `0x801F7E08` / `0x801F7E34` / `0x801F7E4C` | PROT entry 0929, `FUN_801F69FC` | Mule: lockout test / shield test / damage roll | disassembly |
| `FUN_8004AD80` | `SCUS_942.54` | Clip commit: playing id takes the staged id | repo doc |
| Monster record 180 | PROT entry 0867 | Cort: HP 50000, element dark, size byte `0x16` | disc data |

### Scope

- Only Cort id `0xB4` (scene `chitei2`, formation row 13) while `0x8007BD84` is non-null: his first
  25000 HP. The Mystic Shield body is that word's only non-zero writer.
- The cause is an all-zero reaction map, so a monster whose record supplies no flinch entry at all
  would be hit the same way by these two summons. That set is not enumerated here.
- Each extra hit runs the finisher's tail as well: summon spell experience, the Seru side-effect
  shave and the damage popup all accrue per hit.
- Mule is the opposite case: under the shield its sweep does not hit at all.

**Size of the excess (estimate, not measured).** Theeder's beam swings `0x100` either side of its
facing through one sine period of the phase word `ctx[+0x6D8]`, and the victim stands on the centre
line ([cast-module.md](../../subsystems/cast-module.md)). The `0x30` cone holds the centre for about
12% of the sweep. If the sweep lasts 64 ticks, that is roughly 6 to 8 hits, each a full roll halved
by the shield: on the order of three to four times an unshielded single hit, where the shield's
rule alone would give half. The tick count is an assumption, since the retail step scales with the frame
delta. Mushura deals one eighth of a roll per contact tick; its contact length is not computed.

### Verifying it

- Cast Mushura on shielded Cort with an execute breakpoint at `0x801F789C` (Theeder:
  `0x801F7D38`) and count the hits in one cast. Repeat once Cort is at or below 25000 HP.
- Watch `+0x1D9` of the actor at `*(0x801C9370 + 0xC)` through the cast: predicted to stay `0`
  under the shield and to read `3` after a hit without it.
- Write `3` to that actor's `+0x1EF` before the cast while the shield is up. If the extra hits
  stop, the lockout is the cause and the shield handle is not.

### Open questions

- Hit counts per cast, which depend on the frame delta.
- Whether Theeder is meant to hit a second time when the beam crosses back over a victim that has
  returned to idle. That sets the unshielded baseline.
- Which monster records, if any, carry an empty reaction map.
