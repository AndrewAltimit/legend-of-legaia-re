# Lapis and Mystic Shield Cort

Three boss-specific oversights, traced through the battle overlay and the cast / summon modules of
the USA disc (`SCUS_942.54`): why the optional super-boss Lapis stops casting Lapis Wave after the
party leaves and returns, why that fight can be fled, and why two sweep summons land extra hits on
Cort while his Mystic Shield is up. The mechanisms are read from the disassembly and the disc data.
The Mystic Shield hit counts and damage, Lapis's installed stats, the Lapis Wave MP wipe, the MP
gate and Jedo's kill are also measured in a running game, in fights started by forcing the boss's
formation row into another scripted battle ([capture pedigree](#capture-pedigree)). Where a
statement is still a prediction it says so.

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

## Capture pedigree

Every `capture` on this page is PCSX-Redux running the retail USA disc. None of them is a played
fight at the boss, and each citation below says which of these apply:

- **Forced formation.** The Lapis and Cort fights are not reached by walking to the boss. Each
  starts from a retail save state at the Queen Bee fight in Rim Elm, a scripted battle about to
  begin with Vahn alone, and overwrites that battle's formation row in RAM at the instruction that
  reads its first byte (`0x801DA600`). For Cort the row written is `01 00 00 01 B4`; for Lapis it is
  `00 00 00 01 A7 00 00 00`, the exact bytes of the disc's row. Everything after that read - the
  flag latch, the monster load, the stat profile, the monster's turn logic, Cort casting and
  losing his own Mystic Shield - is the game's code running on the game's data. What is not retail:
  the party (Vahn alone at opening-chapter stats), the battle stage (Rim Elm) and the frame load,
  which is lighter than in the real three-character fight.
- **Injected casts.** Vahn owns none of the spells involved. When his action is seeded, the queued
  action is rewritten to the spell (`+0x1DE = 2`, `+0x1DF` = spell id); the game then loads and
  runs the module itself.
- **Poked values.** Vahn's HP is held at 9000 so that he survives, and his MP is topped up. For the
  summon measurements his magic level for the spell is set to 1, because at the default the roll
  saturates at 9999 and hides the shield's halving. The unshielded Cort baseline pokes Cort's HP to
  25000 and lets the game break the shield itself on the next frame. The Lapis MP-gate run writes
  Vahn's current MP to 0 once. "Captured on a poked state" below means one of these.
- **Which runs are cited.** A first batch of these runs held Vahn's HP with a write that started
  before the battle existed, and so also wrote field-mode memory for the first few seconds. Every
  experiment was rerun with that write confined to the battle; the reruns gave the same counts,
  and they are the ones cited. The one exception is flagged where it is used (the shield breaking
  in the middle of a Mushura cast).

## Lapis Wave checks current MP

**Status:** explained

**Reported as:** Lapis Wave tests the party's current MP where it was meant to test maximum MP, so
Lapis can be killed outright with magic the party is not supposed to have (community report).

Lapis Wave is not a damage attack and not a conditional kill. It sets both the current and the
maximum MP of every party member to zero for the rest of that battle, which is why MP items do
nothing afterwards. In a capture of the forced fight Lapis casts it before the party can enter a
command, and the lone party member's MP goes from 27 / 27 to 0 / 0.

The test the report describes sits in Lapis's turn logic, not in the attack. Each time Lapis acts
it counts the party members whose **current** MP is above zero, and casts Lapis Wave only when
that count is non-zero. A party at zero current MP is left alone, whatever its maximum is: with
current MP set to 0 and the maximum left at 27, Lapis attacks physically and never casts the wave
(captured on a poked state).

Leaving a battle writes each member's current MP back to the character record and never writes the
maximum. A party that has taken one Lapis Wave and then fled therefore stands at zero current MP
with its real maximum intact. On the second visit Lapis sees nobody with MP and never casts the
wave, an MP item now works because the maximum is no longer zero, and a restored caster who acts
before Lapis's next turn can cast.

The cast that ends the fight is Jedo. Jedo's module has two outcomes chosen by the scripted-fight
flag: on a flagged fight it deals rolled damage, on an unflagged one it sets every living enemy's
HP to zero with no roll. The Lapis fight is unflagged ([next entry](#lapis-can-be-escaped-from)),
so the kill is certain. An injected Jedo in the forced fight takes Lapis from 64800 HP to 0 in
one store.

The steps are captured separately, not as one played route: the wave, the gate at zero current MP
and the Jedo kill are each observed, while the escape, the return visit and the MP item between
them are read from the code.

A gate that read maximum MP would fire on the returning party, zero the maximum again and close the
route. Whether the designers intended that field is an inference; the code is equally consistent
with a gate written for the in-battle case, where both fields are zero after a wave.

### Mechanism

1. **The ambush is hardcoded.** The formation setup `FUN_80051D84` in `SCUS_942.54` compares the
   first formation id against `0xA7` (Lapis) and stores `1` into `ctx[+0x290]`, the back-attack
   code. A back attack skips the party's first command band
   ([battle-round-loop.md](../../subsystems/battle-round-loop.md)), so Lapis acts before any input.
   In the forced fight `ctx[+0x290]` reads `1` at battle start and the first action belongs to
   seat 3.

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

   The body calls no damage wrapper. In the capture the lone party member's `+0x150` / `+0x152`
   read 27 / 27 before the cast and 0 / 0 at this arm, and Lapis's own MP drops from 4000 to 3745
   when the cast starts.

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
   `0x801F833C`. Captured with an injected cast in the forced fight: the test at `0x801F87DC` runs
   with `ctx[+0x287] = 0`, the store at `0x801F8814` runs once on seat 3 while its HP reads 64800,
   and `0x801F8438` is never reached.

Nighto (PROT entry 0907, `FUN_801F69E8`) also reaches its kill on this fight, as a roll rather than
a certainty. Its resist test is skipped when the flag is clear (`0x801F6BF0`), the resist roll is
`rand % (0x13 - magic_level) < 9` (`0x801F6C28..0x801F6CAC`), and the kill fork is `rand % 8 == 0`
(`0x801F6B50..0x801F6B7C`, store at `0x801F7E58`). At magic level 9 that is 0.9 x 1/8, about 11%
per cast. Lapis's record carries the resist byte `+0x20 = 1`, so a flagged fight would have forced
the resist.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x80051FC0..0x80051FE0` | `SCUS_942.54`, `FUN_80051D84` | Formation id `0xA7` forces `ctx[+0x290] = 1` (back attack) | disassembly, capture |
| `0x801CF1CC`, word at `0x801CF458` | PROT entry 0898 | AI formation jump table; id `0xA7` arm is `0x801EBC38` | disc data |
| `0x801EBC38..0x801EBCB4` | PROT entry 0898, `FUN_801E9FD4` | Lapis arm: count party `+0x150`, require own MP `>= 0xFF`, queue `0xB5` | disassembly, capture |
| `0x801F6A04` | PROT entry 0943 | Lapis Wave body, five arms, head table at `0x801F69D8` | disassembly |
| `0x801F6CFC`, `0x801F6D08`, `0x801F6D1C` | PROT entry 0943 | Stores to `+0x178`, `+0x150`, `+0x152` per party seat | disassembly, capture |
| `0x801E7C78..0x801E7C80` | PROT entry 0898, `FUN_801E791C` | Escape write-back of current MP to record `+0x10A` | disassembly |
| `0x80084708 + n * 0x414` | RAM | Character record; `+0x108` max MP, `+0x10A` current MP | repo doc |
| `0x801F87D4..0x801F8820` | PROT entry 0931, `FUN_801F6A58` | Jedo: flag clear sets every living enemy's HP to 0 | disassembly, capture |
| `0x801F833C`, `0x801F8438` | PROT entry 0931 | Jedo: flag set takes the rolled-damage path | disassembly |
| `0x801F6BF0`, `0x801F6C28`, `0x801F6B50`, `0x801F7E58` | PROT entry 0907, `FUN_801F69E8` | Nighto resist gate, resist roll, kill roll, kill store | disassembly |
| Spell record `0xB5` | `SCUS_942.54` spell table | Lapis Wave, cost 255, targets the whole party | disc data |
| Monster record 167, `+0x20` | PROT entry 0867 | Resist byte is `1` | disc data |

### Scope

- The escape is not required by the code. Any party that enters with every member at zero current
  MP is never waved. Captured on a poked state: with the lone party member's current MP written
  to 0 before Lapis's first pick and the maximum left at 27, Lapis opens with a physical attack,
  attacks physically again on its next turn, keeps its 4000 MP, and the maximum stays 27. Those
  two turns are all the capture covers. The community route is escape and return, which works
  because the wave itself produces the zero.
- One member with a single point of MP re-arms the wave for the whole party. The count has no
  liveness test, so a fallen member's MP counts.
- Lapis needs 255 MP of its own for each cast.
- No other monster arm in the AI pick uses this "who has MP" count.
- That MP items do nothing at zero maximum MP is a community report consistent with the item
  routine's use of `+0x152` ([battle-action-exit-gates.md](../../subsystems/battle-action-exit-gates.md));
  the clamp itself is not re-read here.

### Verifying it

Observed in the forced Lapis fight (forced formation, Vahn alone with 27 MP; see
[capture pedigree](#capture-pedigree)):

- Lapis acts first, with no command band before it, and the action is Lapis Wave: the cast loads
  PROT entry 0943 and costs Lapis 255 MP.
- The party actor's `+0x150` and `+0x152` both go from 27 to 0 in the wave's third arm. On its
  next turn, with the party at 0 / 0, Lapis attacks physically.
- With `+0x150` poked to 0 and `+0x152` left at 27, no wave is cast. The maximum being non-zero
  when the gate runs is what separates the two fields.
- Jedo, injected: `ctx[+0x287]` reads 0 at `0x801F87DC`, the store at `0x801F8814` zeroes Lapis's
  64800 HP, and the damage roll at `0x801F8438` does not execute.

The run that shows the wave also carries two unrelated one-byte writes to Vahn's character record
(a summon level stash and his Ra-Seru item byte), made for a different question.

Still to check in a played game:

- After a real escape, read the character record at `0x80084708 + n * 0x414`: `+0x108` unchanged,
  `+0x10A` zero.
- On the return visit, break at `0x801EBC68`: `+0x150` reads 0, `+0x152` the real maximum, and
  the branch at `0x801EBC90` is taken.

### Open questions

- Whether the gate was meant to read maximum MP.
- Whether the restored caster reliably acts before Lapis (speed 451) was not simulated.
- The route as a whole (wave, escape, return, MP item, Jedo) has not been played through in one
  capture; the escape write-back and the MP item are disassembly only.

## Lapis can be escaped from

**Status:** explained

**Reported as:** the party can run from Lapis, unlike other bosses (community report).

A fight is closed to escape only when the formation row that launched it has a non-zero first
byte. Nothing else marks a battle or a monster as a boss for the escape roll: not the scripted
battle opcode that starts the fight, and not the monster record.

Lapis's row has zero in that byte. The fight is launched by a scripted opcode like any boss fight,
yet the battle code treats it as an ordinary encounter, so Run, the escape accessories and the
Spikefish Flute all work. A fight started from that row's exact bytes reads the flag as zero from
start to finish; the Run command itself was not driven in that capture.

The same byte feeds every other boss protection, so the Lapis fight loses those too. That is what
lets the instant-kill summons of the [previous entry](#lapis-wave-checks-current-mp) land. It also
means Lapis does not get the boss stat boost: the fight installs ATK 680, upper defence 1705,
lower defence 1726 and INT 1248, not the 850 / 1950 / 1974 / 1123 that stat lists print.

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
   `0x80051430..0x80051444`, before it loads any monster; that store is the only writer of
   `+0x287`. The same routine then clears bit `0x80` of `0x8007BD60` (`0x80051A14`), so the flags
   byte reads clear during every fight and only `ctx[+0x287]` tells a flagged fight from an
   unflagged one.

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
   `0xA7`. Monster id 167 appears in no other formation row and in no random region.

5. **The stat profile.** The monster loader's boost `FUN_80054CB0` in `SCUS_942.54` picks one of
   two blocks on the same flag (`s4` is the monster record):

   ```text
   80055234  lbu  v0,0x287(v0)
   8005523c  beq  v0,zero,0x800552ac ; flag clear: second block
   80055248  lhu  v0,0x12(s4)        ; flagged: ATK += ATK >> 2
   80055264  lhu  v1,0x14(s4)        ;          UDF += UDF
   80055280  lhu  v1,0x16(s4)        ;          LDF += LDF
   8005529c  lhu  v0,0x18(s4)        ;          INT += INT >> 3
   800552ac  lhu  v1,0x14(s4)        ; clear:   UDF += (UDF >> 1) + (UDF >> 2)
   800552d0  lhu  v1,0x16(s4)        ;          LDF += (LDF >> 1) + (LDF >> 2)
   800552f8  lhu  v0,0x18(s4)        ;          INT += INT >> 2
   ```

   The clear block has no ATK write. The two shifts truncate separately, so the result is not
   `x * 7 / 4` for every value: Lapis's 975 becomes `975 + 487 + 243 = 1705`.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801DA5F8..0x801DA61C` | PROT entry 0897, `FUN_801DA51C` | Row byte 0 non-zero raises bit `0x80` of `0x8007BD60` | disassembly |
| `0x80051430..0x80051444` | `SCUS_942.54`, `FUN_800513F0` | `ctx[+0x287] = (flags >> 5) & 4` | disassembly, capture |
| `0x80051A14` | `SCUS_942.54`, `FUN_800513F0` | Bit `0x80` of `0x8007BD60` cleared again after the latch | disassembly |
| `0x80055234..0x80055308` | `SCUS_942.54`, `FUN_80054CB0` | Stat boost: flagged block, or the clear block at `0x800552AC` | disassembly, capture |
| `0x801E7AFC..0x801E7B24` | PROT entry 0898, `FUN_801E791C` | Escape roll: flag set forces the failure arm | disassembly |
| MAN offset `0x3840` | PROT entry 0383 (`deene`) | `3E FF 0B`, scripted battle on formation row 11 | disc data |
| MAN offset `0x38C3` | PROT entry 0383 (`deene`) | Row 11: `00 00 00 01 A7 00 00 00` | disc data |
| Monster record 167 | PROT entry 0867 | Lapis: ATK 680, UDF 975, LDF 987, INT 999 before the boost | disc data |

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
every Caruban, the Rikuroa Golem, Dohati, every Berserker, the Kemaro pair, Van Saryu, Xain, the
late Tetsu, both Gaza fights, the three Delilas siblings, Koru, Rogue, the Puera fights, Zora, Jette
and both Cort forms. The monster ids
that appear only in unflagged rows and in no random region are 79 (Tetsu), 125 (Gold Face) and 167
(Lapis).

What the clear flag changes for Lapis, consumer by consumer:

| Consumer of `ctx[+0x287]` | Effect on the Lapis fight | Evidence |
|---|---|---|
| Escape roll, `0x801E7B14` | The party can flee | disassembly |
| Jedo kill arm, `0x801F87D4` | Certain kill | disassembly, capture |
| Nighto resist gate, `0x801F6BF0` | The kill and confuse rolls run despite record `+0x20 = 1` | disassembly |
| Aluru resist gate, `0x801F6D44` | Same as Nighto: the kill roll runs | disassembly |
| Zenoir shatter gate, `0x801F81F0` | A Lapis killed by Zenoir shatters like an ordinary monster and fades (Lapis is not lone-scripted, so no grey stamp); no damage change | disassembly |
| Seru side-effect stager `FUN_801F3D3C` | Stat debuffs land on every hit, with no suppression roll | repo doc |
| Stat boost `FUN_80054CB0` | The random-encounter profile is installed instead of the boss profile | disassembly, capture |
| Spikefish Flute, PROT entry 0925 | The flee arm runs | repo doc |

The stat-boost row, in numbers:

| | ATK | UDF | LDF | INT |
|---|---|---|---|---|
| Monster record 167 | 680 | 975 | 987 | 999 |
| Installed in the Lapis fight (flag clear) | 680 | 1705 | 1726 | 1248 |
| What a flagged fight would install | 850 | 1950 | 1974 | 1123 |

The middle row is read from Lapis's actor in the forced fight (`+0x158`, `+0x15C`, `+0x160`,
`+0x168`), which also reads HP 64800 and MP 4000. The same rule shows on unmodified save states of
another unflagged scripted fight, Tetsu's opening spar: the flag
reads 0 and the record's 24 / 16 / 12 / 99 is installed as 24 / 28 / 21 / 123, while flagged Gaza
and evolved Cort states read 4 and carry the flagged block.

Published stat lists, this project's curated bestiary included, show 850 / 1950 / 1974 / 1123 for
Lapis. That is the flagged block applied uniformly to every enemy, a derived column and not a
reading of this fight ([battle.md](../../subsystems/battle.md)). The fight itself gives Lapis less
attack, less defence and more INT than those lists say.

### Verifying it

Observed in a fight forced from the disc row's bytes ([capture pedigree](#capture-pedigree)):

- `ctx[+0x287]` reads 0 for the whole fight; a flagged boss reads 4. `0x8007BD60` is no use for
  this check, since its bit `0x80` is cleared during setup in every fight.
- Lapis's actor at `*(0x801C9370 + 0xC)` reads 680 / 1705 / 1726 / 1248 at `+0x158` / `+0x15C` /
  `+0x160` / `+0x168`.

Still to check: choose Run in the Lapis fight and break at `0x801E7B14`. The prediction is
`ctx[+0x287] = 0` and, on a winning roll, the success arm at `0x801E7B40`.

### Open questions

- The escape itself was not driven in a capture; that it succeeds rests on the disassembly of the
  roll and on the flag reading 0.
- The fight was started by forcing the formation row, not by reaching Lapis on Mt. Dhini.
  Everything keyed on the row and on monster 167 is retail; the party and the stage are not.

## Mushura and Theeder against Mystic Shield

**Status:** explained

**Reported as:** Mushura and Theeder do excessive damage to Cort while his Mystic Shield is up;
Crazy Driver racks up hits on the shield, the only place in the game where that happens
(community report).

The two summons are not the same case. Mushura's Crazy Driver really does deal excess damage to
shielded Cort: measured, 28 run-over hits where an unshielded cast lands 4, and 2252 damage against
784, about 2.9 times an unshielded cast. Theeder only escapes the shield: its beam lands 2 hits
where an unshielded cast lands 1, each halved, for 284 damage against 282. The shield is cancelled
out, not exceeded.

Element, overflow and a special formula play no part. Every summon uses the same damage roll, and
the shield halves every hit on Cort, these two included.

Mushura and Theeder are sweep attacks. On each tick they test every enemy for contact with the
moving body or beam and hit whatever they touch. The only thing that stops them hitting the same
enemy again is a test that the enemy is on its idle animation: a hit stages a flinch or a
knockdown, and that reaction is the lockout.

Mystic Shield zeroes Cort's four reaction-animation ids so that he does not flinch behind it. Clip
id zero is the idle clip. A hit therefore stages "idle", the lockout never engages, and the sweep
hits Cort again on every tick it overlaps him. Mushura rolls back and forth across him and
overlaps him for many ticks; Theeder's beam crosses him exactly twice.

Mule, a Ra-Seru summon with the same sweep shape, carries an explicit test in the same place that
skips its hit while the shield is up. Mushura and Theeder have no such test. Three modules share
the pattern and one of them is guarded.

The numbers are from a forced Cort fight with injected casts and a one-character party
([capture pedigree](#capture-pedigree)), one cast each. Mushura's count is a count of contact
ticks, so it moves with the frame rate; Theeder's does not at ordinary frame rates.

### Mechanism

1. **The shield.** Cort's turn logic at `0x801EB988` in PROT entry 0898 queues action `0xAC` on his
   first action, while `ctx[+0x28A]` is zero. The `0xAC` body in `FUN_801F7240`, PROT entry 0940,
   spawns the shield effect, stores its handle in `0x8007BD84`, and blanks the reaction map of the
   first enemy seat.

   ```text
   801f7678  sw  v0,-0x427c(v1)      ; 0x8007BD84 = effect handle
   801f7690  sb  zero,0x1ef(v0)
   801f769c  sb  zero,0x1f0(v0)
   801f76a8  sb  zero,0x1f1(v0)
   801f76b4  sb  zero,0x1f2(v0)
   ```

2. **The halving.** The damage finisher `FUN_801DDB30` in PROT entry 0898, for an enemy defender
   with the handle non-null, rewrites the attack value as `def + (atk - def) / 2`
   (`0x801DDB98..0x801DDBC8`). It applies to every caller. Measured at magic level 1: Theeder rolls
   282 on unshielded Cort and 142 on shielded Cort; Mushura rolls 610 to 639 and 304 to 321.

3. **The break.** The per-frame actor pass `FUN_8004CE2C` in `SCUS_942.54`, with first formation id
   `0xB4`, restores the map to `3, 2, 4, 5` and clears the handle once the seat's HP is at or below
   half its maximum (`0x8004D534..0x8004D658`). Cort has 50000 HP, so the shield covers the first
   25000. With Cort's HP poked to 25000 the pass breaks the shield within one frame.

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

   The clip load at `0x801F7870` is reached only on a tick whose range metric reads contact, so
   each execution of it is one contact tick. An opening strike before the run-over is a separate
   hit, also quartered.

5. **Theeder's lockout.** `FUN_801F69D8` in PROT entry 0904, the sweep arm, every tick and for
   every enemy seat:

   ```text
   801f7ce0  lhu  v1,0x14c(a0)       ; alive
   801f7cf0  lbu  v0,0x1d9(a0)       ; playing clip
   801f7cf8  bne  v0,zero,<next>     ; the same lockout
   801f7d0c  addiu v0,v0,-0x30       ; beam bearing vs victim bearing
   801f7d10  sltiu v0,v0,0xfb1       ; inside +-0x30
   801f7d38  jal  0x801dd0ac         ; damage roll, power 0x11, not quartered
   801f7df0  lbu  v0,0x1f2(v1)       ; get-up id non-zero: stage the knockdown id +0x1F1
   801f7e0c  lbu  v0,0x1ef(v1)       ; otherwise the flinch id (0x1f0 if zero)
   801f7e24  sb   v0,0x1da(v1)       ; stage it
   ```

   A living victim with a get-up clip is knocked down (`0x801F7DF0..0x801F7E08`); the flinch id is
   used only when `+0x1F2` is zero. Unshielded Cort, whose map is `3, 2, 4, 5`, is staged clip 4.

6. **Theeder's sweep crosses the victim twice.** The sweep arm adds `(frame delta * speed scalar)
   << 3` to the phase word `ctx[+0x6D8]` each tick (`0x801F7AC8..0x801F7AF0`) and ends when the
   phase reaches `0x1000` (`0x801F7EE8..0x801F7EF4`). The beam's bearing is the summon's facing
   plus `sin[phase & 0xFFF] >> 4`, a swing of `0x100` either side
   ([cast-module.md](../../subsystems/cast-module.md)), and the summon is seated facing its target,
   so the targeted victim is on the centre line. It is inside the `0x30` cone only where the sine
   is near zero: at phase `0x800` and again at `0x1000`. At the measured delta 4 and scalar 8 the
   step is 256, the arm runs 16 ticks over 64 frames, and the two crossings are ticks 8 and 16.

7. **Why the lockout never engages.** With the map blanked both modules stage clip `0`, so the
   playing clip stays `0` and the test passes again at the next contact. In a capture of each cast
   on shielded Cort, `+0x1D9` reads `0` at every execution of the lockout load: 16 of 16 for
   Theeder, 28 of 28 for Mushura. On Cort after the break it reads `4` for the rest of Theeder's
   sweep and `3` for the rest of each Mushura pass.

8. **Mule's guard.** `FUN_801F69FC` in PROT entry 0929 has the same lockout at `0x801F7E08`, then
   loads `0x8007BD84` at `0x801F7E34` and branches past the roll at `0x801F7E4C` when it is
   non-null. This is disassembly only. A Mule cast on shielded Cort made no damage call, but both
   passes that reached the lockout load left at the position test `0x801F7E2C`, in front of the
   shield test, so the guard itself was never exercised.

Across all 64 cast and summon images, a clip-id test that sits directly in front of a damage-roll
call occurs three times: Theeder, Mushura and Mule. Every other read of `+0x1D9` in those images is
a wait for a clip to end. The other summons hit on a fixed step of their own sequence, once per
scripted beat, so a missing flinch changes nothing for them.

### Measured hits and damage

One cast each on Cort in the forced fight, magic level 1, damage rolls read at the damage call's
return ([capture pedigree](#capture-pedigree): forced formation, injected cast, poked HP and
magic level).

| Cast | Cort's state | Hits | Roll per hit | HP lost |
|---|---|---|---|---|
| Theeder | shield broken by the game at 25000 HP | 1 | 282 | 282 |
| Theeder | shield up | 2 | 142, 142 | 284 |
| Theeder | shield up, reaction map written back to `03 02 04 05` | 1 | 141 | 141 |
| Mushura | shield broken by the game at 25000 HP | 4 run-over + 1 opening | 610 to 639, quartered | 784 |
| Mushura | shield up | 28 run-over + 1 opening | 304 to 321, quartered | 2252 |
| Mushura | shield up, reaction map written back to `03 02 04 05` | 4 run-over + 1 opening | 304 to 320, quartered | 390 |

- **Theeder.** Unshielded Cort is hit at the first crossing and is on his knockdown clip at the
  second: one hit. Shielded Cort is hit at both crossings, each roll halved: 284 against 282.
  That is twice what the shield's rule allows (141) and level with an unshielded cast.
- **Mushura.** Under the shield the run-over touches Cort on 28 ticks, in five contact windows of
  4, 2, 9, 8 and 5 ticks, and hits on every one. Without the shield it touches him on 38 ticks and
  hits 4 times, once per window. The cast takes 2252 HP against 784: 2.9 times an unshielded cast
  and 5.8 times what the shield's rule allows (390).
- **The control rows** settle the cause. With the shield handle still set and only the reaction
  map restored by hand, both casts return to their unshielded hit counts and keep the halved
  rolls: the shield then works as designed. The extra hits come from the blanked map alone. The
  same holds on another victim: an ordinary Gobu Gobu with its HP raised and its reaction map
  poked to zero, and no shield anywhere, takes 2 Theeder hits where it normally takes 1 and 12 to
  24 Mushura hits (until it dies) where it normally takes 4.

How far the counts carry:

- Theeder's two does not depend on the victim or the formation, only on the phase step. Stepping
  the same arithmetic through other constant frame deltas gives two cone hits at delta 2, 3 and 4,
  one at delta 5 and six at delta 1. Only delta 4 is observed; the rest is arithmetic on the arm's
  constants.
- Mushura's 28 is a property of that run. The cast ran at a frame delta of 3 to 4 with one
  character on screen. A heavier frame gives a larger delta and fewer contact ticks, a lighter one
  more, so the 28 : 4 ratio is a measurement and not a constant.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801EB988..0x801EB9B4` | PROT entry 0898, `FUN_801E9FD4` | Cort's arm queues `0xAC` while `ctx[+0x28A]` is zero | disassembly, capture |
| `0x801F7678` | PROT entry 0940, `FUN_801F7240` | Shield handle stored to `0x8007BD84` | disassembly, capture |
| `0x801F7690..0x801F76B4` | PROT entry 0940 | Seat-3 reaction map `+0x1EF..+0x1F2` zeroed | disassembly, capture |
| `0x801DDB98..0x801DDBC8` | PROT entry 0898, `FUN_801DDB30` | Halve the hit on an enemy while the handle is non-null | disassembly, capture |
| `0x8004D534..0x8004D658` | `SCUS_942.54`, `FUN_8004CE2C` | Break at half HP: map restored to `3, 2, 4, 5`, handle cleared | disassembly, capture |
| `0x801F7870` / `0x801F789C` | PROT entry 0915, `FUN_801F69D8` | Mushura: lockout test / damage roll | disassembly, capture |
| `0x801F78F0` | PROT entry 0915 | Mushura: roll quartered, floor 1 | disassembly, capture |
| `0x801F79C4..0x801F79E8` | PROT entry 0915 | Mushura: flinch staged from `+0x1EF` / `+0x1F0` | disassembly |
| `0x801F7CF0` / `0x801F7D38` | PROT entry 0904, `FUN_801F69D8` | Theeder: lockout test / damage roll | disassembly, capture |
| `0x801F7D0C..0x801F7D14` | PROT entry 0904 | Theeder: cone test, `+-0x30` | disassembly |
| `0x801F7AC8..0x801F7AF0` | PROT entry 0904 | Theeder: phase step `(delta * scalar) << 3` per tick | disassembly, capture |
| `0x801F7EE8..0x801F7EF4` | PROT entry 0904 | Theeder: sweep ends at phase `0x1000` | disassembly, capture |
| `0x801F7DF0..0x801F7E08` | PROT entry 0904 | Theeder: knockdown `+0x1F1` staged when `+0x1F2` is non-zero | disassembly, capture |
| `0x801F7E0C..0x801F7E24` | PROT entry 0904 | Theeder: flinch staged from `+0x1EF` / `+0x1F0` otherwise | disassembly |
| `0x801F7E08` / `0x801F7E34` / `0x801F7E4C` | PROT entry 0929, `FUN_801F69FC` | Mule: lockout test / shield test / damage roll | disassembly |
| `0x801F7E2C` | PROT entry 0929 | Mule: position test in front of the shield test | disassembly, capture |
| Monster record 180 | PROT entry 0867 | Cort: HP 50000, element dark, size byte `0x16` | disc data |

### Scope

- Only Cort id `0xB4` (scene `chitei2`, formation row 13) while `0x8007BD84` is non-null: his first
  25000 HP. The Mystic Shield body is that word's only non-zero writer.
- The cause is an all-zero reaction map, so a monster whose record supplies no reaction clips at
  all would be hit the same way by these two summons. That set is not enumerated here.
- Each extra hit runs the finisher's tail as well: summon spell experience, the Seru side-effect
  shave and the damage popup accrue per hit, 2 times for Theeder and 28 for the measured Mushura
  cast.
- The shield can break in the middle of a Mushura cast. Once Cort is at or below half HP the
  per-frame pass restores the map, one more hit lands on the clip that was already staged, and the
  rest of the cast is one hit per contact window. This is seen in a single run from the first
  batch described under [capture pedigree](#capture-pedigree) (the one whose HP hold also wrote
  field-mode memory before the battle), with the magic level left unseeded so that each hit took
  2499: Cort crossed 25000 on the tenth run-over hit and the cast ended with 13.
- Mule is the opposite case by its code: under the shield its sweep is written not to hit at all.

### Verifying it

The three checks, as run on the forced Cort fight:

- Execute breakpoints at `0x801F789C` (Mushura) and `0x801F7D38` (Theeder) count 28 and 2 with
  the shield up, 4 and 1 once the game has broken it.
- `+0x1D9` of the actor at `*(0x801C9370 + 0xC)` reads `0` throughout under the shield; without
  it, `3` after a Mushura hit and `4` after a Theeder hit.
- Writing `03 02 04 05` to that actor's `+0x1EF..+0x1F2` with the shield up returns the counts to
  4 and 1 while the rolls stay halved. The lockout is the cause and the shield handle is not.

Still to check: the same breakpoints in the real `chitei2` fight, with three party members on
screen, give Mushura's count at the real frame delta.

### Open questions

- Mushura's hit count in the real fight. The measured 28 comes from a one-character party on
  another stage, and the count follows the frame delta.
- A victim that is only flinched (no get-up clip) and is back on idle within 32 frames is hit
  again at Theeder's second crossing; whether that is intended is unknown. Cort, once unshielded,
  is knocked down and is not.
- Which monster records, if any, carry an empty reaction map.
- Mule's shield guard has not been seen to execute.
