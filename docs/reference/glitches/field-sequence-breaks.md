# Incense, doors and the Door of Light

Four field sequence breaks, each traced to the code and scene data that produce it: the Incense
notice that swallows a walk-on trigger, the Incense effect that outlives a save file, the door to
the Juggernaut Room in Ratayu, and the one spot in Nivora Ravine where a Door of Light works.
Everything on this page is read from the disassembly of `SCUS_942.54` and the field overlay (PROT
entry 0897, load base `0x801CE818`) and from the scene files on the USA disc. None of it is an
emulator capture: where the page says what a debugger shows, that is a prediction from the code.

Shared terms:

| Thing | Where |
|---|---|
| Player actor | `*0x8007C364` (`0x8007C348 + 0x1C`) |
| Movement lock | bit `0x80000` of `player[+0x10]` |
| Player tile | `player[+0x14] >> 7`, `player[+0x18] >> 7` (128-unit tiles) |
| Transient flag word | scratchpad `0x1F800394`; field-VM ops `2E n` / `2F n` set / clear bit `n` |
| System (story) flag bank | `0x80085758`, field-VM ops `0x50..0x7F` |
| Incense window | `0x8007B600` (`gp+0x2E8`), counted in walk ticks |

## Incense trigger bypass

**Status:** explained

**Reported as:** when the notice that the Incense has worn off appears on the step that would have
entered a cutscene trigger or loading zone, the notice wins and the trigger never fires (community
report).

The trigger code never looks at the Incense. What the two share is the **movement lock**, the bit
that tells the game a script or window owns the player.

Each frame the game asks, once, whether the player is free. If so it runs the walk tick, then moves
the player, then compares the player's tile against the last one to see whether a trigger tile was
entered. When the Incense runs out, the walk tick raises the lock so its notice window can take the
pad. That happens after the frame's "is the player free" question and before the move. The player
still moves that frame.

If that move crosses onto a trigger tile, the trigger compare finds a new tile and a locked player.
Its rule for that case is to write the new tile down as handled and do nothing else. Once the
notice is dismissed the player is standing on the trigger tile, and only a fresh crossing fires a
trigger. The trigger is spent without running, and the player walks on.

Incense is simply the one way a player can raise the lock on a chosen frame of free walking. The
window drains by time spent moving, which is why a fixed walking route reproduces the skip.

### Mechanism

The per-frame player routine `FUN_801D1344` runs four things in this order:

1. **Gate** (`0x801D1694..0x801D16A0`): `lw v0,0x10(s2)`, `lui v1,8`, `and`, `bnez 0x801D1704`. A
   locked player skips the walk tick and the mover. The same skip is taken on `0x1F800394 & 0x400`,
   a running teleport timer (`0x8007B6B0 > 0`) and the post-teleport pad hold (`0x8007B6B4 != 0`).
2. **Walk tick** `FUN_801D0B90` (`jal` at `0x801D16EC`).
3. **Mover** `FUN_801D01B0` (`jal` at `0x801D16F4`).
4. **Settle** `FUN_801D1BA0` (`jal` at `0x801D1744`), whose tail calls the walk-on dispatcher
   `FUN_801D1EC4` at `0x801D1D80`. Every path through step 1 arrives here.

The walk tick runs only when the accumulator at `0x801F2274` holds at least `0x21`, and takes
`0x20` off it. The mover feeds the accumulator at `0x801D08EC..0x801D0928`: on a frame with a
non-zero movement pair (`0x8007BDE0` / `0x8007BDE4`) it adds the scratchpad byte `0x1F800393`, the
number of vsyncs the frame spans. One walk tick is therefore `0x20` vsyncs of actual movement.

The Incense part of the walk tick:

```text
801d0cd4  lw    v0,-0x4a00(v1)     ; 0x8007B600, the window
801d0cdc  beq   v0,zero,exit       ; already 0: nothing to do
801d0ce0  addiu v0,v0,-1
801d0ce4  bne   v0,zero,exit
801d0ce8  sw    v0,-0x4a00(v1)     ; delay slot: store window - 1
801d0d04  sw    v0,-0x4bb0(v1)     ; 0x8007B450 = 0x801F2278 (notice record, kind 0x0B)
801d0d08  lw    v0,0x10(a0)        ; a0 = player
801d0d0c  lui   v1,0x8
801d0d10  or    v0,v0,v1
801d0d14  sw    v0,0x10(a0)        ; the lock
801d0d20  jal   0x80020de0         ; spawn the notice actor
```

The lines from `0x801D0D04` on run only on the tick that takes the window from 1 to 0.

The mover tests the lock too, but the branch does not stop movement:

```text
801d01e4  lw   v0,0x10(v0)         ; player[+0x10]
801d01ec  and  v0,v0,v1            ; & 0x80000
801d01f0  bnez v0,0x801d0334       ; locked: skip the menu-button block only
801d0334  addiu s4,zero,8          ; speed select, pad read, stepping
```

`0x801D0334` is the movement body. The lock skips `0x801D01F8..0x801D0330`, which opens the pause
menu, and nothing else.

The dispatcher's tile compare (`0x801D2068..0x801D227C`):

```text
801d2068  tile_x = player[+0x14] >> 7 ; tile_z = player[+0x18] >> 7
801d2088  bne  last_x,tile_x -> crossing
801d2110  0x1F800394 &= ~0x80000   ; re-poll bit cleared on every crossing
801d2140  andi v0,v0,0x600         ; grid cell: is this a trigger tile?
801d2144  beqz v0,0x801d226c       ; no: store tile, return
801d2154  and  v0,v0,v1            ; player[+0x10] & 0x80000
801d2158  bnez v0,0x801d2270       ; LOCKED: store tile, return
801d2168  jal  0x801d5630          ; kind 1 lookup, then jal 0x8003bde0 (record spawn)
801d21c8  jal  0x801d5630          ; kind 0 lookup, then timer 0x8007B6B0 = 0x26
801d2270  sw   s0,-0x4238(v0)      ; last tile = this tile (0x8007BDC8 / 0x8007BDCC)
```

The locked exit at `0x801D2158` sits before both lookups, so it swallows a record spawn and an
intra-scene teleport alike. Afterwards the last tile equals the current tile. The unchanged-tile
arm (`0x801D2090..0x801D20F8`) re-runs a record only while `0x1F800394 & 0x80000` is set, and the
crossing has just cleared that bit.

So the skip needs three things on one frame: the gate passes; the accumulator is at `0x21` or more
with the window at exactly 1; and that frame's movement changes the player's tile into one whose
grid cell has `0x600` bits. The first two are read from the instructions; that they can coincide
with the third is an inference from the call order.

Timing: one Incense adds `0x40` ticks (`FUN_80046870`: add `0x40`, `slti 0x100`, clamp to
`0x100`), which is 2048 vsyncs of movement, about 34 seconds. The cap of `0x100` is about 137
seconds. The Use list refuses another at `0xE0` or more (`FUN_80046898`).

The notice is `FUN_801F1E48`, reached through kind table `0x801F33A4`. It opens a window, waits
for confirm or cancel, clears `0x8007B450` and hands back.

### Which triggers it can and cannot skip

| Trigger | Path | Skippable |
|---|---|---|
| Walk-on record (cutscene, walk-on boss fight, named scene exit) | crossing arm, `FUN_801D5630(1)`, `FUN_8003BDE0` | yes |
| Intra-scene teleport (a map door) | crossing arm, `FUN_801D5630(0)`, timer `0x8007B6B0` | yes, same locked exit |
| A strip more than one tile deep along the path | each further crossing is a new compare | first tile only; the next one fires |
| "Stand here and press" records armed by `2E 13` | unchanged-tile arm | not applicable: only a record that already ran sets the bit |
| Landing-tile record after a teleport | timer half, `0x801D2030..0x801D2058` | no |
| Scene-entry scripts and position polls in the scene's system script | field VM, every tick | no |
| NPC talk, doors opened by body contact | mover touch dispatch | no; contact is re-posted on every step |

The skip lasts for one pass. The record never ran, so its one-shot latch flag is not set, and any
later crossing of the same tile fires it.

Two known skips match one-tile-deep triggers on the disc:

- **Jette.** Scene `chitei2` (Jette's Fortress). The `.PCH` record `(18, 14, 11, 1)` puts
  partition-2 record 11 on the single tile `(18, 14)`. Its header gates are C1 `{0x470}` and C2
  `{0x4F0}`, and its script carries `3E FF 0C`, a scripted battle on formation row 12.
- **Uru Mais dreams.** Scene `uru`. Records 29, 33 and 31 start Vahn's, Noa's and Gala's dream on
  the strips `(110..112, 35)`, `(10..12, 98)` and `(80..82, 99)`. Each is three tiles wide, one
  deep, with no header gates, and runs a scene change to `urudre1`, `urudre2` or `urudre3`.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801D1344` | PROT 0897 | Per-frame player routine; gate `0x801D1694`, calls at `0x801D16EC`, `0x801D16F4`, `0x801D1744` | disassembly |
| `0x801D0B90` | PROT 0897 | Walk tick; drain `0x801D0CD4..0x801D0CE8`, lock `0x801D0D14` | disassembly |
| `0x801D01B0` | PROT 0897 | Mover; lock branch `0x801D01F0`, body `0x801D0334`, accumulator feed `0x801D08EC` | disassembly |
| `0x801D1BA0` | PROT 0897 | Settle; calls the dispatcher at `0x801D1D80` | disassembly |
| `0x801D1EC4` | PROT 0897 | Walk-on dispatcher; locked exit `0x801D2158` | disassembly |
| `0x801D5630` | PROT 0897 | Per-tile trigger lookup | disassembly |
| `0x801F1E48` | PROT 0897 | Wear-off notice; record `0x801F2278`, kind table `0x801F33A4` | disassembly |
| `0x801DA174` | PROT 0897 | Encounter roll skips while the window is non-zero | disassembly |
| `0x80046870` | SCUS | Window top-up: `+0x40`, cap `0x100` | disassembly |
| `0x80046898` | SCUS | Use validator: window `< 0xE0` | disassembly |
| `0x8003BDE0` | SCUS | Record spawn with the C1 / C2 header gates | repo doc |
| `0x8007B600` | RAM | Incense window | disassembly |
| `0x801F2274` | RAM | Walk-tick accumulator, in vsyncs | disassembly |
| `0x8007BDC8`, `0x8007BDCC` | RAM | Last tile seen by the dispatcher | disassembly |
| `0x8007B450` | RAM | Submode cell the notice record is installed in | disassembly |
| `chitei2` `.PCH` `(18,14,11,1)` | disc | Jette trigger tile | disc data |
| `uru` `.PCH` records 29 / 33 / 31 | disc | Dream trigger strips | disc data |

Incense is item `0x8A`, effect class `0x82`.

### Scope

Between the gate and the mover call the only code is the walk tick, and the Incense edge is its
only lock write, so Incense is the only source of this on the walking path. The kingdom overworld
runs the same routine. Every walk-on record tile and every intra-scene teleport tile takes the
locked exit; practical use needs a strip that is one tile deep on the line walked.

Related pages: [field locomotion](../../subsystems/field-locomotion.md),
[the `.PCH` trigger sidecar](../../formats/scene-v12-table.md),
[the pause menu's Incense route](../../subsystems/field-menu.md).

### Verifying it

Break on `0x801D0D14`. On the hit, note the player position, run to `0x801D2068` in the same frame
and compare the new tile against `0x8007BDC8` / `0x8007BDCC`. To force the case, write `1` to
`0x8007B600` and `0x21` to `0x801F2274` on the frame before the player's centre crosses into
`chitei2` tile `(18, 14)`. The prediction: the branch at `0x801D2158` is taken, `0x8003BDE0` is not
called from `0x801D2184`, and flag `0x470` stays clear.

### Open questions

- The community reports that the Jette skip costs the Door of Light for the rest of the game. That
  is not traced. `chitei2`'s region records allow the Door in every story group but one, and no
  script on the disc touches the Door bits (see the last section). A capture of `0x1F800394` after
  the skip, and of whether `FUN_801D9E1C` still reaches `0x801DA058`, would settle it.
- The routine that releases the lock after the notice is not traced.
- The out-of-bounds uses in the community reports are not matched to specific tiles.

## Incense carry-over into a new game

**Status:** explained

**Reported as:** use Incense, then load a different save or start a New Game without resetting the
console, and the no-encounter effect is still running. Speedrunners call the trick the Incense File
Transfer and name the categories that use it "New Game+" (community report).

"New Game+" here is the community's name for this glitch, not a carry-over mode of the kind other
games ship. The only thing that transfers is the remaining Incense steps. The name stuck because the
trick splits the speedrun categories: a run that starts with Incense already burning skips the early
random battles, and it also has the [trigger bypass](#incense-trigger-bypass) available from the
first minutes, long before the item can be bought, so the route looks completely different from a
run without it.

The window is a single number kept in the game program's own memory rather than in the block that
gets saved to the memory card. Loading a file does not restore it and starting a new game does not
clear it, so whatever was left keeps draining as the party walks. Only a reset or power cycle,
which reloads the program, returns it to zero.

Two consequences follow. Saving, using Incense and reloading keeps both the effect and the item.
And because the wear-off notice still appears, the trigger bypass above becomes available in a
file that owns no Incense.

### Mechanism

- The window is at `0x8007B600`, inside the `SCUS_942.54` load image (`0x80010000`, size
  `0x6B800`, so the image ends at `0x8007B800`). Its value in the file is zero.
- The save block is a `0x1A18`-byte copy of the live window at `0x80084140`. `0x8007B600` is
  below it.
- The disc holds seven instructions that touch the window. Four are in SCUS, in gp-relative form
  (`0x2E8(gp)`): `0x80046870`, `0x8004687C`, `0x8004688C` (the top-up) and `0x80046898` (the
  validator). Three are in PROT 0897, in absolute form: `0x801D0CD4` (read), `0x801D0CE8`
  (decrement) and `0x801DA174` (encounter test). None stores a constant.
- The mode-entry reset `FUN_8001DCF8` writes a fixed list of globals (among them `0x8007B798`,
  `0x8007BA90` and `0x1F8003A0`) and, for a new game, calls `FUN_80034A6C` at `0x8001DED8` to
  rebuild the saved window. The Incense window is not on the list.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x8007B600` | SCUS data | Incense window; file value `0` | disc data |
| `0x80046870`, `0x8004687C`, `0x8004688C` | SCUS | Top-up: the only writes besides the drain | disassembly |
| `0x80046898` | SCUS | Validator read | disassembly |
| `0x801D0CD4`, `0x801D0CE8` | PROT 0897 | Walk-tick read and decrement | disassembly |
| `0x801DA174` | PROT 0897 | Encounter test | disassembly |
| `0x8001DCF8` | SCUS | Mode-entry reset; called from `0x800160AC`, `0x8001D7D8`, `0x8002577C`, `0x80025FD4`, `0x80055D5C` | disassembly |
| `0x80034A6C` | SCUS | New-game data init | repo doc |
| `0x80084140` + `0x1A18` | RAM | The window that is saved and reset | repo doc |

### Scope

The most that can be carried is the cap, `0x100` ticks, about 137 seconds of walking. Any other
global kept outside `0x80084140..0x80085B58` and off the reset list behaves the same way; the
encounter step counter `0x8007B5FC` is a candidate and is not checked here.

See [the save screen](../../subsystems/save-screen.md) for the saved window.

### Verifying it

Write `0x100` to `0x8007B600`, reach the title screen without resetting the console (a Game Over
does it), start a New Game and read the address on the first frame of free walking. The prediction
is `0x100` less the ticks walked. After a reset it reads zero.

## Juggernaut's Room door

**Status:** partially explained

**Reported as:** the two guards in front of the Juggernaut Room in Ratayu castle, who stand aside
only for a party at level 99, can be passed early by standing at an exact spot in front of one of
them (community report).

The door has no lock of its own while the guards are there. The level-99 rule is enforced by two
guard bodies standing in front of an open door, and by nothing else.

The reason is the story. On the first trip through the castle this door is the route to the boss,
so it has to open. After the boss the room is closed again, and it is closed by placing two guards
in front of it; the door's script is left as it was. A party that reaches level 99 and talks to
Saryu gets the guards removed and a key, and only then does the door ask for the key.

So any way of touching the door or stepping on the tile behind the guards enters the room. How the
trick gets past the guards is the part not traced: the collision arithmetic leaves no gap between
them.

### Mechanism

The door is in `retockin` (Ratayu interior) and leads to `jagaroom` (Juggernaut Room).

The door prop is partition-0 record 0, bound to tile `(28, 58)`. Its touch script begins:

```text
+0x0E  76 50   test flag 0x650 -> +0x5B    ; already unlocked: open
+0x12  76 4F   test flag 0x64F -> +0x19    ; guards dismissed: insert the key
+0x16  26      jump            -> +0x5B    ; neither flag: open anyway
+0x19  56 50   set flag 0x650 ; 4C 52 BF   ; take item 0xBF
+0x5B  door swing ...
+0xAC  3F ..   scene change to jagaroom
```

A flag test jumps when the flag is set. With `0x64F` and `0x650` both clear the script goes
straight to the open-and-leave sequence.

The `.PCH` record `(27, 57, 32, 1)` puts partition-2 record 32 on tile `(27, 57)`. That record has
no header gates and its body is the same scene change, with no flag test.

The guards are partition-1 records 20 and 21, at tile columns 27 and 28. Their entry script parks
them off the map unless `!0x64F && (0x33E || (0x357 && !0x502))`.

Saryu is partition-1 record 29. It compares each of the three party slots against 99 (`4E pp 20
63`). When all pass and `0x64F` is clear it sets `0x64F`, moves both guards off the map and gives
item `0xBF`. That is the only place the disc sets `0x64F`.

With `0x33E`, `0x33B` and `0x357` all clear, the scene's live encounter regions have a real
encounter rate and the guards are parked: this is the castle as a dungeon, with the door open.
That the designers closed the room afterwards with guards alone is an inference from these states.

**What is not traced.** A guard blocks through the actor probe `FUN_801CFC40`, which
`FUN_801CFE4C` calls three times per step with offsets from the table at `0x801F21B4`: three
points 63 or 64 units ahead, spread 32 to each side. A moving actor stops a probe point within 40
units on each axis. That makes each guard a square 144 units wide that the player's centre cannot
enter, and two guards one tile (128 units) apart overlap by 16 units. A straight walk between them
does not fit.

One reading consistent with the community wording is that the interact button does the work. It
uses a different test, a single point 64 units ahead of the player, checked through
`FUN_801CF9F4`. If that point reaches the door prop from in front of the guards, the door's touch
event `FUN_801D5B5C` runs its script from there. This is a hypothesis; the door's collision
footprint and the order the candidates are tested in are not computed.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `retockin` P0[0], MAN offset `0x172` | disc | Door script; tests at `+0x0E`, `+0x12`, jump at `+0x16`, exit at `+0xAC` | disc data |
| `retockin` P2[32], MAN offset `0x30E2` | disc | Ungated walk-on exit to `jagaroom` | disc data |
| `retockin` P1[20] `0x1424`, P1[21] `0x151A` | disc | The two guards | disc data |
| `retockin` P1[29], MAN offset `0x1E17` | disc | Level compares at `+0x26C` and `+0x387`; sets `0x64F` at `+0x503`; gives `0xBF` at `+0x5DA` | disc data |
| `.MAP` trigger `(28,58,0,0)` | disc | Binds the door prop to its tile | disc data |
| `.PCH` trigger `(27,57,32,1)` | disc | Walk-on tile behind the guards | disc data |
| `0x64F`, `0x650` | flag bank | Guards dismissed; door unlocked | disc data |
| `0x33E`, `0x357`, `0x502` | flag bank | Story states that place the guards | disc data |
| `0x801CFE4C`, `0x801CFC40` | PROT 0897 | Per-step collision and the actor probe | repo doc |
| `0x801F21B4` | PROT 0897 | Actor probe offsets (file offset `0x2399C`) | disc data |
| `0x801CF9F4`, `0x801D5B5C` | PROT 0897 | Interact box test; touch event post | repo doc |

### Scope

`edretoin`, the ending's copy of the scene, carries the same door record and walk-on record.

See [field locomotion](../../subsystems/field-locomotion.md) for the collision probes and
[scene names](../scene-names.md) for the scene table.

### Verifying it

In `retockin` with `0x33E` set and `0x64F` clear, move the player to tile `(27, 57)`. The
prediction is a call `FUN_8003BDE0(27, 57, 32, 1)` and the scene change, which shows the tile has
no gate. To settle how the trick works, perform it with breakpoints on `FUN_801D5B5C` and on the
return of `FUN_801CF9F4`, and log which actor the probe hit and the player's position.

### Open questions

- How the player gets past, or reaches through, the guards.
- The exact position the guards' z byte `0xB8` encodes.
- What `jagaroom` does when entered after the boss with `0x64F` clear.

## Noa's sigil and the Door of Light in Nivora

**Status:** explained

**Reported as:** in Nivora Ravine, where the Door of Light is greyed out, standing on the glowing
crest on Noa's route makes it usable (community report).

The crest does nothing to the Door. It stands inside a small rectangle of the map that is marked
"Door of Light allowed", surrounded by tunnels that are marked "not allowed".

Every dungeon map is divided into rectangles that set the random encounters for that area. Each
rectangle also carries one bit saying whether a Door of Light may be used there. Every time the
player steps onto a new tile the game finds the first rectangle that contains it and copies that
bit into a flag; the item menu greys the Door when the flag says no.

In Nivora the tunnel rectangles say no. A nine-by-nine box over the ravine's two-level crossing
says yes, and it comes earlier in the list, so it wins wherever it overlaps. The crest sits on the
east edge of that box. Step onto it and the flag flips; step one tile further east and it flips
back.

### Mechanism

The Use list builder `FUN_80030628` dims the Door of Light (item `0x88`) when bit `0x100000` of
`0x1F800394` is set, and the Door of Wind (`0x89`) on bit `0x200000`:

```text
80030930  addiu v0,zero,0x88
8003093c  lui   v0,0x1f80 ; lw v0,0x394(v0)
80030944  lui   v1,0x10   ; and v0,v0,v1
8003094c  bnez  v0,0x800309ec       ; dimmed row
```

Those bits have one writer, the region re-read in `FUN_801D9E1C`, which runs on every tile step:

```text
801da064  lbu  v0,8(s2)             ; region byte +8
801da068  lui  a0,0x30
801da080  or   a0,v1,a0             ; 0x1F800394 |= 0x300000: both Doors blocked
801da090  sltiu v0,v0,0xc           ; needs a region stride of 12 or more
801da0b8  andi v0,v0,0x80           ; bit 7: clear 0x100000, Door of Light allowed
801da0d8  andi v0,v0,0x40           ; bit 6: clear 0x200000, Door of Wind allowed
```

The match before it (`0x801D9FE8..0x801DA050`) takes the first record in the live story group
whose inclusive tile box contains the player. The same block stores where the Door of Light goes:
`0x80084628 = region[+9] | (region[+5] & 0x7F) << 8` (the overworld map) and `0x80084624`,
`0x8008462C` from `region[+0xA]`, `region[+0xB]` (the tile).

No field script changes these bits. Across the disc, ops `2E` and `2F` appear only with bit
numbers 0, 16 to 19, 22 and 24 to 27. A scene forbids the Door through its region records alone.

Nivora is scene `nilboa`. Its region table has 38 twelve-byte records in two story groups (flag
`0x456` selects records 0 to 18, otherwise 19 to 37), with the same Door bits in both. Every
record returns to `map03` tile `(93, 66)`.

| Records | Tile box (x, z) | Encounter rate | Byte `+8` | Door of Light |
|---|---|---|---|---|
| 0 / 19 | 75..128, 90..128 | 0 | `A2` / `A1` | allowed |
| **1 / 20** | **46..54, 60..68** | 0 | **`80`** | **allowed** |
| 2 / 21 | 55..79, 0..32 | 0 | `80` | allowed |
| 3..6 / 22..25 | strips at x 0..26 | 20 or 0 | `80` / `A0` | allowed |
| 7..18 / 26..37 | the twelve tunnel boxes | 20 | `20` | blocked |

Record 1 is `2E 3C 36 44 00 81 00 01 80 87 5D 42`; a tunnel record, 7, is
`2E 24 35 55 14 81 07 0D 20 87 5D 42`. The box of record 1 lies inside tunnel boxes 7
(`46..53, 36..85`) and 12 (`26..79, 36..89`) and precedes both.

The box is the ravine's two-level crossing. The "rotating road" prop is bound at its centre
`(50, 64)`, and the walk-on strips that switch between the upper and lower level run along its
four edges: `(46, 63..65)` and `(54, 62..64)`, `(48..51, 60)` and `(48..51, 68)`.

The crest is partition-0 record 24. The scene's system script polls a box test on the player over
tiles x `53..55`, z `62..64`; a hit lights the crest and shows its message. Columns 53 and 54 of
that box are inside record 1 (x up to 54). Column 55 is not, and falls to tunnel record 12.

So the crest's own script plays no part. Any walkable tile of the nine-by-nine box has the same
effect; the crest is the landmark that happens to be in it.

**Why the tunnels are blocked.** During the party split (flag `0x00D`) each character is alone.
The intended way out is a prompt at each tunnel mouth (partition-2 records 4, 5 and 6, gated on
`0x00D`), which chains to record 28, the record that regroups the party and clears `0x00D`. A Door
of Light used from the crossing leaves for `map03` with `0x00D` still set. What the game does next
in that state is not traced; that blocking the tunnels exists to force the prompt is an inference.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x80030628` | SCUS | Use list builder; Door tests at `0x80030930..0x80030974` | disassembly |
| `0x801D9E1C` | PROT 0897 | Region re-read; match `0x801D9FE8..0x801DA050`, Door bits `0x801DA064..0x801DA0F0` | disassembly |
| `0x1F800394` bits `0x100000`, `0x200000` | scratchpad | Door of Light / Door of Wind blocked | disassembly |
| `0x80084624`, `0x80084628`, `0x8008462C` | RAM | Door of Light return point | disassembly |
| `*0x801C6EA4` `+0x28`, `+0x5F` | RAM | Region table pointer and record stride | disassembly |
| `0x801D8A58` | PROT 0899 | Door of Light confirm; consumes item `0x88` | repo doc |
| `nilboa` region records 1 and 20 | disc | The allowed box, tiles `46..54, 60..68`, byte `+8 = 0x80` | disc data |
| `nilboa` region records 7..18, 26..37 | disc | Tunnel boxes, byte `+8 = 0x20` | disc data |
| `nilboa` P0[24], P0[7] | disc | Crest prop; rotating-road prop at `(50, 64)` | disc data |
| `nilboa` P1[0] `+0x4D4` | disc | Crest box test, tiles `53..55, 62..64` | disc data |
| `nilboa` P2[4..6], P2[28] | disc | Tunnel-mouth prompts; regroup record that clears `0x00D` | disc data |

### Scope

Across every scene's bundle MAN, `nilboa` is the only scene where a live story group mixes allowed
and blocked records so as to leave a walk-in pocket: 7 of 19 allowed, in both groups. The one
other mixed group is `rikuroa`'s default group, with a single blocked record among eleven. Every
other scene is all-allowed or all-blocked per group. The per-scene variant MANs are not part of
this scan.

The two bits change only when a region record matches. On a tile no record covers, or in a scene
whose region records are shorter than 12 bytes, the Door state is whatever the last match left.

See [the encounter and region tables](../../formats/encounter.md) and
[the region battle setup](../../subsystems/script-vm.md#the-region-battle-setup).

### Verifying it

In `nilboa` during the split, watch bit 20 of `0x1F800394`. The prediction: it reads 1 in the
tunnels and drops to 0 on the step that brings the player's tile to x `46..54`, z `60..68`. A
breakpoint at `0x801DA0C8` stops on the clear, with `s2` pointing at the matched record.

### Open questions

- Whether the box was left allowed on purpose or kept a default byte nobody restricted.
- Which of the three split characters can reach the box; the community report says Noa.
- What the game does after a lone character leaves with `0x00D` set.
