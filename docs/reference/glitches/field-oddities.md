# WORK ERR, missing encounters and script slips

Four community-reported oddities that live in the field and battle runtime rather than in one
attack or item: a debug counter that reaches the screen on Mt. Rikuroa, the random battles that
stop in Jette's Absolute Fortress, the camera orbit that ends the Gaza rematch, and three small
slips in the English script. A closing section records the identity of the two summon flutes, as
support for the flute pages.

The base of every section is static analysis of the USA disc: disassembly of `SCUS_942.54` and its
overlays, and decoded disc data. Two sections also rest on emulator captures, and each says which
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
after the Golem fight, as the cave collapses, and stays (community reports).

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
four slots and no slots to spare. One or two more actors alive on that frame and their requests
fail: the counter reads 1 or 2 and stays on screen until the next screen change. Nothing else
breaks; a rock or two never appears.

What the message means is proven from the instructions, and the event that fills the pool is
captured. The failed request itself is not: the replay reaches exactly 143 of 143 and never asks
for a 144th, so the "2" and the identity of the last one or two actors in a player's run remain
inferred. That is why the entry stays partial.

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
transition. The "transition" in the reports is the return from the battle into the field.

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

#### The number on screen

The counter goes up once per spawn attempted while the stack is empty and is cleared only by the
next stage init. At the peak the stager asks for two to seven slots a frame, and parts free
themselves two list walks after halting. A run that is one or two actors heavier than the replay
therefore fails one or two requests in a single frame and then recovers: `WORK ERR1` or
`WORK ERR2`, left on screen until the next mode switch. This is inference from the measured margin
(zero to four free at the peak) and the measured spawn rate. The caller whose request fails is
most likely `FUN_80021B04` creating a rock or dust part, or the dialogue box if it opens on the
full frame.

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
Golem tile (flag `0x284` clear).

### Open questions

- The failed spawn is not captured. The replay ties the limit and does not cross it, so the caller
  and the "2" are inferred.
- What supplies the last one or two actors in a player's run. The replay is a teleported route
  with an assisted kill, not a walked climb with random battles and a fought Golem.
- The dev text formatter `FUN_8001A068` is not traced for a gate of its own; the reports are the
  evidence that it draws on a retail console.

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
flag, which also repairs a file that already has it.

Two other quiet states exist and are intended: once Jette is beaten a second flag switches the
Fortress off permanently, and the boss room, the room before it, the council and Mist Generator
rooms, the elevators and the bridge never roll.

The clock code, the castle's script and the Fortress tables are read from the disc. The flag being
raised by the clock and the Fortress going silent as a result are captured, on an edited save.
One step is inferred: nobody played the castle escape to its last minute and then read the flag.

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

The clock is not gentle. The scene loader takes 1200 ticks (20 seconds) off a running clock on
every scene entry (`0x8003B62C..0x8003B648`: `lw a0,0x478(a1)` / `beqz` / `addiu v0,v0,-0x4b0` /
`sw v0,0x460(a1)`). The castle is two scenes, so each crossing between them costs 20 seconds on
top of the walking. Loading a save while a clock is running restores it to its full duration
(`0x8003B96C..0x8003B98C`).

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
post-Jette flags. The gate record `P2[4]` needs flag `0x3E1`, which the Conkram scenes write, so
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
post-Jette group: condition 0 tests flag `0x521` instead of `0x15`, and its row becomes a copy of
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

The only field-script writers of flag `0x521` on the disc are `chitei2` `P2[15]` and `P2[19]`,
each opening with `55 21`. They are the two variants of the scene that follows Jette's defeat and
the Cort confrontation, in which the party is told the Fortress is collapsing. No record clears
the flag. From then on both scenes select a zero-rate group, so the escape and any later visit
have no random battles. This is the designers switching the dungeon off, not a defect, and it is
not what the report describes: there Jette is still present.

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
  `0x1E` is also set. Flags `0x1D` and `0x1E` are set by two drinkable-water props in Rogue's
  Tower (`rugi`, partition-1 records 29 and 30) and nothing on the disc clears either. At rate 4
  the counter lasts between 122 and 365 steps.
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
| `0x801DA198..0x801DA204` | PROT entry 0897 | rate scaling: setting byte, two passives, flags `0x1D` / `0x1E` | disassembly |
| `0x801DA208..0x801DA21C` | PROT entry 0897 | counter subtract and the `bgtz` test | disassembly |
| `0x801E2BDC..0x801E2C38` | PROT entry 0897 | op `4C D3` handler: arms the countdown | disassembly |
| `0x801D2EBC` | PROT entry 0897 | clock actor; flag sets at `0x801D2F60` (expiry) and `0x801D2F84` (below threshold) | disassembly; capture (edited save) |
| `0x801F1360`, `0x801F1684` | PROT entry 0897 | set and clear of bit `0x8000` of `0x1F800394` by the menu / submode installer `FUN_801F1278` | disassembly |
| `0x8003CE08` | `SCUS_942.54` | system-flag set | disassembly |
| `0x8003CE64` | `SCUS_942.54` | system-flag test: byte `0x80085758 + (id >> 3)`, mask `0x80 >> (id & 7)` | disassembly |
| `0x8003B62C..0x8003B648` | `SCUS_942.54` | scene loader: 1200 ticks off a running clock per scene entry | disassembly |
| `0x8003B96C..0x8003B98C` | `SCUS_942.54` | card load restores a running clock to its full duration | disassembly; capture (edited save) |
| `0x8003AC7C..0x8003AC90` | `SCUS_942.54` | scene-entry counter top-up in `FUN_8003AB2C` | disassembly |
| `0x800845A0`, `0x800845B8`, `0x800845BC`, `0x800845C0` | RAM | clock: remaining, duration, threshold, flag pair; save-block `+0x460`, `+0x478`, `+0x47C`, `+0x480` | disassembly; capture (played saves) |
| `0x8007B5FC` | RAM | encounter step counter, signed word; not saved, 974 after power-on | disassembly; capture |
| `0x8007B5F8` | RAM | rate setting byte | disassembly |
| `0x8007B600` | RAM | Incense window | disassembly |
| `0x8008575A` mask `0x04` | RAM | system flag `0x15`; save-block `+0x161A` | disassembly |
| `0x800857FC` mask `0x40` | RAM | system flag `0x521` | disassembly |
| `0x8008575B` masks `0x04`, `0x02` | RAM | system flags `0x1D`, `0x1E` | disassembly |
| PROT entry 373, `P2[29]` `+0x1508`, `+0x1533` | disc | `taiku`: the one clear of flag `0x15`, then the arming op (6:00, warning flag `0x15` at 1:00) | disc data |
| PROT entry 373, `P2[16]` `+0x13` | disc | `taiku`: escape success; stops the clock, no clear of `0x15` | disc data |
| PROT entry 373, `P2[12]` | disc | `taiku`: the script-side set of flag `0x15` (`50 15`) | disc data |
| PROT entry 606, MAN offset `0x5B4B` | disc | `deroa` encounter section, `0x152` bytes | disc data |
| PROT entry 706, MAN offset `0x6A1A` | disc | `chitei2` encounter section, `0x166` bytes | disc data |
| PROT entry 706, `P2[15]` and `P2[19]` | disc | the two setters of flag `0x521` | disc data |
| PROT entries 703, 603 | disc | wall grids of `chitei2` and `deroa`: 39% and 8% of enclosed floor is rate 0 | disc data |
| PROT entry 697, partition-1 records 29 and 30 | disc | the setters of flags `0x1D` and `0x1E` | disc data |
| flag `0x15` set from `0x801D2F84`, Fortress silent | RAM | the clock raising the flag and the roll selecting row 0 | capture (edited save) |
| Fortress rolls at rate 18 | RAM | the control: a played pre-Jette save | capture |
| a real escape under 1:00 leaves the flag set | - | the one step not observed end to end | inference |

### Scope

A story-gated group that is a single zero-rate row is the game's ordinary way of switching a
dungeon off, and it is used across the disc. Flag `0x15` gates only the three scenes named above,
so the leak shows in the Fortress and nowhere else. The clock actor is a general mechanism: the
Fortress's own collapse timer uses it with flag `0x4C7`. The two water flags change the rate in
every scene with random battles, for the rest of the save.

### Verifying it

On an affected file, read byte `0x8008575A` (or save-block `+0x161A`): bit `0x04` is set. In the
Fortress, break at `0x801DA208`: register `s0` holds the final per-step rate and `s2` points at the
region record that won; the branch at `0x801D9FC8` shows which condition group was selected.

To see the cause itself, play the Floating Castle escape, let the clock fall below 1:00 with a
breakpoint at `0x801D2F84`, reach the exit, and read byte `0x8008575A` on the overworld.

### Open questions

- The end-to-end observation: a played castle escape that finishes under 1:00 and a save that then
  shows the flag. Bit `0x04` at save-block `+0x161A` in the file of any player with the silent
  Fortress settles it equally.
- A native clear of flag `0x15` through a computed id is not excluded by the constant-id sweep;
  none of the computed clear sites reads as touching it.
- Whether returning from a battle charges the clock the same 20 seconds as a scene change.
- The locked early visit is read from the scripts, not walked.

## Gaza 2 softlock

**Status:** explained

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

If the monster is still flinching from a hit when its attack begins - a summon is the usual cause
- a leftover "go back to idle when this clip ends" flag is still set. The Move loop plays one
cycle, the flag swaps it for the idle pose, the monster stops short of its target, and the wait
never ends. The camera's idle sweep runs on its own, which is why the screen keeps orbiting.

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
4. Bit 2 is raised by the hit-reaction stager in `FUN_800402F4`. A flinch followed directly by a
   melee installs the Move clip through the mid-clip path with bit 2 still up; the first natural
   end installs idle; the monster is parked.

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
| actor `+0x1D9`, `+0x1DA`, `+0x1DC` | RAM | playing clip, queued clip, anim event flags | disassembly |
| `*0x8007BD24`, `+0x07` | RAM | battle context, action state byte | repo doc |

### Scope

Any monster without a tag-`0x20` action - nearly the whole roster, bosses included, by the roster
sweep on the linked page - attacking an out-of-reach target directly after being flinched. The
Gaza rematch is where it is met because his formation keeps him out of reach and a summon is the
usual opening move. The linked page also describes a second, separate park in state `0x51`.

The fix is the patcher's `--approach-softlock-fix` ([`randomizer.md`](../../tooling/randomizer.md)):
it replaces the nine words with a guard that sends a dead approach back to state `0x14`, so the
game's own staging runs again.

### Verifying it

In a stalled battle read the context state byte: `0x19`, with the attacker's `+0x1D9` and `+0x1DA`
both zero and its position constant. To provoke it, set bit 2 of a monster's `+0x1DC` while it is
in state `0x14`; the write of zero to `+0x1DA` then comes from `0x80047B44`. The capture evidence
is on the linked page.

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
