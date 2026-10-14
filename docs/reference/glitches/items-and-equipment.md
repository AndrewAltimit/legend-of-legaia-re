# Items, stats and equipment

Five reported oversights in how the game handles items, stat boosts and equipment, each traced through the executable
and overlays. Every claim here is read from instructions or decoded from disc bytes. One case, the Speed Elixir one,
was also observed in a running game, captured on a poked state as its section describes, and so was the enemy-item
one; the other three have not been run, and each section says which parts are proven and which are predictions that
follow from the code. The USA build
is the one read throughout; the Elixir and coin-vendor sections also read the Japanese and French builds.

Images and bases used on this page:

| Name | Image | Load base |
|---|---|---|
| SCUS | `SCUS_942.54` | `0x80010000` |
| field | PROT entry 0897 | `0x801CE818` |
| battle | PROT entry 0898 | `0x801CE818` |
| menu | PROT entry 0899 | `0x801CE818` |
| arena | PROT entry 0977 | `0x801CE818` |
| cast 0945 | PROT entry 0945 | `0x801F69D8` |
| Japanese executable | `SCPS_100.59` | `0x80010000` |
| Japanese PROT 0898 / 0899 | the same entries on the Japanese disc | `0x801D05C8` |
| French executable | `SCES_019.44` | `0x80010000` |
| French PROT 0898 / 0899 | the same entries on the French disc | `0x801CF0A8` |

The Japanese build is a separate compile, so its routines sit at different addresses and its globals at different RAM
cells. For every routine cited here the French build is the USA code relocated. An address with no build named is a
USA address.

Shared anchors:

- The game-state window starts at `0x80084140`. Character record `n` is `0x80084708 + n*0x414`, which is window
  `+0x5C8 + n*0x414`. A window displacement of `+0x6E4` is record `+0x11C`, `+0x75E` is record `+0x196`, `+0x6BC` is
  record `+0xF4`. Record layout: [`save-record.md`](../../formats/save-record.md).
- Battle actors are reached through the pointer table at `0x801C9370` (seats `0..2` party, `3..7` monsters). The battle
  context pointer is the word at `0x8007BD24`.
- A system flag `idx` is bit `0x80 >> (idx & 7)` of byte `0x80085758 + (idx >> 3)`. `FUN_8003CE08` sets, `FUN_8003CE34`
  clears, `FUN_8003CE64` tests.
- The bag is `(id, count)` byte pairs at `0x80085958` (window `+0x1818`).

## Speed Elixirs and void turns (stat boost overflow)

**Status:** explained (the freeze is not traced)
**Reported as:** stacking Speed Elixirs in battle makes the turn-order number go negative; the character stops getting
turns and "void turns" appear, where the camera swings to a spot where nobody stands and nothing happens. A dead enemy
can take a turn, and the game can freeze (community report).

The list entry is about Elixirs used in battle, and about Speed in particular. A Speed Elixir or a Wonder Elixir
multiplies a character's battle Speed by 1.2, and nothing stops a second, tenth or twentieth one from doing it again.
On the USA disc the Speed stat itself is safe: it stops at 65,535. The overflow is one step further on, in the number
the game rolls from Speed every round to decide who acts next.

That turn-order number is kept in 16 bits. The routine that picks the next fighter walks the seats in order and
remembers the best number so far, but it reads the remembered number as *signed* and each new candidate as *unsigned*.
A number of 32,768 or more therefore beats everything when it is the candidate and then reads as negative when it is
the remembered best, so the very next seat takes the lead from it - even a seat whose number is zero.

Two things follow. The over-boosted character can never be picked, and neither can anyone seated before them, because
each of those lost the lead to the big number on the way past. And the round still hands out one turn per living
fighter, so the turns those characters should have had go to whichever later seats are left. Once those have all acted
their numbers are zero, every remaining seat ties at zero, and the game picks one of them at random without asking
whether anyone is there. An empty enemy slot gives the **void turn**: the camera frames a spot with nobody in it and
the turn ends at once. A fighter who has already acted gets a second action. A defeated enemy's slot can be picked the
same way.

The effect has a window. From an ordinary Speed it takes roughly 16 to 26 Elixirs on one character to open it, and a
few more close it again: once Speed is pinned at 65,535 the sum wraps all the way round to an ordinary positive number
and the character simply acts first.

The mechanism is read from the instructions and was then observed in a running game, captured on a poked state: Speed
or the turn-order number was written directly in an emulator, not reached by using Elixirs. The reported freeze has
not been reproduced or traced.

### Mechanism

1. **The boost.** Speed Elixir (`0x8D`, tier 1) and Wonder Elixir (`0x8E`, tier 4) are effect class 7, arm
   `0x80041464` of the item applier `FUN_800402F4`. Each does `x += x / 5` on both halfwords of the actor's Speed pair,
   `+0x164` (working) and `+0x166` (base), and stores `0xFFFF` when the sum is larger. There is no "already boosted"
   guard, so `N` uses give `SPD * 1.2^N`, floored at each step. The round reset copies base to working, and both were
   boosted, so the value lasts the whole battle.

   ```text
   800414d8  lhu v1,0x164(a0)        ; SPD
   800414e4  multu v1,v0             ; * 0xCCCCCCCD, then srl 2: v1 / 5
   800414f8  addu s1,v1,v0
   800414fc  sltu v0,a2,s1           ; a2 = 0xFFFF
   80041500  bne v0,zero,0x80041510  ; over: store 0xFFFF
   ```

2. **The key.** Once per round `FUN_801DA780` in PROT entry 0898 seeds every fighter's turn-order key at actor
   `+0x16C`:

   ```text
   801da800  lhu a0,0x164(a1)        ; SPD
   801da840  addu a0,a0,v1           ; + rand() % (SPD/2 + 1)
   801da844  addiu a0,a0,1
   801da84c  sh a0,0x16c(a1)         ; 16-bit store, no ceiling
   ```

   `key = (SPD + rand() % (SPD/2 + 1) + 1) & 0xFFFF`. The adjustments that follow are 16-bit too (`lhu` / `sh`): the
   wounded bonus, the halving under Slow at `0x801DA908`, and `+0x1000` for Speed Chain at `0x801DAA08`.
3. **The picker.** `FUN_801DABA4` picks the next actor in the loop at `0x801DAC7C..0x801DAD60`. It starts with seat 0's
   key as the running maximum and walks seats 1 to 6:

   ```text
   801daca4  sll v0,a1,0x10
   801daca8  lhu v1,0x16c(v1)        ; candidate key, zero-extended
   801dacac  sra v0,v0,0x10          ; running maximum, sign-extended
   801dacb0  slt v0,v0,v1            ; (signed) max < (unsigned) candidate?
   801dad60  sb v0,0x274(a1)         ; the chosen seat
   ```

   When the test passes the candidate becomes the maximum and the tie list restarts with that seat; when the two are
   equal (`0x801DACD8`) the seat joins the tie list; the result is a random member of the list (`jal rand` at
   `0x801DAD0C`).
4. **What a key of `0x8000` or more does.** As a candidate it is read as `32768..65535` and always passes the test, so
   every earlier seat loses the maximum to it. As the running maximum it is read as negative, so the next seat always
   replaces it, whatever that seat's key is, and the equality test never matches it. The seat holding it is never the
   result unless it is seat 6, the last one walked.
5. **Where the freed turns go.** The later seats act in key order as usual, and each key is zeroed as its turn starts.
   After that the maximum collapses to 0 at the first seat behind the over-boosted one, every further seat with key 0
   ties, and one of them is drawn at random. The loop has no HP test, no "is this slot populated" test and no
   "key is not zero" test; the sweep above it only zeroes the keys of fallen fighters.
6. **The void turn.** The turn-start arm of the action state machine `FUN_801E295C` (`0x801E2C50..0x801E2D54`) takes
   the seat from `ctx[+0x274]` unchecked: it loads the actor pointer from `0x801C9370[seat]`, zeroes its key and calls
   the camera framing routine (`jal 0x801d5854` with the seat in `a0`). The unused seats of a short enemy row still
   have allocated, zero-filled actor records, so the camera frames an empty seat and the turn falls straight through
   to its end.

How Speed maps to the key, from the formula in step 2 (the wounded bonus and Speed Chain shift the edges slightly up,
Slow halves the key):

| Battle Speed | Stored key | Result |
|---|---|---|
| below 21,845 | always below `0x8000` | normal turn order |
| 21,845 to 32,766 | `0x8000` or more on some rolls | locked out on those rounds |
| 32,767 to 43,689 (`0x7FFF..0xAAA9`) | always `0x8000` or more | locked out every round |
| 43,690 to 65,534 | `0x8000` or more unless the sum passes `0xFFFF` and wraps | locked out on fewer and fewer rounds |
| 65,535 (the USA ceiling) | `rand() % 0x8000`, always below `0x8000` | acts first |

Number of Speed or Wonder Elixirs used on one character (arithmetic from step 1):

| Battle Speed at the start | First use that can lock out | First use that always does | Use that reaches 65,535 |
|---|---|---|---|
| 200 | 26 | 29 | 32 |
| 331 | 24 | 26 | 30 |
| 500 | 21 | 23 | 27 |
| 999 | 17 | 20 | 23 |
| 1254 | 16 | 18 | 22 |

### The Japanese and French builds

The picker is the same in all three builds: the Japanese compare is at `0x801DE048..0x801DE080` and the French one at
`0x801DB524..0x801DB55C`, instruction for instruction the USA loop. The key seed is the same code too (Japanese
`0x801DDBFC` onward).

The Elixir arm differs. The Japanese executable `SCPS_100.59` has no saturation: its class-7 arm
(`0x800425F0..0x8004295C`) ends every pair of every tier in a plain 16-bit store.

```text
80042608  lhu v0,0x164(a0)
8004261c  srl v0,v0,2             ; after the multiply: v0 / 5
80042620  addu v1,v1,v0
80042624  sh v1,0x164(a0)         ; no compare
```

On the Japanese disc the boosted stat itself wraps to a small number once it passes 65,535: an Attack of 300 becomes
59,077 on the twenty-ninth Power Elixir and 5,356 on the thirtieth. The USA arm (`0x800414D8` onward) and the French
arm (`0x80041E24` onward) both carry the `0xFFFF` ceiling. The localisation closed the stat wrap and left the picker
as it was.

### Captures

Observed in PCSX-Redux on the unpatched USA disc, in a battle of three party members (seats 0 to 2, Speed 331, 364 and
310) against one enemy in seat 3, with seats 4 to 6 empty. Every member queued a plain attack through scripted pad
presses. These are captures on a poked state: either Speed (`+0x164` and `+0x166`) was written at the entry of
`FUN_801DA780`, or the key was written at the first command menu. No Elixir was used, so the Elixir arm itself was not
exercised at run time. Breakpoints logged the sum before the store at `0x801DA84C` and the tie list and result at
`0x801DAD60`.

| What was poked | Keys, seats 0 to 3 | Turns observed |
|---|---|---|
| nothing (control) | `01ED 0186 0140 0094` | seats 0, 1, 2, 3 |
| seat 0 Speed `0xFFFF` | sum `0x146A9` stored as `46A9` | seat 0 acts first; an ordinary round |
| seat 1 Speed `0x8000` | `01ED A9E5 0140 0094` | seat 2, seat 3, seat 5 (empty: a void turn), seat 2 a second time |
| seat 0 Speed `0xFFFF`, seat 1 Speed `0x8000` | `46A9 A9E5 0140 0094` | seat 2, seat 3, seat 5, seat 6 (both empty) |
| seat 0 key `0xA9E5`, written at the first menu | `A9E5 021C 0191 0095` | seat 1 (already chosen when the key was written), seat 2, seat 3, seat 5 (empty) |

In every run the round lasted four turns for four living fighters, whoever took them. In the third and fourth runs
seats 0 and 1 never acted, and in the fourth seat 0 was locked out while holding the largest positive key in the
battle. In the third run seat 2's second turn played its attack out in full again. In the last run seat 0 never acted,
and the draw that landed on seat 5 was logged with a tie list of seats 1 to 6.

### What it is not

The permanent stat items - Honey (`0x65`), Miracle Water (`0x6D`) and the six Waters (`0x82..0x87`) - cannot overflow
anything. They are effect class 6, arm `0x8004112C` of the same applier, and every tier adds a small amount to a 16-bit
stat in the character record and immediately compares the result against a ceiling:

```text
800411e8  lhu v0,0x6ec(v1)      ; record +0x124, ATK
800411f0  addiu v0,v0,0x4
800411f4  sh v0,0x6ec(v1)
800411fc  sltiu v0,v0,0x3e8     ; below 1000?
80041200  bne v0,zero,<exit>
8004120c  _sh v0,0x6ec(v1)      ; otherwise store 999
```

The largest value that exists before the compare is `1003` (`10015` for HP). The item menu then rebuilds the live
stats (`jal 0x80042558` at `0x801D8540` in PROT entry 0899), which clamps again at `0x80042C0C..0x80042CE0`, and the
usability check `FUN_8003FB10` greys a Water out at the ceiling before the applier runs. The Japanese arm
(`0x800420F4`, tier table `0x80015814`) and the French arm (`0x80041A9C`) carry the same thirteen compares with the
same ceilings. A menu figure above 999 is the stored stat plus equipment, summed for display.

Two nearby things are also not this report. The battle stats themselves are never read as signed, so a pinned Attack
or Speed does not turn negative; the sign trouble is confined to the key, which is sign-extended with `sll` / `sra` and
not with a signed load. And the enemy self-buff Jugger Power (cast `0xBA`, PROT entry 0945) adds a quarter to ten stat
halfwords at `0x801F6DA0..0x801F6E34` with no compare at all, which would shrink the caster's own stats after enough
casts; it is an enemy move and has nothing to do with Elixirs.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x800402F4` | SCUS | item applier; class jump table `0x80014FA0` | disassembly |
| `0x80041464` | SCUS | class 7 arm (Elixirs); Speed pair `0x800414D8..0x80041568`, ceiling `0xFFFF` | disassembly |
| `0x800425F0` | `SCPS_100.59` | Japanese class 7 arm, through `0x8004295C`; no ceiling | disassembly |
| `0x80041E24` | `SCES_019.44` | French Elixir arithmetic, with the `0xFFFF` ceiling | disassembly |
| actor `+0x164`, `+0x166` | RAM | battle Speed, working and base | disassembly |
| actor `+0x16C` | RAM | turn-order key, 16 bits | disassembly |
| `0x801DA780` | PROT 0898 | key seed; store at `0x801DA84C` | disassembly |
| `0x801DA84C` | PROT 0898 | sum `0x146A9` stored as `0x46A9`; `0xA9E5` stored from Speed `0x8000` | capture (poked Speed) |
| `0x801DA908`, `0x801DAA08` | PROT 0898 | Slow halving and Speed Chain `+0x1000`, both 16-bit | disassembly |
| `0x801DABA4` | PROT 0898 | next-actor picker; compare `0x801DACA4..0x801DACB0` | disassembly |
| `0x801DAD60` | PROT 0898 | result store to `ctx[+0x274]`; empty seats 5 and 6 drawn, seat 2 drawn twice | capture (poked Speed or key) |
| `0x801E2C50` | PROT 0898 | turn start, through `0x801E2D54`: seat taken unchecked, camera call | disassembly |
| `0x801C9370` | RAM | actor pointer table indexed by seat | disassembly |
| `0x801DDBFC` | Japanese PROT 0898 | key seed, same code | disassembly |
| `0x801DE048` | Japanese PROT 0898 | picker compare, through `0x801DE080` | disassembly |
| `0x801DB524` | French PROT 0898 | picker compare, through `0x801DB55C` | disassembly |
| one turn per seated fighter, less the skipped | PROT 0898 | `0x801E679C..0x801E67E4`: the end-of-action gate bumps the turn cursor `ctx[+0x1A]` and continues the round while it is below `ctx[+0] + ctx[+1] - ctx[+0x25]` (party seats plus monster seats, less the fighters the order dropped without acting) | disassembly |
| `0x801F69D0` | RAM | turns begun this round: zeroed at `0x801E2ABC` and `0x801E2E08`, incremented by the seed arm at `0x801E2C24..0x801E2C38`; its one read (`0x801E2B9C`) tests it against zero to pick the wait arm's camera yaw step, so it does not hand out turns | disassembly |
| a defeated enemy taking a turn | - | the tie list admits any key-0 seat; not captured | community report |
| the freeze | - | not reproduced, not traced | community report |
| `0x8004112C` | SCUS | class 6 arm (permanent stat-up); tier table `0x800151B0` | disassembly |
| `0x80041178` | SCUS | tier 0 Life Water: record `+0x11C` `+16`, ceiling `0x270F` | disassembly |
| `0x800411C4` | SCUS | tier 1 Power Water: `+0x124` `+4`, ceiling `0x3E7` | disassembly |
| `0x800413F4` | SCUS | tier 2 Guardian Water: `+0x126` and `+0x128` `+4`, ceiling `0x3E7` | disassembly |
| `0x80041210` | SCUS | tier 3 Swift Water: `+0x12A` `+4`, ceiling `0x3E7` | disassembly |
| `0x8004125C` | SCUS | tier 4 Wisdom Water: `+0x12C` `+4`, ceiling `0x3E7` | disassembly |
| `0x800412A8` | SCUS | tier 5 Magic Water: `+0x11E` `+8`, ceiling `0x3E7` | disassembly |
| `0x800412F4` | SCUS | tier 6 Honey / Miracle Water: `+4` to eight fields, AGL ceiling `0x118` | disassembly |
| `0x800420F4` | `SCPS_100.59` | Japanese class 6 arm (tier table `0x80015814`), same ceilings | disassembly |
| `0x80041A9C` | `SCES_019.44` | French class 6 arm, same ceilings | disassembly |
| `0x8003FB10` | SCUS | usability check that greys an item with no valid target | disassembly |
| `0x80042558` | SCUS | live-stat rebuild; clamps `0x80042C0C..0x80042CE0` | disassembly |
| `0x801F6DA0` | PROT 0945 | Jugger Power stat block, through `0x801F6E44` | disassembly |

### Scope

Which party members lose their turns depends on where the over-boosted one stands, because the picker walks the seats
in order:

| Over-boosted seat | Never picked while the key is `0x8000` or more |
|---|---|
| 0 (first party member) | seat 0 |
| 1 | seats 0 and 1 |
| 2 | seats 0, 1 and 2: the whole party |

The enemies sit in the later seats and always act. The freed turns are drawn from the seats behind the over-boosted
one, so with seat 2 over-boosted every extra turn goes to an enemy slot - a living enemy acting again, an empty slot or
a fallen one.

The Wonder Elixir boosts Speed along with three other pairs (Attack, both defences, and `+0x168/+0x16A`, which is
Intelligence), so it counts toward the same total as the Speed Elixir. The Power and Shield Elixirs pin their stats at
65,535 on the USA and French discs with no known side effect; on the Japanese disc those stats wrap.

### Verifying it

- On a poked state: at the entry of `0x801DA780`, write `0x8000` to `+0x164` and `+0x166` of the party member in seat
  1. A breakpoint at `0x801DA84C` shows `a0` between `0x8001` and `0xC001`; a breakpoint at `0x801DAD60` never shows
  seat 0 or seat 1 as the result, and shows an empty seat once seats 2 and up have acted.
- Unpoked: use Speed Elixirs on one character until the count in the table above is reached and watch the same two
  breakpoints. This has not been done.
- The permanent items: set Vahn's stored ATK (`0x8008482C`) to `0x03E6` and use a Power Water. It reads back `0x03E7`,
  and at `0x03E7` the menu refuses the item.

### Open questions

- The freeze. A battle with one enemy already defeated and a later-seated party member's Speed in the lock-out band
  would show it: the action state byte `ctx[+0x07]` at the moment it stops advancing, and the fallen actor's
  `+0x1DE` / `+0x1DF`, name the cause.
- A played reproduction, with real Elixirs in place of written values.

## Story equipment removal skips the re-check

**Status:** explained
**Reported as:** when the plot takes a piece of equipment off a character, its bonus keeps working until something else
re-checks equipment.

An accessory's effect is not looked up at the moment it matters. One routine bakes it into each character record: it
rebuilds an "ability bitfield" and the live stat block from whatever sits in the eight equipment slots. Battle, the
encounter roll and the menus all read the baked copy.

The script instruction the story uses to take an item away tries the bag first. If the item is not in the bag because
it is being worn, it empties the equipment slot directly and stops there. It does not call the bake routine, so the
baked copy still says the item is worn.

The game has a second script instruction that writes an equipment slot and *does* re-bake; it is used to grow the
Ra-Seru. The take-item fallback simply does not use it.

How long the stale bonus lasts is only partly traced: it ends at the next event that re-bakes. A scene change is
certain to do it.

### Mechanism

1. **The bake.** `FUN_80042558` runs over three records. For each it zeroes the ability words `+0xF4..+0x100`
   (`0x800425EC..0x800425F8`), copies base stats to live, walks slot bytes `+0x196..+0x19D`
   (`0x80042620..0x8004270C`) setting bit `index` for every item whose passive index is below `0x40`, applies the
   percentage boosts keyed on those bits (`0x80042994..0x80042C08`), clamps, and ORs the words into the party-wide mask
   at `0x80074358` (`0x80042D38..0x80042D88`). Index space:
   [`accessory-passive-table.md`](../../formats/accessory-passive-table.md).
2. **It is event-driven.** Every reference to `0x80042558` on the disc is a `jal`, and none is in the battle overlay.
   The callers are listed under Scope.
3. **The strip path.** Field-VM op `4C 52 <item>`, arm `0x801E1ABC` in PROT entry 0897:

   ```text
   801e1ac4  lbu a0,1(s6)              ; item id
   801e1ac8  jal 0x80042310            ; remove one from the bag
   801e1ad8  li v1,0x100
   801e1adc  bne v0,v1,0x801e00b8      ; found in the bag: done
   801e1ae8  jal 0x800430ac            ; not in the bag: unequip it
   801e1af0  j 0x801e3628              ; exit, with no jal 0x80042558
   ```

   `FUN_800430AC` walks the members currently in the party and slots 5..7 of each (record `+0x19B..+0x19D`, the three
   accessory slots), zeroes the first byte that matches (`sb zero,0x196(v1)` at `0x80043108`) and returns. Its one
   caller on the disc is `0x801E1AE8`.
4. **The contrast.** Field-VM op `0x3B`, arm `0x801E07DC`, takes `(character << 4 | slot, value)`, stores the value
   with `sb v1,0x75e(v0)` at `0x801E0814` (record `+0x196 + slot`) in the delay slot of `jal 0x80042558` at
   `0x801E0810`. Scripts use it for Ra-Seru stage changes (`3B 03 03`, `3B 12 0B`, `3B 23 13`).
5. **Readers of the stale copy.** The battle damage kernels read the record's ability words directly, battle load
   copies the live stat block, and party-wide passives are tested through the global mask
   ([`battle-formulas.md`](../../subsystems/battle-formulas.md)).

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| record `+0x196..+0x19D` | RAM | equipment slots; `+0x19B..+0x19D` are the accessories | disassembly |
| record `+0xF4..+0x103` | RAM | ability bitfield, four words, bit = passive index | disassembly |
| record `+0x104..+0x11A` | RAM | live stats; base at `+0x11C..+0x12C` | disassembly |
| `0x80074358` | SCUS | party-wide ability mask, four words | disassembly |
| `0x80042558` | SCUS | the bake (stat and ability rebuild) | disassembly |
| `0x801E1ABC` | PROT 0897 | op `4C 52` take-item arm, through `0x801E1AF4` | disassembly |
| `0x80042310` | SCUS | bag remove; returns `0x100` when the item is absent | disassembly |
| `0x800430AC` | SCUS | unequip by id, 36 instructions, no rebuild | disassembly |
| `0x801E07DC` | PROT 0897 | op `0x3B` equipment-slot write with rebuild | disassembly |
| `0x8003B2B8` | SCUS | rebuild call inside the scene-entry init `FUN_8003AEB0` | disassembly |
| `0x801D6DA8` | PROT 0897 | `FUN_801D6704` calling `FUN_8003AEB0` on every field scene entry | disassembly |
| `0x80024378` | SCUS | save-screen driver reloading the field overlay without a scene init | disassembly |

### Scope

Decoding every `4C 52` site on the disc with the opcode census
([`field-op-census.md`](../../tooling/field-op-census.md)) and keeping the operands whose descriptor carries a live
passive index leaves nine sites. Every other site takes a key item or consumable with the no-passive sentinel `0x41`
and leaves nothing behind. The effect needs the item to be **equipped** at that moment; from the bag it is removed
cleanly.

| Item | Id | Passive left behind | Scene | PROT / record / offset |
|---|---|---|---|---|
| Yuma's Ring | `0xAF` | `0x02` max MP +10%, `+0xF4` bit 2 | `geremi` (Jeremi) | 166 / P2[16] / `+0x0109` |
| Earth Egg | `0x6E` | `0x1D` Earth Guard, `+0xF4` bit 29 | `geremi` | 166 / P2[19] / `+0x1056` |
| Water Egg | `0x6F` | `0x1E` Water Guard, `+0xF4` bit 30 | `geremi` | 166 / P2[19] / `+0x1192` |
| Light Egg | `0x70` | `0x22` Light Guard, `+0xF8` bit 2 | `geremi` | 166 / P2[19] / `+0x12CE` |
| Dark Stone | `0x71` | `0x23` Dark Guard, `+0xF8` bit 3 | `geremi` | 166 / P2[19] / `+0x1409` |
| Ra-Seru Egg | `0xAB` | `0x21` Thunder Guard, `+0xF8` bit 1 | `vozz` (East Voz Forest) | 104 / P2[17] / `+0x01D7` |
| Star Pearl | `0xAE` | `0x0B` INT +5%, `+0xF4` bit 11 | `stone` (Shadow Gate) | 175 / P1[4] / `+0x018D` |
| Nemesis Gem | `0xB5` | `0x3B` High Encounter, `+0xF8` bit 27 | `deroa` (Jette's Fortress) | 606 / P2[4] / `+0x03D4` |
| Fire Droplet | `0xBC` | `0x2A` Maximum AP, `+0xF8` bit 10 | `doman` variant (Usha Research Center) | 401 / P1[15] / `+0x1F82` |

The events that re-bake, which is what ends the stale state:

| Caller | Image | When |
|---|---|---|
| `0x8003B2B8` | SCUS | every field scene entry (unconditional inside `FUN_8003AEB0`) |
| `0x801E0810` | PROT 0897 | field-VM op `0x3B` |
| `0x801EA28C`, `0x801EA348`, `0x801EAB28` | PROT 0897 | developer panel edits |
| `0x801EDF58` | PROT 0897 | return-to-title sequence |
| `0x801D5038`, `0x801D5158` | PROT 0899 | party equip-compare panel (trial equip) |
| `0x801D751C` | PROT 0899 | developer parameter editor |
| `0x801D8540` | PROT 0899 | field item use |
| `0x801D9440`, `0x801D97AC` | PROT 0899 | field magic use |
| `0x801D9B88` | PROT 0899 | Best Equipment |
| `0x801D9F68`, `0x801DA080`, `0x801DA1D0` | PROT 0899 | equip, swap and remove |
| `0x801DB6E8` | PROT 0899 | shop buy-and-equip |
| `0x801D0EF0`, `0x801D0F30`, `0x801D0FF0` | PROT 0977 | Muscle Dome contest start and settlement |
| `0x801D8194` | PROT 0899 | not identified |
| `0x801F7034` | PROT 0955 | a cast-module body, not examined |

The Muscle Dome's own equipment strip is a different case: it zeroes four slot bytes at contest start and re-bakes.

Duration: a scene change always clears it. The field scene is reloaded after a battle
([`battle-round-loop.md`](../../subsystems/battle-round-loop.md)), which bounds the stale bonus to one battle fought in
the same scene visit; that bound rests on the linked page and is not re-read here. The in-field save screen swaps
overlays without a scene init, so saving does not clear it. Whether closing the pause menu re-enters the scene init is
not traced.

### Verifying it

Equip Yuma's Ring on Vahn. `0x800847FC` (record `+0xF4`) has bit `0x4` set and `0x80084810` (live max MP) is 110% of
`0x80084826` (base max MP). Hand the ring to Zalan. Straight afterwards the slot byte in `0x800848A3..0x800848A5` is
zero while `0x800847FC` still has bit `0x4` and the live max MP is unchanged. An execution breakpoint on `0x80042558`
that logs the return address shows which event finally clears it.

### Open questions

- Whether closing the pause menu re-runs the scene init.
- A capture showing the effect surviving a scene change would mean a second path is involved.

## Enemy items aimed at an ally

<a id="point-card-softlock"></a>

**Status:** explained
**Reported as:** an enemy-targeted battle item can be aimed at one of the party's own characters. With the Point Card
the battle hangs (the "Point Card softlock"); with the Lippian Flute an ally can be killed and stay on their feet
("Lippian Roulette"). The outcome matrix below is a community report by ZetaPhoenix.

Two consumables can be aimed this way: the **Point Card** and the **Lippian Flute**. The Spikefish Flute cannot,
because it targets every enemy at once and never opens a cursor.

Three things combine. The first is shared; the other two decide what each item does to an ally.

1. **The cursor can reach an ally.** When a single-target enemy item is picked, the game builds a ring of selectable
   targets that starts from whoever this character targeted last. It builds the ring first and only afterwards checks
   that the last target was an enemy. If the last action was aimed at an ally - any healing item or Elixir, whose
   cursor starts on the user - that ally is already stored as the ring's first entry. The cursor is moved to an enemy,
   but one press of Left steps back onto the ally.
2. **The Point Card's damage skips the HP readout.** Its routine subtracts HP from the target and does nothing else.
   Every other routine that changes a party member's HP also records how far the on-screen HP number still has to
   count. At the end of an action the game holds, turning the camera, until every party member's on-screen HP equals
   the real HP. Here they differ and nothing is scheduled to close the gap, so the hold never ends.
3. **Lippian's kill is written for a monster.** It zeroes HP, stages the target's knockdown and switches on the
   monster's defeat fade. On a party seat that fade is the one a revive uses: the body goes black, is put back in its
   standing pose and fades in again. HP stays 0, so the character is dead in every way but the pose.

### Outcome matrix

Evidence: **C** = community report (ZetaPhoenix, in game), **D** = disassembly, **P** = captured on a poked state
(see [What was captured](#what-was-captured-enemy-items)).

| Item | Condition | Living ally | Dead ally |
|---|---|---|---|
| Point Card | holds points | Battle hangs: the camera orbits for ever. Lethal or not. (C, D, P) | No hang; the points are still spent. (C, D, P) |
| Point Card | no points | Nothing; the turn is used. (C, D, P) | Nothing. (C, D) |
| Lippian Flute | kill roll fails | Nothing; the flute is used up. (C, D, P) | Nothing. (C, D, P) |
| Lippian Flute | kill roll lands | HP goes to 0 but the ally stands back up. (C, D, P) | The body stands up, still at 0 HP. (C, D, P) |

Notes that go with the matrix:

- **The dead-ally column holds only when the item user is the first fighter to act in the round.** For anyone acting
  later the game re-rolls a dead target among the *living party members*, the user included, and the living-ally
  column applies to whoever it picks (D, P). See [the dead-target redirect](#the-dead-target-redirect).
- **Point Card on a dead ally is not quite nothing.** The whole bank, up to 9999, is spent, a damage number is shown
  and the body replays its collapse. Nothing hangs because real and on-screen HP are both 0 (D, P).
- **The Point Card itself is never used up.** It is taken from the bag at commit and added back when the action
  starts. The Lippian Flute is consumed (D, P).
- **The kill roll is 1 in 128,** not 1 in 126: `rand() & 0x7F`, kill on zero. The number 126 does appear on the item,
  as the effect class byte of its descriptor, which is not a probability (D).
- **A standing character at 0 HP is dead to the rules.** The command ring skips them, and when the last living member
  is "killed" this way the battle ends in the ordinary defeat message with the character upright (D, P).

### Which items the arm serves

The Item window forks on two flag bits of the item's effect descriptor: `0x40` (enemy side) and `0x20` (all targets).
Only `0x40` set with `0x20` clear enters the single-enemy picker, phase `0x5B`, which is the arm whose ring is built
before its target is validated.

| Item | Id | Descriptor | Flags | Phase | Reaches an ally |
|---|---|---|---|---|---|
| Point Card | `0xFE` | subtype `0x7E`: `01 00 C4 41` | one enemy | `0x5B` | yes |
| Lippian Flute | `0x98` | subtype `0x27`: `7E 00 C4 41` | one enemy | `0x5B` | yes |
| Spikefish Flute | `0x99` | subtype `0x28`: `7F 00 E4 41` | all enemies | `0x5D` | no: target byte `9`, no cursor |

Those are the only three consumable records whose descriptor has `0x40` set. Descriptor rows `0x23` (`7E 00 C6 41`)
and `0x24` (`7F 00 E6 41`) carry the same two classes with the field-usable bit added, and no consumable record
selects either. The game's own code names the same pair: the dead-target redirect tests the item id against `0xFE`
and `0x98` and nothing else.

### Mechanism

**Reaching an ally (both items).**

1. **The Item window builds the ring before validating.** In `FUN_801D0748` of PROT entry 0898:

   ```text
   801d18f8  jal 0x801d8a88       ; build the target ring
   801d18fc  _sb v0,0(s3)         ; phase = 0x5B
   801d1918  lbu v0,0x1dd(v0)     ; only now read the stored target
   801d1920  addiu v0,v0,-3
   801d1924  sltiu v0,v0,5        ; a monster seat, 3..7?
   801d1930  jal 0x801db8b4       ; if not: first living monster
   801d1950  sb v0,0x1dd(v1)
   ```

   The Magic window does the same two steps in the other order (validate at `0x801D1C8C..0x801D1CBC`, build at
   `0x801D1CC0`), and the Attack command validates at `0x801D1574..0x801D15AC` before its ring is built. Only the Item
   arm is out of order.
2. **The ring builder** `FUN_801D8A88` counts living monsters into `ctx[+0x244]`, then stores the actor's current
   target unchecked as the ring head: `sb v0,0x245(v1)` at `0x801D8B30`. Up to three other living monsters go to
   `ctx[+0x246..+0x248]`.
3. **The cycler** `FUN_801D8D00` finds the current target at ring index `i`. "Previous" returns `ctx[+0x244 + i]` for
   `i != 0` (`0x801D8DAC..0x801D8DC4`); "next" returns the head when `i` is the last index (`0x801D8DA0`). With a stale
   ally in the head and the cursor on the first ringed monster, Left returns the ally.
4. **The picker** (phase `0x5B`, handler `0x801D23F0`) stores the cycler's result at `0x801D2630`. Its confirm path
   `0x801D24AC..0x801D2540` does not re-validate the target.
5. **Where the stale ally comes from.** The single-ally item arm seeds the cursor with the actor's own seat
   (`sb v1,0x1dd(v0)` at `0x801D19A0`). The target byte is set to `3` once at battle start (`0x801D87CC..0x801D87D8`).
   At a round boundary it is re-picked only when it names a dead seat (`0x801D8A00..0x801D8A54`), so a living ally
   stays there from turn to turn and a dead one does not.

**Point Card.**

1. **The item.** Item `0xFE` has record bytes `02 7E 00 00`. At commit one card leaves the bag (`FUN_80042310` at
   `0x801D24FC`). When the action reaches state `0x3E` of `FUN_801E295C`, the per-item routine `FUN_801F3990` puts it
   back: `jal 0x800421d4` with `(0xFE, 1)` at `0x801F3A58`.
2. **The selector override.** State `0x3E` special-cases the id:

   ```text
   801e4080  lbu v1,0x1df(s3)
   801e4084  addiu v0,zero,0xfe
   801e4088  bne v1,v0,0x801e40c8
   801e40bc  addiu v0,zero,0xe
   801e40c0  sb v0,0x1e8(s3)      ; applier selector = 0x0E
   ```

   State `0x3F` calls `FUN_800402F4` with that selector and the target seat at `0x801E4134`. Neither state tests the
   target's side or HP.
3. **The discharge arm** `0x8004209C` exits at `0x800420AC` when the bank at `0x800845B4` is empty, before any store.
   Otherwise it takes `min(bank, 9999)` off the bank, stages a reaction clip on the target (`+0x1DA`, `+0x1DC`) and
   writes HP, clamped at 0 (`sh v0,0x14c(v1)` at `0x8004219C`). It writes neither the pending readout accumulator
   `+0x10` nor the displayed HP `+0x172`. The heal and revive arms of the same function seed the accumulator at
   `0x800408FC`, `0x80040D28` and `0x800410BC`.
4. **The reaction is always the knockdown on a party seat.** The flinch is taken only when `+0x1F2` is zero and the
   hit is survivable (`0x800420F4..0x80042124`), and every party actor has `+0x1F2 = 5`. A survivor's knockdown chains
   into the get-up; at 0 HP it chains into the collapse (clips `7`, then `8`).
5. **The hold.** State `0x51` turns the orbit yaw every frame (`0x801E6024..0x801E6040`) and counts its exit timer
   down only while `FUN_801E7250` returns 0 (`0x801E6044`). For a party seat that function compares `lhu 0x14c` with
   `lhu 0x172` at `0x801E72A8..0x801E72B4` and returns 1 when they differ. The displayed value only moves through a
   drain that is skipped when the accumulator is zero. This is the first softlock class of
   [`battle-action-exit-gates.md`](../../subsystems/battle-action-exit-gates.md), the state-`0x51` HP-readout park,
   and not the state-`0x19` approach park.
6. **A dead target cannot hang.** HP is already 0, the clamp writes 0 again, and `+0x14C == +0x172`. The bank is
   debited all the same, because the arm never looks at the target before spending it.

**Lippian Flute.** The flute casts spell `0x96`, whose module is `FUN_801F6A18` in PROT entry 0924. The module reads
its victim from the caster's target byte on every tick (`0x801F6A68..0x801F6A78`) and never asks which side it is on.

1. **The roll.** Phase 0 calls `rand` and raises the kill flag `0x801F7C4C` only when the low seven bits are zero
   (`andi v0,v0,0x7f` at `0x801F6B14`).
2. **What every cast does to the victim.** Tint and animation-rate writes only: `+0x04 = 0x20080200`, `+0x21C = 0`
   (`0x801F73E0..0x801F73E8`), `+0x21D = 0` then `8` (`0x801F7468`, `0x801F7634`). No HP is touched.
3. **The failed branch** (`beq a0,zero` at `0x801F7664`) leaves the victim at that. The only later difference is the
   "Ultimate Rave" caption.
4. **The kill branch**, `0x801F7684..0x801F76BC`:

   ```text
   801f7684  lbu v1,0x1f1(s3)     ; the victim's knockdown clip
   801f7688  lhu t0,0x14c(s3)     ; its HP
   801f7690  sb v0,0x225(s3)      ; = 2
   801f7694  sb v0,0x21c(s3)      ; = 2, the defeat fade
   801f76a0  sh zero,0x14c(s3)    ; HP = 0
   801f76a4  sb v1,0x1da(s3)      ; stage the knockdown
   801f76b4  addu v1,v1,t0
   801f76bc  _sw v1,0x10(s3)      ; readout accumulator += old HP
   ```

   The accumulator is seeded, so the on-screen HP counts down to 0 and state `0x51` clears. This is the write the
   Point Card arm is missing.
5. **The fade arm reads a party seat as a revive.** `+0x21C = 2` selects arm 2 of the per-actor presentation tick
   `FUN_80050120` (`0x80050230`). Its gate normally waits for the lying pose, and a non-zero `+0x225` opens it at once
   (`0x8005023C..0x8005027C`). When the colour reaches black on a party seat it stores `+0x21C = 0`, **`+0x1DA = 0`**
   (the idle clip) and `+0x1DC = 1` (commit now) at `0x80050348..0x80050350`. The revive arm of the item applier is
   what normally sets `+0x21C = 2` on a party seat (`0x800410D0`), after it has restored HP.
6. **Living victim.** The knockdown commits, the commit ladder of `FUN_8004AD80` chains it to the collapse because HP
   is 0, and the fade then overwrites the staged clip with idle. The character drops, goes black and stands up.
7. **Dead victim.** HP is 0, so the accumulator gains nothing. The same fade lifts the body out of its lying clip into
   idle: it "gets up" at 0 HP.
8. **Afterwards.** `+0x225` is cleared before the next action begins. Nothing restores HP, and HP is what the rest of the
   battle reads: the next-member scans `FUN_801DBA04` / `FUN_801DB81C` skip a seat at 0 HP, and the party-wipe count
   at state `0x5A` counts it as down.

### The dead-target redirect

`FUN_801DB124` in PROT entry 0898 re-aims a queued action whose target has died. For the item category it acts on
exactly two ids, which is the game treating these as its two single-target enemy items:

```text
801db1e0  _li v0,0xfe
801db1e4  lbu v1,0x1df(s0)
801db1ec  beq v1,v0,0x801db1fc    ; Point Card
801db1f0  _li v0,0x98
801db1f4  bne v1,v0,0x801db300    ; anything but the Lippian Flute: leave it
801db1fc  sltiu v0,a1,0x3         ; is the dead target a party seat?
```

A dead party seat is replaced by `rand() % party_count`, retried until the seat is alive (`0x801DB210..0x801DB274`);
a dead monster seat by a living monster. The roll stays on the dead target's own side, so a card or flute aimed at a
dead ally moves to a living ally.

The redirect is called from the turn picker `FUN_801DABA4`, and for a party seat only while the command flow byte
`ctx[+0x06]` reads `0xFF` (`0x801DAF04..0x801DAF14`). The round's first pick is made by the initiative seeder
`FUN_801DA780`, which the command-flow machine `FUN_801D0748` calls (`0x801D0E8C`, `0x801D0ED8`) while that byte
still holds a menu state: in the capture the first fighter is already chosen on the Begin / Run screen.
The first fighter of a round is therefore never redirected, and every later one is.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x800845B4` | RAM | Point Card bank, u32 | disassembly |
| `0x80074F50`, `0x800754B8` | SCUS | item `0xFE` record and its descriptor `01 00 C4 41` | disc data |
| `0x80074A88`, `0x8007535C` | SCUS | item `0x98` record and its descriptor `7E 00 C4 41` | disc data |
| `0x80074A94`, `0x80075360` | SCUS | item `0x99` record and its descriptor `7F 00 E4 41` | disc data |
| `0x801D18E0` | PROT 0898 | Item window, enemy single-target arm, through `0x801D1964` | disassembly |
| `0x801D8A88` | PROT 0898 | target ring builder; head store `0x801D8B30` | disassembly |
| `0x801D8D00` | PROT 0898 | target ring cycler | disassembly |
| `0x801DB8B4` | PROT 0898 | first-living-monster pick | disassembly |
| `ctx[+0x244..+0x248]` | RAM | ring count, head, three more seats | disassembly |
| `0x801D23F0` | PROT 0898 | phase `0x5B` picker; target store `0x801D2630` | disassembly |
| `0x801D19A0` | PROT 0898 | ally-item arm seeding the actor's own seat | disassembly |
| `0x801D8A00` | PROT 0898 | round-boundary re-pick of a dead stored target, through `0x801D8A54` | disassembly |
| `0x801DB124` | PROT 0898 | dead-target redirect; item-id tests `0x801DB1E0..0x801DB1F4` | disassembly |
| `0x801DAF04` | PROT 0898 | turn picker's gate on the redirect for a party seat | disassembly |
| `0x801F3A58` | PROT 0898 | Point Card handed back to the bag | disassembly |
| `0x801E4080` | PROT 0898 | state `0x3E` selector override for item `0xFE` | disassembly |
| `0x801E4134` | PROT 0898 | state `0x3F` call into the applier | disassembly |
| `0x8004209C` | SCUS | Point Card discharge arm, through `0x8004219C` | disassembly |
| `0x801E7250` | PROT 0898 | HP-readout settle check | disassembly |
| `0x801E6044` | PROT 0898 | state `0x51` gate on the settle check; orbit yaw step at `0x801E6024` | disassembly |
| `0x801F6B0C` | PROT 0924 | Lippian kill roll; flag at `0x801F7C4C` | disassembly |
| `0x801F7684` | PROT 0924 | Lippian kill writes on the victim, through `0x801F76BC` | disassembly |
| `0x80050230` | SCUS | presentation arm 2; party-seat completion at `0x80050348` | disassembly |
| `0x800410D0` | SCUS | revive arm setting `+0x21C = 2` | disassembly |
| actor `+0x10`, `+0x14C`, `+0x172` | RAM | readout accumulator, live HP, displayed HP | disassembly |
| actor `+0x1DA`, `+0x1DC`, `+0x1F1` | RAM | staged clip, clip event flags, knockdown clip id (`4` on a party seat) | disassembly |
| actor `+0x1DD`, `+0x1DF`, `+0x1E8` | RAM | target seat, item id, applier selector | disassembly |
| actor `+0x21C`, `+0x225` | RAM | presentation arm, capture / fade-now byte | disassembly |

<a id="what-was-captured-enemy-items"></a>

### What was captured

Probe `scripts/pcsx-redux/autorun_enemy_item_ally_target.lua`, on the catalogued state
`cort_evolved_battle_first_menu` (three members on the first command screen). The cursor path is the real one: the
probe presses Begin, opens the Item window, picks the item, presses Left and confirms. **Poked** once before the
walk: the item into bag slot 0, the Point Card bank, the user's stored target byte (the stale ally a healing item
leaves), and HP 0 plus a staged knockdown on the two members who are not the user, so that they are lying dead when
the walk starts. For the flute rows the kill flag is forced, since the roll is 1 in 128.

| Run | Logged |
|---|---|
| ring, any run | ring count 1, head = the ally seat, then the monster; Left moves the target byte from `3` to the ally |
| Point Card, 50 points, user on self | bank `50 -> 0`, HP `3437 -> 3387`, displayed `3437`, accumulator 0; clips `4`, `5`, idle; state `0x51` to the end of the run |
| Point Card, 9999 points, user on self | HP 0, displayed `3090`; clips `4`, `7`, then idle at state `0x51`; same hang |
| Point Card, 0 points, user on self | no store on the target; the round goes on to the monster's turn |
| Point Card, 50 points, dead ally, user first | bank `50 -> 0`, target HP 0 and displayed 0, clips `4`, `7`, `8`; no hang |
| Point Card, 50 points, dead ally, user not first | target byte re-rolled to the living user at the pick, then the hang |
| Lippian, kill forced, user on self | HP `3090 -> 0`, accumulator `3090` draining to 0; clips `4`, `7`, idle; `+0x21C` `2 -> 0`; defeat follows |
| Lippian, kill forced, dead ally | clips `8`, `4`, `7`, idle; HP 0 throughout; the round goes on |
| Lippian, roll forced to fail | no HP, clip or accumulator change on the target, living or dead |
| card and flute counts | the card `3 -> 2` at commit, `-> 3` at state `0x3E`; the flute `3 -> 2` and no return |

In the hang the screenshots show the "Points returned" caption still up, the HP bar at its old value and the camera
at a different angle in each frame. In the kill-on-self runs the user is the last living member, so the standing
character is followed by the party-defeat message and the title screen. The dead-ally rows were run with the user as
the round's first fighter (seat 1 in that state) and once with a user who is not.

### Scope

- The hang needs points in the bank and a living party target at the moment the card resolves. It is the same for a
  lethal and a non-lethal hit.
- A lethal Point Card hit on the user's own seat shows the character collapse and stand up again before the hang:
  the end-of-action cleanup sets the acting actor's exit-to-idle flag (`+0x1DC` bit 2), which ends the collapse in
  idle. The readout still shows the old HP. This is what a report of the character being "revived" describes.
- Aimed at a monster both items behave as designed: the settle check returns 0 for seats `3..7`, and the defeat fade
  on a monster seat removes it.
- Japanese and PAL builds were not read for this entry.

### Verifying it

Use a healing item on the acting character, or open and cancel its ally cursor. Next turn pick the Point Card or the
Lippian Flute and press Left. A breakpoint at `0x801D2630` shows `v0` in `0..2`.

- Point Card: confirm, then watch `ctx[+0x07] == 0x51` with `ctx[+0x6D8]` frozen at `0x3C`, the target's `+0x14C`
  different from its `+0x172`, and its `+0x10` zero. Writing `+0x172 = +0x14C` by hand releases the battle.
- Lippian Flute: write a non-zero word to `0x801F7C4C` once the cast's phase byte `ctx[+0x279]` has left 0. At
  `0x801F76A0` register `s3` is the ally's actor. Afterwards its `+0x14C` is 0 and its playing clip `+0x1D9` is 0.
- Redirect: break on `0x801DB260`. It is hit only when the user is not the round's first fighter.

### Open questions

- Whether a revive item works normally on a member left standing at 0 HP, and whether monsters still pick that seat.
- How a dead ally comes to be the stored target at command time. The round-boundary pass replaces a dead target, so
  it has to be stored by an ally cursor within the same command phase; that path is not traced.
- The outcome matrix was observed on the USA release; other builds are unread.

## Coin vendor purchases past 99 (Japanese release)

**Status:** explained
**Reported as:** on the Japanese release a coin vendor lets a purchase through while 99 are already held; the coins are
taken and the count stays at 99.

At a coin prize counter the Japanese game asks one question before the Yes / No box: can the coins cover the price? It
never looks at how many of the item are already held. After Yes it adds the item and subtracts the coins with no
further test.

The bag's add routine never refuses anything. It adds, and then silently trims the stack back to 99. So a hundredth
copy costs the coins, is added, and is trimmed off again - the report exactly.

The USA build added two things in front of that. The prize list draws a row dimmed when 99 are held, and the confirm
step asks "fewer than 99 held?" and buzzes when the answer is no. The French build carries both. The commit and the
trimming add behind them are unchanged in every build, which is why the same hole is still open in one other place:
equipment bought "to the bag" in an ordinary gold shop.

Both the Japanese and the USA code are read from the instructions; the Japanese result on screen has not been
captured.

### Mechanism

The Japanese build, PROT entry 0899 at load base `0x801D05C8`:

1. The prize session is `0x801DE978..0x801DEE64` (file offset `0xE3B0`). Its call list is the USA session's call for
   call with one call missing: the held-count lookup.
2. On confirm (`0x801DEB1C..0x801DEBAC`, file `0xE554`) the only test is coins against price:

   ```text
   801deb6c  slt v0,v0,v1            ; coins < price?
   801deb70  beqz v0,0x801deb88      ; can afford: open the Yes / No box
   801deb78  jal 0x8002cabc          ; otherwise buzz
   ```

   Nothing sits between the compare and the confirm window.
3. The commit (`0x801DEC5C..0x801DED24`, file `0xE694`) calls the bag add with a quantity of 1 (`jal 0x80043210` at
   `0x801DECA8`), ignores what it returns, subtracts the price from the coin bank (`0x801DECD8..0x801DECF0`) and sets
   the row's one-shot flag.
4. The bag add in `SCPS_100.59` is `0x80043210` (file `0x33A10`). It ends with the trim and returns the slot index
   either way:

   ```text
   8004332c  addu v0,v1,v0           ; count + quantity
   80043334  slti v0,v0,0x64
   80043340  addiu v1,zero,0x63      ; over 99: 99
   8004334c  sb v1,-0x4037(at)       ; count byte of the slot
   ```

5. The prize-list row draw (`0x801D8014..0x801D80A8`, file `0x7A4C`) picks the normal text colour, a second colour
   for a one-shot row and the dim colour when the row is unaffordable. It makes no held-count call. A search of the
   whole image agrees: the Japanese PROT entry 0899 contains no signed compare against `0x63` at all.

The USA build, PROT entry 0899 at `0x801CE818`:

1. The prize session is `FUN_801DC1CC`. Its table is at `0x801E4518`: 8-byte records of
   `[i16 item id][i16 one-shot flag][u32 coin price]`, `0x60` bytes per vendor
   ([`shop.md`](../../subsystems/shop.md)). The Japanese table at `0x801E7A3C` has the same record shape.
2. On confirm the held-count test follows the coin test:

   ```text
   801dc3c0  lw v0,0x45a4(a2)        ; coins
   801dc3c4  lw v1,0x4(a0)           ; price
   801dc3d0  bne v0,zero,0x801dc3f8  ; cannot afford: buzz
   801dc3dc  jal 0x80042f4c          ; held count
   801dc3ec  slti v0,v0,0x63
   801dc3f0  bne v0,zero,0x801dc408  ; fewer than 99: open Yes / No
   801dc3f8  jal 0x80035bd0          ; otherwise buzz and stay
   ```

3. The row draw makes the same lookup at `0x801D5ED4..0x801D5EF4` (`jal 0x80042f4c` / `slti v0,v0,0x63`) and uses
   text colour 0, the dim one, when 99 are held. These two are the only compares against `0x63` in the image.
4. The commit has no test, as in the Japanese build:

   ```text
   801dc514  jal 0x800421d4          ; bag add (id, 1), result ignored
   801dc54c  subu v1,v1,v0
   801dc550  sw v1,0x464(a0)         ; coins -= price
   ```

5. The bag add `FUN_800421D4` trims at `0x800422E0..0x80042300` (`slti v0,v0,0x64`, then `0x63` stored).

The French build has both USA additions: the front check at `0x801DCEF8..0x801DCF10` (`jal 0x80043898` /
`slti 0x63`) and the row dim at `0x801D69E4`, in its PROT entry 0899 at load base `0x801CF0A8`.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801DE978` | Japanese PROT 0899 (file `0xE3B0`) | prize-exchange session, through `0x801DEE64` | disassembly |
| `0x801DEB1C` | Japanese PROT 0899 (file `0xE554`) | confirm: coins against price only, then the confirm window | disassembly |
| `0x801DEC5C` | Japanese PROT 0899 (file `0xE694`) | commit: `jal 0x80043210` at `0x801DECA8`, coins minus price | disassembly |
| `0x801D8014` | Japanese PROT 0899 (file `0x7A4C`) | prize-list row draw, no held-count call | disassembly |
| `0x801E7A3C` | Japanese PROT 0899 (file `0x17474`) | prize table, same record shape as USA | disc data |
| `0x80043210` | `SCPS_100.59` (file `0x33A10`) | bag add; trim at `0x80043334..0x8004334C` | disassembly |
| `0x80043088` | `SCPS_100.59` | held count; not called from the prize session | disassembly |
| `0x801CAC14`, `0x801CBFC8` | Japanese RAM | coin bank; bag, two bytes per slot | disassembly |
| `0x801DC1CC` | PROT 0899 | USA prize-exchange session; phase word `0x801E46AC` | disassembly |
| `0x801E4518` | PROT 0899 | USA prize table, 8-byte records, `0x60` per vendor | disc data |
| `0x800845A4` | RAM | USA coin bank | disassembly |
| `0x801DC3C0` | PROT 0899 | USA affordability and held-count checks, through `0x801DC3F8` | disassembly |
| `0x801D5ED4` | PROT 0899 | USA prize-list row draw: dim text colour when 99 are held | disassembly |
| `0x801DC4C0` | PROT 0899 | USA commit: add, debit, set one-shot flag | disassembly |
| `0x80042F4C` | SCUS | held count of an item id, 0 when absent | disassembly |
| `0x800421D4` | SCUS | bag add; trim at `0x800422E0..0x80042300` | disassembly |
| `0x801DCEF8`, `0x801D69E4` | French PROT 0899 | front check and row dim, as on USA | disassembly |
| `0x801DB21C` | PROT 0899 | USA gold-shop buy-list confirm; one compare, gold against price, at `0x801DB314` | disassembly |
| `0x801DB380` | PROT 0899 | USA buy recipient picker; "to the bag" row `0x801DB478..0x801DB4C8` | disassembly |
| `0x801DDB14` | Japanese PROT 0899 | "to the bag" row, through `0x801DDB6C`; `jal 0x80043210` at `0x801DDB2C` | disassembly |
| the Japanese result on screen | - | coins taken, count stays 99 | community report |

### Scope

Every giver on the disc goes through the same trimming add, so a chest, a battle drop or a prize at 99 is discarded
without a message.

Every build has a sibling of the reported fault in the gold shop. Stackable items go through a quantity screen capped
at `min(gold / price, 99, 99 - held)` (`jal 0x80042ee0` at `0x801DB8B0` on USA, `0x801DDEF8..0x801DDF5C` on the
Japanese disc), but equipment does not. The buy-list confirm `FUN_801DB21C` tests only gold before routing on the item
kind, and the recipient picker's "to the bag" row runs `jal 0x800421d4` at `0x801DB490` and then debits gold with no
held-count read for the item between them. Buying a hundredth copy of one piece of equipment takes the gold and leaves
99. This is read from the instructions of the USA and Japanese builds, and the French build is the USA code
relocated; it has not been run.

### Verifying it

On a Japanese disc hold 99 of a prize item and buy another: a breakpoint at `0x801DEB70` shows the branch to the
confirm window taken, and after Yes the coin word at `0x801CAC14` drops by the price while the count byte stays
`0x63`. On the USA disc the same attempt buzzes and no box opens; forcing the branch at `0x801DC3F0` to be taken
reproduces the Japanese behaviour.

### Open questions

- A run of the equipment sibling: hold 99 of one weapon, buy one more to the bag, read gold and the count byte.

## War God Icon can be missed for good

**Status:** explained
**Reported as:** the War God Icon is permanently missed if the Muscle Dome Expert Course is beaten before defeating
Jette.

One correction to the report: the course is **Master**, the third of the dome's three (Beginner, Expert, Master).
Nothing in the script or the overlay ties the Icon to the Expert course.

The prize is split between two places that disagree about what "cleared the course" means. The item is added by the
arena program, and only when the contest ended on its thirteenth fight and an "already awarded" flag is clear. The flag
is set by the lobby's victory scene, which only asks whether this was a Master contest and whether it was won.

Early in the story the Master course is cut short to 8, 11 or 12 fights. Winning the shortened course counts as a win
for the victory scene, so it sets the flag and plays the presentation - the announcer names the Icon - but the arena saw
fewer than thirteen fights and added nothing. From then on the flag is set, so the arena skips the item every time.

The last of the three story flags that lengthen the course is set inside Jette's Fortress, which is where "before
defeating Jette" comes from.

### Mechanism

1. **Course latch.** The sign-up counter is record P1[9] of scene `koin1` (PROT entry 543). Each course arm debits the
   fee, sets flag `0x509`, sets one of `0x536` / `0x537` / `0x538` and clears the other two, then warps into the arena.
   The Master arm is at record `+0x0B6F` and sets `0x538`.
2. **Course length.** In `FUN_801CEA6C` of PROT entry 0977, for the Master course only (`bne v1,2` at `0x801CED28`),
   three story flags are tested in order and each missing one overwrites the length:

   ```text
   801ced44  jal 0x8003ce64   ; a0 = 0x378, missing: length 8
   801ced6c  jal 0x8003ce64   ; a0 = 0x382, missing: length 11
   801ced94  jal 0x8003ce64   ; a0 = 0x471, missing: length 12
   801cedb4  slt v0,s0,v0     ; finished when length < round + 1
   ```

   Finishing with the party standing sets the win latch at `0x801CEE10`, its only non-zero writer.
3. **Settlement.** `FUN_801D0F60` sets flag `0x50A` on a win (`0x801D101C`), then:

   ```text
   801d111c  slti a1,a1,0xd          ; round reached below 13?
   801d1124  bnez a1,0x801d1148      ; yes: no item
   801d112c  jal 0x8003ce64          ; a0 = 0x6cb
   801d1134  bnez v0,0x801d114c      ; flag set: no item
   801d113c  addiu a0,zero,0xcd
   801d1140  jal 0x800421d4          ; War God Icon x1
   ```

   The arena never sets `0x6CB`.
4. **Back in the lobby.** The scene-entry script `koin1` P1[0] at `+0x0181` clears `0x509` and spawns the result
   scenes. Record P2[9] is gated to require `0x50A`, so it is the winner's scene and plays after any won contest.
5. **The winner's scene** branches at record `+0x0369`: `76 CB` tests `0x6CB` and takes the ordinary ending when it is
   set; `75 38` tests `0x538` and jumps to `+0x03E1`, whose first instruction is `56 CB`, set `0x6CB`. The presentation
   text follows. The item appears there as a name token only; the record holds no give-item instruction. Neither test
   looks at the round count.
6. **Together.** A Master contest won at 8, 11 or 12 fights sets `0x50A`, gives nothing, and the lobby sets `0x6CB`.
   Every later settlement fails the test at `0x801D1134`. No instruction on the disc clears `0x6CB`.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `koin1` P1[9] `+0x0B6F` | PROT 543 | Master arm of the course menu, sets `0x538` | disc data |
| `0x801CED28` | PROT 0977 | course-length overrides, through `0x801CEDA4` | disassembly |
| `0x801CEE10` | PROT 0977 | win latch store (`0x801D1ADC`) | disassembly |
| `0x801D1A90`, `0x801D1A94` | PROT 0977 | course index, round reached | disassembly |
| `0x801D0F60` | PROT 0977 | settlement; item test `0x801D111C..0x801D1144` | disassembly |
| `koin1` P1[0] `+0x0181` | PROT 543 | lobby dispatch of the result scenes | disc data |
| `koin1` P2[9] `+0x0369` | PROT 543 | the two flag tests (MAN offset `0x4C93`) | disc data |
| `koin1` P2[9] `+0x03E1` | PROT 543 | set `0x6CB` (MAN offset `0x4D0B`) | disc data |
| `0x80085831` mask `0x10` | RAM | flag `0x6CB`, Icon presented | disc data |
| `0x800857FF` mask `0x80` | RAM | flag `0x538`, Master chosen | disc data |
| `0x800857F9` mask `0x20` | RAM | flag `0x50A`, contest won | disassembly |
| `0x800857C7` mask `0x80` | RAM | flag `0x378`, lifts the 8-fight cut | disassembly |
| `0x800857C8` mask `0x20` | RAM | flag `0x382`, lifts the 11-fight cut | disassembly |
| `0x800857E6` mask `0x40` | RAM | flag `0x471`, lifts the 12-fight cut | disassembly |

### Scope

The tests overwrite in order, so the last missing flag decides: with all three missing the course runs 12 fights.

| Flag | Course length while clear | Set by |
|---|---|---|
| `0x378` | 8 | `map03` P2[15] and scripts in nine other scenes |
| `0x382` | 11 | scripts in eleven scenes, `conc` P1[47] among them |
| `0x471` | 12 | `chitei2` P1[0] only (Jette's Fortress), once `0x470` is set by a staged fight that precedes the Jette fight |

The Icon is the only item the arena program grants. The dome's forfeit prize does not share the fault: its scene gives
the item with a real give-item instruction next to its own flag.

Course menu and arena rules: [`minigame-muscle-dome.md`](../../subsystems/minigame-muscle-dome.md).

### Verifying it

From a save with flag `0x471` clear, win the Master course. It ends after fight 12, a breakpoint at `0x801D1124` shows
`a1 = 1` (the branch is taken and the add is skipped), and after the lobby scene byte `0x80085831` has bit `0x10`.
The counter-test: clear that bit, set `0x471`, win thirteen fights, and the item arrives. Clearing bit `0x10` at
`0x80085831` repairs an affected save.

### Open questions

- Whether the party can leave Jette's Fortress between the setting of `0x471` and the Jette fight.
- Which story beat sets `0x382` first in an ordinary playthrough.
