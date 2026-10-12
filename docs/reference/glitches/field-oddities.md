# WORK ERR, missing encounters and script slips

Four community-reported oddities that live in the field and battle runtime rather than in one
attack or item: a debug counter that reaches the screen on Mt. Rikuroa, the random battles that
stop in Jette's Absolute Fortress, the camera orbit that ends the Gaza rematch, and three small
slips in the English script. Everything on this page is read from the USA disc (`SCUS_942.54` and
its overlays) by static analysis - disassembly and decoded disc data. Nothing here is an emulator
capture; where a claim is a prediction, the section says so and names the debugger check that
settles it. A closing section records the identity of the two summon flutes, as support for the
flute pages.

Addresses are virtual addresses in the named image. PROT numbers are extraction indices; the
scene of a PROT entry is resolved from its index, because extraction filename labels are shifted
against `CDNAME.TXT` ([`cdname.md`](../../formats/cdname.md#numbering-space)). `gp` is
`0x8007B318`.

## WORK ERR on Mt. Rikuroa

**Status:** partially explained

**Reported as:** the debug text "WORK ERR2" appears on screen on Mt. Rikuroa (community report).

Everything that moves in a scene - the player, each townsperson, each door and prop, each part of
a visual effect, each attached light, each running cutscene script - is an *actor*, and all of
them come out of one fixed pool of 143 slots. When something asks for a slot and the pool is
empty, the request fails, an error counter goes up by one, and from then on the frame loop prints
`WORK ERR` followed by that counter every frame.

So "WORK ERR2" is not error number two. It says that two actor requests have failed since the
current stage was set up. There is no separate "WORK ERR1" message: the digit is a `%d`.

What the message means is proven from the instructions. Why Mt. Rikuroa is where players meet it
is only partly explained: the scene starts with 93 of the 143 slots already taken, which is a
heavy but not overflowing load, and the remaining 50 have to be used up by something that happens
during play. Which event does that is a prediction, not a measurement.

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
   `0x8001E32C..0x8001E35C`), so 136 are available to a scene.
4. A failed spawn returns null. Some callers test the result; `FUN_8003AB2C` writes through it
   unchecked. What a failed spawn costs beyond the text - a missing prop or effect part, or a
   write through a null pointer - depends on the caller and is not traced here.

The pool, its free path and the control block in front of it are documented in
[`field-ambient-fx.md`](../../subsystems/field-ambient-fx.md#the-part-pool-and-the-halted-part-free).

#### The sibling messages

`FUN_80016444` prints four more diagnostics the same way:

| String | Address | Counter | Meaning |
|---|---|---|---|
| `WORK ERR%d` | `0x800100F4` | `gp+0x588` = `0x8007B8A0` | failed actor-pool spawns |
| `PRG ERR%d` | `0x80010100` | `gp+0x510` = `0x8007B828` | the error accumulator [`memory-map.md`](../memory-map.md) describes at that address |
| `READ ERR%d`, `NAME %s` | `0x80010134`, `0x80010140` | `gp+0x556`, name buffer `0x801C88F0` | read from the strings; not traced |
| `cd err count %d`, `RETRY %d` | `0x800100D4`, `0x800100E8` | `0x8007BC00`, `0x8007BC7C` | printed only while `gp+0x3B8` is non-zero |

#### What Mt. Rikuroa asks of the pool

The scene is `rikuroa` (PROT block 153..162): its field map is PROT entry 153 and its story-state
script container is the variant MAN in PROT entry 157, with partition counts 13 / 29 / 64. Scene
entry (`FUN_8003AEB0`) takes slots as follows.

| Source | Slots | Read from |
|---|---|---|
| list sentinels | 7 | `FUN_8001E1B4` |
| player, two fixed actors, the system-script actor | 4 | `FUN_801D6704` in PROT entry 0897, `FUN_8003AEB0`, `FUN_8003AB2C` |
| partition-1 placements 1..28 | 28 | the `FUN_8003A1E4` loop over the MAN header count |
| map object binds | 54 | `FUN_8003A55C`: one actor per map tile whose object descriptor has `+0x12 & 4` and a kind-1 trigger at its key tile ([`field-map.md`](../../formats/field-map.md)) |
| **at scene entry** | **93 of 143** | |

The 54 object binds come from nine distinct script records, and only six of the disc's field maps
bind more. The remaining 50 slots serve everything created during play: party followers, motion
script actors, one actor per running partition-2 cutscene record (the MAN carries 64 such records
and nine `0x44` spawn-record sites), one per attached light (20 sites of op `0x34` sub-1), and the
effect trees installed by op `0x34` sub-3 (23 sites), each of which can fan out into several
actors. The scene's two longest cutscene records, partition-2 records 47 and 48, both install
effects. An overflow during one of them is the likely cause, and it is inference.

`rikuroa2` (PROT block 118..125) is the cutscene-only sibling with an empty map: no object binds,
partition counts 14 / 11 / 57. If the report is about that scene, its whole budget is dynamic.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x800100F4` | `SCUS_942.54` | the `WORK ERR%d` string | disc data |
| `0x800164B0..0x800164C8` | `SCUS_942.54` | the print, in `FUN_80016444` | disassembly |
| `0x8007B8A0` | RAM | failed-spawn counter, signed 16-bit | disassembly |
| `0x8001E304` | `SCUS_942.54` | counter clear, in stage init `FUN_8001E1B4` | disassembly |
| `0x80020E00..0x80020E30` | `SCUS_942.54` | failure arm of the spawn `FUN_80020DE0` | disassembly |
| `0x80020454` | `SCUS_942.54` | free-stack pop; null when the top index is negative | disassembly |
| `0x800203EC` | `SCUS_942.54` | seed: top `0x8E`, 143 slots of `0xD8` bytes | disassembly |
| `0x8007C348` | RAM | pool control block: `+0x00` top index, `+0x1C` player, `+0x28` free stack | disassembly |
| `0x8007C5AC` | RAM | slot 0; slot `i` at `+ i * 0xD8`, last slot `0x80083D7C` | disassembly |
| `0x8003A55C` | `SCUS_942.54` | map object-bind sweep at scene entry | repo doc |
| PROT entry 153 | disc | Mt. Rikuroa field map: 54 bound objects | disc data |
| PROT entry 157 | disc | Mt. Rikuroa variant MAN: 13 / 29 / 64 records | disc data |

### Scope

Every field scene, minigame and battle stage spawns through `FUN_80020DE0`, so the message can
appear anywhere the pool runs dry. The counter is cleared only by stage init, so once it shows it
stays until the next stage is set up. By object binds alone the heaviest field maps are PROT
entries 653 (100), 670 (75), 360 (60), 559 and 1226 (58 each), 481 (55) and 153, Mt. Rikuroa (54).

### Verifying it

Put a write breakpoint on the two bytes at `0x8007B8A0` and play through Mt. Rikuroa. A hit from
`0x80020E1C` is a failed spawn: the return address names the caller, and the first argument on
entry to `FUN_80020DE0` is the template of the actor that did not get a slot. Watching the word at
`0x8007C348` shows the load directly: `0x8E` is an empty scene, `-1` is exhaustion.

### Open questions

- Which record or effect tree takes the last slot on Mt. Rikuroa.
- Whether the two failed spawns are visible as missing objects.
- The dev text formatter `FUN_8001A068` is not traced for a gate of its own; the report is the
  evidence that it draws on a retail console.

## No encounters in Jette's Absolute Fortress

**Status:** partially explained

**Reported as:** sometimes no random battles happen in Jette's Absolute Fortress (community
report, repeated but unconfirmed).

Random battles in a dungeon are not a flat chance per step. The map is divided into rectangles,
each with its own rate, and the rectangles are grouped by story state: the game picks one group
according to which story flags are set, and only that group's rectangles count. A rate of zero
means that stepping there never moves the encounter counter.

The Fortress is two scenes, the entrance (`deroa`) and the transport-system interior (`chitei2`).
Their tables contain three ways to walk without a battle. One is plainly intended: once Jette is
beaten and the Fortress starts to collapse, a flag switches both scenes to an all-zero group for
good. One is always present: large parts of the interior are zero-rate rectangles that sit in
front of the rectangles that do roll. The third is a flag borrowed from Zora's Floating Castle
that would silence the Fortress completely if it were ever left set; no path that leaves it set
was found.

Which of these a given report met cannot be told without the save. No wrapping counter and no
encounter-off script op without its matching encounter-on was found.

### Mechanism

The roll is `FUN_801D9E1C` in PROT entry 0897 (the field overlay); the full reader is documented
in [`encounter.md`](../../formats/encounter.md#random-encounter-trigger-path). The parts that
matter here:

1. A step is a change of the player's 128-unit tile by at most one on each axis
   (`0x801D9ED4..0x801D9F0C`).
2. The condition list is walked in order (`0x801D9F30..0x801D9FD8`). Each record is a flag id and
   a region count. The first record whose flag is `0xFFFF` or is set wins, and only its regions
   are searched.
3. Within the winning group, the first region whose box contains the tile wins
   (`0x801D9FE8..0x801DA054`).
4. The step does nothing at all while the player's engaged bit is up, while `0x8007B6B4` is
   non-zero, while the warp timer `0x8007B6B0` is positive, or while the Incense window
   `0x8007B600` is non-zero (`0x801DA130..0x801DA180`).
5. The region's rate is scaled by the setting byte `0x8007B5F8`, by the High and Low Encounter
   accessory passives, and by system flags `0x1D` (doubled) and `0x1E` (halved)
   (`0x801DA198..0x801DA204`).
6. The scaled rate is subtracted from the counter `0x8007B5FC`; a battle starts when it reaches
   zero or below (`bgtz` at `0x801DA218`), and the counter is reset to a value between 488 and
   1460 (`0x801DA2D4..0x801DA35C`).

The Fortress tables, decoded from MAN section 0 of each scene:

| Scene | Order | Flag | Regions in the group | Result |
|---|---|---|---|---|
| `deroa` (PROT entry 606) | 0 | `0x0015` | one, whole map, rate 0 | no encounters |
| | 1 | `0x0521` | one, whole map, rate 0 | no encounters |
| | 2 | `0x046D` | eight: two rate-0 pockets, five rate-18 rooms, rate-0 catch-all | normal |
| | 3 | default | seven: one rate-0 pocket, five rate-18 rooms, rate-0 catch-all | normal |
| `chitei2` (PROT entry 706) | 0 | `0x0015` | one, whole map, rate 0 | no encounters |
| | 1 | `0x0521` | two, both rate 0 | no encounters |
| | 2 | default | sixteen: seven rate-0 boxes (six of them listed first), eight rate-18 boxes, rate-0 catch-all | mixed |

#### Flag `0x521`: the Fortress is collapsing

The only field-script writers of flag `0x521` on the disc are two partition-2 records of
`chitei2`, records 15 and 19, each opening with `55 21`. They are the two variants of the scene
that follows Jette's defeat, in which the party is told the Fortress is collapsing. No record
clears the flag. From then on both scenes select a zero-rate group, so the escape and any later
visit have no random battles. This is the designers switching the dungeon off, not a defect.

#### Zero-rate boxes in front of the rooms

In the interior's default group six of the seven zero-rate boxes are listed before any box that
rolls, and the first match wins. Two of them, `(98,97)..(109,109)` and `(86,100)..(104,106)`, lie inside
the rate-18 box `(86,93)..(128,113)`; `(86,0)..(103,34)` covers the west strip of
`(81,0)..(128,81)`; `(61,25)..(80,52)`, `(0,0)..(36,46)` and `(0,62)..(30,126)` are whole silent
rooms. Measured by tile area rather than walkable floor, about 46% of the interior grid is rate 0
in the default state, against about 13% of the entrance. A player pacing inside one of those boxes
never moves the counter.

#### Flag `0x15`: borrowed from the Floating Castle

Flag `0x15` heads the condition list of exactly three scenes: `taiku2`, `deroa` and `chitei2`. Its
only field-script writers are in the Floating Castle's variant MAN (PROT entry 373): partition-2
record 12 sets it as its first flag op (`50 15`), and record 29 - the scene with Zora and Songi -
clears it as its last (`60 15`). In between it switches the castle's encounters off.

If a save carried `0x15` set outside that window, both Fortress scenes would be silent from the
first step, and nothing in the Fortress can clear it. On the story path the clear always runs, so
this needs some way of leaving the castle between the two records. None was found. It is a
prediction about what the data permits, not an observed state.

#### Smaller contributors

- **Stacked rate modifiers.** Rate 18 is 9 with a Low Encounter accessory, and 4 if system flag
  `0x1E` is also set. Flags `0x1D` and `0x1E` are set by two drinkable-water props in Rogue's
  Tower (`rugi`, partition-1 records 29 and 30) and nothing on the disc clears either. At rate 4
  the counter lasts between 122 and 365 steps.
- **A top-up on every scene entry.** `FUN_8003AB2C` at `0x8003AC7C..0x8003AC90` adds half of a
  fresh counter roll whenever the counter is below 487 as a scene loads. Moving back and forth
  between the two Fortress scenes keeps refilling a low counter.
- **Incense** suspends the roll outright while its window is open.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801D9E1C` | PROT entry 0897 | per-step region encounter roll | disassembly |
| `0x801D9F30..0x801D9FD8` | PROT entry 0897 | condition-list walk; first set flag wins | disassembly |
| `0x801D9FE8..0x801DA054` | PROT entry 0897 | region search; first matching box wins | disassembly |
| `0x801DA130..0x801DA180` | PROT entry 0897 | the four "step does not roll" gates | disassembly |
| `0x801DA198..0x801DA204` | PROT entry 0897 | rate scaling: setting byte, two passives, flags `0x1D` / `0x1E` | disassembly |
| `0x801DA208..0x801DA21C` | PROT entry 0897 | counter subtract and the `bgtz` test | disassembly |
| `0x8003CE64` | `SCUS_942.54` | system-flag test: byte `0x80085758 + (id >> 3)`, mask `0x80 >> (id & 7)` | disassembly |
| `0x8003AC7C..0x8003AC90` | `SCUS_942.54` | scene-entry counter top-up in `FUN_8003AB2C` | disassembly |
| `0x8007B5FC` | RAM | encounter step counter, signed word | disassembly |
| `0x8007B5F8` | RAM | rate setting byte | disassembly |
| `0x8007B600` | RAM | Incense window | disassembly |
| `0x8008575A` mask `0x04` | RAM | system flag `0x15` | disassembly |
| `0x800857FC` mask `0x40` | RAM | system flag `0x521` | disassembly |
| `0x8008575B` masks `0x04`, `0x02` | RAM | system flags `0x1D`, `0x1E` | disassembly |
| PROT entry 606, MAN offset `0x5B4B` | disc | `deroa` encounter section, `0x152` bytes | disc data |
| PROT entry 706, MAN offset `0x6A1A` | disc | `chitei2` encounter section, `0x166` bytes | disc data |
| PROT entry 706, partition-2 records 15 and 19 | disc | the two setters of flag `0x521` | disc data |
| PROT entry 373, partition-2 records 12 and 29 | disc | the setter and the clearer of flag `0x15` | disc data |
| PROT entry 697, partition-1 records 29 and 30 | disc | the setters of flags `0x1D` and `0x1E` | disc data |

### Scope

A story-gated group that is a single zero-rate row is the game's ordinary way of switching a
dungeon off, and it is used across the disc. Flag `0x15` gates only the three scenes named above.
The two water flags change the rate in every scene with random battles, for the rest of the save.

### Verifying it

In the Fortress, read the flag bytes in the table above, the Incense window and the counter. Then
break at `0x801DA208`: register `s0` holds the final per-step rate and `s2` points at the region
record that won. If that breakpoint never fires while the player walks, either one of the four
gates is holding the step or the winning region has rate 0; the branch at `0x801D9FC8` shows
which condition group was selected.

### Open questions

- Whether flag `0x15` can survive the Floating Castle. Checking a save made after Zora settles it.
- Whether the Fortress can be re-entered after it collapses.
- The roll also leaves at once when bit `0x8000` of the transient flag word `0x1F800394` is set;
  what raises that bit is not traced.

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

**Status:** partially explained

**Reported as:** "Van Saryu" against "Bon Saryu", "Moustache" against "Whiskers", and the bath
scene in Usha's Research Center (community report).

All three are in the English text on the disc, located by exporting every string and decoding the
scripts around them. The first two are settled: each is one spelling against another, with the
exact places. The bath scene is located and its structure is decoded, but which of its oddities
the report means is a reading of the scene, which is why the entry as a whole stays partial.

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

The scene is partition-2 record 4 of `doman` (the variant MAN in PROT entry 401), starting at
offset `0x923E`. It plays once: its header blocks the spawn when flag `0x3FB` is set, and the
record ends by setting that flag (`53 FB`). The cast by script channel is `0x1F` and `0x20` for
the Noa and Gala stand-ins the scene places, `0x25` for the researcher who uses the bath
(partition-1 record 28), and `0xF8` for the player.

Three things in it do not line up:

- **The choice box answers for the researcher.** After Noa's question a two-option picker
  (`27 32 00 D2 00` at `0x97BB`) offers two excuses. Every other line on that side of the
  conversation belongs to the researcher - the surrounding ops drive channel `0x25`, and he gives
  both excuses again himself at `0x9AD3..0x9B2D` - while the game's convention is that a choice
  box holds the silent hero's replies. The structure is disc data; who is meant to be speaking is
  inference.
- **The researcher has no name label.** His lines carry no speaker prefix while Noa's and Gala's
  all do, so the exchange reads as if Noa were answering herself.
- **The words for the place disagree.** Noa says `hot spring` (`0x9332`, `0x93EC`, `0x9405`,
  `0x9526`); Gala and the researcher say `bath` (`0x9468`, `0x9654`, `0x97D4`, `0x9AD3`); and
  Gala's line at `0x9487` puts it in a `floating` place, although the scene's own lines at
  `0x9CE5` and `0x9D08` have the Research Center reached by a wound elevator. Whether that is an
  error or characterisation is a judgement.

The record has no branch on party composition. Its only branch is the picker, and both arms rejoin
at script offset `+0x0711`.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| monster id 137 | PROT entry 867 | the `Van` spelling | disc data |
| MAN offset `0x380C` | PROT entry 146 (`retock`) | the `Bon` spelling | disc data |
| MAN offsets `0x6D86`, `0x6DB6`, `0x6E39` | PROT entry 70 (`dolk2`) | `moustache`, `Whiskers`, `moustache` | disc data |
| MAN offset `0x923E` | PROT entry 401 (`doman`) | bath scene, partition-2 record 4 | disc data |
| MAN offset `0x97BB` | PROT entry 401 | the two-option picker | disc data |
| MAN offset `0x9BCB` | PROT entry 401 | `53 FB`, the one-shot flag set | disc data |
| `0x800857D7` mask `0x10` | RAM | system flag `0x3FB` | disassembly |

### Scope

Text only. None of the three changes a flag, an item or a branch.

### Verifying it

Export the disc text with `legaia-patcher translate export` ([translation
tooling](../../tooling/translation/index.md)) and search for the words above; the keys carry the
PROT entry and offset. To replay the bath scene, clear flag `0x3FB` and step onto the record's
trigger tile.

### Open questions

- Whether the Japanese script gives the bath scene's choice to the hero.

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
