# Ra-Seru levels, Orb and Jette's Clone

Three community-reported oddities around summons and one boss, each read against the USA executable
`SCUS_942.54` and its overlays: the Ra-Seru summons that arrive at Palma's level, the Orb summon
whose legs sometimes stay folded, and what Jette's Clone does to his MP and seat. The Ra-Seru case
and the Orb case each carry observations from a running game, made on poked states: a value was
written by hand to set the situation up, and each section says which value. Jette's Clone is static
analysis of the disassembly and the disc data. The Ra-Seru grant is also read on the Japanese
(`SCPS_100.59`) and French (`SCES_019.44`) executables; the other two cases are examined on the USA
disc only.

Conventions used below: the character record is `0x80084708 + char * 0x414` (Vahn 0, Noa 1,
Gala 2; layout in [`save-record.md`](../../formats/save-record.md)). The battle context pointer is
the word at `0x8007BD24` ("ctx"). The battle actor pointer table is `0x801C9370`, eight words:
`0..2` party, `3..6` monsters, `7` the summon creature. Actor records are `0x2D4` bytes apart
([`battle.md`](../../subsystems/battle.md#battle-actor-record)).

## Meta, Terra and Ozma inherit Palma's level

**Status:** explained
**Reported as:** Getting Meta, Terra or Ozma sets their level to Palma's level (community report).

The summons that come from equipment are not learned for good. Each time the game rebuilds a
character's stats it checks the eight equipment bytes and makes sure the matching summon is in the
spell list. When a Talisman comes off, its summon is removed and the summon's level and experience
are parked in a small per-character stash, so they come back when the Talisman goes on again.

Each summon has a stash slot number, written as a literal in the code. Palma uses slot 0, Mule 1,
Horn 2, Jedo 3 and the Evil Talisman's summon 4. The three arms that add Meta, Terra and Ozma also
pass slot 0.

Each Ra-Seru grows through story scenes, and the scene that gives it its last form is also the
moment its summon appears: Meta under Sol Tower, Terra on Mt. Rikuroa, Ozma in East Voz Forest. The
summon is inserted with the level and experience read out of the stash that only Palma ever fills.
It arrives with whatever Palma had on that same character the last time the Earth Talisman was
taken off them. If that character never had the Earth Talisman removed, the stash is still zero and
the insert routine raises level 0 to level 1, which is the ordinary result.

The exact statement is therefore narrower than the report: Meta, Terra and Ozma inherit the
*stashed* Palma level and experience of the *same character*, not the live level of a Palma that is
currently equipped on someone.

The level half of this is observed in a running game, captured on a poked state: with Vahn's
stash level byte written to 7 and his Ra-Seru byte written to the final form, Meta appeared at the
head of his spell list at level 7. The Japanese and French executables pass slot 0 for the three
Ra-Seru summons too, so the behaviour is the same in all three builds.

### Mechanism

The spell list is three parallel arrays plus the stash:

| Record offset | Field |
|---|---|
| `+0x13C` | Spell count (u8). |
| `+0x13D + i` | Spell id (u8), the full id: `0x9A` Palma, `0x9E` Meta. |
| `+0x161 + i` | Spell level (u8). |
| `+0x008 + i * 4` | Spell experience counter (u32). |
| `+0x2B0 + slot * 0x14` | Stash slot. Bytes `+1..+4` hold the u32 experience, byte `+5` the level. |
| `+0x196..+0x19D` | The eight equipment bytes. |

The grant scan in `FUN_80042558` loops over all eight equipment bytes (`lbu v1,0x196(v0)` at
`0x80042718`) and compares each against a fixed list of item ids:

| Item id | Item | Spell (`a1`) | Stash slot (`a2`) | Arm |
|---|---|---|---|---|
| `0x72` | Earth Talisman | `0x9A` Palma | 0 (`clear a2` at `0x80042808`) | `0x800427EC` |
| `0x73` | Water Talisman | `0x9B` Mule | 1 (`li a2,0x1` at `0x80042824`) | `0x8004280C` |
| `0x74` | Light Talisman | `0x9C` Horn | 2 (`0x80042844`) | `0x80042828` |
| `0x75` | Dark Talisman | `0x9D` Jedo | 3 (`0x80042860`) | `0x80042848` |
| `0x76` | Evil Talisman | `0x99` Evil Seru Magic | 4 (`0x80042878`) | `0x80042864` |
| `0x09` | Ra-Seru Meta, form 9 of 9 | `0x9E` Meta | 0 (`clear a2` at `0x800427AC`) | `0x80042794` |
| `0x11` | Ra-Seru Terra, form 8 of 8 | `0x9F` Terra | 0 (`clear a2` at `0x800427CC`) | `0x800427B0` |
| `0x19` | Ra-Seru Ozma, form 7 of 7 | `0xA0` Ozma | 0 (`clear a2` at `0x800427E8`) | `0x800427D0` |

Each arm first asks `FUN_800431FC(char, spell)` whether the spell is already listed, and only when
it is absent falls to the shared `jal 0x800432bc` at `0x8004287C`. The Meta arm:

```
80042794  jal 0x800431fc      ; delay slot: li a1,0x9e
8004279c  bne v0,zero,0x80042884
800427a4  li a1,0x9e
800427a8  j 0x8004287c        ; delay slot: clear a2   (stash slot 0)
```

`FUN_800432BC(char, spell, slot)` inserts at the head of the list from the stash:

```
800432f4  lbu t1,0x2b5(v0)    ; level  = stash[slot] byte +5
800432f8  lbu v1,0x2b4(v0)    ; experience bytes +4..+1, reassembled below
8004331c  bne t1,zero,0x80043328
80043324  addiu t1,t1,0x1     ; level 0 becomes 1
80043378  sb t3,0x13d(t0)     ; id
8004337c  sb t1,0x161(t0)     ; level
80043380  sw t2,0x8(t0)       ; experience
8004338c  sb v0,0x13c(t0)     ; count + 1
```

The insert reads the stash and leaves it as it was.

The only writer of a stash slot is `FUN_80042DBC(char, spell, slot)`, which removes a spell and
parks it: experience through the four `sb` at `0x80042E34..0x80042E5C`, level through
`lbu v0,0x161(v0)` at `0x80042E64` and `sb v0,0x2b5(v1)` at `0x80042E6C`. Its one caller is the
revoke scan of the same `FUN_80042558` (`jal` at `0x80042978`). That scan walks the spell list and
dispatches `id - 0x99` through a five-entry jump table at `0x800151D0`, bounded by
`sltiu v0,v1,0x5` at `0x800428BC`. Each arm asks `FUN_80043264(char, item)` whether the Talisman is
still in one of the three accessory bytes, and if not removes the summon into its own slot:
`0x9A` to slot 0, `0x9B` to 1, `0x9C` to 2, `0x9D` to 3, `0x99` to 4.

The table stops at `0x9D`, so Meta, Terra and Ozma are never removed and never write a stash. Slot
0 is written only by a Palma removal, and read by a Palma re-equip and by all three Ra-Seru grants.

### Where the final form is written

A Ra-Seru changes form when a scene script runs field-VM op `0x3B`, `SET_EQUIP_SLOT`
(`3B, char << 4 | slot, item`; see [`script-vm.md`](../../subsystems/script-vm.md)). Its arm is
`0x801E07DC..0x801E081C` of `FUN_801DE840` in PROT entry 0897:

```
801e07e4  lbu v0,0x0(s6)      ; selector
801e07e8  lbu v1,0x1(s6)      ; item id
801e07ec  srl s2,v0,0x4       ; character
801e07f0  andi s7,v0,0xf      ; slot
801e0810  jal 0x80042558      ; the aggregator, grant scan included
801e0814  sb v1,0x75e(v0)     ; delay slot: record +0x196 + slot = item
```

The store sits in the delay slot of the call, so the opcode that changes the byte also runs the
grant scan. Every Ra-Seru stage on the disc is one such op, and each final form is written at
exactly one site:

| Scene | Script bytes | Meaning |
|---|---|---|
| `korb3`, Sol Tower underground | `3B 03 09` | Vahn, slot 3: Ra-Seru Meta form 9. Grants Meta. |
| `rikuroa`, Mt. Rikuroa | `3B 12 11` | Noa, slot 2: Ra-Seru Terra form 8. Grants Terra. |
| `vozz`, East Voz Forest | `3B 23 19` | Gala, slot 3: Ra-Seru Ozma form 7. Grants Ozma. |

Noa's Ra-Seru is equipment index 2 (`+0x198`); Vahn's and Gala's is index 3 (`+0x199`). The scan
reads all eight bytes, so the index makes no difference to it.

The aggregator has other callers. One of them runs when a battle returns to the field (the call at
`0x8003B2B8`), so an equipment byte changed by any means takes effect at the latest at the next
battle exit.

### The same code on the other discs

| Build | Meta, Terra, Ozma slot arguments | Shared insert | Evidence |
|---|---|---|---|
| USA `SCUS_942.54` | `clear a2` at `0x800427AC`, `0x800427CC`, `0x800427E8` | `FUN_800432BC` | `disassembly` |
| Japan `SCPS_100.59` | `move a2,zero` in the delay slot of each arm's `j 0x80043a88`; arms end at `0x800439B8`, `0x800439D8`, `0x800439F4` | `0x800440EC` | `disassembly` |
| France `SCES_019.44` | `move a2,zero` at `0x800430F8`, `0x80043118`, `0x80043134` | `0x80043C08` | `disassembly` |

On the Japanese executable Palma's arm (`0x80043A14`) passes slot 0 as well, Mule 1, Horn 2, Jedo 3
and the Evil Talisman 4; the insert reads `lbu t0,0x2b5` and `lbu v1,0x2b4` and writes `+0x13D` and
`+0x161`; and the revoke scan is bounded by the same `sltiu v0,v1,5` on `id - 0x99`, at
`0x80043AC0`. The French executable is the USA code relocated.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x80042558` | SCUS | `FUN_80042558`, the stat and equipment aggregator. Grant scan `0x80042714..0x8004288C`, revoke scan `0x80042894..0x80042990`. | `disassembly` |
| `0x800427AC`, `0x800427CC`, `0x800427E8` | SCUS | The three `clear a2` that give Meta, Terra and Ozma stash slot 0. | `disassembly` |
| `0x80042808` | SCUS | Palma's `clear a2`, the same slot. | `disassembly` |
| `0x800432BC` | SCUS | `FUN_800432BC`, head insert from a stash slot, with the level floor at `0x8004331C..0x80043324`. Seen entered with `a0 = 0`, `a1 = 0x9E`, `a2 = 0`, return address `0x80042884`. | `disassembly`, `capture` (poked) |
| `0x80042DBC` | SCUS | `FUN_80042DBC`, remove a spell and park it in a stash slot. | `disassembly` |
| `0x800431FC` | SCUS | `FUN_800431FC`, "is this spell in the list". | `disassembly` |
| `0x80043264` | SCUS | `FUN_80043264`, "is this item in accessory bytes `+0x19B..+0x19D`". | `disassembly` |
| `0x800151D0` | SCUS (file offset `0x59D0`) | Five-word jump table of the revoke scan: `0x8004295C`, `0x800428DC`, `0x800428FC`, `0x8004291C`, `0x8004293C`. | `disc data` |
| `0x801E07DC..0x801E081C` | PROT 0897 | Op `0x3B` arm of `FUN_801DE840`: the equipment-byte store at `0x801E0814`, in the delay slot of the aggregator call at `0x801E0810`. | `disassembly` |
| `0x8003B2B8` | SCUS | Aggregator call on the return from battle to the field. | `capture` (poked) |
| `3B 03 09`, `3B 12 11`, `3B 23 19` | scene scripts of `korb3`, `rikuroa`, `vozz` | The three final-form writes. | `disc data` |
| `0x800849BD`, `0x80084DD1`, `0x800851E5` | RAM | Stash slot 0 level byte for Vahn, Noa, Gala (record `+0x2B5`); the four bytes below each hold the experience. | `disassembly`, `capture` (poked, Vahn) |
| `0x800848A1`, `0x80084CB4`, `0x800850C9` | RAM | The Ra-Seru equipment byte of Vahn (index 3), Noa (index 2) and Gala (index 3). | `disc data`, `capture` |
| `0x80084844`, `0x80084845`, `0x80084869`, `0x80084710` | RAM | Vahn's spell count, first id, first level, first experience word. | `disassembly` |
| item ids `0x01..0x09`, `0x0A..0x11`, `0x13..0x19`, `0x72..0x76` | SCUS item-name table | The nine forms of Ra-Seru Meta, eight of Terra, seven of Ozma, and the five Talismans ([`item-table.md`](../../formats/item-table.md)). `0x12` and `0x1A` are unnamed gaps; `0x1B` is the Ra-Seru Blade. | `disc data` |
| spell ids `0x99..0xA0` | SCUS spell table | The summon ids ([`spell-table.md`](../../formats/spell-table.md)). | `disc data` |

That the stash has no other reader or writer rests on a search of the dumped functions for the
`0x2B1..0x2B5` record offsets and their `0x80084140`-relative forms; it is strong but not a proof
over every byte of every overlay.

### Scope

- Three spells are affected (`0x9E`, `0x9F`, `0xA0`) and one donor (`0x9A` Palma). Mule, Horn, Jedo
  and Evil Seru Magic have their own slots and do not leak.
- The experience counter is copied with the level, so the inherited summon is also the same
  distance from its next level as the parked Palma was. This half is read from the disassembly
  only; the stash experience was zero in the captured run.
- It is per character. Vahn's Meta reads Vahn's stash, Noa's Terra reads Noa's, Gala's Ozma reads
  Gala's. A Palma levelled on Vahn does nothing for Noa's Terra.
- It needs a Palma *removal* before the grant. A Palma that is still equipped has not been parked,
  so the grant reads whatever the previous removal left, or zero (level 1) if there never was one.
- It is one-way. Meta's own progress never flows back to Palma.
- The stash area is fourteen slots wide; this code addresses slots `0..4` and bytes `+1..+5` only.
- The widely circulated cheat codes "Activate Terra at Lv9" and "Activate Ozma at Lv9" write `0x12`
  to `0x80084CB4` and `0x1B` to `0x800850C9`. Neither value is a Ra-Seru form (`0x12` is an unnamed
  gap, `0x1B` is the Ra-Seru Blade) and neither matches an arm of the grant scan, so those two
  codes never insert a summon through it. The Meta code writes `0x09` to `0x800848A1`, which does.
  The three values are each "first form + 8", which is the final form for Meta alone
  (`inference` for how the numbers came about).

### Verifying it

Observed in a running game, captured on a poked state. The run starts from a retail save state in
Rim Elm with Vahn alone, whose record holds Gimard at level 2, an all-zero stash slot 0 and Ra-Seru
Meta form 1. Two bytes are written by hand: `0x800849BD = 0x07` (stash slot 0 level) and
`0x800848A1 = 0x09` (the final form). At the next battle exit the aggregator runs,
`FUN_800432BC` is entered with `a0 = 0`, `a1 = 0x9E`, `a2 = 0`, and on the next frame Vahn's list
reads ids `9E 81`, levels `07 02`, with the stash unchanged. Because both bytes were written by
hand, this shows the read side on live code: the grant reads stash slot 0 and uses its level. The
battle in that run is a forced formation (the monster row was rewritten as the fight loaded), which
touches nothing in the character record.

The write side shows in saves that were played, with nothing poked: records in which the Earth
Talisman has been taken off read level 1 in stash slot 0, and records in which it never was read
zero. No save examined carries a parked Palma above level 1, so every unpoked Ra-Seru summon in
them is at the level floor and they do not discriminate on their own; the poked run does.

To repeat it:

1. Equip the Earth Talisman on Vahn, raise Palma (or write its level byte in `0x80084869..`), then
   unequip it. `0x800849BD` now holds Palma's level and `0x800849B9..0x800849BC` its experience.
2. With Meta not yet in the list, set Vahn's Ra-Seru byte `0x800848A1` to `0x09` (for Noa,
   `0x80084CB4` to `0x11`; for Gala, `0x800850C9` to `0x19`). A poke alone does nothing until the
   aggregator runs: on any equipment change, the script op, or the next return from a battle.
   Afterwards `0x80084845` reads `0x9E` and `0x80084869` equals the stash byte.
3. Control: zero `0x800849B9..0x800849BD` first. Meta then arrives at level 1.
4. A breakpoint at `0x8004287C` shows `a1 = 0x9E`, `a2 = 0` on the grant.

### Open questions

- The experience half of the copy is not observed: repeating the capture with a non-zero counter
  in `0x800849B9..0x800849BC` and reading `0x80084710` afterwards settles it.
- A fully played instance (Earth Talisman worn, Palma levelled, Talisman removed, then the story
  scene) is not captured; the poked run replaces the first three steps with one byte write.

## Orb's legs sometimes do not unfold

**Status:** partially explained
**Reported as:** Orb's animation can glitch sometimes: now and then the legs of the player's Orb
summon do not come out of its sphere during the summon animation. It is hard to recreate, and has
been seen in Rogue's Tower among other places (community report).

Orb's creature arrives curled inside its sphere, drops to the ground, and on the frame it lands
the game queues the "legs unfold" animation to start when the current animation finishes. Nothing
forces it to start; it waits its turn.

The game also has a per-actor "go back to idle when this animation ends" switch, which a hit
reaction uses so the actor returns to its standing pose afterwards. The summon creature's actor is
reused from one summon to the next, and that switch is not reset when a new creature is put in it.

If the switch is on, the queued unfold is thrown away at the very moment it should start, and
idle - the curled pose - is played instead. Orb asks for the unfold only once, so the creature
stays in its sphere for the rest of the spell while the heal, the captions and the timing all play
normally.

Turning the switch on by hand reproduces exactly that picture: captured on a poked state, it shows
what the switch does, not who flips it. What turns it on during ordinary play is not found, which
is why this is partially explained and not explained. It is the same stale switch, read by the
same instructions, that parks a monster in the
[`0x19` approach softlock](../../subsystems/battle-action-exit-gates.md#the-stale-field-0x1dc-bit-2-the-exit-to-idle-anim-event-flag)
known from the Gaza fight; there Kemaro's put-back leaves it set on a monster, here something leaves it set
on the summon seat.

### Mechanism

The cast module is PROT entry 0911, loaded at `0x801F69D8`; its tick is `FUN_801F69D8` and its
phase byte is `ctx[+0x279]` (see [`cast-module.md`](../../subsystems/cast-module.md)). The creature
is battle actor slot 7. Three bytes of the actor steer its clip: `+0x1D9` the clip playing,
`+0x1DA` the clip staged to play next, and `+0x1DC` the animation event flags, whose bit `0x4`
(bit 2) means "exit to idle at the end of this clip".

The creature has two clips. Clip 0 is 18 frames long: the body rides inside the sphere and the
legs come part-way out near its end. Clip 1 is 29 frames long: the full unfold, ending with the
creature standing and the small sphere overhead.

1. **Seat.** Phase 3 pulls sixteen light motes in to the caster, then seats the creature:
   `jal 0x801f19ec` at `0x801F70B0`, the clip cursor preset to `0x30` of clip 0's 288 units
   (`sh v0,0x68(v1)` at `0x801F70E4`, on the node at `actor[+0x22C]`), and the actor's height set
   to `-0x320` (`0x801F70F0`).
2. **Drop.** Phase 4 is a descent, not a timer. Each pass adds the frame delta times a speed
   scalar to the height (`0x801F73DC..0x801F7400`) and holds while it is negative.
3. **Landing frame.** On the pass the height reaches zero the module queues the unfold by adding
   one to the staged-clip byte, and sets no commit flag:

   ```
   801f7408  bltz v0,0x801f7fb4      ; still above the ground: hold
   801f7410  lbu  v0,0x1da(s4)
   801f7414  sh   zero,0x36(s4)      ; height = 0
   801f7418  addiu v0,v0,0x1
   801f741c  sb   v0,0x1da(s4)       ; staged clip = staged clip + 1
   ```

   This is the module's only write to `+0x1DA`. It then seeds the next phase's countdown
   (`0x801F7420..0x801F7440`); phases 5, 9 and 10 advance on their own countdowns whatever the
   creature is playing.
4. **Natural end of the curled clip.** With no commit flag, the staged clip waits for clip 0 to
   end. The animation tick `FUN_80047430` handles that end:

   ```
   80047b30  lbu  v0,0x1dc(s2)
   80047b38  andi v0,v0,0x4
   80047b3c  beq  v0,zero,0x80047b48
   80047b44  sb   zero,0x1da(s2)     ; bit 0x4 set: staged clip = 0 (idle)
   80047b50  andi v0,v0,0xf8
   80047b54  jal  0x8004ad80         ; commit whatever is staged
   80047b58  sb   v0,0x1dc(s2)       ; +0x1DC &= 0xF8
   ```

5. **Two outcomes.** With bit `0x4` clear, clip 1 is committed and the legs unfold. With it set,
   the staged byte goes back to 0, clip 0 is committed again and loops, and nothing stages clip 1
   a second time.

What makes the bit survive into a new summon: the seat routine `FUN_801F19EC` clears the playing
clip (`sb zero,0x1d9(v1)` at `0x801F1DD8`) and writes `+0x1DD` and `+0x21D`, but has no store to
`+0x1DC`. Of the three bytes that steer the creature's clip, `+0x1DC` is the one a new summon
inherits from whatever was done to slot 7 before.

Measured on one cast with nothing written beyond the injected action, in display frames counted
from the seat:

| Frame | Event | Slot 7 |
|---|---|---|
| 0 | Creature seated above the caster. | cursor 48 of 288, `+0x1D9` 0, `+0x1DA` 0 |
| 100 | Lands; the unfold is queued. | cursor 120, `+0x1DA` 0 to 1 |
| 187 | Clip 0 ends; clip 1 is committed. | `+0x1D9` 0 to 1, clip length 18 to 29 frames |
| 281 | The "Holy Orb" close-up begins. | unfold about 40% done |
| 419 | Unfold complete; the creature stands. | |

### What the captures show

Every cast below is injected: at the point where a party member's action is seeded, the queued
action is rewritten to Orb (spell `0x89`), and the game then loads and runs the module itself.
Where something else was written by hand, the table says so.

| Cast | Written by hand | At the end of clip 0 | Rest of the cast |
|---|---|---|---|
| Reference | nothing | `+0x1DC` 0; `+0x1D9` / `+0x1DA` go 0 / 1 to 1 / 1 | Unfolds and stands. |
| Bit set early | slot 7 `+0x1DC = 4`, before the creature is seated | the bit is still 4 after the seat; `+0x1DA` 1 to 0, `+0x1D9` stays 0, `+0x1DC` to 0, cursor wraps | Clip 0 loops to the end. |
| Bit set late | slot 7 `+0x1DC = 4`, four frames after the landing | the same | The same. |
| Stale clip ids | slot 7 `+0x1D9`, `+0x1DA`, `+0x1DB` set to 1, and to 2, before the seat | both read 0 again by the landing | Unfolds and stands. |
| Forced frame step | the frame-step byte `0x1F800393` held at 1, 2, 3, 4 and 6 | landing always at cursor 120 of 288 | Unfolds and stands, every time. |

In the two bit-set casts the creature lands with its legs part-way out, snaps back inside the
sphere when clip 0 wraps, and stays folded through the "Holy Orb" close-up, the ring burst and the
white-out. That is the reported symptom.

### What it is not

- **A disc streaming race.** The unfold is not queued by anything that waits on a load. It is a
  byte increment on the pass the creature's height reaches zero, about 100 frames after a seat that
  itself runs only once the creature is installed. A late stream delays the seat and everything
  after it together.
- **Frame-rate dependence.** The descent and the clip cursor scale with the same frame delta, so
  they cannot drift apart; the forced-step casts land at the same cursor and unfold at every step.
- **Leftover clip ids from the previous summon.** Stale `+0x1D9` / `+0x1DA` values, such as the
  ones Theeder, Orb and Mushura leave behind, are cleared before the landing.
- **A failed sparkle spawn.** The module moves its sixteen motes through pointers it saves without
  testing them (`sw v0,0x0(v1)` at `0x801F6EC0` and `0x801F7064`, read back at `0x801F7210`), and
  the pool spawn `FUN_80050ED4` returns 0 when its 96 allocatable slots are taken. That cannot
  happen in ordinary play: the per-frame battle routine `FUN_800480D8` releases every slot whose
  actor has retired (`0x8004814C..0x80048184`, test `+0x10 & 8`), so occupancy is the count of live
  parts, and a state taken going into an Orb cast reads 0 slots used and 123 free actors against
  the 20 the phase needs. In two live casts, one of them a random battle in Rogue's Tower with
  three party members and two enemies, all sixteen spawns return actors. A null would in any case
  cost one mote, not the creature's pose.
- **The pool flush overrunning the pool.** `FUN_80050E74` walks `0x80` words where the allocator
  fills `0x60`. The array really is `0x80` words: battle init `FUN_80055B6C` zeroes `0x80`
  (`0x8005603C..0x80056050`) and the per-frame release walks `0x80`. The allocator never
  fills the last 32 words; the only actors in them are the ones modules park there by hand
  (Horn's and Jedo's held fades at `0x801C92EC`), which the flush is meant to halt.
- **Rogue's Tower itself.** One Orb cast in a Rogue's Tower random battle, with screenshots through
  the whole animation, matches a reference cast on the plains frame for frame at the sampled rate.
  A single clean cast does not clear a location for a fault that happens "sometimes".

### Where the bit could come from

Not found. What is measured:

- Battle setup leaves slot 7 all zero: every in-battle state examined reads it clear before a
  summon.
- Eighteen other player summons, each cast once and followed to the end (`0x81`, `0x83`, `0x84`,
  `0x86..0x88`, `0x8A..0x8C`, `0x8E..0x90`, `0x92..0x95`, `0x98`), finish with `+0x1DC = 0`, as do
  `0x82`, `0x85`, `0x8D`, `0x89`, `0x96` and `0x99`. `0x8F` and `0x94` raise bit 0 briefly; none
  raises bit `0x4`.
- Seven long summons (`0x91`, `0x9A`, `0x9B`, `0x9D..0xA0`) read 0 as far as they were followed,
  but were not followed to their last phase.
- One Rogue's Tower enemy special cast before the party's Orb leaves the byte at 0.

What remains is `inference`: a routine outside the cast modules that stages a reaction on whatever
actor it is pointed at. After a summon ends, slot 7 keeps a non-zero HP and is no longer animated,
so a bit set on it then is never consumed by a natural clip end and is still there at the next
seat. Code outside the cast modules that sets bit `0x4` includes `0x80042144` (the hit flinch of
`FUN_800402F4`, the item and Spirit flinch), `0x8004F8D0` and the commit ladder at
`0x8004BF08` in `SCUS_942.54`, and `0x801E19DC`, `0x801E4868`, `0x801E5E9C` and `0x801EEC84` in
PROT entry 0898. That list comes from a pattern scan and is a floor. Which of them can be handed
slot 7 is not traced.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F69D8` | PROT 0911 | `FUN_801F69D8`, the Orb cast tick. | `disassembly` |
| `0x801F70B0` | PROT 0911 | `jal 0x801f19ec`, the seat call. | `disassembly` |
| `0x801F70E4`, `0x801F70F0` | PROT 0911 | Cursor preset `0x30`; height `-0x320`. | `disassembly`, `capture` |
| `0x801F73DC..0x801F7408` | PROT 0911 | The descent and its "still above the ground" hold. | `disassembly` |
| `0x801F7410..0x801F741C` | PROT 0911 | Landing frame: `+0x1DA` incremented, no commit flag. The module's only write to `+0x1DA`. | `disassembly`, `capture` |
| `0x801F7420..0x801F7440` | PROT 0911 | Countdown seed for the next phase. | `disassembly` |
| `0x80047430` | SCUS | `FUN_80047430`, the animation tick. | `disassembly` |
| `0x80047B30..0x80047B58` | SCUS | Natural clip end: bit `0x4` of `+0x1DC` zeroes the staged clip, then the commit `FUN_8004AD80`, then `+0x1DC &= 0xF8`. | `disassembly`, `capture` (poked) |
| `0x801F19EC` | PROT 0898 | `FUN_801F19EC`, the summon seat routine. Clears `+0x1D9` at `0x801F1DD8`; no store to `+0x1DC`. | `disassembly`, `capture` (poked) |
| `0x801C938C` | RAM | `actor_table[7]`, the summon creature seat. | `disassembly` |
| `0x800EDDB4` | RAM | The slot 7 actor record in battle. | `capture` |
| `0x800EDF8D`, `0x800EDF8E`, `0x800EDF90` | RAM | Slot 7's `+0x1D9` playing clip, `+0x1DA` staged clip, `+0x1DC` event flags. Writing 4 to the last reproduces the symptom. | `capture` (poked) |
| `0x1F800393` | scratchpad | The frame-step byte ([`move-vm.md`](../../subsystems/move-vm.md)). | `repo doc` |
| `0x801F6EC0`, `0x801F7064` | PROT 0911 | The two unchecked stores of a mote spawn result, into the table at `0x801F85E0`. | `disassembly` |
| `0x80050ED4` | SCUS | `FUN_80050ED4`, pool spawn: searches `0x60` words (`0x80050F10`), returns 0 when none is free. | `disassembly` |
| `0x80050E74` | SCUS | `FUN_80050E74`, pool flush over `0x80` words (`0x80050EC0`). | `disassembly` |
| `0x8004814C..0x80048184` | SCUS | `FUN_800480D8`, per-frame release of retired pool slots, `0x80` words. | `disassembly` |
| `0x8005603C..0x80056050` | SCUS | `FUN_80055B6C`, pool zeroed at battle init, `0x80` words. | `disassembly` |
| `0x801C90F0` | RAM | Part pool: `0x80` words, of which the allocator uses `0x60`. | `disassembly`, `capture` |
| `0x80042144`, `0x8004F8D0`, `0x8004BF08` | SCUS | Sites outside the cast modules that set bit `0x4` of an actor's `+0x1DC`. | `disassembly` |
| `0x801E19DC`, `0x801E4868`, `0x801E5E9C`, `0x801EEC84` | PROT 0898 | The same, in the battle overlay. | `disassembly` |

### Scope

- The exposure belongs to any summon module that queues its next clip without a commit flag and
  only once. Which others do is not enumerated.
- The fault is cosmetic. The heal, the captions and the cast's length do not depend on the
  creature's clip.
- The enemy Orb (monster 83) is a different animation path and is not examined.
- Only the USA disc is examined for this case.

### Verifying it

To reproduce: start an Orb cast and write 4 to `0x800EDF90` at any time from before the creature
is seated to the end of the curled clip, about 187 frames after the seat. The creature stays in
its sphere.

To recognise a real occurrence: log slot 7's `+0x1D9`, `+0x1DA`, `+0x1DC` and the clip cursor
(`+0x68` of the node at `actor[+0x22C]`) on every frame of an Orb cast. In the fault `+0x1DC` has
bit `0x4` set between the seat and the end of clip 0, and `+0x1DA` goes from 1 to 0 at the wrap.

### Open questions

- What sets bit `0x4` on slot 7 in ordinary play. One measurement settles it: a write breakpoint
  on `0x800EDF90` that records the program counter whenever the bit becomes set while no cast is
  running. A save state taken during a bad cast, or just before one with the preceding action
  known, would do the same from the reporter's side.
- The endings of the seven long summons (`0x91`, `0x9A`, `0x9B`, `0x9D..0xA0`).
- Which other summon modules queue a clip once without a commit flag.

## Jette's Clone refills his MP and moves him

**Status:** explained
**Reported as:** Jette restores full MP when using Clone, and the real Jette moves to a new slot
(community report).

The two observations are one event. Clone does not copy Jette. It builds a brand-new monster in
the next free seat from Jette's monster record, exactly as the game does when a battle loads: full
HP, full MP, fresh stats. Then it overwrites only the newcomer's *current HP* with the caster's
current HP. At that moment there are two actors with Jette's real HP; the new one has 1000 MP and
untouched stats, the old one has whatever it had.

Then the Clone branch flips a coin to choose which of the two becomes the decoy, and writes HP 1,
MP 0, attack 1 and a small action gauge onto the loser.

On an even roll the new actor is the decoy and nothing unusual has happened. On an odd roll the
*original* actor is the decoy, and the real Jette is now the freshly built one in the new seat:
same HP as before, MP back at 1000, stats as they were at the start of the fight, and no ailments.

So the MP refill happens exactly when, and because, the real Jette changes seat. It is not a
separate restore, and it does not happen on the even roll.

### Mechanism

**The AI pick.** Jette's arm of the monster AI `FUN_801E9FD4` (PROT entry 0898) starts at
`0x801EB820`. On odd rounds, three times in four, with at least 200 MP, it queues the monster's
own attack (`0x801EB820..0x801EB890`). Otherwise it considers Clone at `0x801EB894..0x801EB90C`,
and all four conditions must hold:

- the monster id byte is `0xAA`, Jette (`li v0,0xaa` at `0x801EB8A0`);
- the monster count `ctx[+1]` is not 3 (`li v0,0x3` at `0x801EB8B8`);
- `rand() % 3 == 0` (`0x801EB8C4..0x801EB8EC`);
- the caster's MP `+0x150` is not zero (`0x801EB8F4..0x801EB8FC`).

It then writes action category `+0x1DE = 2`, action id `+0x1DF = 0xAE` (`0x801EB90C`) and targets
its own seat (`0x801EB9AC`). Two consequences follow: there are at most two Clone casts in a
fight, and a decoy, whose MP is 0, can never cast Clone.

**The cast.** Clone shares one routine with Divide: `FUN_801F78B8` in PROT entry 0940, reached for
action ids `0x50` and `0xAE`. The work is in its phase 2. `s2` is the caster's actor.

```
801f7e08  lbu v0,0x1(v1)      ; s3 = ctx[+1], the old monster count = the new seat index
801f7e1c  sb v0,0x1(v1)       ; ctx[+1] + 1
801f7e3c  sb v0,0x0(v1)       ; monster id copied to the new seat
801f7e70  sw v0,0x0(a2)       ; monster record pointer copied to the new seat
801f7f18  jal 0x80054cb0      ; delay slot: move a1,s3   - full monster init of the new seat
```

`FUN_80054CB0` is the ordinary battle-load monster initialiser
([`battle.md`](../../subsystems/battle.md#monster-init-fun_80054cb0)). Its copy block
`0x8005516C..0x8005520C` fills the new actor from the record: HP into `+0x14C` / `+0x14E` /
`+0x172`, MP into `+0x150` / `+0x152` / `+0x174`, then the action gauge, attack, both defences,
speed and intelligence, followed by the usual battle-load stat boost. It does not write the
ailment word `+0x16E`.

Back in the cast, the only stat carried from caster to newcomer is current HP:

```
801f8064  sh zero,0x16c(v0)   ; newcomer +0x16C = 0
801f8090  lhu v0,0x14c(s2)    ; caster's current HP
801f8098  sh v0,0x172(v1)     ; newcomer HP mirror
801f809c  sh v0,0x14c(v1)     ; newcomer current HP
```

The tail runs for Clone only (`li v0,0xae` / `bne` at `0x801F80F0..0x801F80F4`). It gives the new
seat the module's own name string at `0x801F8644`, so both read as Jette, and then:

```
801f8134  jal 0x80056798      ; rand
801f813c  andi v0,v0,0x1
801f8140  beq v0,zero,0x801f8164
801f8148  sh v0,0x14c(s2)     ; odd: caster HP = 1
801f814c  sh v0,0x158(s2)     ;      caster attack = 1
801f8154  sh zero,0x150(s2)   ;      caster MP = 0
801f8158  sh v0,0x156(s2)     ;      caster action gauge = 0x20
801f8160  sh v0,0x154(s2)
```

The even branch at `0x801F8164` makes the same five stores on the newcomer
(`0x801F816C`, `0x801F8178`, `0x801F8184`, `0x801F8190`, `0x801F819C`).

**Why the new seat is clean.** Battle init `FUN_80055B6C` zero-fills the whole actor arena
(`0x80055F14..0x80055F24`) and lays the eight actor records out `0x2D4` apart
(`0x80055ED0..0x80055EF0`). Seats are only ever appended, so the newcomer's record has not been
used before in this fight, and neither the initialiser nor the cast copies `+0x16E` from the
caster.

What the "real" Jette carries after an odd roll:

| Field | Result |
|---|---|
| MP `+0x150` | Record maximum, 1000. A new actor, not a restore. |
| HP `+0x14C` | Not reset: copied from the caster at `0x801F809C`. Maximum HP `+0x14E` is the record's 34567. |
| Ailment word `+0x16E` | Empty. Ailments stay on the old seat, now the decoy. |
| Attack, defences, intelligence, speed, action gauge | Record values plus the battle-load boost. Stat changes made during the fight stay on the decoy. |
| Seat | From its old seat to `3 + old ctx[+1]`: seat 3 to seat 4 on the first Clone, to seat 5 on the second. |

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801EB820..0x801EB90C` | PROT 0898 | Jette's arm of `FUN_801E9FD4`; the Clone conditions from `0x801EB894`; the `0xAE` id at `0x801EB90C`. | `disassembly` |
| `0x801EB9AC` | PROT 0898 | Target store: the caster's own seat. | `disassembly` |
| `0x801F8228` | PROT 0940 | Router sending `0x50` and `0xAE` to `0x801F78B8`. | `repo doc` |
| `0x801F78B8..0x801F8224` | PROT 0940 | `FUN_801F78B8`, the Divide / Clone body. | `disassembly` |
| `0x801F7E08..0x801F7E70` | PROT 0940 | Seat allocation: count bump, id copy, record-pointer copy. | `disassembly` |
| `0x801F7F18` | PROT 0940 | `jal 0x80054cb0`, the full monster init of the new seat. | `disassembly` |
| `0x801F8090..0x801F809C` | PROT 0940 | The HP-only copy from caster to newcomer. | `disassembly` |
| `0x801F8134..0x801F8140` | PROT 0940 | The coin: rand, `andi 1`, branch. | `disassembly` |
| `0x801F8148..0x801F8160` | PROT 0940 | Odd branch: the caster becomes the decoy. | `disassembly` |
| `0x801F816C..0x801F819C` | PROT 0940 | Even branch: the newcomer becomes the decoy. | `disassembly` |
| `0x801F8644` | PROT 0940 (file offset `0x1C6C`) | The name string given to the new seat. | `disc data` |
| `0x80054CB0` | SCUS | `FUN_80054CB0`, monster init; stat copy `0x8005516C..0x8005520C`. | `disassembly` |
| `0x80055B6C` | SCUS | `FUN_80055B6C`, battle init; arena clear at `0x80055F14`. | `disassembly` |
| `0x80056798` | SCUS | `FUN_80056798`, rand. | `repo doc` |
| `0x8007BD24` | RAM | Battle context pointer. `ctx[+1]` monster count, `ctx[+0x13]` acting seat, `ctx[+0x279]` cast phase, `ctx[+0x28A]` round counter, `ctx[+0x292 + n * 0x20]` monster display names. | `disassembly` |
| `0x801C9370`, `0x801C9348`, `0x8007BD0C` | RAM | Actor pointer table, monster-record pointers, monster id bytes. | `disassembly` |
| monster 170 (`0xAA`) | PROT 0867 | Jette's record: HP 34567 at `+0x0C`, MP 1000 at `+0x10`. | `disc data` |
| spells `0xAE`, `0x50` | SCUS spell table | Clone and Divide, each with MP cost 0. | `disc data` |

The clean ailment word is graded in two parts: "neither routine writes it" is `disassembly`; that
nothing else touched the never-used record earlier in the fight is `inference`.

### Scope

- The routine is shared with Divide (`0x50`). Every Divide copy is likewise a fresh actor with
  record MP and stats carrying the caster's current HP. Divide has no coin and no weakening, so
  both halves stay real. The AI picks Divide at three sites in `FUN_801E9FD4`
  (`0x801EABC4`, `0x801EAC34`, `0x801EAC84`), each refusing at a monster count of 4; which monster
  ids reach those arms is not enumerated here.
- Clone (`0xAE`) has one user, the AI arm that requires monster id `0xAA`. No monster record lists
  `0x50` or `0xAE` among its three magic-attack bytes at `+0x21..+0x23`; both are picked by code.
- Each Clone has an even chance of refilling MP and shedding stat changes and ailments. Across the
  two possible casts the chance of at least one refill is three in four.
- The decoy's HP mirror `+0x172` is not rewritten by the weakening stores; what that does to any
  display reading it is not followed.

### Verifying it

- In the Jette fight, break at `0x801F8134` and step to `0x801F8140`. `v0 = 0` makes the new seat
  the decoy; `v0 = 1` makes the caster the decoy.
- For seats 3 and 4 read the actor pointer `p` from `0x801C9370 + 4 * seat`, then `p + 0x14C`
  (HP), `p + 0x150` (MP), `p + 0x158` (attack) and `p + 0x16E` (ailments) before and after the
  cast. The predicted odd result: seat 3 reads HP 1, MP 0, attack 1; seat 4 reads the old HP,
  MP `0x03E8`, the boosted record attack and ailments 0.
- Replacing the `andi v0,v0,0x1` at `0x801F813C` with a constant load forces either outcome.
- With Jette's MP at 0 the gate at `0x801EB8FC` never selects Clone.

### Open questions

- Whether any targeting or display state still refers to the old seat after an odd roll. The
  caster's target byte `+0x1DD` is saved at `0x801F8658` and restored in the `0xFF` phase at
  `0x801F81F0`.
- Whether the shell game is intended and the MP and stat reset is a side effect of reusing the
  monster initialiser. The code reads that way; intent is inference.
- The Japanese and PAL releases are not examined for this case: the Clone module and its AI gate are
  not located on those discs.
