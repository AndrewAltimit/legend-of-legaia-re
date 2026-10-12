# Ra-Seru levels, Orb and Jette's Clone

Three community-reported oddities around summons and one boss, each read against the USA executable
`SCUS_942.54` and its overlays: the Ra-Seru summons that arrive at Palma's level, the Orb summon's
animation, and what Jette's Clone does to his MP and seat. Everything here is static analysis of
the disassembly and the disc data; none of it is an emulator capture, and each section says what
is proven and what is predicted. Only the USA disc is examined.

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

So when a Ra-Seru reaches its final form and its summon is inserted, the level and experience are
read out of the stash that only Palma ever fills. The new summon arrives with whatever Palma had on
that same character the last time the Earth Talisman was taken off them. If that character never
had the Earth Talisman removed, the stash is still zero and the insert routine raises level 0 to
level 1, which is the ordinary result.

The exact statement is therefore narrower than the report: Meta, Terra and Ozma inherit the
*stashed* Palma level and experience of the *same character*, not the live level of a Palma that is
currently equipped on someone.

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
| `0x09` | Ra-Seru Meta, final form | `0x9E` Meta | 0 (`clear a2` at `0x800427AC`) | `0x80042794` |
| `0x11` | Ra-Seru Terra, final form | `0x9F` Terra | 0 (`clear a2` at `0x800427CC`) | `0x800427B0` |
| `0x19` | Ra-Seru Ozma, final form | `0xA0` Ozma | 0 (`clear a2` at `0x800427E8`) | `0x800427D0` |

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

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x80042558` | SCUS | `FUN_80042558`, the stat and equipment aggregator. Grant scan `0x80042714..0x8004288C`, revoke scan `0x80042894..0x80042990`. | `disassembly` |
| `0x800427AC`, `0x800427CC`, `0x800427E8` | SCUS | The three `clear a2` that give Meta, Terra and Ozma stash slot 0. | `disassembly` |
| `0x80042808` | SCUS | Palma's `clear a2`, the same slot. | `disassembly` |
| `0x800432BC` | SCUS | `FUN_800432BC`, head insert from a stash slot, with the level floor at `0x8004331C..0x80043324`. | `disassembly` |
| `0x80042DBC` | SCUS | `FUN_80042DBC`, remove a spell and park it in a stash slot. | `disassembly` |
| `0x800431FC` | SCUS | `FUN_800431FC`, "is this spell in the list". | `disassembly` |
| `0x80043264` | SCUS | `FUN_80043264`, "is this item in accessory bytes `+0x19B..+0x19D`". | `disassembly` |
| `0x800151D0` | SCUS (file offset `0x59D0`) | Five-word jump table of the revoke scan: `0x8004295C`, `0x800428DC`, `0x800428FC`, `0x8004291C`, `0x8004293C`. | `disc data` |
| `0x800849BD`, `0x80084DD1`, `0x800851E5` | RAM | Stash slot 0 level byte for Vahn, Noa, Gala (record `+0x2B5`); the four bytes below each hold the experience. | `disassembly` |
| `0x80084844`, `0x80084845`, `0x80084869`, `0x80084710` | RAM | Vahn's spell count, first id, first level, first experience word. | `disassembly` |
| item ids `0x09`, `0x11`, `0x19`, `0x72..0x76` | SCUS item-name table | Final Ra-Seru forms and the five Talismans ([`item-table.md`](../../formats/item-table.md)). | `disc data` |
| spell ids `0x99..0xA0` | SCUS spell table | The summon ids ([`spell-table.md`](../../formats/spell-table.md)). | `disc data` |

That the stash has no other reader or writer rests on a search of the dumped functions for the
`0x2B1..0x2B5` record offsets and their `0x80084140`-relative forms; it is strong but not a proof
over every byte of every overlay. That the story grant *is* the equipment byte changing to
`0x09` / `0x11` / `0x19` is an inference from this scan being the only code found that inserts
`0x9E..0xA0`; the script that upgrades the Ra-Seru item byte is not traced.

### Scope

- Three spells are affected (`0x9E`, `0x9F`, `0xA0`) and one donor (`0x9A` Palma). Mule, Horn, Jedo
  and Evil Seru Magic have their own slots and do not leak.
- The experience counter is copied with the level, so the inherited summon is also the same
  distance from its next level as the parked Palma was.
- It is per character. Vahn's Meta reads Vahn's stash, Noa's Terra reads Noa's, Gala's Ozma reads
  Gala's. A Palma levelled on Vahn does nothing for Noa's Terra.
- It needs a Palma *removal* before the grant. A Palma that is still equipped has not been parked,
  so the grant reads whatever the previous removal left, or zero (level 1) if there never was one.
- It is one-way. Meta's own progress never flows back to Palma.
- The stash area is fourteen slots wide; this code addresses slots `0..4` and bytes `+1..+5` only.

### Verifying it

1. Equip the Earth Talisman on Vahn, raise Palma (or write its level byte in `0x80084869..`), then
   unequip it. `0x800849BD` now holds Palma's level and `0x800849B9..0x800849BC` its experience.
2. With Meta not yet in the list, set Vahn's Ra-Seru equipment byte to `0x09` (one of
   `0x8008489E..0x800848A5`; the scan reads all eight). After the next aggregator pass
   `0x80084845` reads `0x9E` and `0x80084869` equals the stash byte.
3. Control: zero `0x800849B9..0x800849BD` first. Meta then arrives at level 1.
4. A breakpoint at `0x8004287C` shows `a1 = 0x9E`, `a2 = 0` on the grant.

### Open questions

- Which script or menu path writes item id `0x09` / `0x11` / `0x19` into the equipment byte.
- Whether the Japanese and PAL executables pass distinct slots for the three Ra-Seru summons.

## Orb's animation can glitch

**Status:** not confirmed
**Reported as:** Orb's animation can glitch sometimes (community report).

The report does not say what the glitch looks like, nor whether it means the Seru spell Orb
(spell id `0x89`) or the enemy Orb (monster 83). This section reads the spell's cast module and
lists what in it depends on state outside the module. None of the candidates is shown to produce a
visible fault, so the status stays "not confirmed" until someone describes or captures the symptom.

What the module does, in order: it spawns four parts and then sixteen light motes at random
offsets around the caster, pulls the motes in toward the caster over a fixed countdown, drops the
Orb creature in from above, and heals the party row. Unlike most summons, whose parts move under
their own scripts, Orb's motes are positioned every pass by the module itself, through sixteen
actor pointers it saved when it spawned them.

### Mechanism

The module is PROT entry 0911, loaded at `0x801F69D8`; its tick is `FUN_801F69D8` and its phase
byte is `ctx[+0x279]` (see [`cast-module.md`](../../subsystems/cast-module.md)).

- **Phase 2** (`0x801F6BB8`) spawns four parts through `FUN_80021B04` (records `0x801F804C`,
  `0x801F8098`, `0x801F80E4`, `0x801F8130`), then loops sixteen motes (`sltiu v0,s2,0x10` at
  `0x801F7068`, two per pass) through the pool wrapper `FUN_80050ED4` on record `0x801F8190`. Each
  pair calls `FUN_80056798` (rand) four times, at `0x801F6CDC`, `0x801F6D08`, `0x801F6D38` and
  `0x801F6D70`: X is `rand % 511 + 0x100` with a random sign, Y is `-(rand % 511)`, Z is
  `rand % 255 + 0x300`. Offsets are kept at `0x801F8560 + i * 8`, the returned actor pointers at
  `0x801F85E0 + i * 4`, and the countdown at `0x801F8558` is seeded with `rate << 7`.
- **Phase 3** (`0x801F707C`) loads each saved pointer (`lw a0,0x0(a3)` at `0x801F7210`) and writes
  the mote's position (`sh` to `+0x16` at `0x801F7230`, `+0x18` at `0x801F72CC`, `+0x14` at
  `0x801F7368`), interpolating by `0x80 - countdown / rate`. When the countdown runs out it flushes
  the part pool with `FUN_80050E74` and places the creature, `actor_table[7]`.
- **Phase 4** lowers the creature until its Y reaches zero; **phase 5** jumps straight to **9**
  (`sb s7,0x279(s5)` at `0x801F7780`), which flushes the pool again and runs the heal.

The candidates:

1. **Unchecked null part pointers.** `FUN_80050ED4` returns 0 when none of the first `0x60` words
   of the part pool `0x801C90F0` is free (`0x80050F10..0x80050F1C`), or when `FUN_80021B04` itself
   returns 0 because the actor allocator is empty (`0x80021B84..0x80021B90`). Orb stores the result
   without a test (`sw v0,0x0(v1)` at `0x801F6EC0` and `0x801F7064`) and phase 3 dereferences all
   sixteen. A failed mote is simply absent, and the tick then writes three halfwords at absolute
   `0x14`, `0x16` and `0x18` every pass. The same shape recurs with the object pointer kept at
   `ctx[+0x102C]` (stored at `0x801F76B0`, dereferenced at `0x801F78D0..0x801F78E4`). Whether the
   pools can be that full during an Orb cast is not established; Orb itself takes sixteen of the
   ninety-six pool words.
2. **Random by design.** The sixteen start offsets are rolled fresh each cast, so no two casts look
   alike. This is variation, not a defect, but may be what the report describes.
3. **Tick-driven motes.** Saved pointers would go stale if a mote retired early. The mote script at
   `0x801F8194` ends in a wait of `0xFFF` frames and then a halt, far longer than phase 3, and the
   pointers are not used after the phase 3 flush. This one reads as safe.
4. **The pool flush walks further than the pool.** `FUN_80050E74` clears `0x80` words from
   `0x801C90F0` (`slti v0,a1,0x80` at `0x80050EC0`) while the allocator fills only `0x60`. Any
   non-zero word in `0x801C9270..0x801C92EF` is treated as an actor pointer and halted. What lives
   in those words is not identified, and every module that calls the flush shares this, so it is
   not specific to Orb.
5. **In-image scratch.** The countdown and the two tables sit inside the module image and are zero
   on disc; all three are written in phase 2 before phase 3 reads them, so nothing carries over
   from an earlier cast. This one reads as safe.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801F69D8` | PROT 0911 | `FUN_801F69D8`, the Orb cast tick. | `disassembly` |
| `0x801F7FE8` | PROT 0911 | Stager hook called from part scripts. | `disassembly` |
| `0x801F8190` | PROT 0911 | The mote part record; its script starts at `0x801F8194`. | `disc data` |
| `0x801F8558`, `0x801F8560`, `0x801F85E0` | PROT 0911 | Countdown, sixteen 8-byte offsets, sixteen saved actor pointers. | `disassembly` |
| `0x801F6EC0`, `0x801F7064` | PROT 0911 | The two unchecked stores of a spawn result. | `disassembly` |
| `0x80050ED4` | SCUS | `FUN_80050ED4`, pool spawn; returns 0 when the pool or the allocator is full. | `disassembly` |
| `0x80050E74` | SCUS | `FUN_80050E74`, pool flush over `0x80` words. | `disassembly` |
| `0x80021B04` | SCUS | `FUN_80021B04`, the part seater. | `disassembly` |
| `0x80056798` | SCUS | `FUN_80056798`, rand. | `repo doc` |
| `0x801C90F0` | RAM | Part pool: `0x60` words allocated, `0x80` flushed. | `disassembly` |
| `0x1F80037D`, `0x1F800393` | scratchpad | Rate byte and frames-elapsed byte that drain the countdown ([`move-vm.md`](../../subsystems/move-vm.md)). | `repo doc` |
| `0x801C938C` | RAM | `actor_table[7]`, the summon creature seat. | `disassembly` |

### Scope

Candidates 1 and 4 apply to any cast module that uses the pool wrapper. Which other modules keep
and later dereference a spawn result is not surveyed.

### Verifying it

The first step is a description of the symptom: missing sparkles, a mis-posed creature, a stall,
and whether it is the spell or the enemy. With the spell loaded in a debugger:

- Break at `0x801F6EC0` and `0x801F7064` on `v0 == 0`. A hit means a mote failed to spawn.
- Write-watch `0x00000014..0x00000019` during the cast.
- At entry to `0x80050E74`, check `0x801C9270..0x801C92EF` for non-zero words.

A cast where one of these fires, recorded together with what the screen shows, settles which
candidate, if any, is the reported glitch.

### Open questions

- What the reported glitch looks like, and which Orb it concerns.
- How full the part pool and the actor allocator are at cast time.
- What occupies `0x801C9270..0x801C92EF`.

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
- The Japanese and PAL releases are not examined.
