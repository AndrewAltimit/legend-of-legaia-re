# Status rolls, jewels and boss immunities

Six battle oversights that share one theme: a rule the player assumes is general turns out to be
a special case in the code. Each section states the cause in plain words, then the instructions
and disc bytes that carry it.

Most of the page is read statically from the USA disc (`SCUS_942.54`). Two things go further. The
Tough Love section is also measured in a running game, on a poked state that the section
describes. The boss-debuff section also reads the Japanese (`SCPS_100.59`) and French
(`SCES_019.44`) builds. The Zenoir shatter section is captured in a running game on forced
formations with an injected cast. The jewel, Berserker and Evil Fly sections are static analysis
only; each says which parts are proven from the code and which are predicted from it.

Address conventions used on this page:

- `SCUS_942.54`: file offset = `VA - 0x80010000 + 0x800`.
- PROT entry 0898 (the battle-action overlay): load base `0x801CE818`, file offset = `VA - 0x801CE818`.
- PROT entries 0903..0966 (the slot-B cast and summon modules): load base `0x801F69D8`.
- `ctx` is the battle context `*(0x8007BD24)`. The live actor table is `0x801C9370[slot]` (slots
  0..2 party, 3..6 monsters, 7 the summon body). The monster **record** pointer table is
  `0x801C9348[slot - 3]`. `0x8007BD10[slot]` holds the party character ids (1 Vahn, 2 Noa, 3 Gala)
  and `0x8007BD0C[i]` the formation's monster ids. The RNG is `FUN_80056798`.
- A **battle frame** is one pass of the battle logic. The game spends 1 to 4 vertical blanks
  (60ths of a second) on each, and stores that number, the **frame step**, at `0x1F800393`.

Related pages: [battle.md](../../subsystems/battle.md),
[battle-formulas.md](../../subsystems/battle-formulas.md),
[battle-round-loop.md](../../subsystems/battle-round-loop.md),
[battle-actor-rendering.md](../../subsystems/battle-actor-rendering.md),
[cast-module.md](../../subsystems/cast-module.md),
[art-data.md](../../formats/art-data.md), [spell-table.md](../../formats/spell-table.md),
[accessory-passive-table.md](../../formats/accessory-passive-table.md).

## Tough Love and Love You roll their status every frame

**Status:** explained
**Reported as:** Tough Love and Love You attempt to apply the status they cause seven times during
the animation, which makes it nearly guaranteed (community report).

Noa's Tough Love and her Super Art Love You do not roll for their status when they hit. The normal
"status on hit" slot of an art is empty on both. Their effect is written by hand into a routine
that runs once per battle frame.

From the frame in which the first blow lands until the animation runs out, 20 keyframes later,
that routine flips a coin every battle frame and puts the confusion status on Noa's target
whenever it comes up heads. Nothing records that the coin has already come up heads and nothing
checks a resistance.

In ordinary play that is **20 flips for Tough Love** and **80 for Love You**:

| Art | Rolls | Rolls that decide the outcome | Chance the status is on at the end |
|---|---|---|---|
| Tough Love | 20 | 20 | 99.9999% (a miss is 1 in 1,048,576) |
| Love You | 80 | 68 | all but certain |

Each roll is exactly one in two, so after `N` rolls the chance is `1 - 0.5^N`. The two roll columns
differ only for Love You, and for one reason: every hit wipes the status again, so the rolls made
before its fourth and last hit cannot decide anything. Tough Love has one hit and the window opens
on it, so all of its rolls count.

Those figures hold while the battle runs at its usual pace of one battle frame every two video
frames. The roll is made once per battle frame, and the animation advances by a fixed amount of
*time* per battle frame, so when the game drops frames the same animation is covered in fewer
battle frames and fewer rolls. One battle frame every three video frames gives 14 rolls for Tough
Love and 54 (46 deciding) for Love You; the floor, one in four, gives 10 and 40 (34 deciding).
Even the floor leaves Tough Love at 99.90%.

So the report is right in kind - it is a repeated roll, and it is close to unmissable - but the
roll is tied to frames, not to hits, and the count is never seven. Seven rolls would be 99.22%;
10 is the fewest either art can make. Another community figure, "about 13", is the Tough Love
count on a battle that is dropping to one frame in three.

The status is the confusion group: the monster keeps its chosen action but aims it at its own
side. It has no timer; it ends when the monster next takes damage. That is why it sticks only when
the art is the last hit the monster takes: a later swing in the same chain removes it.

The effect is switched off in every scripted fight, which covers the bosses, and against one
optional boss by monster id.

### Mechanism

1. **The art records carry no status.** Noa's art bank sits in the decoded `record[0]` of her
   battle file (PROT entry 0864), `0xD0` bytes per record. Tough Love is bank record 19 (staged
   animation id `0x23`): one strike at keyframe `0x19`, clip key `0x29`, playback rate 4. Love You
   is record 33 (id `0x31`): four strikes at keyframes `0x19 0x1A 0x1B 0x1C`, clip key `0x2D`,
   rate 2. Both name the same 44-keyframe stream. The status selector at entry `+0x7A` is `0` on
   both, so the on-hit ladder in `FUN_801EC3E4` (`lbu v0,0x7a(t4)` at `0x801EE3D4`) does nothing
   for them. Both entries carry `+0x76 = 1`, which disables the early cut a queued follow-up would
   otherwise make, so each always plays all 44 keyframes.
2. **The applier is the per-frame actor pass.** `FUN_8004CE2C` in `SCUS_942.54` is the last call
   of `FUN_8004DA00` (`jal` at `0x8004DC4C`), the per-frame battle voice-seek selector that a
   resident system actor ticks; every path through that tick reaches the call, so the pass is
   entered once per battle frame. It loads the acting actor from `ctx[+0x13]` and the target from
   that actor's `+0x1DD`, and for character id 2 runs:

   ```text
   8004d0a4  lw   v0,0x22c(s0)         ; animation node
   8004d0ac  lw   v0,0x4c(v0)          ; the clip record being played
   8004d0b4  lbu  v1,0x77(v0)          ; clip key
   8004d0b8  li   v0,0x29  / beq       ; Tough Love
   8004d0c0  li   v0,0x2d  / bne out   ; Love You
   8004d0cc  lbu  v0,0x1f4(s0) / beq v0,zero,out    ; a strike has fired
   8004d0dc  lbu  v0,0x287(a2) / bne v0,zero,out    ; not a scripted fight
   8004d0ec  jal  0x80056798                        ; rand()
   8004d0f4  andi v0,v0,0x1 / bne v0,zero,out       ; one chance in two
   8004d100  lbu  v1,0x9f4(gp) ; li v0,0xa7 ; beq out   ; first monster is not id 167
   8004d110  lhu  v0,0x16e(s1)
   8004d118  ori  v0,v0,0x380
   8004d120  sh   v0,0x16e(s1)
   ```

   There is no latch and no edge test: every pass inside the window reaches `0x8004D0F4`.
3. **Nothing ends the window but the clip.** `actor[+0x1F4]` is the per-clip strike counter.
   `FUN_801EC3E4` raises it each time a strike event fires (`0x801EECDC..0x801EECE8`), and the
   animation commit `FUN_8004AD80` zeroes it (`0x8004B064`) when the next clip is installed, which
   for these two arts happens only when the cursor reaches the end of the stream
   (`0x80047A48..0x80047A60` in `FUN_80047430`). The animation driver runs before the actor pass
   within a frame, so the frame of the first strike already rolls and the frame that ends the clip
   does not.
4. **Every hit wipes the status, and the roll puts it back.** Every damaging physical hit clears
   the "wakes when hit" status group on the defender: `andi v0,a1,0xf07f` / `sh v0,0x16e(a2)` at
   `0x801EDA5C`. The bits `0x380` are inside that group. In the running game Love You's second,
   third and fourth hits each take the word from `0x380` back to 0, and a roll in the same frame
   or a later one sets it again. The same strip removes the status when Tough Love is not the last
   hit of the chain. That a status applied by one hit would not survive the next is the likely
   reason the designers made it a per-frame roll; the intent is inference.
5. **What the bits are.** `0x0080 | 0x0100 | 0x0200` are always set and tested as one group, the
   "action handed to the AI" status. On a monster that is confusion: when its action is seeded the
   target is re-rolled onto the monster side, and the pick can be the monster itself
   ([battle-round-loop.md](../../subsystems/battle-round-loop.md)). No code counts the group down.
   Its only clears in `SCUS_942.54` and PROT 0898 are the two damage strips (`0x801EDA5C` for
   physical hits and `0x801DE2EC` in the magic finisher `FUN_801DDB30`), a KO and the end of the
   battle.

### The roll count

Two different numbers enter the count, and they are easy to mix up.

- The **playback rate** is a byte of the art record, entry `+0x78`: `4` on Tough Love, `2` on Love
  You. It is disc data, fixed per art, and nothing in play changes it.
- The **frame step** is the scratchpad byte `0x1F800393`: how many video frames (60ths of a second)
  the game is spending on each battle frame. It belongs to no art. The frame driver rebuilds it
  every frame from how long the last sixteen frames took to draw, so it measures how hard the
  console is working: `2` in a normal battle, `3` while the screen is busy.

The rate is why Love You makes four times as many rolls as Tough Love. The frame step is why
neither art has one single count.

The window is a stretch of the clip cursor, which counts sixteenths of a keyframe.

- It opens in the frame the cursor first satisfies `(cursor >> 4) + 1 >= 0x19`, the first strike
  keyframe of both arts: cursor 384, keyframe 24.
- It closes in the frame the cursor reaches the stream length, `44 * 16 = 704`.
- The cut two keyframes after the last strike, which a queued follow-up action triggers for most
  clips, does not apply: it is skipped for an entry with `+0x76` set (`0x80047940..0x80047948`).
  With two more swings queued behind it Tough Love still plays to 704.

The cursor advances each frame by `(frame_step * speed * rate) >> 1` (`0x80047730..0x80047764`;
speed `actor[+0x21D]`, rate entry `+0x78`). The art commit writes speed 2 to every actor when
`ctx[+0x243]` is set and 4 otherwise (`0x8004BB64..0x8004BBA4`). Tough Love runs at 4; Love You,
staged as a Super Art, at 2. Tough Love therefore advances `8 * step` per frame and Love You
`2 * step`, and the number of rolls is the number of frames the cursor spends in `[384, 704)`:
`320 / (8 * step)` and `320 / (2 * step)`, rounded up.

| Art | Step 2 | Step 3 | Step 4 |
|---|---|---|---|
| Tough Love | 20 | 14 | 10 |
| Love You | 80 | 54 | 40 |
| Love You, at or after its last hit (cursor 432 on) | 68 | 46 | 34 |

At step 3 the Tough Love count is 13 instead of 14 when the cursor does not land on 384 exactly.

Which column a player gets depends on the frame step. The frame driver `FUN_80016B6C` keeps the
draw time of the last sixteen frames, in scanlines, and takes the slowest: under `0xF1` is step 1,
under `0x1FF` step 2, under `0x2D1` step 3, and 4 only when the newest frame alone ran past that
(`0x80017098..0x8001715C`). A per-mode floor is then applied, and the battle's floor is 1
([battle-stage-camera.md](../../subsystems/battle-stage-camera.md#the-frame-step)).

Battle save states from a timing-accurate emulator show what a console holds. Of 76 states that
are in a battle, 57 hold step 2, 18 hold step 3, one (a loading frame) holds 1 and none holds 4.
The step-3 states are summon close-ups, cast modules and other effect-heavy moments. Step 2, so 20
rolls for Tough Love and 80 for Love You, is therefore the normal case, and step 3 the case for a
busy screen. None of the 76 is taken inside either of these two animations, so which of the two
steps Love You's own effects produce is inferred, not captured.

Step 4 is what the capture emulator below runs at on its own. That is a property of its timing,
not of the game: it is why the measured table starts at 10 and 40, the counts a console would
rarely if ever show.

Nothing in the window happens seven times: one strike or four, 20 keyframes, and 10 or more
passes. The tempting reading - one roll per hit, or a fixed seven - fails on the first count.

### How it was measured

The counts come from a running game (PCSX-Redux, retail disc), with breakpoints on the pass entry
`0x8004CE2C`, on `0x8004D0F4` where the roll is read, on the status write, and on the strike
counter's bump and clears.

They are **captured on a poked state**, which is weaker than played input. The starting point is a
random encounter (three monsters, scripted-fight flag clear) in which Noa is on the first swing of
an auto-executing chain. The unconsumed tail of her action queue at `actor[+0x1DF]` is rewritten so
that the retail queue consumer stages Tough Love behind the ordinary art starter, or Love You
behind the Super Art starter. The art commit, the animation, the strikes, the damage, the roll and
the status write are then retail code running. The code under test was compared word for word with
the disc images first.

| Art | Frame step | Rolls | Rolls at or after the last hit | Status writes |
|---|---|---|---|---|
| Tough Love | 4 (the emulator's own pacing) | 10 | 10 | 7 |
| Tough Love | 3 (forced) | 14 | 14 | 6 |
| Tough Love | 2 (forced) | 20 | 20 | 8 |
| Tough Love, two swings queued after it | 4 | 10 | 10 | 3 |
| Love You | 4 (the emulator's own pacing) | 40 | 34 | 26 |
| Love You | 3 (forced) | 54 | 46 | 28 |

"Forced" means the frame-step byte `0x1F800393` was also overwritten each frame. Love You at step 2
(80 and 68) is not captured; it is the same formula the six captures fit.

The last column is not a property of the art. It is how many of that run's rolls happened to come
up heads, and each of those writes the same bits again. It is there to show the write really
happens more than once; another run of the same art gives a different number. In the first run the
ten rolls read `1 0 0 1 0 0 0 0 0 1` (0 applies): seven writes, the first on the second roll, and
the word then stays `0x0380` for the rest of the capture. Seven writes out of ten rolls is chance,
not a count the code makes. In the queued-swings run the status is on when the clip ends and Noa's
next swing clears it.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x8004CE2C` | `SCUS_942.54` | Per-frame battle actor pass; entered once per battle frame | disassembly, capture |
| `0x8004D0A4..0x8004D120` | `SCUS_942.54` (file `0x3D8A4`) | Noa's clip-key arm: gates, roll, write | disassembly, capture |
| `0x8004D0EC` | `SCUS_942.54` | The `rand()` call of the 1-in-2 roll | disassembly |
| `0x8004D0F4` | `SCUS_942.54` | Instruction after the call, where the roll is read; one hit per roll | disassembly, capture |
| `0x8004DC4C` | `SCUS_942.54` | `jal 0x8004CE2C`, the last call of `FUN_8004DA00` | disassembly |
| entry `+0x77` | PROT 0864 `record[0]`, art bank at decoded `+0x5A30`, records 19 and 33 | Clip keys `0x29` (Tough Love) and `0x2D` (Love You) | disc data |
| entry `+0x7A` | same records | Status selector, `0` on both arts | disc data |
| entry `+0x76` | same records | `1` on both: no early cut after the last strike | disc data |
| entry `+0x78` | same records | Playback rate: 4 (Tough Love), 2 (Love You). Fixed per art; not the frame step | disc data |
| `actor[+0x1F4]` | RAM | Per-clip strike counter; raised at `0x801EECE8`, zeroed at `0x8004B064` | disassembly, capture |
| `actor[+0x16E]` bits `0x0380` | RAM | AI-delegation (confusion) group the arm sets | disassembly |
| `ctx[+0x287]` | RAM | Scripted-fight flag; non-zero disables the arm | disassembly |
| `0x8007BD0C` | RAM | First formation monster id, compared with `0xA7` | disassembly |
| `0x801EDA5C` | PROT 0898 | A damaging physical hit clears status mask `0x0F80` on the defender | disassembly, capture |
| `0x801DE2EC` | PROT 0898 | The same clear in the magic damage finisher `FUN_801DDB30` | disassembly |
| `0x80047730..0x80047764` | `SCUS_942.54` | Clip cursor advance | disassembly, capture |
| `0x80047A48..0x80047A60` | `SCUS_942.54` | Clip-end test against the stream length | disassembly, capture |
| `0x80047940..0x80047948` | `SCUS_942.54` | Early-cut veto on entry `+0x76` | disassembly, capture |
| `0x8004BB64..0x8004BBA4` | `SCUS_942.54` | Art commit: speed 2 or 4 from `ctx[+0x243]` | disassembly, capture |
| `0x1F800393` | RAM (scratchpad) | Frame step, 1..4 | disassembly |
| `0x80017098..0x8001715C` | `SCUS_942.54` | Pacer that sets the frame step; frame-cost ring at `0x80084098` | disassembly |
| `0x80084098` | RAM | Draw time of the last sixteen frames, `s16[16]`, in scanlines; its maximum picks the step | disassembly, save states |

### Scope

- The arm never fires in a scripted fight (`ctx[+0x287] != 0`) or when the first monster of the
  formation is id 167 (Lapis).
- The status holds only until the target's next damage. Used mid-chain, or followed by another
  character's attack on the same monster, it is gone again.
- No other player art inflicts a status, single-roll or otherwise. Across the four party art banks
  (PROT entries 0863..0866) entry `+0x7A` is only ever `0`, `1` (Vahn's fire arts: Fiery Miyawaki,
  Beatdunk, Beatfire, Tornado Flame, Fire Tackle) or `2` (Gala's Thunder Punch records). Selectors
  `1` and `2` stamp an elemental tint on the target and set no status bit.
- The other clip-key arms of the same pass (Gala, Vahn, one monster key) write tints and pose
  freezes only; see [battle.md](../../subsystems/battle.md).
- A real per-hit status roll, for comparison, is what a monster's physical attack uses: one draw
  per strike event in `FUN_801EC3E4` (`rand & 7 == 0` for the two poisons at `0x801EE48C` /
  `0x801EE4D0`, `rand & 3 == 0` for Curse at `0x801EE690`).

### Verifying it

Count executions of `0x8004D0F4` while Noa uses either art in a random encounter: that is the
number of rolls, and `v0 & 1 == 0` marks the ones that apply. Read `0x1F800393` in the same frames
to see which column of the table the run belongs to. A write watch on the target's `+0x16E` shows
each Love You hit clearing `0x380` at `0x801EDA60` and a later roll setting it again.

### Open questions

- The frame step held while these two animations play, measured on a timing-accurate emulator or
  a console. Battle states in general hold 2, and 3 on a busy screen; no state inside either
  animation is on hand. It does not change the outcome: every possible value gives 10 or more.
- A played confirmation, with the art entered from the arts menu instead of a rewritten queue.
- Whether a strike that misses still opens the window. The counter is raised on the routine's
  common exit path, which suggests it does.
- Which action-state store raises `ctx[+0x243]` for a Super Art. The speed-2 case is measured; its
  setter is not named.

## Seven boss attacks ignore the defensive jewels

**Status:** explained
**Reported as:** Bloody Horns, Terio Punch, Megaton Press, Plasma Strike, Blazing Slash and Guilty
Cross ignore the damage reduction of the defensive jewels (community report).

The elemental jewels do not change the damage roll. Their halving is a single block at the top of
the routine that finishes every magic and special-attack hit, and that block is skipped when the
routine's fifth argument is non-zero.

Boss cinematic attacks reach the finisher through one of two small wrappers. One passes `0`, so
jewels apply. The other passes `1`, and it also builds the damage from the attacker's ATK against
the target's upper and lower defence instead of from INT. The reported attacks are exactly the
ones whose code calls the second wrapper - plus one more, Xain's **Bull Charge**, which shares its
code with Terio Punch.

This reads as intended design rather than a slip. The second wrapper is a physical-damage formula,
and an elemental guard has nothing to say about a physical hit. What the game never does is tell
the player which boss attacks belong to that class. The Rainbow Jewel's all-element guard is
skipped along with the single-element ones; defending still halves these attacks.

### Mechanism

1. **The jewel block.** `FUN_801DDB30(attacker, defender, *atk, *def, bypass)` in PROT entry 0898
   reads its fifth argument from the stack (`lw a3,0x40(sp)` at `0x801DDB74`). For a party
   defender:

   ```text
   801ddbd4  bne  a3,zero,0x801de094    ; bypass set: skip everything below
   801ddc0c  lw   v0,0x6c0(v0) ; andi 0x10       ; All Guard: damage x 3/4
   801ddca8  lw   v0,0x6bc(v0) ; and 0x20000000  ; Earth Guard vs an earth attacker: halve
   801ddd3c  lw   v0,0x6bc(v0) ; and 0x40000000  ; Water Guard vs a water attacker: halve
   ...                                           ; fire, wind, thunder, light, dark likewise
   801de094  lbu  v1,0x1de(s1) ; li v0,4         ; the guard halve, after the skipped block
   ```

   The two words are the character's accessory-passive bitfield. Passive index `0x1D + element` is
   that element's guard and `0x24` is All Guard. The attacker's element is byte `+0x1D` of its
   monster record.
2. **Only one caller sets the argument.** The disc holds three `jal 0x801DDB30` words, all in PROT
   0898: `0x801DD478` (`FUN_801DD0AC`) and `0x801DD678` (`FUN_801DD4B0`) store zero in the delay
   slot; `0x801DD828` (`FUN_801DD6B4`) follows `li v0,0x1` at `0x801DD824` with `sw v0,0x10(sp)`.
3. **The bypass wrapper is a physical kernel.** `FUN_801DD6B4(power, attacker, defender)` adds the
   attacker's ATK (`lhu v0,0x158(s0)` at `0x801DD738`) to the power and a random term, and scales
   the defender's roll by `(UDF + LDF) >> 3` (`0x801DD74C..0x801DD764`). INT is never read.
4. **Who calls the bypass wrapper.** The word `jal 0x801DD6B4` (`0x0C0775AD`) occurs at thirteen
   places on the disc, in six cast modules and nowhere else. Each module's head reads the queued
   spell id from `actor[+0x1DF]` and picks a routine, which ties every call to a spell.

| PROT | Call sites | Power argument | Reached for spell | Attack |
|---|---|---|---|---|
| 0944 | `0x801F6AD8` | `0x38E` | `0x37` (id test at `0x801F7EE8`; `0x53` takes another routine) | Guilty Cross |
| 0952 | `0x801F7948` | `0x1D0` | `0x5C` (id test at `0x801F7B54`; `0xB8` takes another routine) | Bloody Horns |
| 0953 | `0x801F7410` | `0x274` | one routine at `0x801F69FC`, no id test | Terio Punch `0x5D`, Bull Charge `0x5E` |
| 0958 | `0x801F74EC`, `0x801F7854`, `0x801F7CA4`, `0x801F80E4`, `0x801F8754`, `0x801F88D8` | `0x30`, `0x38` three times, `0x40`, one computed | `0x79` | Blazing Slash |
| 0959 | `0x801F71E8`, `0x801F7A90`, `0x801F7EBC` | `0x80`, `0x80`, `0x30` | `0x7A` | Megaton Press |
| 0960 | `0x801F8168` | `0x1C0` | `0x7B` (id test at `0x801F8664`; `0xA6` takes the other wrapper at `0x801F7280`) | Plasma Strike |

The spell-to-module link is static data: the spell record at `0x800754C8 + id * 12` has class byte
`0x63` at `+0` and a module index at `+1`, and the module is PROT entry `935 + index`.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801DDB30` | PROT 0898 (file `0xF318`) | Damage finisher | disassembly |
| `0x801DDBD4` | PROT 0898 | The branch that skips the jewel block | disassembly |
| `0x801DDC0C`, `0x801DDCA8`, `0x801DDD3C` | PROT 0898 | All Guard, Earth Guard, Water Guard tests | disassembly |
| `0x801DE094` | PROT 0898 | Guard halve, still applied | disassembly |
| `0x801DD6B4` | PROT 0898 (file `0xEE9C`) | Bypass wrapper; sets the argument at `0x801DD824` | disassembly |
| `0x801DD4B0`, `0x801DD0AC` | PROT 0898 | The two callers that pass zero | disassembly |
| `0x0C0775AD` | PROT 0944, 0952, 0953, 0958, 0959, 0960 | `jal 0x801DD6B4`, thirteen sites, table above | disc data |
| `0x800754C8 + id * 12` | `SCUS_942.54` | Spell record: class at `+0`, module index at `+1` | disc data |
| record `+0xF4` / `+0xF8` | character record (`+0x6BC` / `+0x6C0` off `0x80084140` for character 1) | Accessory-passive bitfield | disassembly |
| record `+0x1D` | PROT 0867 monster record | Attacker element | disassembly |

### Scope

The complete set that reaches the finisher with the bypass argument set is seven attacks: Guilty
Cross `0x37`, Bloody Horns `0x5C`, Terio Punch `0x5D`, Bull Charge `0x5E`, Blazing Slash `0x79`,
Megaton Press `0x7A` and Plasma Strike `0x7B`. Nothing else on the disc does.

Their users: Xain's record carries Bloody Horns and Cort's carries Guilty Cross in the record's
magic bytes (disc data). Bull Charge and Terio Punch (Xain), the three Delilas attacks and Gaza's
Plasma Strike are queued by the enemy AI's per-boss switch, described in
[spell-table.md](../../formats/spell-table.md) (repo doc).

Because the bypass wrapper reads ATK, a boss's INT does not move these attacks either.

### Verifying it

Break on `0x801DDBD4` as each attack lands and read `a3`: it is `1` for the seven attacks above and
`0` for any other enemy cast, for example Neo Star Slash from the same module as Plasma Strike. In
play, a matching jewel leaves the seven unchanged and halves the others.

### Astral Slash is a melee hit

Astral Slash `0xB8` shares PROT 0952 with Bloody Horns, and its routine (`0x801F6A0C`) calls none
of the three wrappers. It stages clip `0x0A` on Gaza and the ordinary melee kernel rolls that clip's
four beats, so a matching jewel halves it through the kernel's own copy of the guard block, keyed
on Gaza's light element ([battle-formulas.md](../../subsystems/battle-formulas.md#capture-class-wrappers---fun_801dd4b0--fun_801dd6b4),
repo doc). The same block covers every plain monster swing: an elemental monster's ordinary attack
is halved by the matching jewel too.

## Bosses on the USA disc cannot have ATK, INT or DEF lowered

**Status:** explained
**Reported as:** In the US version, bosses' ATK, INT and DEF cannot be reduced with spell effects,
because of the way it is programmed (community report).

Levelled Seru magic carries a side effect by element: fire lowers ATK, earth lowers DEF, thunder
lowers INT. There is no boss-immunity flag for these. Two unrelated pieces of code collide.

The first is a rule in the debuff code for scripted fights: lower a stat only if it is still at
its original value. The code tests that by comparing the monster's live base stat with the number
in its disc record. After a debuff lands the base has moved, so the rule lets each debuff work
once per boss fight.

The second is the USA difficulty boost. When a scripted fight loads, the USA executable raises the
monster's ATK, defence and INT above the record values, and it writes the raised numbers into the
base as well. From the first frame of every boss fight the base no longer equals the record, so
the debuff code concludes the stat has already been lowered and prints "No effect.".

Speed and MP are not boosted, which is why those two side effects still work on bosses.

The boost is in the USA executable only. The Japanese (`SCPS_100.59`) and French (`SCES_019.44`)
loaders go from the stat copy straight to the spell-list loop, and both carry the same
base-versus-record compare in the debuff code. On those discs the compare passes until the first
debuff lands: the first attack-down, defence-down or intelligence-down on a boss works, and a
second one of the same kind is refused.

### Mechanism

1. **The stager.** `FUN_801F3D3C` in PROT entry 0898 runs once per cast. It returns early for a
   spell below level 3 (`0x801F3DE0`), applies a four-in-five suppression roll in scripted fights
   (`0x801F3DEC..0x801F3E7C`), then jumps through a table at `0x801CFA2C` on the summoned
   creature's element.
2. **The compare.** The earth arm, as an example:

   ```text
   801f3ec8  lbu  v0,0x287(v1)
   801f3ed0  beqz v0,0x801f4414        ; random encounter: no compare
   801f3f38  lhu  v1,0x15e(v1)         ; live upper-defence base
   801f3f3c  lhu  v0,0x14(v0)          ; the record's upper defence
   801f43f0  beq  v1,v0,0x801f4414     ; equal: go on and stage the debuff
   801f43fc  addiu s0,s0,1             ; different: mark as blocked
   801f4414  bnez s0,0x801f4484        ; blocked: return, nothing staged
   ```

   | Element | Arm | Live base | Record field | Stat |
   |---|---|---|---|---|
   | earth | `0x801F3EBC` | `+0x15E` | `+0x14` | upper defence |
   | water | `0x801F3FA4` | `+0x156` | `+0x0E` | AGL (compared in every fight) |
   | fire | `0x801F407C` | `+0x15A` | `+0x12` | ATK |
   | wind | `0x801F4164` | `+0x166` | `+0x1A` | SPD |
   | thunder | `0x801F424C` | `+0x16A` | `+0x18` | INT |
   | dark | `0x801F4334` | `+0x152` | `+0x10` | MP |

3. **The debuff moves the base.** A staged percentage (5, 10, 15 or 20 by spell level, table at
   `0x801F6870`) is applied by the finisher to both halfwords of the stat pair
   (`0x801DE648..0x801DE8EC`), which is what turns the compare into a once-per-fight latch.
4. **The loader boost.** `FUN_80054CB0` in `SCUS_942.54` copies each record stat into both
   halfwords (`0x8005516C..0x8005520C`) and then, for a scripted fight:

   ```text
   80055234  lbu v0,0x287(v0) ; beq v0,zero,0x800552ac
   80055248  ATK  += ATK >> 2      ; sh 0x15a / sh 0x158
   80055264  UDF  += UDF           ; sh 0x15e / sh 0x15c
   80055280  LDF  += LDF           ; sh 0x162 / sh 0x160
   8005529c  INT  += INT >> 3      ; sh 0x16a / sh 0x168
   ```

   SPD, AGL, HP and MP are left alone. The same block holds the random-encounter arm
   (`0x800552AC..0x80055300`: both defences `+ half + quarter`, INT `+ quarter`, ATK untouched),
   so ordinary monsters are tougher on the USA disc as well. It never shows in the debuffs because
   the stager skips the compare for them.
5. **The other builds.** The whole block `0x80055228..0x8005530C` - the flag load and both arms -
   is absent from the Japanese and French loaders. Each writes the record stat unchanged into the
   current and the base halfword, clears two bytes and falls into the spell-list loop. The stager
   is unchanged: the French one matches the USA routine instruction for instruction once
   addresses are masked, and the Japanese one is a different compile with the same level gate,
   suppression roll, jump table, stat pairs and shared compare.

   | Build | Loader: what follows the stat copy | Stager | Shared compare | Scripted-fight flag |
   |---|---|---|---|---|
   | USA `SCUS_942.54` | the boost block `0x80055228..0x8005530C`, then the spell loop at `0x80055310` | `0x801F3D3C`, PROT 0898 at base `0x801CE818` | `0x801F43F0` | `ctx[+0x287]` |
   | France `SCES_019.44` | byte clears at `0x80055B0C` / `0x80055B18`, then the spell loop at `0x80055B1C` | `0x801F468C`, PROT 0898 at base `0x801CF0A8` | `0x801F4D40` | `ctx[+0x2A7]` |
   | Japan `SCPS_100.59` | byte clears at `0x80057004` / `0x80057010`, then the spell loop at `0x80057014` | `0x801F7394`, PROT 0898 at base `0x801D05C8` | `0x801F79BC` | `ctx[+0x1E7]` |

   The Japanese jump table is at `0x801D1880`; its arms compare the same pairs (earth `+0x15E`
   against record `+0x14` at `0x801F7588`, fire `+0x15A` against `+0x12` at `0x801F7728`, thunder
   `+0x16A` against `+0x18` at `0x801F78D8`).

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F3D3C` | PROT 0898 (file `0x25524`) | Debuff stager | disassembly |
| `0x801CFA2C` | PROT 0898 | Seven-entry element jump table | disc data |
| `0x801F43F0` | PROT 0898 | The base-versus-record compare all arms share | disassembly |
| `0x801F6870` | PROT 0898 (file `0x28058`) | Debuff percentages, `[element][level band]`, 8 bytes each | disc data |
| `0x801F6960` | RAM | Staged percentage; zero means "No effect." | disassembly |
| `0x801DE648..0x801DE8EC` | PROT 0898 | Finisher tail that applies the debuff to both halfwords | disassembly |
| `0x80054CB0` | `SCUS_942.54` | Monster stat loader | disassembly |
| `0x80055234..0x800552A8` | `SCUS_942.54` | Scripted-fight boost | disassembly |
| `0x800552AC..0x80055300` | `SCUS_942.54` | Random-encounter boost | disassembly |
| `0x80055B1C` | `SCES_019.44` | Spell-list loop directly after the stat copy: no boost block | disassembly |
| `0x80057014` | `SCPS_100.59` | Spell-list loop directly after the stat copy: no boost block | disassembly |
| `0x801F468C`, `0x801F4D40` | French PROT 0898 (base `0x801CF0A8`) | Debuff stager and its shared compare | disassembly |
| `0x801F7394`, `0x801F79BC` | Japanese PROT 0898 (base `0x801D05C8`) | Debuff stager and its shared compare | disassembly |
| `0x801D1880` | Japanese PROT 0898 (file `0x12B8`) | Seven-entry element jump table | disc data |
| `ctx[+0x287]` | RAM | Scripted-fight flag | disassembly |
| record `+0x12`, `+0x14`, `+0x18` | PROT 0867 monster record | ATK, upper defence, INT | disc data |

### Scope

The boost fails to move a stat only when ATK is below 4, upper defence is 0 or INT is below 8.
Joining the monster records with every formation row on the disc, no monster that appears on a
scripted row meets any of those conditions. On the USA disc, then, ATK-down, DEF-down and INT-down
never land in any scripted fight. That includes the scripted fights that are not bosses: the
tutorial Piuras, and the scripted Golem, Viguro, Kemaro and Puera rows.

Still available in a scripted fight: SPD-down once, AGL-down once, MP-down on every cast that
survives the suppression roll. In random encounters the three compares are skipped and the debuffs
stack.

One example with the real numbers: Zeto's record reads ATK 108, upper defence 95, INT 117, SPD 66.
The USA loader installs 135, 190, 131 and 66 (`108 + 27`, `95 + 95`, `117 + 14`). Published stat
lists, this project's curated table included, print the boosted boss figures for every enemy, so
the list value is the installed base and the record value is the lower number.

On the Japanese and French discs the fight starts with base equal to record, so the first of each
of the three debuffs is staged and the second is refused. The code that decides this is read in
both builds; the on-screen result there is inference, not a capture.

### Verifying it

In a boss fight, cast a level-3 or higher fire spell and break on `0x801F43F0`: `v1` holds the
boosted base (135 for Zeto) and `v0` the record value (108), the branch is not taken, and
`0x801F6960` stays zero. Writing the record value back into the actor's `+0x15A` and casting again
lets the debuff land. On a Japanese disc the same break is `0x801F79BC`, where `a0` and `v0` are
equal on the first cast.

### Open questions

- A boss debuff landing on a Japanese or PAL disc has not been watched in a running game.
- The arms treat target byte `8` and the multi-target values differently (`0x801F3F04`,
  `0x801F3F48`); which player spells use which value is not enumerated.

## Every Berserker can be killed outright

**Status:** explained
**Reported as:** All Berserker bosses can be insta-killed (community report).

The monster data has no "immune to instant death" field. The two player summons that can kill
outright, Nighto and Aluru, decide whether their target is a boss with a makeshift test: the fight
is scripted, and one particular byte of the monster record is non-zero.

That byte is a graphics setting. It tells the model loader that the monster's texture needs a
double-width page of video memory. Nearly every large boss model has it set, so it works as a
stand-in for "boss" - except for Berserker, whose texture fits in a normal page. Its byte is zero,
the test fails, and the summon rolls its ordinary kill chance.

All three Berserker records are the same in this respect, and among the story bosses they are the
only ones. A third summon shows the byte's blind spot was noticed somewhere: Zenoir uses the same
test to keep its shatter effect off bosses, and adds the three Berserker ids by hand. Nighto and
Aluru never received that list. Zenoir's list is about how a dead monster is drawn, not about
damage, and nothing in the code says its author knew the kill rolls shared the gap
([below](#zenoir-leaves-a-lone-golem-in-pieces)).

### Mechanism

1. **The byte.** Monster record `+0x20` is `0` or `1`. Its primary reader is the battle model
   upload, where it widens the texture rectangle ([battle.md](../../subsystems/battle.md)).
2. **Nighto** (spell `0x85`, PROT entry 0907):

   ```text
   801f6bf0  lbu  v0,0x287(a1)         ; scripted fight?
   801f6bf8  beqz v0,0x801f6c28        ; no: roll
   801f6c18  lbu  v0,0x20(v0)          ; the target's record +0x20
   801f6c20  bnez v0,0x801f6cb8        ; non-zero: resisted
   801f6c28  jal  0x80056798           ; rand()
   801f6c70  rand % (19 - spell level)
   801f6ca8  slti v1,v1,9              ; below 9: it lands
   ```

   The chance to land is `9 / (19 - level)`: 50% at level 1, 90% at level 9. When Gala is the
   caster one landed roll in three is turned into a failure (`0x801F6CE4..0x801F6D28`). The outcome
   is drawn separately as `rand() % 8` (`0x801F6B50..0x801F6B7C`): a `0` kills (`sh zero,0x14c` at
   `0x801F7E58`), anything else confuses (`ori v0,v0,0x380` at `0x801F7F50`). A landed Nighto is a
   kill one time in eight.
3. **Aluru** (spell `0x8E`, PROT entry 0916) has the same gate (`0x801F6D44`, `0x801F6D70`) and
   lands on `rand % (17 - level) < 8` (`0x801F6DE4..0x801F6E1C`): 50% at level 1 and every time at
   level 9. A landed Aluru always kills (`sh zero,0x14c(s3)` at `0x801F7E14`).
4. **Zenoir's hand-written list.** PROT entry 0908 applies the same gate and then compares the
   first formation monster id with `0x4D`, `0xAD` and `0xAE` (`0x801F8218..0x801F8230`). Those are
   77, 173 and 174: the three Berserkers. The test guards a visual effect, not damage; the next
   section explains it.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| record `+0x20` | PROT 0867, slot `(id - 1) * 0x14000`, LZS after a 4-byte size | Double-width texture flag reused as the boss test | disc data |
| `0x801C9348[slot - 3]` | RAM | Pointer to the monster's record | disassembly |
| `0x801F6BF0`, `0x801F6C18` | PROT 0907 | Nighto's gate | disassembly |
| `0x801F6C28..0x801F6CC4` | PROT 0907 | Nighto's roll; resist word `0x801F853C` | disassembly |
| `0x801F6B50..0x801F6B7C` | PROT 0907 | Outcome draw into `0x801F8534` | disassembly |
| `0x801F7E58`, `0x801F7F50` | PROT 0907 | Kill store, confuse store | disassembly |
| `0x801F6D44`, `0x801F6D70` | PROT 0916 | Aluru's gate | disassembly |
| `0x801F6DE4..0x801F6E1C` | PROT 0916 | Aluru's roll; resist word `0x80209010` | disassembly |
| `0x801F7E14` | PROT 0916 | Aluru's kill store | disassembly |
| `0x801F81F0..0x801F8230` | PROT 0908 | Zenoir's gate and Berserker id list (shatter effect only) | disassembly, capture |
| `0x801F87DC`, `0x801F8814`, `0x801F8438` | PROT 0931 | Jedo: flag test, kill store (flag clear), rolled damage (flag set) | disassembly; capture on a forced Lapis formation with an injected cast |
| `ctx[+0x287]` | RAM | Scripted-fight flag | disassembly |

### Scope

Every monster that appears on a scripted formation row, with the byte read from its record.
"Exposed" means the gate falls through to the roll.

| Id | Monster | HP | `+0x20` | Nighto / Aluru |
|---|---|---|---|---|
| 19 | Red Piura (tutorial) | 75 | 0 | exposed |
| 20 | Black Piura (tutorial) | 85 | 0 | exposed |
| 73 | Caruban | 1024 | 1 | immune |
| 74 | Viguro | 1280 | 0 | exposed |
| 75 | Zeto | 5000 | 1 | immune |
| 76 | Songi | 888 | 1 | immune |
| 77 | **Berserker** | 5000 | 0 | **exposed** |
| 128 | Golem | 640 | 0 | exposed |
| 136 | Songi | 12888 | 1 | immune |
| 137 | Van Saryu | 13245 | 1 | immune |
| 138 | Dohati | 17200 | 1 | immune |
| 139 | Xain | 9511 | 1 | immune |
| 144 | Kemaro | 1400 | 0 | exposed |
| 156 | Puera | 1600 | 0 | exposed |
| 162 | Gi Delilas | 10000 | 1 | immune |
| 163 | Che Delilas | 12000 | 1 | immune |
| 164 | Lu Delilas | 9500 | 1 | immune |
| 165 | Gaza | 12000 | 1 | immune |
| 166 | Gaza | 15000 | 1 | immune |
| 168 | Rogue | 28200 | 1 | immune |
| 169 | Zora | 22422 | 1 | immune |
| 170 | Jette | 34567 | 1 | immune |
| 171 | Caruban | 4096 | 1 | immune |
| 172 | Caruban | 8192 | 1 | immune |
| 173 | **Berserker** | 10000 | 0 | **exposed** |
| 174 | **Berserker** | 15000 | 0 | **exposed** |
| 175 | Tetsu | 9999 | 1 | immune |
| 179 | Songi | 48000 | 1 | immune |
| 180 | Cort | 50000 | 1 | immune |
| 181 | Cort | 65535 | 1 | immune |
| 182 | Koru | 20000 | 1 | immune |

The exposed rows other than the Berserkers are ordinary monsters and capturable Seru fought through
a scripted row. Records with the byte set that never sit on a scripted row, where it therefore
plays no part: the Evil Fly family, Gizam, Gilium, the low-HP Tetsu and Lapis.

Lapis is the case that shows the byte alone protects nothing. Its only formation row has a zero
header byte, so the scripted-fight flag is clear in that fight, the gate is never consulted, and
Lapis rolls like a random encounter although its byte is `1`. The flag reads 0 for the whole
fight in a capture on a forced formation (the Lapis row's bytes written over another scripted
fight's row as it loads); see [lapis-and-cort.md](lapis-and-cort.md). The hand-written id-167
exception in the Tough Love arm above exists for the same reason.

Jedo is no third route against a Berserker. Its module (PROT entry 0931, spell `0x9D`) branches on
the scripted-fight flag alone at `0x801F87DC`: clear, it sets every living enemy's HP to zero
(`0x801F8814`); set, it takes the rolled-damage path (`0x801F8438`). A Berserker fight is flagged,
so Jedo only deals damage there.

### Verifying it

In a Berserker fight, break on `0x801F6C20` during a Nighto cast or `0x801F6D78` during an Aluru
cast: `v0` reads 0 and execution continues into the `rand()` call. In the Zeto or Xain fight it
reads 1 and the resist arm is taken. A level-9 Aluru should kill a Berserker on every cast.

### Open questions

- Nighto and Aluru have not been cast on a Berserker in a running game; the result above is read
  from the gate and the three records.
- Instant-death sources outside these two summons and Jedo are not surveyed.

## Zenoir leaves a lone Golem in pieces

**Status:** explained
**Reported as:** What is special about Zenoir against Berserker? (community question, following
the Berserker write-up above).

When Zenoir kills a monster, the monster does not just fade. Its model breaks into its own
triangles, which fly outward and spin while the body fades to black. Most of the time the fire
covers it and the shards are gone with the fade.

A boss is different. When the last monster of a scripted fight dies, the game does not fade it
away: it keeps drawing the body at full colour so it stays on stage until the victory scene. A
shattered boss would leave a lit cloud of shards hanging over the stage. Zenoir therefore skips
bosses, using the same makeshift test as Nighto and Aluru (scripted fight, and record byte `+0x20`
set). That byte is `0` on Berserker, so the three Berserker ids are listed by hand, and the
Berserker's body stays whole.

The list stops there. Two other lone scripted monsters have the byte at `0`: the Golem (id 128,
the `rikuroa` scenes) and the lone Puera (id 156, the `taiku` scenes). Zenoir shatters them, and
the shards stay lit on stage until the battle ends.

### Mechanism

1. **The effect.** Every frame of Zenoir's phase `0xC` (`0x801F8108`, after the damage of phase 8),
   the loop at `0x801F81C8..0x801F8264` adds `[0x1F800393] * [0x1F80037D]` (32 per tick in the
   capture) to `+0x1BA` of each monster seat with `+0x14C == 0`. The battle anim tick copies the word to the draw node's `+0x7A`
   (`0x800480A4`), and a non-zero `+0x7A` sends the body through the shatter renderer
   `FUN_80029888` / `FUN_80029DD8`: each triangle spins about its own centre and is pushed out by
   `+0x7A / 128` of its distance from the body's centre
   ([renderer.md](../../subsystems/renderer.md#the-shatter-renderer-fun_80029888--fun_80029dd8)).
   A seat stops growing once its `+0x04` colour word, which the fade below drains, reaches zero,
   and the arm itself ends when the countdown `ctx[+0x6D8] = scalar * 192` the finisher seeded
   runs out: `+0x1BA` ends near 1000 in the capture, about 8.8 times the starting distance.
   Nothing resets it.
2. **The fade.** A killed monster enters arm 2 of the tint state machine (`+0x21C = 2`), which
   walks its colour to black; a body at black is no longer drawn.
3. **The exception.** The draw tick `FUN_800480D8` overrides the colour with `0x00808080` and
   keeps drawing when the fight is scripted (`ctx[+0x287]`), the formation has one monster
   (`0x8007BD0D == 0`) and the seat is in arm 2 (`0x800481D4..0x8004823C`,
   `0x80048278..0x800482DC`).
4. **Zenoir's gate.** The loop skips a seat when the fight is scripted and record `+0x20` is set
   (`0x801F81F0..0x801F8210`), or when the first formation id is `0x4D`, `0xAD` or `0xAE`
   (`0x801F8218..0x801F8230`). The record it tests is the latched victim's for every seat (the
   pointer is set from the victim seat at `0x801F813C` and never stepped), so in a two-monster
   scripted fight the boss byte of whichever monster was targeted decides for both.
5. **Jedo** (PROT entry 0931) shatters every monster it kills in a random fight
   (`0x801F86E0..0x801F874C`) and skips the effect in every scripted fight, so it never meets the
   problem.

### How it was measured

Captured under PCSX-Redux on the USA disc from a Rim Elm battle state with Vahn alone. The
formation row was rewritten as the battle loaded, Zenoir (`0x86`) was injected into Vahn's queued
action, and the target's HP was set to 1. Every fight here is forced and every cast injected.

| Forced row | Scripted | Result |
|---|---|---|
| Queen Bee + Killer Bee | no | Both bodies shatter; the shards fade to black with the bodies and vanish |
| Golem alone | no | Shatters and fades to black |
| Golem alone | yes | Shatters; the colour word reads `0x00808080` from the first shattered frame and the shards stay lit over the stage until the victory camera |
| Berserker (`0x4D`) alone | yes | The list test runs every frame and the store never does; `+0x1BA` stays `0` and the body stays whole |
| Berserker alone, id cell poked to `0x4E` during the list test | yes | Shatters into a lit cloud of green and purple shards |

The battle node's `+0x6D` reads `0xFF`, and the row it names reads zero, so the shatter takes its
default form: spin and outward push, no shrink.

The port reproduces the shatter and this gate (`zenoir_shatter_pass`, drawn by both hosts through
`legaia_render_kernels::shatter`), and the grey stamp keeps the scripted lone Golem's shards lit as
retail does ([cast-module.md](../../subsystems/cast-module.md#what-the-port-runs)).

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F8108` | PROT 0908 | Phase `0xC` arm | disassembly |
| `0x801F81C8..0x801F8264` | PROT 0908 | Shatter loop: dead seats, gate, list, `+0x1BA` add | disassembly, capture |
| `0x801F76CC` | PROT 0908 | Phase-8 damage call (`FUN_801DD0AC`) | disassembly |
| `0x800480A4` | SCUS_942.54 | `+0x1BA` copied to the node's `+0x7A` | disassembly |
| `0x80048EAC..0x80048FC0` | SCUS_942.54 | Non-zero `+0x7A` routes the body to `FUN_80029888` | disassembly, capture |
| `0x80029DD8` | SCUS_942.54 | Per-triangle centroid spin and push | disassembly |
| `0x800481D4..0x800482DC` | SCUS_942.54 | Grey stamp for a lone scripted monster in arm 2 | disassembly, capture |
| `0x801F86E0..0x801F874C` | PROT 0931 | Jedo's shatter loop, skipped in scripted fights | disassembly |

### Scope

Every lone scripted fight, with record `+0x20` and what a Zenoir kill does to the body:

| Id | Monster | Scenes | `+0x20` | Body after a Zenoir kill |
|---|---|---|---|---|
| 19, 20 | Red / Black Piura | `cave01` (tutorial) | 0 | would shatter and stay; Zenoir cannot be cast there |
| 77, 173, 174 | Berserker | `teien`, `rugi`, the `ed*` scenes | 0 | whole (the hand list) |
| **128** | **Golem** | `rikuroa`, `rikuroa2` | 0 | **shattered, shards stay lit** |
| **156** | **Puera** (lone row) | `taiku`, `taiku2` | 0 | **shattered, shards stay lit** (predicted, not run) |
| all other lone scripted bosses | Zeto, Xain, Cort, Songi and the rest | - | 1 | whole |

The scripted rows with two monsters (Viguro, Kemaro, the paired Puera) fade normally, because the
grey stamp needs a lone monster.

### Verifying it

Kill the Rikuroa Golem with Zenoir: the shards should still be on screen, lit, after the fire has
cleared. Break on `0x801F8254` to watch the store, and read `+0x74` of the Golem's draw node for
`0x00808080`. In a Berserker fight the same breakpoint never fires.

### Open questions

- The lone Puera is predicted from the same code and record; it was not run.
- The Golem capture is a forced formation in Rim Elm with an injected cast, not the real Rikuroa
  fight.

## Evil Fly and other boss-byte monsters can be killed outright

**Status:** explained
**Reported as:** Evil Fly has the non-insta-killable flag set, but you can still kill it outright
because it is not a scripted fight (old forum note).

The report is right. Nighto and Aluru only read record byte `+0x20` after they have checked that
the fight is scripted. In any other fight they roll the kill chance without looking at the
monster. The game has no "boss" field. Immunity needs both halves:

| | Record `+0x20 = 1` | Record `+0x20 = 0` |
|---|---|---|
| **Scripted fight** | resists (Zeto, Xain, Cort, ...) | rolls: Berserker, Golem, Puera, Viguro, Kemaro, the tutorial Piuras |
| **Any other fight** | rolls - the byte is never read (Evil Fly, Gizam, Gilium, Lapis, ...) | rolls (every ordinary monster) |

The two cells of the bottom row behave identically; the split only shows that the byte makes no
difference there.

**What the byte is.** Record `+0x20` is a graphics setting: the model upload
(`0x801F1D0C` -> `FUN_80055468`) doubles the monster's texture rectangle in video memory when it is
`1`. Large detailed models need the wide page, which is why most big bosses carry it, but so do a
few ordinary monsters with wide textures. It is set on 37 of 186 records. Apart from the upload,
exactly three places on the disc read it, all borrowing it as a "big boss" test: Nighto's and
Aluru's resist gates and Zenoir's shatter gate ([battle.md](../../subsystems/battle.md)). No stat,
AI or reward code reads it.

The byte is set on eleven monsters that never stand on a scripted formation row, so it protects
none of them:

| Id | Monster | HP | Where it is met |
|---|---|---|---|
| 1, 2, 3 | Evil Fly, Death Wings, Demon Fly | 82, 209, 324 | random encounters |
| 55, 56, 57 | Gizam (three versions) | 266, 567, 2192 | random encounters; the strongest also on unflagged event rows |
| 79 | Tetsu | 999 | unflagged event rows only |
| 159, 160, 161 | Gilium (three versions) | 2880, 3280, 3542 | random encounters and unflagged event rows |
| 167 | Lapis | 64800 | one event row with a zero header byte ([lapis-and-cort.md](lapis-and-cort.md)) |

Lapis is the striking one: a story boss with the byte set, met on a row whose header byte is zero,
so the fight is not flagged as scripted and Nighto, Aluru and Jedo treat it as a random encounter.

### Mechanism

The gate is the one quoted in the Berserker section: `lbu v0,0x287(a1)` / `beqz` at
`0x801F6BF0..0x801F6BF8` (Nighto) and `0x801F6D44` (Aluru) branch straight to the roll when the
fight is not scripted, before record `+0x20` is loaded. The scripted-fight flag comes only from the
formation row's first header byte (`FUN_801DA51C`, `0x801DA5F8..0x801DA61C`).

### How it was counted

The table joins each monster record's `+0x20` (PROT 0867) with a census of every formation row in
every scene MAN, split by the row's header byte (`asset formation-census`).

### Verifying it

Cast a level-9 Aluru on an Evil Fly in a random encounter: it lands every time (`rand % 8 < 8`) and
the fly dies. Breaking on `0x801F6D44` shows `ctx[+0x287] = 0`, and execution never reaches the
record load at `0x801F6D70`.

### Open questions

- Not yet cast on an Evil Fly in a running game; the outcome is read from the gate.
