# Items, stats and equipment

Five reported oversights in how the game handles items, stat boosts and equipment, each traced through the USA
executable and overlays. Every claim here is read from instructions or decoded from disc bytes; none of it is a runtime
capture, and each section says which parts are proven and which are predictions that follow from the code.

Images and bases used on this page:

| Name | Image | Load base |
|---|---|---|
| SCUS | `SCUS_942.54` | `0x80010000` |
| field | PROT entry 0897 | `0x801CE818` |
| battle | PROT entry 0898 | `0x801CE818` |
| menu | PROT entry 0899 | `0x801CE818` |
| arena | PROT entry 0977 | `0x801CE818` |
| cast 0945 | PROT entry 0945 | `0x801F69D8` |

Shared anchors:

- The game-state window starts at `0x80084140`. Character record `n` is `0x80084708 + n*0x414`, which is window
  `+0x5C8 + n*0x414`. A window displacement of `+0x6E4` is record `+0x11C`, `+0x75E` is record `+0x196`, `+0x6BC` is
  record `+0xF4`. Record layout: [`save-record.md`](../../formats/save-record.md).
- Battle actors are reached through the pointer table at `0x801C9370` (seats `0..2` party, `3..7` monsters). The battle
  context pointer is the word at `0x8007BD24`.
- A system flag `idx` is bit `0x80 >> (idx & 7)` of byte `0x80085758 + (idx >> 3)`. `FUN_8003CE08` sets, `FUN_8003CE34`
  clears, `FUN_8003CE64` tests.
- The bag is `(id, count)` byte pairs at `0x80085958` (window `+0x1818`).

## Stat boost overflow

**Status:** not confirmed
**Reported as:** using permanent stat-boost items past a stat's limit overflows the stat.

The permanent stat items cannot overflow anything on the USA disc. Each one adds a small fixed amount to a 16-bit stat
and then immediately compares the result against a ceiling, writing the ceiling back when it is exceeded. The largest
value that ever exists before the compare is `1003` (or `10015` for HP), far below the `65535` a 16-bit field holds.

What does misbehave is nearby: the boosts that only last one battle. Elixirs multiply a battle stat by 1.2 with no limit
on how many times they stack, the turn-order number derived from Speed is stored in 16 bits without a ceiling, and one
enemy self-buff has no ceiling at all. Those are the candidates for what the report describes, and they are presented
as candidates, not as a confirmed match.

Three numbers are easy to confuse. The **stored base** stat is what the items write. The **live** stat is the base plus
accessory percentages, capped again. Equipment is in neither: the menu adds equipment bonuses into a display total,
and battle folds them in separately, so a menu figure above 999 is a sum on screen and not a stored value.

### Mechanism

The permanent items:

1. Honey (`0x65`), Miracle Water (`0x6D`) and the six Waters (`0x82..0x87`) carry effect class 6 with tier `0..6`
   ([`item-effect-table.md`](../../formats/item-effect-table.md)).
2. Class 6 is arm `0x8004112C` of the item applier `FUN_800402F4`. The tier indexes a seven-entry jump table at
   `0x800151B0`.
3. Every arm has one shape - load, add, store, compare, store the ceiling:

   ```text
   800411e8  lhu v0,0x6ec(v1)      ; record +0x124, ATK
   800411f0  addiu v0,v0,0x4
   800411f4  sh v0,0x6ec(v1)
   800411fc  sltiu v0,v0,0x3e8     ; below 1000?
   80041200  bne v0,zero,<exit>
   80041204  _li v0,0x3e7
   8004120c  _sh v0,0x6ec(v1)      ; otherwise store 999
   ```

4. The item menu re-derives the live stats straight afterwards: `jal 0x80042558` at `0x801D8540` in PROT entry 0899,
   following the `jal 0x800402F4` at `0x801D8538`.
5. `FUN_80042558` copies record `+0x11C..+0x12C` to the live block `+0x104..+0x11A`, applies accessory percentages,
   and clamps every field at `0x80042C0C..0x80042CE0` (HP `0x270F`, MP `0x3E7`, AGL `0x118`, the rest `0x3E7`).
6. The usability check `FUN_8003FB10` greys the item out when no target can benefit (`sltiu 0x270f` at `0x8003FF44`
   is the HP arm), so the menu refuses a Water at the ceiling before the applier runs.

The in-battle candidates:

- **Elixirs saturate.** Class 7 (Power, Shield, Speed, Wonder Elixir) multiplies both halfwords of a battle-actor stat
  pair by 6/5 and pins the result at `0xFFFF`. There is no "already boosted" guard, so `N` uses give `stat * 1.2^N`
  until the pin. A stat of 300 pins after 30 uses, 999 after 23.

  ```text
  800414d8  lhu v1,0x164(a0)        ; SPD
  800414e4  multu v1,v0             ; * 0xCCCCCCCD, then srl 2: v1 / 5
  800414f8  addu s1,v1,v0
  800414fc  sltu v0,a2,s1           ; a2 = 0xFFFF
  80041500  bne v0,zero,0x80041510  ; over: store 0xFFFF
  ```

- **The turn-order key wraps.** `FUN_801DA780` in PROT entry 0898 computes `key = SPD + rand() % (SPD/2 + 1) + 1` in
  32 bits and stores it with `sh a0,0x16c(a1)` at `0x801DA84C`. Nothing clamps it. Once SPD is `0x8000` or more the sum
  can pass `0xFFFF` and the stored key becomes a small number, so an over-boosted character acts late instead of first.
  With SPD pinned at `0xFFFF` the stored key is only the random term. This is arithmetic read from the instructions;
  that a player reaches it (roughly thirty Speed or Wonder Elixirs on one character in one battle) is inference.
- **No stat is read as signed.** A sweep of PROT entry 0898 and SCUS for sign-extending halfword loads at actor
  `+0x154..+0x16A` finds one, the AGL gauge at `0x801F0A30`. ATK, UDF, LDF, SPD and INT are always loaded unsigned, so a
  pinned attack value does not turn negative.
- **Jugger Power wraps.** Cast `0xBA` in PROT entry 0945 adds a quarter to all ten stat halfwords with
  `lhu` / `srl 2` / `addu` / `sh` at `0x801F6DA0..0x801F6E34` and never compares the result. It is an enemy self-buff;
  the wrap would shrink the caster's own stats after enough casts (22 from a stat of 500).

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x80074368 + id*0xC` | SCUS | item record: `+0` kind, `+1` subtype, `+2` price | disc data |
| `0x800752C0 + subtype*4` | SCUS | effect descriptor: class, tier, flags, passive index | disc data |
| `0x800402F4` | SCUS | item applier; class jump table `0x80014FA0` | disassembly |
| `0x8004112C` | SCUS | class 6 arm (permanent stat-up); tier table `0x800151B0` | disassembly |
| `0x80041178` | SCUS | tier 0 Life Water: record `+0x11C` `+16`, ceiling `0x270F` | disassembly |
| `0x800411C4` | SCUS | tier 1 Power Water: `+0x124` `+4`, ceiling `0x3E7` | disassembly |
| `0x800413F4` | SCUS | tier 2 Guardian Water: `+0x126` and `+0x128` `+4`, ceiling `0x3E7` | disassembly |
| `0x80041210` | SCUS | tier 3 Swift Water: `+0x12A` `+4`, ceiling `0x3E7` | disassembly |
| `0x8004125C` | SCUS | tier 4 Wisdom Water: `+0x12C` `+4`, ceiling `0x3E7` | disassembly |
| `0x800412A8` | SCUS | tier 5 Magic Water: `+0x11E` `+8`, ceiling `0x3E7` | disassembly |
| `0x800412F4` | SCUS | tier 6 Honey / Miracle Water: `+4` to eight fields, AGL ceiling `0x118` | disassembly |
| `0x8003FB10` | SCUS | usability check that greys an item with no valid target | disassembly |
| `0x80042558` | SCUS | live-stat rebuild; clamps `0x80042C0C..0x80042CE0` | disassembly |
| `0x80041464` | SCUS | class 7 arm (Elixirs); SPD pair clamp `0x800414D8..0x80041568` | disassembly |
| `0x801DA780` | PROT 0898 | turn-order key; unclamped store at `0x801DA84C` to actor `+0x16C` | disassembly |
| `0x801F6DA0` | PROT 0945 | Jugger Power stat block, through `0x801F6E44` | disassembly |

### Scope

The Elixir pin and the key wrap apply to any actor an Elixir can target and to every pair class 7 touches: SPD,
UDF + LDF, ATK, and the fourth pair `+0x168/+0x16A` on the Wonder Elixir. The Jugger Power wrap applies to whichever
monster casts `0xBA`.

### Verifying it

- Set Vahn's stored ATK (`0x8008482C`) to `0x03E6` and use a Power Water: it reads back `0x03E7`. At `0x03E7` the menu
  refuses the item.
- In battle, write `0xFFFF` to a party actor's `+0x164` and `+0x166`, break at `0x801DA84C` and read `a0`: the value
  stored is below `0x8000`, and that character acts late.

### Open questions

- What the original report observed. If it is a Water or Honey effect, the record halfword before and after the use
  settles it; no USA code path wraps there.
- Whether the Japanese and PAL appliers carry the same compares.
- Whether the fourth class-7 pair `+0x168/+0x16A` is INT or AGL; it is clamped the same way under either name.

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

## Point Card softlock

**Status:** explained
**Reported as:** using the Point Card in battle with one of the party's own characters as the target hangs the battle.

Two separate defects combine, and both are read directly from the instructions. Neither has been run.

The first lets the cursor reach an ally. The Point Card is an enemy-targeted item. When it is picked, the game builds a
ring of selectable targets that starts from whoever this character targeted last. It builds the ring first and only
afterwards checks that the last target was an enemy. If the last action was aimed at an ally - any healing item or
Elixir, whose cursor starts on the user - that ally is already stored as the ring's first entry. The cursor is moved to
an enemy, but one press of Left steps back onto the ally.

The second makes it hang. The Point Card's damage routine subtracts HP from its target and does nothing else. Every
other routine that changes a party member's HP also records how far the on-screen HP number still has to count. At the
end of an action the game waits until every targeted party member's on-screen HP equals the real HP. Here they differ
and nothing is scheduled to close the gap, so it waits forever with the camera orbiting.

This is the first of the two softlock classes described in
[`battle-action-exit-gates.md`](../../subsystems/battle-action-exit-gates.md): the state-`0x51` HP-readout park. It is
not the state-`0x19` approach park. The trigger is a party-side HP write with no readout seed, reachable from the pad
on an unmodified disc.

### Mechanism

1. **The item.** Item `0xFE` has record bytes `02 7E 00 00` and descriptor `01 00 C4 41`: flags `0xC4` are enemy side
   (`0x40`), battle-usable (`0x04`), single target.
2. **The Item window builds the ring before validating.** In `FUN_801D0748` of PROT entry 0898:

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
3. **The ring builder** `FUN_801D8A88` counts living monsters into `ctx[+0x244]`, then stores the actor's current
   target unchecked as the ring head: `sb v0,0x245(v1)` at `0x801D8B30`. Up to three other living monsters go to
   `ctx[+0x246..+0x248]`.
4. **The cycler** `FUN_801D8D00` finds the current target at ring index `i`. "Previous" returns `ctx[+0x244 + i]` for
   `i != 0` (`0x801D8DAC..0x801D8DC4`); "next" returns the head when `i` is the last index (`0x801D8DA0`). With a stale
   ally in the head and the cursor on the first ringed monster, Left returns the ally.
5. **The picker** (phase `0x5B`, handler `0x801D23F0`) stores the cycler's result at `0x801D2630`. Its confirm path
   `0x801D24AC..0x801D2540` does not re-validate the target.
6. **Where the stale ally comes from.** The single-ally item arm seeds the cursor with the actor's own seat
   (`sb v1,0x1dd(v0)` at `0x801D19A0`). The target byte is set to `3` once at battle start (`0x801D87CC..0x801D87D8`)
   and is only re-picked at a round boundary when its target is dead (`0x801D8A34..0x801D8A54`), so a living ally stays
   there from turn to turn.
7. **The action.** State `0x3E` of the action state machine `FUN_801E295C` special-cases the id:

   ```text
   801e4080  lbu v1,0x1df(s3)
   801e4084  addiu v0,zero,0xfe
   801e4088  bne v1,v0,0x801e40c8
   801e40bc  addiu v0,zero,0xe
   801e40c0  sb v0,0x1e8(s3)      ; applier selector = 0x0E
   ```

   State `0x3F` calls `FUN_800402F4` with that selector and the target seat at `0x801E4134`.
8. **The discharge arm** `0x8004209C` takes `min(bank, 9999)` points off the bank at `0x800845B4`, queues a flinch or
   death clip (`+0x1DA`, `+0x1DC`) and writes HP (`sh v0,0x14c(v1)` at `0x8004219C`). It writes neither the pending
   readout accumulator `+0x10` nor the displayed HP `+0x172`. The heal and revive arms of the same function seed the
   accumulator at `0x800408FC`, `0x80040D28` and `0x800410BC`. An empty bank exits at `0x800420AC` before any store.
9. **The park.** State `0x51` only counts its exit timer down while `FUN_801E7250` returns 0 (`0x801E6044`). For a
   party target that function compares `lhu 0x14c` with `lhu 0x172` at `0x801E72A8..0x801E72B4` and returns 1 when they
   differ. The displayed value only moves through a drain that is skipped when the accumulator is zero.

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x800845B4` | RAM | Point Card bank, u32 | disassembly |
| `0x80074F50` | SCUS | item `0xFE` record | disc data |
| `0x800754B8` | SCUS | its effect descriptor `01 00 C4 41` | disc data |
| `0x801D18E0` | PROT 0898 | Item window, enemy single-target arm, through `0x801D1964` | disassembly |
| `0x801D8A88` | PROT 0898 | target ring builder; head store `0x801D8B30` | disassembly |
| `0x801D8D00` | PROT 0898 | target ring cycler | disassembly |
| `0x801DB8B4` | PROT 0898 | first-living-monster pick | disassembly |
| `ctx[+0x244..+0x248]` | RAM | ring count, head, three more seats | disassembly |
| `0x801D23F0` | PROT 0898 | phase `0x5B` picker; target store `0x801D2630` | disassembly |
| `0x801D19A0` | PROT 0898 | ally-item arm seeding the actor's own seat | disassembly |
| `0x801E4080` | PROT 0898 | state `0x3E` selector override for item `0xFE` | disassembly |
| `0x801E4134` | PROT 0898 | state `0x3F` call into the applier | disassembly |
| `0x8004209C` | SCUS | Point Card discharge arm, through `0x8004219C` | disassembly |
| `0x801E7250` | PROT 0898 | HP-readout settle check | disassembly |
| `0x801E6044` | PROT 0898 | state `0x51` gate on the settle check | disassembly |
| actor `+0x10`, `+0x14C`, `+0x172` | RAM | readout accumulator, live HP, displayed HP | disassembly |
| actor `+0x1DD`, `+0x1DF`, `+0x1E8` | RAM | target seat, item id, applier selector | disassembly |

### Scope

These follow from the code and are untested:

- It hangs on any party target, not only the user. The user is just the seat the ally cursor leaves behind.
- It hangs whether or not the hit is lethal. In the lethal case the readout shows the old HP on a character at 0.
- With an empty bank nothing is written and the battle continues.
- Aimed at a monster it is harmless: the settle check returns 0 for seats `3..7`.

The out-of-order arm serves every single-target enemy-side item, which by the descriptor table is the Point Card and
the summon flutes' single-target members. A flute takes a different applier path, so whether it also hangs is open.

### Verifying it

Use a healing item on the acting character, or open and cancel its ally cursor. Next turn pick the Point Card and press
Left. A breakpoint at `0x801D2630` shows `v0` in `0..2`. Confirm, then watch `ctx[+0x07] == 0x51` with `ctx[+0x6D8]`
frozen at `0x3C`, the target's `+0x14C` different from its `+0x172`, and its `+0x10` zero. Writing `+0x172 = +0x14C`
by hand releases the battle.

### Open questions

- One community report says the character is revived at once before the hang. State `0x50` runs the Lost Grail
  auto-revive, which may be what was seen; it may also be the stale readout. Not traced.
- Whether the Point Card debited at commit (`FUN_80042310` at `0x801D24FC`) is handed back.

## Coin vendor purchases past 99 (Japanese release)

**Status:** partially explained
**Reported as:** on the Japanese release a coin vendor lets a purchase through while 99 are already held; the coins are
taken and the count stays at 99.

Only the USA disc is read here. The USA purchase path and the USA bag routine are both proven; what the Japanese build
lacks is inferred from them.

The coin counter never asks the bag whether an item fitted. It has one check before the Yes / No box - "fewer than 99
held?" - and after that it adds the item and subtracts the coins unconditionally. The bag's add routine never refuses
either: it adds and then silently trims the stack back to 99.

On the USA disc the front check is present, so the trim never matters. Take the front check away and the result is
exactly the report: the coins go, the item is added and trimmed off again. The opposite hypothesis - the check present
but the trim missing - would give stacks above 99, which is not what is reported.

### Mechanism

1. The prize session is `FUN_801DC1CC` in PROT entry 0899. Its table is at `0x801E4518`: 8-byte records of
   `[i16 item id][i16 one-shot flag][u32 coin price]`, `0x60` bytes per vendor
   ([`shop.md`](../../subsystems/shop.md)).
2. On confirm:

   ```text
   801dc3c0  lw v0,0x45a4(a2)        ; coins
   801dc3c4  lw v1,0x4(a0)           ; price
   801dc3d0  bne v0,zero,0x801dc3f8  ; cannot afford: buzz
   801dc3dc  jal 0x80042f4c          ; held count
   801dc3ec  slti v0,v0,0x63
   801dc3f0  bne v0,zero,0x801dc408  ; fewer than 99: open Yes / No
   801dc3f8  jal 0x80035bd0          ; otherwise buzz and stay
   ```

3. The commit has no test:

   ```text
   801dc514  jal 0x800421d4          ; bag add (id, 1), result ignored
   801dc54c  subu v1,v1,v0
   801dc550  sw v1,0x464(a0)         ; coins -= price
   ```

4. The bag add `FUN_800421D4` ends with the trim and returns the slot index either way:

   ```text
   800422e8  addu v0,v1,v0           ; count + quantity
   800422f0  slti v0,v0,0x64
   800422fc  li v1,0x63              ; over 99: 99
   80042300  sb v1,0x1819(a0)
   ```

### Addresses

| Address | Image | What | Evidence |
|---|---|---|---|
| `0x801DC1CC` | PROT 0899 | prize-exchange session; phase word `0x801E46AC` | disassembly |
| `0x801E4518` | PROT 0899 | prize table, 8-byte records, `0x60` per vendor | disc data |
| `0x800845A4` | RAM | coin bank | disassembly |
| `0x801DC3C0` | PROT 0899 | affordability and held-count checks, through `0x801DC3F8` | disassembly |
| `0x801DC4C0` | PROT 0899 | commit: add, debit, set one-shot flag | disassembly |
| `0x80042F4C` | SCUS | held count of an item id, 0 when absent | disassembly |
| `0x800421D4` | SCUS | bag add; trim at `0x800422E0..0x80042300` | disassembly |
| `0x801DB21C` | PROT 0899 | gold-shop buy-list confirm; tests gold only | disassembly |
| `0x801DB380` | PROT 0899 | buy recipient picker; "to the bag" row at `0x801DB478` | disassembly |
| the missing check on the Japanese disc | - | the `jal 0x80042f4c` / `slti 0x63` pair | inference |

### Scope

Every giver on the disc goes through the same trimming add, so a chest, a battle drop or a prize at 99 is discarded
without a message.

The USA gold shop has a sibling of the reported fault. Stackable items go through a quantity screen capped at
`99 - held`, but equipment does not. The buy-list confirm `FUN_801DB21C` tests only gold before routing on the item
kind, and the recipient picker's "to the bag" row runs `jal 0x800421d4` at `0x801DB490` and then debits gold with no
held-count read between them. Buying a hundredth copy of one piece of equipment should take the gold and leave 99.
This is inferred from the instructions and untested.

### Verifying it

On the USA disc hold 99 of a prize item and try the counter: the buzz plays and no box opens. Force the branch at
`0x801DC3F0` to be taken and buy: the coins drop and the count stays `0x63`, which is the reported behaviour. On a
Japanese disc, find the matching routine and look for the absent `slti ...,0x63`.

### Open questions

- The Japanese code itself.
- Whether the USA equipment sibling is reachable in normal play.

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
