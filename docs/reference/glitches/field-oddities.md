# WORK ERR, missing encounters and script slips

Five community-reported oddities that live in the field and battle runtime rather than in one
attack or item: a debug counter that reaches the screen on Mt. Rikuroa, the random battles that
stop in Jette's Absolute Fortress, the freeze after a Spikefish Flute escape, the camera orbit
that ends the Gaza rematch, and three small slips in the English script. One section is a mechanic rather than a fault: the two springs in
Rogue's Tower that change the encounter rate. One is a freeze that needs an emulator to trigger it:
the heap running out in Conkram's mirror scene when the emulated CPU is too fast. A closing section records the identity of
the two summon flutes, as support for the flute pages.

The base of every section is static analysis of the USA disc: disassembly of `SCUS_942.54` and its
overlays, and decoded disc data. Several sections also rest on emulator captures, and each says which
kind. The Mt. Rikuroa capture is on a poked state: a retail cold boot from a memory-card save, with
story flags cleared and the player teleported. The Fortress captures are one played memory-card
save and one edited copy of it. The bath scene is read against the Japanese disc (`SCPS_100.59`)
as well. Where a step is inferred rather than read or observed, the section says so.

Addresses are virtual addresses in the named image. PROT numbers are extraction indices; the
scene of a PROT entry is resolved from its index, because extraction filename labels are shifted
against `CDNAME.TXT` ([`cdname.md`](../../formats/cdname.md#numbering-space)). `gp` is
`0x8007B318`. `P1[n]` and `P2[n]` are records of a scene MAN's partitions 1 and 2.

## WORK ERR on Mt. Rikuroa

**Status:** partially explained

**Reported as:** the debug text "WORK ERR2" appears in the top-left corner on Mt. Rikuroa right
after the Golem fight, as the cave collapses, and stays (community reports; one
[recorded playthrough](https://www.youtube.com/watch?v=Y212XJ_qQEQ) shows it, and two viewers of
that recording report the same text at the same place). It is reported on a console and in
DuckStation as well as in the emulator of the recording, so it is not one emulator's artefact; the
console and DuckStation reports are relayed, not recorded.

Everything that moves in a scene - the player, each townsperson, each door and prop, each rock and
dust puff of a visual effect, each open dialogue box - is an *actor*, and all of them come out of
one fixed pool of 143 slots. When something asks for a slot and the pool is empty, the request
fails, an error counter goes up by one, and from then on the frame loop prints `WORK ERR` followed
by that counter every frame.

So "WORK ERR2" is not error number two. It says that two actor requests have failed since the
current stage was set up. There is no separate "WORK ERR1" message: the digit is a `%d`.

Mt. Rikuroa uses 97 of the 143 slots just standing still. When the Golem is beaten the game does
not rebuild the scene: it restores the mountain exactly as it was before the fight, every actor
included, and at once plays the cave collapse, which throws up to 40 rocks and dust puffs at the
same moment a dialogue box opens. Replayed from a memory-card save, that moment leaves between
four slots and no slots to spare. A few more actors alive at the peak and the next requests fail:
the counter reads 2 in every report and stays on screen until the next battle or scene load.
Nothing else breaks; a part or two of the collapse never appears.

What the message means is proven from the instructions, and the event that fills the pool is
captured. The failed request itself is not: the replay reaches exactly 143 of 143 and never asks
for a 144th. A recorded playthrough shows the text arrive at the moment the replay's pool is
fullest, but with the dialogue box already closed, which the replay survives with three or four
slots free. A played run is therefore several actors heavier than the replay, and which actors
those are is not known. That is why the entry stays partial.

### Mechanism

1. The string `WORK ERR%d` is at `0x800100F4` in `SCUS_942.54`. One reference to it exists on the
   disc, a `lui` / `addiu` pair in the per-frame driver `FUN_80016444`:

   ```
   800164b0  lh    a1,0x588(gp)        ; s16 counter at 0x8007B8A0
   800164b8  beq   a1,zero,0x800164cc
   800164c0  lui   a0,0x8001
   800164c4  jal   0x8001a068          ; dev text formatter
   800164c8  _addiu a0,a0,0xf4         ; "WORK ERR%d"
   ```

   The two prints above it in the same routine sit behind `gp+0x3B8`, the development counter.
   This one does not: it runs whenever the counter is non-zero.
2. The counter `0x8007B8A0` has three other references. Stage init `FUN_8001E1B4` clears it
   (`sh zero` at `0x8001E304`). The other two are a load and a store in the actor spawn
   `FUN_80020DE0`:

   ```
   80020df4  jal   0x80020454          ; pop a slot off the free stack
   80020dfc  move  s0,v0
   80020e00  bne   s0,zero,0x80020e34  ; got one: initialise it
   80020e0c  lhu   a1,-0x4760(a1)      ; 0x8007B8A0
   80020e14  addiu a1,a1,0x1
   80020e1c  sh    a1,-0x4760(at)
   80020e24  jal   0x800567a8          ; a0 = 0x800106A8, the dev-console twin of the message
   80020e2c  j     0x80020f74          ; return 0
   ```

3. The pop `FUN_80020454` reads the free-stack top index at `0x8007C348` and returns null when it
   is negative (`bltz`). The seed `FUN_800203EC` stores `0x8E` as the top and fills the stack at
   `0x8007C370` with `0x8007C5AC + i * 0xD8` for `i = 0..0x8E`: 143 slots of `0xD8` bytes. Stage
   init then pops seven of them as list sentinels (seven `jal 0x80020424` at
   `0x8001E32C..0x8001E35C`), so 136 are available to a scene. The number of free slots is the top
   index plus one.
4. A failed spawn returns null. Some callers test the result; `FUN_8003AB2C` writes through it
   unchecked. The effect-part stager that matters here, `FUN_80021B04`, tests it (`bnez s0` at
   `0x80021B84`), so a failed part is a part that is not drawn.

The pool, its free path and the control block in front of it are documented in
[`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md#the-part-pool-and-the-halted-part-free).

#### The pool survives the battle

The counter is cleared at every stage set-up, including the one that follows a battle. The pool is
not. Stage init skips the reseed and the seven sentinel pops when the field resumes from a battle:

```
8001e304  sh    zero,-0x4760(at)      ; 0x8007B8A0 = 0, always
8001e308  bne   v1,v0,0x8001e324      ; resume word 0x8007B8B8 != 2 -> reseed
8001e314  lh    v0,-0x47c4(v0)        ; 0x8007B83C, next mode
8001e31c  beq   v0,v1,0x8001e36c      ; == 2 -> skip FUN_800203EC and the seven pops
8001e324  jal   0x800203ec
```

On the way into the battle the mode-entry prologue `FUN_80016230` parks the whole pool block
(`0x8007C348` onwards) in VRAM, and the field overlay copies `0x7B0C` bytes back on return - the
exact span `0x8007C348..0x80083E54`. Scene entry `FUN_8003AEB0` then takes its resume arm: no
placements, no map binds, but a fresh system-script actor from `FUN_8003AB2C`. The stage-init
branch is read from the instructions; the park and the copy-back call sites are read from the
decompiled C and not re-read as instructions. The consequence is captured: every actor alive when
the fight started is alive again when the cutscene starts.

A real scene change does not carry the resume word and does reseed, so nothing survives an area
transition. The "transition to the next area" in the reports is not one. `P2[47]` runs on after
the collapse without leaving the scene: it fades the screen (`34 01 FF FF FF 3C 00` at `+0x131`,
then `4A 3C 00`), moves the player and Terra to tile `(0x5D,0x5D)` (`A3 F8 5D 5D` at `+0x13D`,
`A3 12 5E 5D` at `+0x145`) and plays the mountainside dialogue from the same record. No stage init
runs across that fade, so the counter and the text carry over to the mountainside.

#### The Golem fight and the collapse are one record

The scene is `rikuroa` (PROT block 153..162): its field map is PROT entry 153, its walk-on trigger
sidecar PROT entry 154, and its story-state script container the variant MAN in PROT entry 157,
with partition counts 13 / 29 / 64.

`P2[47]` is the Golem record. Its walk-on tiles are `(69,97)`, `(69,98)` and `(69,99)`. It tests
and sets flag `0x285` (`72 85`, `52 85`), waits (`4A 08 00`) and starts a scripted battle against
formation row 16 (`3E FF 10`; monster id `0x80`, the Golem). When the field returns, the system
script `P1[0]` finds `0x285` set (`72 85 A4 06` at `+0x136`), respawns the same record (`44 59` at
`+0x7E0`; `0x59` is `P2[47]` in the global record numbering), and the record's post-battle arm at
`+0x23` installs two effect trees, `34 30 0D` and `34 30 07`, before the dialogue. Those two trees
are the rockfall.

#### What the pool holds, measured

The capture is a retail disc cold-booted in PCSX-Redux and a memory-card save loaded through the
title screen: Vahn alone on `rikuroa`, just after the Golem. It is **captured on a poked state**,
because no save from before the Golem was available. To put the climb back before the fight, story
flags `0x281..0x286`, `0x300` and `0x366` were cleared; the player actor's position was written to
stand on each walk-on trigger tile in route order; and during the battle the Golem's HP was set to
1 so one hit ends it. Everything else is the game's own code: the triggers fired by themselves,
the battle was fought and won, and the return and the collapse ran unassisted.

Scene entry, counted from the loaded scene:

| Source | Slots | Read from |
|---|---|---|
| list sentinels | 7 | `FUN_8001E1B4` |
| props and NPCs (tick `0x8003BC08`): partition-1 placements 1..28 | 28 | the `FUN_8003A1E4` loop over the MAN header count |
| props and NPCs: map object binds | 54 | `FUN_8003A55C`: one actor per map tile whose object descriptor has `+0x12 & 4` and a kind-1 trigger at its key tile ([`field-map.md`](../../formats/field-map.md)) |
| the player and the other system actors | 8 | among them list-0 ticks `0x801D6058`, `0x801DA51C`, `0x801E76D4`, `0x80036D80`, `0x801D84D0`, and two list-8 actors |
| **at scene entry** | **97 of 143** | free-stack top `45`: 46 free |

The 82 props and NPCs are the count the disc data predicts (28 + 54), now measured.

From there to the collapse, one replay in full (vsyncs counted from the field's return):

| Moment | Free-stack top | Free slots |
|---|---|---|
| scene loaded from the card | 45 | 46 |
| after the four trigger records on the climb, before the Golem tile | 43 | 44 |
| field resumes after the battle | 32 | 33 |
| first rock burst, +160 vsyncs | 14 | 15 |
| second rock burst, +237 vsyncs | 2 | 3 |
| third burst, +446 vsyncs | 8 | 9 |
| at rest afterwards | 42 | 43 |

Two of the slots lost on the climb are persistent effect parts left by `P2[44]` and `P2[45]`; they
are still held at the peak.

Thirty-two replays of the same battle that differ only in when Cross is pressed after the fight
bottom out as follows:

| Fewest free slots | 4 | 3 | 2 | 1 | 0 |
|---|---|---|---|---|---|
| Replays | 2 | 15 | 2 | 6 | 7 |

At zero free the pool holds 82 props and NPCs, 40 effect parts (tick `0x80021DF4`), 7 sentinels,
3 dialogue-box actors, 4 list-8 actors and 7 others: 143 of 143. The spread between replays is
whether the three-actor "!" text box is open on the burst's peak frame. In none of the 32 did a
spawn arrive while the stack was empty: the counter `0x8007B8A0` stayed zero.

#### Who spawns the burst

With breakpoints on `FUN_80020DE0` and its failure arm, 81 of the 96 spawns between the field's
return and the peak come from return address `0x80021B80`: the `jal 0x80020de0` at `0x80021B78` in
the effect-tree stager `FUN_80021B04`, template `0x8007062C`, list `0x8007C350`. It issues five
parts on the first field frame, one about every 15 vsyncs, then 21 in 18 vsyncs and 41 in 42
vsyncs - two to seven per frame at the peak. The rest are the dialogue box (3), the system-script
actor (1), a record runner (1) and two other spawn sites, `0x80024CA8` and `0x80024EB0` (9).

#### The recorded run

One [community recording](https://www.youtube.com/watch?v=Y212XJ_qQEQ) shows the event in an
ordinary playthrough. It is the only played run in this section; times are positions in the video.
The grade of everything read off it is **community report**: a video of someone else's session,
with no access to its RAM.

| Video time | What is on screen |
|---|---|
| 3:08..5:19 | the Golem fight: Noa alone at level 5, Terra beside her; Noa reaches level 6 |
| 5:26.5 | the field returns; the "Terra: !" box is already open |
| 5:29.7 | the second box ("the cave is about to collapse! Follow me!") closes |
| 5:30.7 | `WORK ERR2` appears, reading 2 on its first frame; Terra has run off and Noa is running |
| 5:31.3 | the scripted fade starts; the mountainside fades in at about 5:32.5 with the text still up |
| 6:48 | Noa walks through a cave mouth, a short fade; the text is still up |
| 6:52.3 | a random battle starts; the text is gone from the first battle frame |
| 9:12 | the field returns after that battle; the text does not come back |

What it settles:

- **When.** The text arrives about four seconds, some 240 vsyncs, after the field returns. That is
  the replay's second burst (+237 vsyncs, the 41-part run), the moment the replay measures the
  pool at its fullest. It is not the return from the battle and not the fade to the mountainside,
  which starts about 0.6 seconds later.
- **The digit.** It reads 2 from the first frame it is drawn and never changes: both requests fail
  within one frame of the recording (30 per second). No frame reads 1.
- **How long.** About 82 seconds, across the fade to the mountainside and one walk through a cave
  mouth, until the next battle. That matches the counter's one clear, in stage init: the fade is a
  move inside the scene, and the battle entry and the battle return each run stage init. The save
  menu later in the recording names the cave "Mt. Rikuroa", the same scene. Whether the cave mouth
  at 6:48 is a move inside the scene is inferred from the text surviving it.
- **The run.** Noa alone with Terra following, in Noa's opening chapter, after ordinary random
  battles and a menu visit on the climb. The player does nothing unusual: both boxes are dismissed
  promptly and the cutscene plays by itself. Two commenters on the recording report the same text
  at the same place, so in a played run this is the common outcome, not a rare one.
- **The platform.** The recording is an emulator at a raised internal resolution, with filtered
  textures and a mouse pointer in some frames; the uploader names none. This is inference from the
  picture.

What it contradicts in the replay: the dialogue box. In the replay the pool only fills when the
three-actor box is open on the peak frame, and with the box closed three or four slots stay free.
In the recording the box has been closed for about a second when two requests fail. A played run
therefore holds at least five more actors at the peak than the replay does. The replay is a
different party on a poked state (Vahn alone, teleported between trigger tiles); the reported runs
are Noa with Terra, walked. Which of those differences supplies the actors is not measured.

#### The number on screen

The counter goes up once per spawn attempted while the stack is empty and is cleared only by the
next stage init. At the peak the stager asks for two to seven slots a frame, and parts free
themselves two list walks after halting. A run that reaches the peak with the pool already full
therefore fails the requests of one frame and then recovers, and the count is left on screen until
the next stage init. Every report, the recording included, reads 2; nothing in the instructions
fixes that number, so its constancy says the played run is the same few actors over the limit each
time. This is inference from the measured spawn rate and the recording. The caller whose request
fails is most likely `FUN_80021B04` creating a part of the collapse: the dialogue box, the other
candidate, is closed when the text appears in the recording.

Three other sources of the last slots are tested on the same set-up and ruled out. Finished
cutscene records do not accumulate: seven trigger records run in a row return the pool to the same
level each time, with one parked runner at most. Ordinary random battles do not leak: after each
of two forced encounters the pool rests where it started. And nothing survives a scene change.

#### The sibling messages

`FUN_80016444` prints four more diagnostics the same way:

| String | Address | Counter | Meaning |
|---|---|---|---|
| `WORK ERR%d` | `0x800100F4` | `gp+0x588` = `0x8007B8A0` | failed actor-pool spawns |
| `PRG ERR%d` | `0x80010100` | `gp+0x510` = `0x8007B828` | the error accumulator [`memory-map.md`](../memory-map.md) describes at that address |
| `READ ERR%d`, `NAME %s` | `0x80010134`, `0x80010140` | `gp+0x556`, name buffer `0x801C88F0` | read from the strings; not traced |
| `cd err count %d`, `RETRY %d` | `0x800100D4`, `0x800100E8` | `0x8007BC00`, `0x8007BC7C` | printed only while `gp+0x3B8` is non-zero |

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x800100F4` | `SCUS_942.54` | the `WORK ERR%d` string | disc data |
| `0x800164B0..0x800164C8` | `SCUS_942.54` | the print, in `FUN_80016444` | disassembly |
| `0x8007B8A0` | RAM | failed-spawn counter, signed 16-bit | disassembly |
| `0x8001E304` | `SCUS_942.54` | counter clear, in stage init `FUN_8001E1B4` | disassembly |
| `0x8001E308..0x8001E31C` | `SCUS_942.54` | battle-return test: resume word 2 and next mode 2 skip the reseed, to `0x8001E36C` | disassembly |
| `0x8007B8B8`, `0x8007B83C` | RAM | resume word; next mode | disassembly |
| `0x80020E00..0x80020E30` | `SCUS_942.54` | failure arm of the spawn `FUN_80020DE0` | disassembly |
| `0x80020454` | `SCUS_942.54` | free-stack pop; null when the top index is negative | disassembly |
| `0x800203EC` | `SCUS_942.54` | seed: top `0x8E`, 143 slots of `0xD8` bytes | disassembly |
| `0x8007C348` | RAM | pool control block: `+0x00` top index, `+0x1C` player, `+0x28` free stack | disassembly |
| `0x8007C5AC` | RAM | slot 0; slot `i` at `+ i * 0xD8`, last slot `0x80083D7C` | disassembly |
| `0x80016230` | `SCUS_942.54` | mode-entry prologue: parks the pool block in VRAM when the field is left for a battle | inference (decompiled C) |
| `0x8003A55C` | `SCUS_942.54` | map object-bind sweep at scene entry | repo doc |
| `0x80021B78` | `SCUS_942.54` | the spawn call in the effect-tree stager `FUN_80021B04`; result tested at `0x80021B84` | disassembly |
| return address `0x80021B80` | `SCUS_942.54` | 81 of 96 spawns between the battle return and the peak | capture (poked state) |
| `0x8007062C` | `SCUS_942.54` | the effect-part template; tick `FUN_80021DF4` | capture (poked state) |
| PROT entry 153 | disc | Mt. Rikuroa field map: 54 bound objects | disc data |
| PROT entry 154 | disc | walk-on triggers; the Golem record on tiles `(69,97)..(69,99)` | disc data |
| PROT entry 157 | disc | Mt. Rikuroa variant MAN: 13 / 29 / 64 records; `P2[47]` is the Golem record | disc data |
| `P1[0]` `+0x136`, `+0x7E0`; `P2[47]` `+0x23` | PROT entry 157 | flag `0x285` test, respawn `44 59`, effect installs `34 30 0D` and `34 30 07` | disc data |
| 97 of 143 at scene entry | RAM | 82 props and NPCs, 7 sentinels, 8 system actors | capture |
| 143 of 143 at the collapse peak | RAM | zero free in 7 of 32 replays; no failed spawn | capture (poked state) |

### Scope

Every field scene, minigame and battle stage spawns through `FUN_80020DE0`, so the message can
appear anywhere the pool runs dry. The counter is cleared only by stage init, so once it shows it
stays until the next stage is set up.

Mt. Rikuroa is singled out by the burst, not by its standing load. By object binds alone the
heaviest field maps are PROT entries 653 (100), 670 (75), 360 (60), 559 and 1226 (58 each), 481
(55) and 153, Mt. Rikuroa (54). Read from the RAM of catalogued save states at rest, `rikuroa` has
44 to 46 slots free, and `ropeway` (15), `garmel` (20 to 27), `koin3` (21) and `dolk` (25) are
tighter.

### Verifying it

Put an execute breakpoint on `0x80020E0C` and play the climb from the mountain entrance through
the Golem. A hit is a failed spawn: the caller's return address is at `sp+0x38` and `s1` is the
template of the actor that did not get a slot. Logging the word at `0x8007C348` every frame shows
the load directly: `0x8E` is an empty pool, `-1` is exhaustion. It needs a save from before the
Golem tile (flag `0x284` clear) in Noa's opening chapter, with Terra following: that is the run
the reports and the recording describe, and the one the poked replay does not reproduce.

Without a debugger, the recording gives the check: beat the Golem, dismiss both of Terra's boxes,
and the text appears about a second later, before the screen fades. It is gone after the next
battle.

### Open questions

- The failed spawn is not captured. The replay ties the limit and does not cross it, so the caller
  is inferred. The recording shows that two requests fail and when, not which.
- What makes a played run at least five actors heavier than the replay at the peak. The replay is
  Vahn alone on a teleported route with an assisted kill; the reported runs are Noa with Terra on
  a walked climb with random battles and a fought Golem. Terra's own actors, parts left by trigger
  records the replay skips, and actors the walked route leaves behind are all untested.
- Why the count is 2 in every report and never 1 or 3.
- The dev text formatter `FUN_8001A068` is not traced for a gate of its own. The recording shows
  it drawing in an emulator; that it draws on a retail console rests on a relayed report.

## No encounters in Jette's Absolute Fortress

**Status:** explained

**Reported as:** in some files Jette's Absolute Fortress has no random battles at all, before
Jette is fought; it survives a reset and a memory-card load, and Incense and the encounter
accessories make no difference (community reports, one first-hand with the route described:
Reddit, r/LegendofLegaia, "Jette's fortress no random encounter").

The cause is in Zora's Floating Castle, hours earlier. When the castle is falling the game gives
six minutes to get out, and it switches random battles off for the last minute by raising a story
flag. The record that ends a successful escape stops the clock but never lowers the flag, so a
file that reached the exit with less than a minute showing keeps it for good.

In the castle that no longer matters. But the Fortress's two encounter tables were built from the
castle's and still begin with the same "this flag means no battles" group. In that file the whole
Fortress is silent, and the Door of Light is greyed out there as well. No item, accessory or reset
changes it, because the flag is saved with the file. A file that escaped with more than a minute
to spare is unaffected.

The flag is one bit in the save and can be cleared; the check and the save-side fix are in a table
below. On the disc side the patcher's `--fortress-encounter-fix` makes the Fortress ignore the
flag, which also repairs a file that already has it. The port does the same at runtime with its
Fortress battles fix, on unless retail-faithful mode holds it off
([gameplay tweaks](../../subsystems/gameplay-tweaks.md#fortress-battles-fix)).

Two other quiet states exist and are intended. The boss room, the room before it, the council and
Mist Generator rooms, the elevators and the bridge never roll. And a second flag, `0x521`,
switches both scenes off - but not when Jette is beaten. It is set in the Mist Generator room
after the Cort fight, as the Fortress starts to collapse, and from there the party is shut in that
room until a cutscene carries it outside. Beating Jette changes nothing about encounters: the walk
from his room to the Mist Generator rolls exactly as the walk in did.

The clock code, the castle's script and the Fortress tables are read from the disc. The flag being
raised by the clock and the Fortress going silent as a result are captured, on an edited save.
One step is inferred here: nobody played the castle escape to its last minute and then read the
flag. A player's own test supplies the observation from the other end - leaving the castle with
under a minute left gives a Fortress with no encounters and no Door of Light (community report,
first-hand) - without the flag byte being read.

### Mechanism

#### How a step is rolled

The roll is `FUN_801D9E1C` in PROT entry 0897 (the field overlay); the full reader is documented
in [`encounter.md`](../../formats/encounter.md#random-encounter-trigger-path). The map is divided
into rectangles, each with its own rate, grouped by story state. The parts that matter here:

1. A step is a change of the player's 128-unit tile by at most one on each axis
   (`0x801D9ED4..0x801D9F0C`).
2. The condition list is walked in order (`0x801D9F30..0x801D9FD8`). Each record is a flag id and
   a region count. The first record whose flag is `0xFFFF` or is set wins, and only its regions
   are searched.
3. Within the winning group, the first region whose box contains the tile wins
   (`0x801D9FE8..0x801DA054`).
4. The step does nothing at all while the player's engaged bit is up, while `0x8007B6B4` is
   non-zero, while the warp timer `0x8007B6B0` is positive, or while the Incense window
   `0x8007B600` is non-zero (`0x801DA130..0x801DA180`). The roll is not reached at all while a
   menu or scripted submode is open (bit `0x8000` of `0x1F800394`, raised at `0x801F1360` and
   dropped at `0x801F1684`), and it does not drain on the one frame the party's battle files
   finish streaming (second argument zero, `0x801DA16C`).
5. The region's rate is scaled by the setting byte `0x8007B5F8`, by the High and Low Encounter
   accessory passives, and by system flags `0x1D` (doubled) and `0x1E` (halved)
   (`0x801DA198..0x801DA204`).
6. The scaled rate is subtracted from the counter `0x8007B5FC`; a battle starts when it reaches
   zero or below (`bgtz` at `0x801DA218`), and the counter is reset to a value between 488 and
   1460 (`0x801DA2D4..0x801DA35C`). The counter is not saved: after power-on it holds the
   `SCUS_942.54` file value 974, so the first battle after a cold load in a rate-18 room is rolled
   on step 55. A counter already at zero starts a battle on the next step even at rate 0.

A rate of zero therefore never moves the counter, whatever scales it.

The Fortress is two scenes, the entrance (`deroa`) and the transport-system interior (`chitei2`).
Their tables, decoded from MAN section 0 of each scene:

| Scene | Order | Flag | Regions in the group | Result |
|---|---|---|---|---|
| `deroa` (PROT entry 606) | 0 | `0x0015` | one, whole map, rate 0 | no encounters |
| | 1 | `0x0521` | one, whole map, rate 0 | no encounters |
| | 2 | `0x046D` | eight: two rate-0 pockets, five rate-18 rooms, rate-0 catch-all | normal |
| | 3 | default | seven: one rate-0 pocket, five rate-18 rooms, rate-0 catch-all | normal |
| `chitei2` (PROT entry 706) | 0 | `0x0015` | one, whole map, rate 0 | no encounters |
| | 1 | `0x0521` | two, both rate 0 | no encounters |
| | 2 | default | sixteen: seven rate-0 boxes (six of them listed first), eight rate-18 boxes, rate-0 catch-all | mixed |

Row 0 of both scenes is the Floating Castle's row carried over: whole map, rate 0, both Doors
blocked, return tile `(0, 0)`. Every other Fortress row allows the Door of Light and returns to
tile `(66, 113)`. Neither scene has a second MAN variant, and both carry 14 formations.

#### Flag `0x15`: the Floating Castle's last minute

1. **The countdown.** Field-VM op `4C D3`
   ([`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md)) arms a countdown kept in
   the saved window. Its handler is at `0x801E2BDC..0x801E2C38` in PROT entry 0897:

   ```
   801e2bdc  jal 0x8003ce9c ; addiu a0,s6,1      ; u16 operand 1
   801e2be8  jal 0x8003ce9c ; move  s1,v0        ; u16 operand 2
   801e2bfc  sll s1,s1,0x10 ; or s1,s1,v0
   801e2c08  sw  s1,0x480(s0)                    ; +0x482 = operand 1, +0x480 = operand 2
   801e2c14  sw  v0,0x478(s0)                    ; u32 operand 3: duration (non-zero = running)
   801e2c1c  sw  v0,0x47c(s0)                    ; u32 operand 4: threshold
   801e2c20  sw  v1,0x460(s0)                    ; remaining = duration
   801e2c2c  jal 0x801d596c                      ; spawn the clock actor
   ```

2. **The clock actor** `0x801D2EBC` takes the play-clock delta off the remaining time each tick
   and raises two system flags by computed id:

   ```
   801d2f44  lw   v0,0x460(s1) ; subu v0,v0,v1 ; sw v0,0x460(s1)
   801d2f50  bgtz v0,0x801d2f68
   801d2f58  lh   a0,0x482(s1) ; sw zero,0x478(s1)
   801d2f60  jal  0x8003ce08   ; andi a0,a0,0xfff     ; expired: set flag "operand 1"
   801d2f68  lw   v0,0x460(s1) ; lw v1,0x47c(s1)
   801d2f74  slt  v0,v0,v1     ; beqz v0,0x801d2f8c
   801d2f80  lw   a0,0x480(s1)
   801d2f84  jal  0x8003ce08   ; andi a0,a0,0xfff     ; remaining < threshold: set flag "operand 2"
   ```

   The flag id is an operand loaded from RAM, not a constant in the code. A search for flag `0x15`
   that covers script ops and constant-id native calls finds only script writers and concludes
   the flag cannot leak; this call is the writer that search misses.
3. **The castle's arming op.** `taiku` (Zora's Floating Castle, variant MAN in PROT entry 373)
   `P2[29]` is the Mist Generator scene that starts the escape. It clears `0x15` (`60 15` at
   `+0x1508`) and then arms the clock at `+0x1533`:
   `4C D3 96 03 15 00 60 54 00 00 10 0E 00 00` - expiry flag `0x396`, warning flag `0x15`,
   duration 21600 ticks (6:00), threshold 3600 ticks (1:00). The castle's encounter table, in both
   `taiku` and `taiku2`, opens with a flag-`0x15` group that is one whole-map rate-0 row. The
   design is that the last minute of the escape has no random battles. The script-side `50 15` in
   `P2[12]` reuses the same switch for the walk from Zora's throne to the Mist Generator.
4. **Nothing clears it on the way out.** The success record `taiku` `P2[16]` opens `53 80`, then
   issues `4C D3` with expiry flag `0x396`, warning flag `0x382` and duration 0, which stops the
   clock, then `53 82`. It never issues `60 15`. The disc-wide script census has exactly one clear
   of flag `0x15`, the one in `P2[29]` that runs before the clock is armed, and no native call
   clears it by constant id. A file that had less than a minute left at any moment of a successful
   escape leaves the castle with `0x15` set.
5. **Where it bites.** Flag `0x15` heads the condition list of exactly three scenes: `taiku2`,
   which is gone with the castle, and `deroa` and `chitei2`. The flag does nothing anywhere else,
   which is why the fault stays hidden from the end of the castle until the Fortress.

The clock is not gentle. MAIN INIT `FUN_8003AEB0` takes 1200 ticks (20 seconds) off a running
clock on every return to the field from a battle (`0x8003B62C..0x8003B648`: `lw a0,0x478(a1)` /
`beqz` / `addiu v0,v0,-0x4b0` / `sw v0,0x460(a1)`). The block sits in the arm the field-entry word
`_DAT_8007B8B8 == 2` selects (`bne v1,2` at `0x8003B510` skips to `0x8003B658` past it), the warp
entry a battle, a minigame or an FMV resume returns through; a door or a scene change enters cold
and pays nothing. Every fight on the way out costs 20 seconds on top of its own length. Loading a
save while a clock is running restores it to its full duration (`0x8003B96C..0x8003B98C`).

#### What the report describes

| Report | Explanation |
|---|---|
| No encounters anywhere in the Fortress | the flag-`0x15` group: one whole-map rate-0 row in both scenes |
| Jette and Cort still there | the file is before Jette; the post-collapse flag `0x521` is not involved |
| Survives a full restart and a memory-card load | the flag bank is in the saved window, save-block `+0x1618` |
| Incense and both Bells (the encounter-rate accessories) change nothing | all three scale or suspend a rate that is already 0 |
| The locked Fortress was visited before Conkram | coincidence: that visit writes nothing that matters; the flag was set in the castle |
| "Sometimes", across the community | it depends on how much time was left on the castle clock |

The early visit, read from the scripts: the overworld entrance `map03` `P2[8]` has no header gate
and changes scene to `deroa`. `deroa`'s entry script `P1[0]` sets its arrival latch `0x49B`,
clears two transient flag bands, sets `0x1F` and clears `0x60E`; its one-shot arms sit behind
flags set after the Cort fight. The gate record `P2[4]` needs flag `0x3E1`, which the Conkram scenes write, so
an early visit does not consume it. The three condition flags (`0x15`, `0x521`, `0x46D`) leave the
visit as they entered it. This is a reading of the scripts; the route was not walked.

#### Check and fix

| | |
|---|---|
| Check, no tools | in the Fortress, open Items: the Door of Light is greyed out. In a healthy file it is usable there |
| Check, RAM | byte `0x8008575A`, bit `0x04` set (system flag `0x15`) |
| Check, memory card | byte `+0x161A` of the save block, bit `0x04` |
| Fix, RAM | clear bit `0x04` of `0x8008575A` once, then save. It stays clear: nothing sets it after the castle |
| Fix, memory card | clear bit `0x04` at save-block `+0x161A` and re-sum the block (`+0x1FFC` is the wrapping sum of the first `0x7FF` little-endian words) |
| Avoid | reach the Floating Castle's exit with more than 1:00 on the clock |

#### The fix

The patcher's `--fortress-encounter-fix`
([`randomizer.md`](../../tooling/randomizer.md#fortress-encounter-fix)) closes it on the disc, and
it heals a save that already carries the flag. In both Fortress scenes it rewrites the leading
condition group - 14 bytes of each scene's MAN, same size - into a duplicate of the scene's own
collapse group: condition 0 tests flag `0x521` instead of `0x15`, and its row becomes a copy of
that group's whole-map row. The Fortress then never reads flag `0x15`. With `0x521` clear the walk
falls through to the same rooms and rates as on a healthy file; with it set it selects a row that
reads the same as the one retail reaches one record later.

The castle is not touched: its last minute stays quiet, the flag is still raised there, and it
still stays set afterwards - with nothing left to read it. The missing op itself, a `60 15` in
`taiku` `P2[16]`, is not added: the castle's MAN ships as a raw chunk of a streamed file, where a
two-byte insertion cannot be made in place.

#### What was captured

PCSX-Redux on the retail disc, cold boot, Continue, card load. The save is a played one from
inside the Fortress, before Jette, standing in the interior's main hall; the pad walks up and down
the hall with breakpoints on the roll and on the flag setter `0x8003CE08`.

| Run | State | Winning row | Result |
|---|---|---|---|
| played | the save as played | `(36,0)..(61,51)`, rate 18 on all 55 steps | counter 974 down to 2, battle rolled |
| edited save | the castle's clock words written into the save block (flag pair `0x03960015` at `+0x480`, threshold 3600, duration 3900), checksum restamped, then a normal cold boot and card load | rate 18 for 23 steps, then the whole-map row | the clock actor calls `FUN_8003CE08` with flag `0x15` from return address `0x801D2F8C` at 3598 ticks left; the next 104 steps drain 0; the Door of Light is blocked |
| RAM poke | byte `0x8008575A` bit `0x04` set | row 0, rate 0 | 128 steps, counter fixed at 974, no battle |
| RAM poke | byte `0x800857FC` bit `0x40` set (flag `0x521`) | the `0x521` row, rate 0 | the same; the Door of Light stays allowed |
| RAM poke | flag `0x15` set and the counter written to 0 | row 0, rate 0 | battle on the first step |

The second run is the important one, and it is **captured on a poked state**: the countdown is put
into a Fortress save by editing four words, because the castle escape was not played to its end.
What it shows is the game's own code acting on those words - the scene loader spawns the clock
actor, it ticks, it makes the flag-set call from the address the disassembly names - and the
Fortress going silent as a direct result.

Played saves support the reading without any poke. The saves of one full playthrough between the
castle and the Fortress's collapse hold `0x03960382` at save-block `+0x480`, the operand pair of
the castle's stop op exactly as the handler stores it; later ones hold the pair of the Fortress's
own stop op. All of that playthrough's post-castle saves have `0x15` clear, and it has normal
Fortress battles (the first run): a file that left the castle with more than a minute to spare.

The last run settles a tempting theory, that the fault is a counter resting at zero: at zero the
`bgtz` at `0x801DA218` falls through and a battle starts, in a zero-rate group as well.

#### Flag `0x521`: the Fortress is collapsing

Flag `0x521` is not a "Jette is beaten" flag. Jette's record sets `0x470` and nothing the roll
reads, so the Fortress rolls the same before and after him. The interior's story runs in this
order, each step a `chitei2` record:

| Step | Record | What happens | Flags it sets |
|---|---|---|---|
| 1 | `P2[11]`, Jette's room | Jette's speech, then the fight (formation row 12) | `0x470` |
| 2 | - | the walk to the council chamber, through rooms that roll at rate 18 | - |
| 3 | `P2[13]`, Mist Generator room | "We must destroy the Mist Generator!", Cort appears, the fight (row 13) | `0x4C4` |
| 4 | `P2[14]` | Cort falls; the scene cuts to the entrance scene and back | - |
| 5 | `P2[15]` or `P2[19]` | "Why did you do it?"; the room starts to shake | `0x521` |
| 6 | `P2[18]`, or the end of `P2[19]` | "The Fortress is collapsing"; a 40-second clock is armed | - |
| 7 | `P2[17]`, tile `(18, 94)` | a boulder seals the room's exit | `0x4C9` |
| 8 | `P2[16]` | about 25 seconds in, Cara arrives and the scene changes to the entrance | `0x1B` |

The only field-script writers of flag `0x521` on the disc are step 5's two records, each opening
with `55 21`; the scene-entry script picks between them on flag `0x510`, and `P2[15]` hands over
to a movie where `P2[19]` plays the collapse lines itself. No record clears the flag.

From step 5 to step 8 the player has control in the Mist Generator room, which is rate 0 in every
group, with the boulder trigger between the party and the way out. The clock at step 6 is the same
op as the castle's, `4C D3` with flag `0x4C7` in both slots, duration 2400 ticks and threshold
910; the scene-entry script runs `P2[16]` when the flag comes up. So inside the interior flag
`0x521` has about 25 seconds to matter and a sealed zero-rate room to matter in: a player does not
get to stand in a room that used to roll. That the boulder cannot be walked around is read from
its wall paint over tiles `(15..19, 92)`, not tested.

Where it does apply is the entrance scene, `deroa`. Its group for flag `0x521` is one whole-map
rate-0 row, where its normal groups have five rate-18 rooms. `P2[16]` ends there, in a long
cutscene with Cara outside the Fortress. `deroa`'s entry script has a branch for the collapsed
state (flag `0x4C8`): it paints a solid wall over tiles `(55..59, 41..42)`, shows two props that
are otherwise parked off the map, and changes the music. The overworld entrance record has no
gate, so the scene reads as open to a later visit, with one passage sealed and no random battles.
That is read from the scripts; the ground was not walked after the collapse.

This is the designers switching the dungeon off, not a defect, and it is not what the report
describes: there Jette is still present.

#### Zero-rate rooms

Laid over the interior's wall grid (the scene `.MAP` at `+0x4000`, PROT entry 703), each zero-rate
box of the default group outlines one room; the "box inside a box" cases are a centre room cut out
of a ring room. Listing them ahead of the boxes that roll is how the table carves them out.

| Region row | Box (x, z) | Rate | Room |
|---|---|---|---|
| 8 | `0..36, 0..46` | 0 | Jette's room: the Jette trigger at `(18, 14)` (`P2[11]`), the three levers and lights, the hidden entrance |
| 10 | `61..80, 25..52` | 0 | the antechamber, the last room before the boss; its doors lead to the main hall and to the room below Jette's |
| 4, 3 | `0..30, 62..126` | 0 | the council chamber and the Mist Generator room, with the Cort confrontation `(18, 89)` (`P2[13]`) and the rockfall `(18, 94)` (`P2[17]`) |
| 7 | `86..103, 0..34` | 0 | the upper elevator shaft |
| 5 | `98..109, 97..109` | 0 | the lower elevator shaft, in the middle of the ring room of row 11 |
| 6 | `86..104, 100..106` | 0 | the three-part bridge to that shaft |
| 9, 11..17 | - | 18 | the main hall (save point at `(48, 38)`), the north room with the second save point, the long west corridor, the twin-tower block, the south-east rooms, the east complex |

The boss room, the room before it, the two cutscene rooms and the moving transport pieces are
zero; every ordinary room and corridor is 18. Measured over the enclosed, non-solid tiles of the
wall grid, 1287 of 3304 interior tiles (39%) are rate 0 in the default state. For the entrance
(PROT entry 603) it is 274 of 3341 (8%), and its one pocket `(48..70, 78..112)` is the transport
boarding room. In the `0x46D` group the extra pocket `(90..112, 47..50)` is a strip of cutscene
triggers.

#### Things that thin battles without stopping them

None of these produces a silent Fortress; they change how long the counter lasts.

- **Stacked rate modifiers.** Rate 18 is 9 with a Low Encounter accessory, and 4 if system flag
  `0x1E` is also set; at rate 4 the counter lasts between 122 and 365 steps. Flags `0x1D` and
  `0x1E` come from the two springs in Rogue's Tower, which is reached after the Fortress, so in a
  normal file neither is set here; each lasts only until the next battle ends. They are traced in
  [their own section](#rogues-tower-water-changes-the-encounter-rate).
- **A top-up on every scene entry.** `FUN_8003AB2C` at `0x8003AC7C..0x8003AC90` adds half of a
  fresh counter roll whenever the counter is below 487 as a scene loads.
- **Incense** suspends the roll outright while its window is open, for at most `0x100` ticks.

The roll's remaining early exits cannot stick either. The formation-count exit (`0x801D9E68`) does
not apply to scenes with 14 formations. The setting byte `0x8007B5F8` is zeroed only by the
developer menu's `ENCOUNT` row (`0x801EAA5C`) and reads 1 in the capture. The menu bit is up only
while a menu or submode is open.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801D9E1C` | PROT entry 0897 | per-step region encounter roll | disassembly |
| `0x801D9F30..0x801D9FD8` | PROT entry 0897 | condition-list walk; first set flag wins | disassembly |
| `0x801D9FE8..0x801DA054` | PROT entry 0897 | region search; first matching box wins | disassembly |
| `0x801DA130..0x801DA180` | PROT entry 0897 | the four "step does not roll" gates | disassembly |
| `0x801DA198..0x801DA204` | PROT entry 0897 | rate scaling: setting byte, two passives, the two water flags | disassembly |
| `0x801DA208..0x801DA21C` | PROT entry 0897 | counter subtract and the `bgtz` test | disassembly |
| `0x801E2BDC..0x801E2C38` | PROT entry 0897 | op `4C D3` handler: arms the countdown | disassembly |
| `0x801D2EBC` | PROT entry 0897 | clock actor; flag sets at `0x801D2F60` (expiry) and `0x801D2F84` (below threshold) | disassembly; capture (edited save) |
| `0x801F1360`, `0x801F1684` | PROT entry 0897 | set and clear of bit `0x8000` of `0x1F800394` by the menu / submode installer `FUN_801F1278` | disassembly |
| `0x8003CE08` | `SCUS_942.54` | system-flag set | disassembly |
| `0x8003CE64` | `SCUS_942.54` | system-flag test: byte `0x80085758 + (id >> 3)`, mask `0x80 >> (id & 7)` | disassembly |
| `0x8003B62C..0x8003B648` | `SCUS_942.54` | MAIN INIT's warp-return arm: 1200 ticks off a running clock per return from battle | disassembly |
| `0x8003B96C..0x8003B98C` | `SCUS_942.54` | card load restores a running clock to its full duration | disassembly; capture (edited save) |
| `0x8003AC7C..0x8003AC90` | `SCUS_942.54` | scene-entry counter top-up in `FUN_8003AB2C` | disassembly |
| `0x800845A0`, `0x800845B8`, `0x800845BC`, `0x800845C0` | RAM | clock: remaining, duration, threshold, flag pair; save-block `+0x460`, `+0x478`, `+0x47C`, `+0x480` | disassembly; capture (played saves) |
| `0x8007B5FC` | RAM | encounter step counter, signed word; not saved, 974 after power-on | disassembly; capture |
| `0x8007B5F8` | RAM | rate setting byte | disassembly |
| `0x8007B600` | RAM | Incense window | disassembly |
| `0x8008575A` mask `0x04` | RAM | system flag `0x15`; save-block `+0x161A` | disassembly |
| `0x800857FC` mask `0x40` | RAM | system flag `0x521` | disassembly |
| PROT entry 373, `P2[29]` `+0x1508`, `+0x1533` | disc | `taiku`: the one clear of flag `0x15`, then the arming op (6:00, warning flag `0x15` at 1:00) | disc data |
| PROT entry 373, `P2[16]` `+0x13` | disc | `taiku`: escape success; stops the clock, no clear of `0x15` | disc data |
| PROT entry 373, `P2[12]` | disc | `taiku`: the script-side set of flag `0x15` (`50 15`) | disc data |
| PROT entry 606, MAN offset `0x5B4B` | disc | `deroa` encounter section, `0x152` bytes | disc data |
| PROT entry 706, MAN offset `0x6A1A` | disc | `chitei2` encounter section, `0x166` bytes | disc data |
| PROT entry 706, `P2[15]` and `P2[19]` | disc | the two setters of flag `0x521`: the scene after the Cort fight | disc data |
| PROT entry 706, `P2[11]`, `P2[13]` | disc | the Jette fight (row 12, sets `0x470`) and the Cort fight (row 13, sets `0x4C4`) | disc data |
| PROT entry 706, `P2[18]` `+0x3DD`, `P2[19]` `+0x42D` | disc | the collapse clock: `4C D3`, flag `0x4C7` twice, 2400 ticks, threshold 910 | disc data |
| PROT entry 706, `P2[17]`, `P2[16]` | disc | the boulder at tile `(18, 94)`; Cara's arrival and the change to `deroa` | disc data |
| PROT entry 606, `P1[0]` `+0x58..+0x72` | disc | `deroa` entry script, collapsed state: wall paint over `(55..59, 41..42)`, music | disc data |
| PROT entries 703, 603 | disc | wall grids of `chitei2` and `deroa`: 39% and 8% of enclosed floor is rate 0 | disc data |
| flag `0x15` set from `0x801D2F84`, Fortress silent | RAM | the clock raising the flag and the roll selecting row 0 | capture (edited save) |
| Fortress rolls at rate 18 | RAM | the control: a played pre-Jette save | capture |
| a real escape under 1:00 leaves the flag set | - | the flag read is not observed; the silent Fortress and blocked Door of Light after such an escape are | inference; community report |
| the entrance scene is silent after the collapse | - | read from the table and the scripts, not walked | inference |

### Scope

A story-gated group that is a single zero-rate row is the game's ordinary way of switching a
dungeon off, and it is used across the disc. Flag `0x15` gates only the three scenes named above,
so the leak shows in the Fortress and nowhere else. The clock actor is a general mechanism: the
Fortress's own collapse timer uses it with flag `0x4C7`. The two water flags are a
separate mechanism with [their own section](#rogues-tower-water-changes-the-encounter-rate).

### Verifying it

On an affected file, read byte `0x8008575A` (or save-block `+0x161A`): bit `0x04` is set. In the
Fortress, break at `0x801DA208`: register `s0` holds the final per-step rate and `s2` points at the
region record that won; the branch at `0x801D9FC8` shows which condition group was selected.

To see the cause itself, play the Floating Castle escape, let the clock fall below 1:00 with a
breakpoint at `0x801D2F84`, reach the exit, and read byte `0x8008575A` on the overworld.

### Open questions

- The end-to-end observation: a played castle escape that finishes under 1:00 and a save that then
  shows the flag. A player's test confirms the outcome (no encounters, no Door of Light); the bit
  itself, `0x04` at save-block `+0x161A` in such a file, has not been read.
- A native clear of flag `0x15` through a computed id is not excluded by the constant-id sweep;
  none of the computed clear sites reads as touching it.
- The locked early visit is read from the scripts, not walked.
- Whether the entrance scene is walked again once the collapse flag is set: after the closing
  cutscene, or on a later visit from the overworld.

## Rogue's Tower water changes the encounter rate

**Status:** explained

**Reported as:** not a glitch report. The roll traced for the Fortress reads two more flags, and
the question came back from the community: which water does what, does it last, and why is no
difference visible in an old save. ZetaPhoenix then observed that of the three bits the springs
set, only the shared one survives a random battle (community report, first-hand), which the
capture below confirms.

Rogue's Tower has two springs side by side. Examining one shows "It's strange green water!" or
"It's strange blue water!" and asks whether to drink. Either one fully restores the party's HP and
MP. Each also sets a flag that the random-battle roll reads on every step, **until the next random
battle is over**:

| Water | Flag | Effect on the roll | Steps to the next battle in the tower's rate-20 rooms |
|---|---|---|---|
| neither | - | rate as written | 25 to 73, 49 on average |
| green | `0x1D` | rate doubled: the next battle comes in half the steps | 13 to 37, 25 on average |
| blue | `0x1E` | rate halved: the next battle takes twice the steps | 49 to 146, 98 on average |
| both | both | doubled, then halved: exactly the written rate | as with neither |

When the field comes back after a battle, the game clears both water flags. The drink is a
one-battle charm: green pulls the next fight closer, blue pushes it away, and from the fight after
that on the rate is back to normal. Drinking again re-arms it for one more battle, and the springs
work on every visit. That is why an old save shows no difference: whatever was drunk wore off at
its next battle.

Both springs also set a third flag, `0x1C`, which nothing ever clears. It is the switch for the
clean-up: the battle-return code clears the two water flags only when `0x1C` is set, and `0x1C` is
only ever set together with one of them. A file that has drunk once keeps `0x1C` for good, and
the bits read `0x08` after any later battle.

The effect is real but easy to miss even inside its one battle. The step count between battles is
re-rolled over a three-to-one range after every fight, so the bands for "normal" and "green"
overlap, and so do "normal" and "blue". A fair test is to drink, count steps to the next battle,
and repeat many times, or to read the rate in a debugger.

The roll and both springs' scripts are read from the disc. The clean-up is **captured on a poked
state**: the two water bits were written into a Rogue's Tower save, a random battle was forced
and won, and the write that cleared them was logged on the return to the field.

### Mechanism

#### The scaling code

The tail of the rate scaling in `FUN_801D9E1C` (PROT entry 0897), with the rate in `s0`:

```
801da1e0  jal  0x8003ce64 ; li a0,0x1d      ; test system flag 0x1D
801da1e8  beqz v0,0x801da1f4
801da1f0  sll  s0,s0,0x1                    ; set: rate * 2
801da1f4  jal  0x8003ce64 ; li a0,0x1e      ; test system flag 0x1E
801da1fc  beqz v0,0x801da208 ; andi v0,s0,0xffff
801da204  srl  s0,v0,0x1                    ; set: rate / 2
801da208  lw   v1,-0x4a04(a0)               ; the step counter 0x8007B5FC
801da214  subu v1,v1,v0                     ; counter -= rate
801da218  bgtz v1,0x801da360                ; still positive: no battle
```

The rate is what one step takes off the counter, so a larger rate means more battles. The counter
restarts between 488 and 1460 after each battle, 974 on average
([`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md#encounter-step-counter-reroll-fun_801ddf48)),
and a battle comes on step `ceil(counter / rate)`. Doubling the rate halves that step count and
halving it doubles it.

The two tests are independent and run in that order. With both flags set the rate is shifted left
and then right by one bit, which returns the same number for every rate, odd or even. There is no
cap and no floor other than the integer shift: a rate of 1 halves to 0, and a rate of 0 stays 0
whatever is set.

The same block applies the accessories first: High Encounter (Bad Luck Bell, Nemesis Gem)
multiplies by four at `0x801DA1C8` and Low Encounter (Good Luck Bell, Evil Talisman) halves at
`0x801DA1DC`. Everything stacks by plain shifts. Blue water with a Low Encounter accessory is a
quarter of the written rate; green water with one is the written rate; green water with a High
Encounter accessory is eight times it.

`FUN_801D9E1C` is the roll for every field scene and for the overworld
([`world-map.md`](../../subsystems/world-map.md)), so while a flag is set the scaling is not local
to the tower. It does nothing in rooms whose rate is 0, to scripted fights, or while Incense holds
the roll.

#### The clean-up after a battle

The scene-entry map init `FUN_8003AEB0` in `SCUS_942.54` has an arm for a field re-entry that
stays in the current scene (`_DAT_8007B8B8 == 2`, tested at `0x8003B504..0x8003B510`), which is
how the field resumes after a battle. With `a0 = 0x80084140`, the base of the flag bank at
`+0x1618`:

```
8003b528  li   a3,-0x3                      ; mask 0xFD
8003b538  addiu a2,a0,0x3                   ; flag byte 3: flags 0x18..0x1F
8003b544  lbu  v1,0x1618(a2)
8003b54c  andi v0,v1,0x8                    ; flag 0x1C set?
8003b55c  beq  a1,zero,0x8003b56c           ; no: leave the byte alone
8003b560  _andi v0,v1,0xfb                  ; clear 0x04, flag 0x1D (green)
8003b564  and  v0,v0,a3                     ; clear 0x02, flag 0x1E (blue)
8003b568  sb   v0,0x1618(a2)
```

The same arm clears flag `0x0E` one byte earlier (`0x8003B540`) with no condition. The arm runs on
every re-entry of that kind, not only after a random battle; a scene change through a door takes
the other side of the `0x8003B510` branch and leaves the water flags alone (read from the branch,
not captured).

#### The two springs

Both are partition-1 placement records of `rugi` (Rogue's Tower, PROT entry 697). The developer
name in each record's header is Shift-JIS for "green water" (record 29) and "blue water" (record
30), matching the English line.

| | `P1[29]` | `P1[30]` |
|---|---|---|
| First line | "It's strange green water!" | "It's strange blue water!" |
| Placement | tile `(108, 89)` | tile `(105.5, 89)`, two and a half tiles west of it |
| Flag writes after "Yes" | `50 1C`, `50 1D` at `+0xF1` | `50 1C`, `50 1E` at `+0xF0` |
| Other effect | `4C 82 00`, `4C 82 01`, `4C 82 02`: full HP and MP for party slots 0 to 2 | the same |

The records are otherwise the same script: the prompt, a two-way picker, the drinking animation,
the three restores, the two flag writes, a fade, "What an amazing taste!" and "HP and MP are
completely restored!", then a jump back to the prompt's entry so the spring can be used again.
Answering "No" writes nothing. Neither record tests a flag anywhere, so nothing gates a spring
and it works on every visit, any number of times.

Both springs stand in the part of the tower that rolls at rate 20. The colour is taken from the
script text and the record names; the water's texture was not examined.

#### Who else touches the flags

| Search | Flag `0x1C` | Flag `0x1D` | Flag `0x1E` |
|---|---|---|---|
| Field-script set, clear and test ops in every scene MAN and variant | both springs set it | one set, `P1[29]` | one set, `P1[30]` |
| Motion-VM flag ops in every scene MAN | none | none | none |
| Native calls to the flag helpers with a constant id | none | one test, `0x801DA1E0` | one test, `0x801DA1F4` |
| Inline byte access to the bank in code | the test at `0x8003B54C` | the clear at `0x8003B568` | the clear at `0x8003B568` |

The clean-up is an inline `lbu` / `sb` on the bank through a base register pointed at byte 3, so
a search for calls to the flag helpers, or for the absolute address of the byte, does not find it.

The flag bank is inside the saved window:

| | Flag `0x1C` | Green, flag `0x1D` | Blue, flag `0x1E` |
|---|---|---|---|
| RAM | byte `0x8008575B`, bit `0x08` | same byte, bit `0x04` | same byte, bit `0x02` |
| Memory card | save-block `+0x161B`, bit `0x08` | same byte, bit `0x04` | same byte, bit `0x02` |

The same byte holds flag `0x1F` (bit `0x01`), which scene scripts set and clear all the time, so
test the bits and do not compare the whole byte. Every late-game save state in the project's
library reads `0x08` in this byte, never `0x0C` or `0x0A`: a file that drank, after its next
battle.

### What was captured

Probe `scripts/pcsx-redux/autorun_water_flag_clear.lua` on the catalogued state
`rugi_field_card_boot` (Rogue's Tower, rate-20 region, a card-loaded endgame file whose byte reads
`0x08`). **Poked:** the byte raised to `0x0E` and the step counter set to 1 once the state runs.
The walk, the battle entry, the fight and the return are the game's own code; the fight was won by
holding the monsters at 1 HP while Cross was pressed.

| Moment | Byte `0x8008575B` | Game mode |
|---|---|---|
| after the poke, walking | `0x0E` | `0x03` field |
| battle entry and the whole fight | `0x0E` | `0x09`, then `0x15` battle |
| return to the field, before the arm | `0x0E` | `0x02` field init |
| after `sb` at `0x8003B568` (caller `0x801D6DB0`) | `0x08` | `0x02` |
| field resumed, to the end of the run | `0x08` | `0x03` |

No other write touched the byte during the run.

ZetaPhoenix's in-game tests in Bio Castle agree with the gate and add the escape case
(community report, first-hand; bits named here by mask). The tests used the blue-water bit; the
green bit is expected to behave the same but was not tested:

| Byte before the battle | Won | Escaped | Escaped with the Spikefish Flute |
|---|---|---|---|
| `0x08` and `0x02` set (`0x0A`) | `0x02` cleared | `0x02` cleared | the game freezes |
| `0x02` alone | `0x02` stays | `0x02` stays | `0x02` stays |

A plain escape returns through the same clean-up as a win, and so does a flute escape that gets
that far. The freeze is real, but it is not caused by the flag byte: it is a separate glitch of the
Spikefish Flute, traced in
[its own section](#spikefish-flute-escape-can-freeze-the-game).

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801DA1B8..0x801DA1DC` | PROT entry 0897 | High Encounter passive `0x3B` (rate * 4), Low Encounter passive `0x3C` (rate / 2) | disassembly |
| `0x801DA1E0..0x801DA1F0` | PROT entry 0897 | flag `0x1D` test; rate * 2 | disassembly |
| `0x801DA1F4..0x801DA204` | PROT entry 0897 | flag `0x1E` test; rate / 2 | disassembly |
| `0x801DA208..0x801DA21C` | PROT entry 0897 | counter subtract; `s0` is the final rate | disassembly |
| `0x8003B504..0x8003B510` | `SCUS_942.54` | `FUN_8003AEB0`: the same-scene re-entry arm, `_DAT_8007B8B8 == 2` | disassembly |
| `0x8003B544..0x8003B568` | `SCUS_942.54` | clears flags `0x1D` and `0x1E` when flag `0x1C` is set | disassembly, capture |
| `0x8008575B` masks `0x08`, `0x04`, `0x02` | RAM | system flags `0x1C`, `0x1D`, `0x1E`; save-block `+0x161B` | disassembly, capture |
| PROT entry 697, `P1[29]` `+0xF1` | disc | green water: `50 1C`, `50 1D` after three `4C 82` restores | disc data |
| PROT entry 697, `P1[30]` `+0xF0` | disc | blue water: `50 1C`, `50 1E` after three `4C 82` restores | disc data |
| PROT entry 697, MAN section 0 | disc | tower regions: `(0..58, 0..72)` rate 0, the rest rate 20 | disc data |

### Scope

Every scene with random battles and the overworld, from the drink to the end of the next battle;
after a drink in the tower that battle is almost always in the tower. No other flag is read by the
scaling code, and no other prop on the disc sets these two.

### Verifying it

Read byte `0x8008575B` before and after drinking: green raises bit `0x04`, blue raises bit `0x02`,
and either raises bit `0x08`. Break at `0x801DA208` while walking a room that rolls: `s0` is the
per-step rate, 20 in the tower with neither or both, 40 with green only and 10 with blue only.
Win the next battle and read the byte again: `0x04` and `0x02` are gone and `0x08` stays. A write
watch on the byte catches the clear at `0x8003B568`.

### Open questions

- Which other same-scene re-entries (`_DAT_8007B8B8 == 2`) besides a battle return reach the
  clean-up, for example a scripted fight or a cutscene that resumes in place.
- Which spring is on which side on screen: the placements give tiles, and the camera decides left
  and right.
- Whether the colour is also visible in the water's texture.

## Spikefish Flute escape can freeze the game

**Status:** explained

**Reported as:** escaping a battle in Bio Castle with the Spikefish Flute freezes the game, first
noticed while testing the Rogue's Tower water flags. Narrowed down further: escaping a fight with
three Gillium Lv3 with the flute always freezes, right after the escape sequence (community
report, first-hand, ZetaPhoenix). The captured fight below is exactly that formation.

The freeze is **captured on a poked state** (pokes listed below). It reproduces in Bio Castle with
every value of the water byte tried, so the water flags play no part. The cause is the flute's own
on-screen caption.

The flute's attack shows a caption with the attack's name, "Blowfish". The flute's escape skips
the end of its own attack, so the caption is never taken down. It stays on screen through the
escape and keeps pointing at its text after the battle has freed that memory. When the game reads
the field's data back into the freed memory, the caption's "text" becomes picture data. Drawing
it as text can send the text routine into a copy that never ends, and the game locks up on the
black screen after the escape. Whether a given fight hits that depends on where the caption's
text landed and on what overwrites it, not on the water.

### The runs

State: a cold boot of the retail disc plus a memory-card load of the Bio Castle save
(`playthrough-endgame-7saves.mcr` block 6, scene `jouind`), checkpointed in the field by
`autorun_boot_continue.lua`. **Poked** once the field runs: bits `0x0E` of byte `0x8008575B` set
to the value below (bit `0x01` kept), bag slot 0 set to the Spikefish Flute (`0x99`) x9, and the
step counter `0x8007B5FC` set to 1. The walk, the battle (three Gillium Lv3, monster id 161 = `0xA1`, `Gilium $3` in the archive), the commands
and the return are the game's own code, driven by the pad. Probe
`scripts/pcsx-redux/autorun_spikefish_flee_return.lua` on the fast core.

| Scene | Byte | Escape | Result |
|---|---|---|---|
| `jouind` | `0x0A` | Spikefish Flute | stops 165 vsyncs after the teardown, still in battle mode; two runs |
| `jouind` | `0x02` | Spikefish Flute | the same |
| `jouind` | `0x08` | Spikefish Flute | the same |
| `jouind` | `0x0C` | Spikefish Flute | the same |
| `jouind` | `0x00` | Spikefish Flute | the same |
| `jouind` | `0x0A` | Run | back in the field; the byte reads `0x08` |
| `rugi` | `0x0A` | Spikefish Flute | back in the field; the byte reads `0x08` |

Six more `jouind` runs had the formation rows rewritten in RAM before the forced battle (a seventh
poke), to test the prediction below. The byte was `0x0A` in each:

| Formation forced in `jouind` | Field-back read | Predicted | Result |
|---|---|---|---|
| three Gillium Lv3 (161), the scene's own row | 127 sectors | freeze | freezes |
| three Puera Lv3 (158), the scene's other row | 115 | survive | back in the field |
| one Gimard Lv3 (12) | 112 | survive | back in the field |
| three Gimard Lv3 (12) | 112 | survive | back in the field |
| three Gizam Lv3 (57) | 115 | survive | back in the field |
| three Barra Lv3 (143) | 116 | freeze | freezes |
| Aluru (80) + Orb Lv2 (84) | 134 | freeze | freezes |

The `rugi` row uses the catalogued `rugi_field_card_boot` state with the same pokes. It confirms
that a flute escape which survives reaches the same clean-up as a plain one. Every Bio Castle
flute run takes the same frames, so the flute's flee always lands at action state `0x65` on frame
2732 or 2733.

The runs disagree with one row of the community table: the blue bit alone also freezes here. The
table's runs were each a different fight, so a fight that lays out memory differently can survive.

### The mechanism

1. **The caption outlives the attack** (capture). While the flute's attack runs, the retained
   widget list `FUN_80031D00` draws a text widget every frame: `jal 0x80036888` at `0x800321E0`,
   string pointer at widget `+0x18`. In this fight the string is `"Blowfish"` at heap address
   `0x800B995C`. Arm 8 of `FUN_801F6A00` in PROT entry 0925 moves the action state machine to the
   run wait `0x65` and never removes that widget. The caption is still drawn every frame through
   states `0x65`, `0x66` and `0x67`, beside the "successfully escaped" banner.
2. **Its text is overwritten** (capture, disassembly). At state `0x67` the teardown in
   `FUN_8004E568` frees the enemy record buffers and the side-band stream buffer, and the loader-B
   id `0x8007BC4C` goes from `0x1E` (PROT 0925) to `0x53` (PROT 0978, the field-back-read module).
   PROT 0978 re-reads the scene's own data from disc into the largest free heap block, which now
   covers the caption. From then on the widget's string at `0x800B995C` reads
   `f8 f2 f2 ff fa ef da ...`, which is `jouind`'s PROT entry 681 at offset `0xE4BC`, with no zero
   byte nearby.
3. **The text routine never comes back** (capture, interpreter core). The renderer `FUN_80036888`
   hands that "string" to the substitution expander `FUN_80036514`. The expander's first pass
   rewrites each `0xFF` in the source to `0xCF` in place, so it also corrupts the freed memory it
   reads. The copy loop at `0x800367E4` then follows a substitution code whose source pointer
   `t0` lands in the expander's own output buffer (from `0x800740EC`). The read cursor trails the
   write cursor by `0x1FE` bytes and never meets a zero. The copy runs on through `0x80074723`,
   `0x80075E93` and `0x80079543` (logged every 2000 passes), overwriting everything after the
   buffer. Under the fast core the emulator stops with "Unrecoverable error while running
   recompiler" at pc `0x80036868`, the loop's tail. On hardware the game hangs.

A plain escape never installs the caption, so it cannot hit this. The water flags are read only
after the battle has ended, by which time the copy has already locked up.

### Why one fight and not another

The outcome is a fixed function of the scene and the formation's distinct monster species. Three
links make it so.

**Where the caption text sits** (capture). The caption string is the attack name inside the
Spikefish's actor record. That record is streamed into the side-band slot at the bottom of the
enemy stager buffer, so the text sits at `0x800B990C + 0x50` = `0x800B995C`. The stager buffer
(`0x2E390` bytes) sits at `0x800B9900` in every captured fight, in both scenes.

**How much the field-back read loads** (disassembly, capture). PROT 0978's routine `0x801F6B24`
works in three steps:

1. It takes the largest free heap block (`FUN_8002B790`, at `0x801F6F08`), counts whole sectors
   in it, and caps the count at `0xC5`.
2. It allocates that many sectors less 4 bytes (`FUN_80017888`, at `0x801F6F74`). The best-fit
   allocator `FUN_8002B468` cuts the allocation from the top of the block, so the buffer ends at
   `0x800E7C9C`, where the freed stager buffer ended.
3. It reads that many sectors from 32 sectors into the scene's first PROT entry (`0x801F726C..`
   `0x801F7298`), the scene's own field data.

After the teardown the largest free block is the stager plus the monster blocks allocated directly
below it. There is one monster block per distinct species, each the species' battle-heap cost
(`block[+0x08]` of its PROT 867 record) plus a 12-byte header:

```
sectors = min((0x2E390 + 0xA8 + sum(cost + 12 over distinct species)) / 0x800, 0xC5)
caption reads scene_data[sectors * 0x800 - 0x2E344]   (reached only from 93 sectors up)
```

The `0xA8` is fitted: it is one more freed `0x9C` node, and without it Gizam Lv3 predicts 114
sectors where the capture reads 115. The formula gives the measured count for all eight captured
fights, in both scenes.

**What the text routine does with those bytes** (simulation). The script
`scripts/asset-investigation/spikefish_caption_predict.py` places the scene's disc bytes at the
caption and runs the renderer and expander logic from the disassembly over them:

- a first byte below `0x20` draws nothing;
- otherwise it follows the normalisation pass, the token count, the `C1..C7` substitution table at
  `0x80010EC0` and the nested name copy.

A copy that runs past 64 KiB, or touches an address outside RAM and its mirrors (a bus error on
hardware), is called a freeze. It needs a battle RAM image as a backdrop for the tables it
dereferences; two different backdrops give identical results. The script agrees with all eight
captured outcomes, among them the three Barra Lv3 and Aluru + Orb Lv2 freezes, which were predicted
before those runs.

#### Predicted freezes

The script was run over every random formation row (encounter regions with a non-zero rate) of the
bundle MAN of every scene in the scene table: 406 rows in 51 scenes. It predicts a freeze for 14
rows; every other row survives:

| Scene | Formation | Sectors | Failure |
|---|---|---|---|
| `jouind`, Bio Castle (D) | three Gillium Lv3 | 127 | runaway copy (captured) |
| `jouinc`, Bio Castle (C) | three Barra Lv3 | 116 | bus error (the same fight froze when forced in `jouind`) |
| `jouina`, Bio Castle (A) | three Zenoir Lv3 | 114 | runaway copy |
| `jouina`, Bio Castle (A) | three Gola Gola Lv3 | 110 | bus error |
| `cave01`, Snowdrift Cave | one Mushroom; one Blue Piura | 110 | runaway copy |
| `map01`, world map (Drake Kingdom) | one or two Gobu Gobu | 117 | runaway copy |
| `dohaty`, Dohati's Castle | Aluru + Orb Lv2; Nova Lv2 + Gola Gola | 134 | runaway copy |
| `tunnelb`, Fire Path | Nova + Gel Frog | 139 | bus error |
| `tunnelb`, Fire Path | Stone Lizard + two Warman | 134 | runaway copy |
| `nilboa`, Nivora Ravine | two Amethyst + Neo Grude | 131 | bus error |
| `nilboa`, Nivora Ravine | one Amethyst | 102 | runaway copy |

Three rows sit within the fit's error of a sector boundary, where the neighbouring count gives the
other answer:

- Fire Path two Nova (117; 116 would freeze);
- Sol Tower `koin2` Spoon + Shade (141; 142 would freeze);
- Fire Path Stone Lizard + two Warman (134; 135 survives).

The same formation can differ between rooms, because the bytes come from each room's own data:
three Gillium Lv3 freezes in Bio Castle (D) but is predicted to survive in Bio Castle (E)
(`jouine`).

That fits both community reports. The three-Gillium-Lv3 freeze is first-hand. A Bio Castle flute
escape that did not freeze is consistent with three Puera Lv3, the room's other formation, which
survives here.

The first two rows of the community table are first-hand play. The freeze rows above are
**captured on a poked state** (the byte, the bag slot and the step counter, plus the formation
row for the forced formations). Probe `scripts/pcsx-redux/autorun_spikefish_text_hang.lua` replays
the end of a run on the interpreter from the escape-wait checkpoint, logging the renderer calls and
the runaway loop.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F7400`, `0x801F78C4..0x801F78F8` | PROT entry 0925 | Spikefish Flute flee: outcome, survived latch, escape counter, `ctx[7] = 0x65` | disassembly |
| `0x801E7B5C..0x801E7B84`, `0x801E8028..0x801E802C` | PROT entry 0898 | the run command's matching success stores in `FUN_801E791C` | disassembly |
| `0x80046DD8..0x80046E60` | `SCUS_942.54` | battle-exit mode choice in `FUN_80046A20`; reads no escape kind | disassembly |
| `0x800321E0` | `SCUS_942.54` | `FUN_80031D00`: the retained text widget's draw, string at widget `+0x18` | disassembly, capture |
| `0x80036514`, loop `0x800367E4..0x80036868` | `SCUS_942.54` | substitution expander; the runaway copy into and past `0x800740EC` | disassembly, capture |

### Scope

Any Spikefish Flute escape leaves the caption behind; whether it hangs is fixed by the scene and
the fight's distinct species. The predicted freezes are the eleven scene-and-fight pairs in the
table above (simulated; three of them also captured in `jouind`). Everything else on the random
tables survives. A plain Run escape never shows the caption and is safe everywhere. The prediction
was measured with one party (the endgame card's Vahn, Noa and Gala). It covers only the bundle MAN
of each scene, not the streamed MAN variants a scene swaps in at later story points.

### Verifying it

In a battle in Bio Castle, escape with the Spikefish Flute. Before the freeze, the "Blowfish"
caption is still on screen next to the "successfully escaped" banner. In a debugger, break at
`0x800367E4` once the action state reaches `0x67`: the loop keeps running, its write cursor
climbing past `0x800740EC`. Escaping the same fight with Run returns to the field.

### Open questions

- Whether the party (members, equipment) moves the stager or the free block. Every capture used
  one party.
- The `0xA8` slack is fitted, not traced to the node it stands for.
- The world-map rows (Drake Kingdom, Gobu Gobu) assume the world map's field-back read is the same
  as a field scene's; no world-map flute escape was captured.
- Formations in the streamed MAN variants are not evaluated.
- Whether other summon-flute or item captions survive an escape the same way.

## Gaza 2 softlock

**Status:** explained

**Evidence:** two parks caught in human play on Gaza 2 (`capture`, played); one park
reproduced from scripted pad input with Gaza's HP and MP poked lower (`capture`, poked); the
mechanism (`disassembly`).

**Reported as:** in the Gaza rematch the battle stops and the camera circles the party forever
(community report).

This one is traced in full on its own pages:
[`battle-action-exit-gates.md`](../../subsystems/battle-action-exit-gates.md#the-0x19-attack-approach-park)
is the source of record. What follows is a summary and an independent re-read of its key
addresses.

A monster that starts a melee attack out of reach has to walk to its target. Almost no monster has
a dedicated approach animation, so the game plays the ordinary Move loop and waits in a state that
only re-measures the distance. That state has no movement code and no timeout: the animation's own
forward motion is what carries the monster in.

A hit sets a "go back to idle when this clip ends" flag on the monster it strikes. Normally the
flinch plays out, the flag does its job and is gone before anyone else acts. The Seru spell Kemaro
is the exception: it carries its target off, flinches it there, and when it puts the target back
it starts the idle pose in a way that leaves the flag set. The flag then waits for the next clip
on that monster to end. If that monster's next move is a melee attack from out of reach, the flag
cuts the Move loop off after one cycle, the monster stops short of its target, and the wait never
ends. The camera's idle sweep runs on its own, which is why the screen keeps orbiting.

### Mechanism

1. In the action state machine `FUN_801E295C` in PROT entry 0898, state `0x14` runs the range
   check `FUN_8004E2F0`. Out of range, for a monster, it searches the monster's action table for
   tag `0x20` (`FUN_80050E2C`). When the tag is missing it stages the tag-1 Move clip into
   `actor+0x1DA` and enters state `0x19`.
2. State `0x19` re-derives facing, repeats the range check and moves on to the strike only when
   the check passes. Otherwise it falls to the stall label `0x801E35D0`, which only adds to a
   counter.
3. The per-actor tick `FUN_80047430` applies the playing clip's root motion. At a clip's natural
   end it tests bit 2 of `actor+0x1DC`; when set it queues idle and clears bits 0..2. The mid-clip
   commit path clears bits 0..1 only and keeps bit 2.
4. Bit 2 is raised by every hit-reaction stager. An ordinary reaction ends naturally before the
   action is over and takes the bit with it. Kemaro (PROT 0918) stages its target's reaction with
   `+0x1DC |= 5` at `0x801F80BC` and pins that clip on frame 3 by zeroing the target's speed scale
   `+0x21D` (`0x801F8370..0x801F8380`), so it cannot end while carried. It then puts the target
   back by storing `0` to `+0x1DA` and adding
   1 to `+0x1DC` (`0x801F897C..0x801F8984`): idle is installed through the mid-clip path, bit 2
   survives, and it lasts until the idle loop's first natural end. A melee inside that window
   installs the Move clip through the mid-clip path with bit 2 still up; the first natural end
   installs idle; the monster is parked.

The four address claims the explanation rests on, re-read from the disassembly:

| Claim | Instructions | Holds |
|---|---|---|
| walk-tag fallback, `0x801E31F4..0x801E32DC` | `jal 0x8004e2f0` at `0x801E31F4`; `li a1,0x20` and `jal 0x80050e2c` at `0x801E3260` / `0x801E3268`; `bne v0,v1` against `0xFF` at `0x801E327C`; `li a1,0x1` and the second scan at `0x801E329C` / `0x801E32A4`; `sb v0,0x1da(s3)` at `0x801E32B0`; state `0x19` stored at `0x801E32D0` | yes |
| natural end installs idle | `andi v0,v0,0x4` at `0x80047B38`, `sb zero,0x1da(s2)` at `0x80047B44`, `andi v0,v0,0xf8` at `0x80047B50`; the mid-clip path masks with `0xfc` at `0x80047A38` | yes |
| the flinch raises bit 2 | `lbu v0,0x1ef(a0)` and `sb v0,0x1da(a0)` at `0x80042124` / `0x8004212C`; `ori v0,v0,0x4` at `0x80042140`; `ori v0,v0,0x1` at `0x8004216C` | yes |
| the patch site is nine redundant words | `0x801E3568..0x801E3588` is the `0x19` arm's facing recompute, nine instructions ending in `sh v0,0x46(s3)`, followed at `0x801E358C` by the range poll; `0x801E3568 - 0x801CE818 = 0x14D50` | yes |

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801E295C` | PROT entry 0898 | battle action state machine; states `0x14` / `0x19` at `0x801E31F4..0x801E35D0` | disassembly |
| `0x801E3568` | PROT entry 0898 | nine-word patch site, overlay offset `+0x14D50` | disassembly |
| `0x8004E2F0` | `SCUS_942.54` | range check | repo doc |
| `0x80050E2C` | `SCUS_942.54` | action-tag scan over the monster record | repo doc |
| `0x80047430` | `SCUS_942.54` | per-actor tick; idle install at `0x80047B44` | disassembly |
| `0x80042124..0x80042170` | `SCUS_942.54` | hit-reaction stager in `FUN_800402F4` | disassembly |
| `0x801F80A0..0x801F80BC`, `0x801F895C..0x801F8984` | PROT entry 0918 (Kemaro) | target reaction staging; put-back that keeps bit 2 | disassembly, capture |
| actor `+0x1D9`, `+0x1DA`, `+0x1DC` | RAM | playing clip, queued clip, anim event flags | disassembly |
| `*0x8007BD24`, `+0x07` | RAM | battle context, action state byte | repo doc |

### Scope

Narrower than "any flinched monster". 180 of the 186 monster records lack a tag-`0x20` action,
but that is only the first of four conditions, all of which have to hold:

- the attacker has no tag-`0x20` action (180 records);
- bit 2 is still set when its approach starts - in retail play that takes Kemaro, whose put-back
  is the only one that keeps the bit (a hit alone always clears it before the action ends);
- its melee comes inside one idle cycle of the put-back, which in practice means directly after
  the Kemaro in the same round;
- one Move cycle does not carry it into reach.

Gaza 2 is at the extreme on both clip numbers: the shortest Move loop of the walk-less roster (five
frames) and the longest idle loop (360 vsyncs at the speed Kemaro leaves him), with 15000 HP to
survive Kemaro repeatedly. The next-closest candidates are Songi (179) and the Drake Ghost family
(16-18), each needing a wide gap; the slimes have the short Move loop but die to Kemaro. The table
and the measurements are on the linked page. It also describes a second, separate park in state
`0x51`.

**Reproducing it on Gaza 2:** have the Kemaro caster act directly before Gaza in the round, with
nobody else acting in between, and let Gaza survive. If his next action is a melee aimed at a
party member well out of his reach (the formation usually does this), he walks one short step,
stops and the round never ends. If he casts instead, the flag is spent harmlessly; try again next
round. This recipe has been run from scripted pad input, with Gaza's HP and MP poked lower (with
his MP intact he answered Kemaro with spells every time): it parked on the first Kemaro, and the
patcher's guard words, poked into the parked state, unparked it.

The fix is the patcher's `--approach-softlock-fix` ([`randomizer.md`](../../tooling/randomizer.md)):
it replaces the nine words with a guard that sends a dead approach back to state `0x14`, so the
game's own staging runs again.

### Verifying it

In a stalled battle read the context state byte: `0x19`, with the attacker's `+0x1D9` and `+0x1DA`
both zero and its position constant. To provoke it, set bit 2 of a monster's `+0x1DC` while it is
in state `0x14`; the write of zero to `+0x1DA` then comes from `0x80047B44`. To see the natural
trigger, watch the Kemaro target's `+0x1DC`: it still reads `0x04` after the put-back sets the
pair to `0/0`, and it drops at the next clip end on that monster
(`scripts/pcsx-redux/autorun_approach_hazard.lua` records both). The capture evidence is on the
linked page.

## Dialogue inconsistencies

**Status:** explained

**Reported as:** "Van Saryu" against "Bon Saryu", "Moustache" against "Whiskers", and the bath
scene in Usha's Research Center, where Noa turns to Vahn and the answer on offer reads "I'm too
shy" where it should be "He's too shy" (community report).

All three are in the English text on the disc, located by exporting every string and decoding the
scripts around them. The first two are one spelling against another, with the exact places. The
third is a pronoun error in the translation, settled against the Japanese disc (`SCPS_100.59`).

In the bath scene a researcher refuses to share the bath with Noa. Noa turns to Vahn and, in the
Japanese script, asks him whether "this person" dislikes her; the player picks Vahn's answer,
either that the man is embarrassed or that the bath is too small. The English changed her question
to "You don't like me?" and the first answer to "I'm too shy", so it reads as if Vahn were the one
refusing. The rest of the scene is translated straight, which is why Noa's next line - said while
turning back to the researcher - and Gala's scolding no longer fit.

Positions below are the PROT entry of the scene's MAN and a byte offset into the decoded MAN.

### Mechanism

#### Van Saryu and Bon Saryu

The lord of Ratayu has a title in front of his name exactly twice on the disc, spelled
differently each time.

| Spelling | Where |
|---|---|
| `Van` | the battle name of monster id 137, in the monster archive (PROT entry 867) |
| `Bon` | the signature on a letter in Ratayu: scene `retock`, PROT entry 146, offset `0x380C` |

Every other mention is the bare name, spread over `retockin`, `retock`, `jagaroom` and single
lines in `balden`, `balden2` and `dohaty`. Neither spelling is the established one; they are two
romanisations of one title that never met (the second half of that sentence is inference).

#### Moustache and Whiskers

Both words occur in one exchange and nowhere else: the audience with King Drake in `dolk2` (the
variant MAN in PROT entry 70).

| Offset | Speaker | Word |
|---|---|---|
| `0x6D86` | King Drake | `moustache` |
| `0x6DB6` | Noa | `Whiskers`, twice |
| `0x6E39` | King Drake | `moustache` |

Drake names the thing and Noa repeats it back as a different word, echoing something he did not
say.

#### The bath scene

The scene is `P2[4]` of `doman` (the variant MAN in PROT entry 401): offset `0x923E` on the USA
disc, `0x9BC7` in the same entry of the Japanese disc. It plays once: its header blocks the spawn
when flag `0x3FB` is set, and the record ends by setting that flag (`53 FB`). The cast by script
channel is `0x1F` and `0x20` for the Noa and Gala stand-ins the scene places, `0x25` for the
researcher, and `0xF8` for the player.

The structure is identical on both discs: the same nineteen face-turn ops in the same order, one
picker, the same rejoin, the same one-shot flag, and no branch on party membership. Offsets in
this table are relative to the record start.

| Beat | USA | Japanese | What the script does |
|---|---|---|---|
| Noa turns to the player | `+0x4F4` | `+0x562` | `CC 1F 85 0F 00 F8`: channel `0x1F` faces channel `0xF8` |
| the player turns to Noa | `+0x516` | `+0x584` | `CC F8 85 0F 00 1F` |
| Noa's question | `+0x55B` | `+0x5DB` | spoken to Vahn. Japanese: about "this person" (`kono hito`), the researcher. English: "You don't like me?" |
| the picker | `+0x57D` | `+0x600` | two options, Vahn's replies. Option 1 in Japanese has no subject: an explanation that someone is embarrassed. English: "I'm too shy" |
| after option 1, Noa turns to the researcher | `+0x5DB` | `+0x678` | `CC 1F 85 14 00 25`, then she calls him odd |
| after option 2, Noa turns to the researcher | `+0x66E` | `+0x71B` | the same turn before her reply |
| both arms rejoin | `+0x711` | - | |
| the researcher gives the shy excuse himself | `+0x8C3` | - | in English, the same words as option 1 |

At the choice Vahn is the speaker and Noa the listener, as in every choice box; the two face-turn
ops just before the question are the evidence of who is addressed. The correct sense of option 1
is "He's too shy". The first-person "I" is wrong, and so is the "You" in the question above it.
Reading the subjectless Japanese sentence as third person is inference, but it is forced by the
question it answers and by Noa's reply.

The tempting reading from the English alone is that the choice box speaks for the researcher. It
does not: the picker is in the same place and belongs to the player on both discs. Only the
pronouns are wrong.

Three other things in the English scene look odd and are not errors, because the Japanese script
has them too: the researcher's lines carry no name tag; Noa calls the place a hot spring where
Gala and the researcher call it a bath; and Gala's "floating" place renders a Japanese phrase for
a building up in the air.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| monster id 137 | PROT entry 867 | the `Van` spelling | disc data |
| MAN offset `0x380C` | PROT entry 146 (`retock`) | the `Bon` spelling | disc data |
| MAN offsets `0x6D86`, `0x6DB6`, `0x6E39` | PROT entry 70 (`dolk2`) | `moustache`, `Whiskers`, `moustache` | disc data |
| MAN offset `0x923E` | PROT entry 401 (`doman`), USA | bath scene, `P2[4]`, 2451 bytes | disc data |
| MAN offset `0x9BC7` | PROT entry 401, Japanese disc | the same record, 2708 bytes | disc data |
| MAN offsets `0x9732`, `0x9754` | PROT entry 401, USA | the two face-turn ops before the question (record `+0x4F4`, `+0x516`) | disc data |
| MAN offset `0x97BB` | PROT entry 401, USA | the two-option picker `27 32 00 D2 00` (record `+0x57D`); Japanese `27 4C 00 FC 00` at record `+0x600` | disc data |
| MAN offset `0x9819` | PROT entry 401, USA | Noa turns to the researcher after option 1 (record `+0x5DB`) | disc data |
| MAN offset `0x9BCB` | PROT entry 401, USA | `53 FB`, the one-shot flag set | disc data |
| `0x800857D7` mask `0x10` | RAM | system flag `0x3FB` | disassembly |
| option 1 means "he is too shy" | - | the reading of the subjectless Japanese line | inference |

### Scope

Text only. None of the three changes a flag, an item or a branch. The bath record's only branch is
the picker, and both arms rejoin.

### Verifying it

Export the disc text with `legaia-patcher translate export` ([translation
tooling](../../tooling/translation/index.md)) and search for the words above; the keys carry the
PROT entry and offset. The bath record decodes on either disc with
`legaia-engine man-scripts --variant 401 --disasm-partition 2 --disasm-record 4`. To replay the
scene, clear flag `0x3FB` and step onto the record's trigger tile.

## Conkram mirror freeze

**Status:** explained

**Reported as:** the game freezes in present-day Conkram, in the Room of Mirrors scene that sends
the party into the past, on many emulators; mashing Cross through the scene's text avoids it
(community reports). The details differ by teller:

- a pSX 1.13 user logs `sound: underrun` and then repeated `r3000: executed illegal opcode` during
  the scene ([pSX forum thread](https://psxemulator.proboards.com/thread/2041/legend-legaia));
  the same thread reports that the PAL release does not freeze, in ePSXe or any pSX version;
- one ePSXe user reports that version 1.7.0 passes the scene and that 1.8.0 and versions before
  1.7.0 freeze;
- a PS3 user reports a freeze every time on the PS3's PS1 emulation; the workaround offered there
  is to play the scene on a PS1 or PS2 and copy the save back;
- a play guide ([Before I Play](https://beforeiplay.com/wiki/Legend_of_Legaia)) has players move a
  DuckStation save to ePSXe for this scene and skip the text by pressing repeatedly, "it may crash
  if you don't";
- relayed by a player: older emulators freeze here, and an early PS3 with PS2 hardware may not.

Nobody reports it on a PS1 or PS2. The players call it an "FMV"; the disc plays no movie here (the
nine movie triggers are listed in [`str-fmv-table.md`](../../formats/str-fmv-table.md#per-scene-trigger-assignment-disc-sourced),
and none is in a Conkram scene). The scene is drawn by the field engine like any other cutscene.

**The cause is a heap that runs out when the emulated CPU is faster than a console's.** The scene
runs with almost no free heap. Its effect parts are allocated per frame, so an emulator that
gives the game more frames per second keeps more of them alive. Past the edge, an allocation
returns NULL, and the move VM writes animation data through the NULL pointer over the bottom of
RAM. That wipes the CPU's exception vector, and the next interrupt jumps into data: a
reserved-instruction exception at the vector itself, raised again on every attempt. The picture
stops; pSX logs exactly that loop as repeated illegal opcodes. Reproduced on two emulators by
raising or lowering only the emulated CPU speed (capture, below).

### The scene

Two partition-2 records of `concnow` (extraction 0363, raw define 362), read with
`legaia-engine man-scripts --scene concnow --disasm-partition 2 --disasm-record N` (disc data):

- **`P2[17]`** is the walk-on on tiles `(29..32, 103)` (inline records 38..41 of the `.PCH`,
  extraction 0361). Its header gates are C1 `0x3E1` (blocks once set) and C2 `0x3CE` (King
  Nebular's Seru Flame scene `P2[16]` has run). The party walks to the mirror at tile `(31, 106)`,
  places the Seru Flame ("Over here! Put the Seru Flame here!"), and after a white flash one
  character says "That mirror is the door to the past." Its tail fades the score out (`35 3C 00
  05`), waits 60 frames and spawns global record `0x62` = 48 + 32 + 18, `P2[18]`.
- **`P2[18]`** (C1 `0x423`) is the crossing. In order: `35 DE 07 09` (start track 2014, sound-test
  14, behind the load barrier); `CFlag` bit 3 on 54 actors; the three party members raised off the
  floor (`4C 42`, bit 29); system flags `0x0A` and `0x0B` set; `B4 F8 30 01`, which spawns
  record 2 of the scene's move-VM stager table (extraction 0362); a 60-frame wait; `35 00 00 0A`
  (the swap commit); camera moves and 400 frames of waits; `B4 32 10 ...`, a breathing green glow
  on actor `0x32`; `60 0A`; then **three text pages** spoken by Terra ("...listen! In the world of
  the past, you will meet your parents when they were young." and two more); `B4 32 20` (the glow
  shrinks out); `60 0B`; 340 frames of camera moves and waits; system flag `0x423` set; a white
  fade; `3F` to `conc2`.

The room also carries three reflection controllers installed at scene entry (`4C 86` in `P1[1]`,
`P1[4]`, `P1[5]`), the mirror images of the party
([`script-vm-menuctrl.md`](../../subsystems/script-vm-menuctrl.md#4c-86--4c-87-are-the-reflection-controllers-install-and-teardown)).

### Mechanism

1. **The heap is nearly full in this room** (capture). The game's allocator (`FUN_80017888` over
   `FUN_8002B468`, best-fit over one free list) has about 16.6 KB free in `concnow` before the
   mirror, the arena from `0x80091800` being otherwise taken by the scene. On PCSX-Redux at console
   speed the crossing takes the largest free block down to 3,708 bytes and the total to 5,152, the
   low point coming in the waits after the music commit.
2. **After the music swap the game pre-stages battle data** (capture, disassembly). The per-frame
   music poller `FUN_800243F0`, once a swap has committed and the game is in field mode (`*(u16
   *)0x8007B83C == 3`), steps a small machine at `0x8007BA98` (`0x80024BB0..0x80024C64`): an
   8-sector read through `FUN_8003E360`, a wait for the drive, then the party battle-file loader
   `FUN_80052770` (`jal` at `0x80024C50`), which opens `PLAYER1..3` (raw TOC `0x361..0x363` =
   extraction 863..865, [`battle-data-pack.md`](../../formats/battle-data-pack.md)) through the
   stream-file API and reads about 316 KiB in twenty chunks into heap buffers. In PCSX-Redux this
   runs for about 400 vsyncs from the commit, during the waits before the first page.
3. **The effect's allocations scale with the frame rate** (capture; why is inference). While the
   crossing runs, the scene's effect parts each take heap blocks: a `0x9C`-byte render node
   (`FUN_80020F88`, `jal` at `0x80021184`) and a keyframe-pose block for the move VM's
   keyframe-pose seat (op `0x3C`, jump-table entry `0x80023CA4`). Sampling the rendered frames: at console speed the field draws 15
   frames per 30 vsyncs before the scene and 10 during it; at double CPU speed it draws 20 before
   it. With the faster CPU the free heap falls faster and further: at 100 % its low point is about
   6 KB, and at the faster settings below it runs out.
4. **The keyframe-pose seat does not check its allocation** (disassembly). At
   `0x80023CD0..0x80023CE0` in the move VM `FUN_80023070`, op `0x3C` calls
   `FUN_80017888(0, count * 32 | 8)` and stores the result at `actor+0x4C` with no NULL test, then
   copies each part's keyframe through it (`sh` at `0x80023D30`, `0x80023D3C`); its retarget op
   `0x3D` writes through the same pointer untested (`sh` at `0x80023EF4`). The part tick's mode-6
   pack does test it (`beqz` at `0x80022F14`). The malloc wrapper's failure arm (`0x800178B8`) only
   adds `0x20000` to the `PRG ERR` counter `0x8007B828` and returns NULL. `FUN_80020F88` does test
   its own `0x9C` allocation (`bne` at `0x8002118C`) but still stores the NULL at `actor+0x44`,
   and the draw path writes through it later (`sw` at `0x8001B098` in `FUN_8001ADA4`).
5. **The NULL write lands on the exception vector** (capture). With a 4-lane keyframe (`0x88`
   bytes) the copy covers `0x80000028..0x8000008C`. `0x80000080` holds the general exception
   vector the BIOS installs (`lui k0,0` / `addiu k0,k0,0xC80` / `jr k0`). The next interrupt
   executes the copied halfwords: the CPU stops with Cause `0x428` (ExcCode `0x0A`, reserved
   instruction) and EPC `0x80000084`, and every retry enters the same vector. If the copied data
   happens to decode as harmless instructions the game limps on, which one capture shows (below).

The CD waits the scene leans on are not involved: at the stop the read state words are idle
(`gp+0x988 = 0`, `gp+0x980 = 0`), the music swap has finished, and the PC is the exception vector,
not a CD wait loop.

### What was captured

Two routes into the scene, both from the retail disc, staged in a fresh directory away from the
`.ppf` that sits beside the retail image:

- **a poked state** on PCSX-Redux: the catalogued late-game `map03` state, with flags `0x3E1`,
  `0x423`, `0x3F2` and `0x4C8` cleared, the player placed on the Conkram walk-on at map tile
  `(66, 101)` so the game's own warp loads `concnow`, then on tile `(30, 103)`;
- **two edited memory-card saves**, booted cold on every emulator. One lifts its save body from
  that poked state's RAM (a level-37 late-game party); the other is the **played** save from Zora's
  Castle that precedes Conkram (`playthrough-ladder-pro00-14.mcr` PRO-14, a level-27 party), with
  only the scene label, the position (the `P2[17]` tile) and the five `concnow` vision flags
  edited. Loading either starts `P2[17]` by itself.

Cross was pressed by the harness, every 100 vsyncs unless noted. The libretro cores ran under a
small headless host that injects the card, presses the pad and reads RAM; ePSXe ran under box64
on a virtual X display.

| Emulator | Setting | Card | Result |
|---|---|---|---|
| PCSX-Redux | interpreter or dynarec, `SpuIrq`, `RCntFix`, PAL video, 8 MB, Cross every 4..300 vsyncs | poked state, late-game card | passes; no allocation fails |
| SwanStation (DuckStation core) | 100 % CPU, recompiler | both | passes |
| SwanStation | 50 % CPU | late-game | passes |
| SwanStation | 125 %, 150 %, 200 % CPU | played | freezes: `PRG ERR` set, vector overwritten |
| SwanStation | 200 %, 400 % CPU | late-game | allocation fails and the vector is overwritten; stops in `concnow` |
| PCSX-ReARMed (current) | its default for this game: dynarec (both cards), interpreter (late-game) | both | freezes; exception loop at `0x80000080` |
| PCSX-ReARMed | `psxclock` 30, 45 (both cards), 50 (played) | both | passes |
| PCSX-ReARMed | `psxclock` 62, 80, 100 | late-game | freezes |
| ePSXe 2.0.5 (Linux x64) | defaults | both | passes |

PCSX-ReARMed's database sets a faster CPU for exactly this game (cycle multiplier 160 against its
default 175, commented "some attack moves lag"), which is why its default freezes. The edge sits
close to a console's speed: SwanStation passes at 100 % and freezes at 125 % with the played
party. The two parties put the edge in different places, since their field data leaves different
headroom.

**Pressing faster did not save the frozen settings.** At 200 % SwanStation the allocation fails
about 70 vsyncs into `P2[18]`, before any page, so text speed cannot matter there; on
PCSX-ReARMed's default it fails about 560 vsyncs in, around the first page. One PCSX-ReARMed run
that pressed every 4 vsyncs reached `conc2` with the vector already overwritten (the limping case
above); every other fast-pressing run on a freezing setting froze or stalled. That pressing helps on an emulator that is only marginally too fast, where the
failure would fall while the pages are open, is inference.

Two emulators named by the reports were not run: ePSXe 1.6.0 reaches its plugin start-up under
box64's 32-bit mode with plugins written for the test, then crashes as soon as its CPU core runs
(box64 cannot execute that build's generated code); ePSXe 1.9.0 needs a 32-bit GTK 2 stack that
box64 cannot wrap. pSX and the PS3 were not available.

The port does not freeze here: the full-game ladder's pad tier walks `taiku`, `map03`, `concnow`,
`conc2` and `conc` and reaches the `conkram_past` milestone
([`full-game-ladder.md`](../../tooling/full-game-ladder.md)). The port allocates from its own
memory, not the game's 16 KB arena: its op-`0x3C` keeps the pose in a Rust `Vec`, so there is no
NULL block to write through, and an allocation failure would abort the process.

### The fix

The patcher's `--conkram-freeze-fix`
([`randomizer.md`](../../tooling/randomizer.md#conkram-freeze-fix)) makes both of a part's
allocations safe, with four same-size edits of `SCUS_942.54`:

- **Op `0x3C`.** Nine words of the handler are rearranged in place so the word after the `jal`
  becomes `beq v0,zero` to op `0x08`'s HALT arm: a failed pose block ends the part the way a
  script's own halt does, the list walk releases it and frees its render node, and `+0x4C` stays
  NULL, which the release path skips. The effect loses one particle.
- **The render node.** `FUN_80020F88`'s failure block stores a shared stand-in node, kept in free
  RAM above the slot-B images, instead of leaving NULL at `actor+0x44`. Its `0x800` flag stays
  clear, so nothing ever frees the stand-in.
- **The readout.** Both calls go through a quiet copy of the malloc wrapper, placed over an
  unreferenced thunk, that counts the allocation and calls the allocator but has no failure arm,
  and the render-node block no longer ORs `0x4000` in. A failure the fix absorbs leaves `PRG ERR`
  at 0, so the readout never appears for it.

Every allocation that succeeds runs the retail instructions.

With the fix, the played and late-game saves both pass the scene on SwanStation at 200 % and on
PCSX-ReARMed's default, the settings that froze above, with `PRG ERR` 0 at the end (capture, cold
boots of a patched scratch disc). A write watch on the bottom 256 bytes of RAM in PCSX-ReARMed's
interpreter records no store at all through the scene, where the retail disc records 72. At
console speed nothing fails and the scene loads `conc2` on the same vsync as retail. The design,
and why halting and a stand-in beat the alternatives, is under
[`randomizer-internals.md`](../../tooling/randomizer-internals.md#conkram-freeze-fix).

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `concnow` `P2[17]`, `P2[18]` | MAN, extraction 0363 | the mirror walk-on and the crossing | disc data |
| `0x80023CD0..0x80023CE0`, `0x80023D30`, `0x80023D3C` | `SCUS_942.54` | move-VM op `0x3C`: unchecked malloc, then the pose copy through the result | disassembly, capture |
| `0x80023EF4` | `SCUS_942.54` | move-VM op `0x3D`: the retarget's store through the same block | disassembly |
| `0x800232B8` | `SCUS_942.54` | move-VM op `0x08` HALT arm, the fix's failure exit | disassembly |
| `0x80025B10` | `SCUS_942.54` | unreferenced thunk, the fix's quiet malloc entry | disassembly |
| `0x801FAA00` | RAM | the fix's stand-in render node | capture |
| `0x80017888`, failure arm `0x800178B8` | `SCUS_942.54` | malloc wrapper: on NULL adds `0x20000` to `PRG ERR` and returns NULL | disassembly |
| `0x8002B468` | `SCUS_942.54` | allocator: best-fit over the free list at `*(gp+0x840) + 0xC` | disassembly |
| `0x80021184..0x80021190` | `SCUS_942.54` | `FUN_80020F88`: `0x9C` render-node malloc, NULL stored at `actor+0x44` | disassembly |
| `0x8001B098` | `SCUS_942.54` | `FUN_8001ADA4`: draw-path store through `actor+0x44` | disassembly, capture |
| `0x80024BB0..0x80024C64` | `SCUS_942.54` | `FUN_800243F0`: post-swap battle pre-stage (`FUN_8003E360`, then `FUN_80052770`) | disassembly, capture |
| `0x80000080` | RAM | the general exception vector the NULL copy overwrites | capture |
| `0x8007B828` | RAM | `PRG ERR` counter, `0x20000` per failed allocation | disassembly, capture |

### Scope

This scene on any emulator that runs the CPU fast enough; how fast is enough depends on the party.
Any other place where the free heap gets this low behaves the same way, and the unchecked
keyframe-pose allocation serves every move-VM effect in the game, but no other report of this
kind is known. On a console the allocation does not fail in either captured party. The
[WORK ERR](#work-err-on-mt-rikuroa) section is the sibling limit: the fixed actor pool, not the
heap.

### Verifying it

On an emulator with a CPU-speed setting, load a save in this room and raise the speed: the freeze
comes back on demand, and lowering the speed clears it. In a debugger, break at `0x800178B8` (the
malloc failure arm) during the crossing, and watch `0x80000080`: the vector changes from
`00001A3C 800C5A27 08004003` to animation data, and the next interrupt ends at
`0x80000080`/`0x80000084` with Cause `0x428`. `PRG ERR` (`0x8007B828`) reads `0x20000` or more.

### Open questions

- Why the frame rate changes the live part count: whether the spawner (stager record 2, `25 03`
  four times a pass) and the parts' lifetimes count different clocks. The rendered frame rate and
  the heap fall are measured; the link between them is inference.
- Why the PAL release would not freeze. Its 50 Hz field gives the CPU more time per frame, which
  would help, but no PAL disc was run.
- Which ePSXe changes the reported 1.7.0 / 1.8.0 split matches; 1.6.0 and 1.9.0 did not run here.
- Whether pressing fast saves a marginal setting; none of the freezing settings tried was marginal.

## The two summon flutes

Support data for the flute pages: which item is which action, and which cast module it loads. The
tables are read from `SCUS_942.54` and PROT entry 0898; the layout is in
[`item-effect-table.md`](../../formats/item-effect-table.md) and
[`cast-module.md`](../../subsystems/cast-module.md#the-three-tables-in-prot-0898).

| | Lippian Flute | Spikefish Flute |
|---|---|---|
| Item id | `0x98` | `0x99` |
| Item record, `0x80074368 + id * 0xC` | `0x80074A88`, kind 2, subtype `0x27` | `0x80074A94`, kind 2, subtype `0x28` |
| Effect descriptor, `0x800752C0 + subtype * 4` | `0x8007535C`: `7E 00 C4 41` | `0x80075360`: `7F 00 E4 41` |
| Class, target | 126, one enemy, battle only | 127, all enemies, battle only |
| Action id | `0x96` | `0x97` |
| Tick table `0x801CF4EC`, row `id - 0x81` | row 21, arm `0x801F208C`: `jal 0x801F6A18` | row 22, arm `0x801F209C`: `jal 0x801F6A00` |
| Stager table `0x801F6734` | row 21: `0x801F7820` | row 22: `0x801F7AE8` |
| Cast module, slot-B base `0x801F69D8` | PROT entry 0924 | PROT entry 0925 |

Item id `0x98` and action id `0x98` are different things. The Lippian Flute's cast is action
`0x96`; action `0x98` is the null row whose tick slot is the dispatcher's shared epilogue
`0x801F2128` and whose module, PROT entry 0926, is a stub. The step from effect class to action id
is taken from [`cast-module.md`](../../subsystems/cast-module.md) and the spell names, not re-read
from the enqueue code.
