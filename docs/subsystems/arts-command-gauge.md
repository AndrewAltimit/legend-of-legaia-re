# Arts command gauge - weapon-specialty arm width

When a character's turn opens the Arts command input, the battle UI draws an **action gauge**: a fixed pool of AP (Action Points) the player spends by inputting directional commands (High / Low / and the two **arm** swings). Each command consumes a per-command AP cost. The cost of the **arm** command is **not constant** - it depends on the class of the equipped weapon relative to the character's favored class. Equip a weapon outside your class and the arm command costs more AP, so fewer commands fit in the gauge; the Astral Sword costs the most of all. This is the engine side of the "weapon specialty" mechanic.

The popular description is "an off-class weapon **doubles** the arm command." The byte-level behaviour is a base cost plus an **escalating class penalty**, not a flat ×2 - see [the measured values](#measured-arm-cost).

## Summary

1. Each of the four direction commands on the Arts bar carries an 8-bit **AP cost**. The gauge builder uses that one byte both as the command's price and as the width of the pennant drawn for it (`cost - 6` pixels wide, next pennant `cost` pixels later). A larger button and a more expensive command are the same value. The pennant is a text window and its label has to fit: 30 (retail's floor) is the lowest clean width, 24 is still legible, below that the label smears - see [How the gauge consumes it](#how-the-gauge-consumes-it).
2. Only the command that swings the **weapon hand** depends on equipment - Left (`0x0C`) for Vahn and Gala, Right (`0x0D`) for Noa. It takes one of three authored values: `0x1E` (30) favored class, `0x2A` (42) off-class, `0x36` (54) far off-class. The Ra-Seru arm and the two leg commands are always 30.
3. The game performs **no class comparison** at runtime. The cost is authored per (character, weapon) inside each character's player battle file and copied into RAM unchanged at battle load.
4. The turn budget the costs are spent against is the character's **AGL** stat. A turn admits commands until the next one would exceed the remaining budget, then the input ends by itself.
5. The Astral Sword is Vahn's only weapon authored at 54. It is not a special case in code; it sits on the same tier Noa reaches with any club or axe.

## Contents

- [Summary](#summary)
- [Where the cost lives](#where-the-cost-lives)
- [Measured arm cost](#measured-arm-cost)
- [How the gauge consumes it](#how-the-gauge-consumes-it)
- [Where the gauge pool comes from](#where-the-gauge-pool-comes-from)
- [Re-arming the gauge at art start](#re-arming-the-gauge-at-art-start)
- [Status limb gating](#status-limb-gating)
- [Weapon classes and favored mapping](#weapon-classes-and-favored-mapping)
- [Execution path](#execution-path)
- [Who writes the cost](#who-writes-the-cost)
- [Disc location](#disc-location)
- [Confidence and open threads](#confidence-and-open-threads)
- [What an art costs in AP](#what-an-art-costs-in-ap)
- [Arts AP override hook](#arts-ap-override-hook)
- [If the Astral Sword is forced onto another character](#if-the-astral-sword-is-forced-onto-another-character)
- [Common misconceptions](#common-misconceptions)
- [Address reference](#address-reference)
- [See also](#see-also)

## Where the cost lives

The per-command AP cost is a runtime field, not a static table row:

| Symbol | Meaning |
|---|---|
| `DAT_801C9360` | Per-character **command-data pointer table** (one pointer per active party member), in battle bss. Each entry points into the **loaded player battle data** (the `battle_data` block, [extraction 863..866](../formats/battle-data-pack.md)). |
| `DAT_801C9360[char]` | Pointer to that character's array of per-command struct pointers, indexed by **command code** (`cmd * 4`). |
| `…[cmd] + 0x74` | The **arm width / AP cost** byte for that command. This is the field the weapon-specialty mechanic writes. |

So the full access is `*(u8 *)( *(u32 *)( *(u32 *)(DAT_801C9360 + char*4) + cmd*4 ) + 0x74 )`.

The command codes are a small fixed set; the default 4-command display uses `DAT_801F4B8C = [0x0C 0x0F 0x0E 0x0D]` (overlay `0898` rodata), with a sibling icon-base table `DAT_801F4B94 = [0x0D 0x10 0x11 0x0C]`. Command **`0x0C` is the arm command** whose `+0x74` cost varies with the weapon; the other command codes hold a constant cost.

## Measured arm cost

Reading `DAT_801C9360[Gala][0x0C] + 0x74` from a live battle for the **same character** with different weapons equipped (and Vahn holding the Astral Sword) isolates the class penalty exactly:

| Equip | Class vs character | Arm (`0x0C`) cost |
|---|---|---|
| Gala + Ra-Seru Club | favored (club user, club weapon) | `0x1E` (30) |
| Gala + Nail Glove | off-class (club user, claw weapon) | `0x2A` (42) |
| Vahn + Astral Sword | always-double exception | `0x36` (54) |

The other commands (`0x0D`/`0x0E`/`0x0F`) stay at `0x1E` (30) in every case. So the model is a **base `0x1E` plus a class penalty**: `+0x0C` for an off-class weapon, `+0x18` for the Astral Sword. The Astral penalty is twice the off-class penalty, which is where the "double" shorthand comes from - but the off-class case itself is `+0x0C` over base, i.e. ×1.4, not ×2.

## How the gauge consumes it

The gauge is assembled by `FUN_801D388C` (the battle action/animation event handler, driven by the battle main dispatcher [`FUN_801D0748`](../reference/functions.md)). In its **case `9` / `0x2C`** (gauge build) it reads the cost:

```c
bVar3 = *(u8 *)( *(u32 *)(DAT_801C9360[char][cmd]) + 0x74 );   // arm width / AP cost
ctx[slot + 0x14] = bVar3;                                       // per-slot AP cost
gauge_slot.icon_pos = bVar3 - 6;                               // visual width on the bar
```

and **case `0xB`** spends it against the remaining AP at `ctx + 0x6DC`:

```c
if (ctx[0x6DC] < ctx[slot + 0x14]) return;   // not enough AP for this command
ctx[0x6DC] -= ctx[slot + 0x14];              // consume the command's cost
```

Because a higher `+0x74` widens the gauge slot (`bVar3 - 6`) **and** drains more of the AP pool, an off-class arm both *looks* wider and lets fewer total commands fit - the visible "longer arm input."

**Two budgets, not one.** The AP pool bounds a turn by *cost*, but the
committed command buffer `actor[+0x1DF..+0x1EE]` bounds it by *count*: sixteen
bytes, a cap the cancel wipe (`sltiu v0,s3,0x10` over `sb zero,0x1df`), the
preseed `FUN_801DA34C` and the Super applier `FUN_801EF9E4` all share. A plain
direction press is one byte; a matched art keeps its leading arrows and
replaces its last one with the two-byte `[0x19 starter][art id]` pair, so an
N-arrow art occupies N + 1 bytes, and a Super / Miracle tail-replaces the
buffer with its own finisher bytes (`super-art-queue-capture.md` shows Vahn's
`0F 0E | 19 27 0F | 19 1F 0E | 1A 2B 2B 2B`). At retail costs the pool binds
long before sixteen tokens do; a mod that prices commands low enough that
`pool / cost` passes sixteen hits the buffer instead.

**The lowest drawable cost.** Each pennant is registered as a text-window
actor of width `cost - 6` (`FUN_801D8DE8` → `FUN_8003541C(kind, .., label,
x, y - 2, width, height, style)`, four `0x18`-byte window records at
`0x80076E98..` in SCUS `.data`), and the window renderer condenses the
label (`High` / `Arms` / `RaSeru` / `Low`) horizontally to fit that width
rather than clipping it. An emulator sweep injecting one cost into all four
`+0x74` bytes and rebuilding the gauge (`scripts/pcsx-redux/autorun_apcost_visual.lua`)
puts the floor where retail put it: 30 (24 px) is the tightest width that
draws every label cleanly, 24 (18 px) is condensed but legible, 20 and below
smear into glyph fragments, and 7 (1 px) leaves only the arrow caps. The
`(cost - 30) * DAT_8007B650[slot] / 2` term at `0x801D3B64` only re-centres
the pennant; the AP fill bar itself is unaffected at any value. A cost below
6 would wrap the width negative. The patcher's equipment editor refuses
costs below 24 for this reason.

The same case-`9`/`0xB` machinery also deals and spends the **Muscle Dome hand**: a dome card is one of the four direction commands (`0xC..=0xF`, the deck table `DAT_801f4b8c`), its cost is this same `+0x74` byte, and the commit debits the same `ctx+0x6DC` pool - see [`minigame-muscle-dome.md`](minigame-muscle-dome.md#hand-deck-decoded).

What the input *screen* draws while this machinery runs - the High/Left/
Right/Low chips, the pennant input bar, the Triangle "Hyper Arts list"
overlay and their texture sources - is packet-pinned in
[`minigame-muscle-dome.md` § Arts command input](minigame-muscle-dome.md#arts-command-input-packet-pinned)
(the dome runs the standard battle input verbatim, so the decomposition
there is the battle one).

The **enemy analogue** is the AGL action-budget in `FUN_801E9FD4`: a monster fills its per-turn action queue by rolling candidate moves and paying each move's `+0x74` cost out of the per-round AGL gauge (`actor[+0x154]`), the same "wider cost = fewer commands" mechanic on the AI side - see [`battle-action.md` § Enemy AGL action-budget](battle-action.md#enemy-agl-action-budget-fun_801e9fd4).

> A separate `+2` in the same case (`icon = DAT_801F4B94[i] + 2`, gated on an *empty* equip slot, `equip[cmd] == 0`) is an empty-slot icon tweak, **not** the class penalty - a fully-equipped off-class character still shows the widened arm via the `+0x74` cost above.

## Re-arming the gauge at art start

`FUN_801e93c8` (battle overlay, PROT 0898;
`see ghidra/scripts/funcs/overlay_battle_action_801e93c8.txt`) resets the
per-actor gauge slot flags when a committed action **finishes** - its only call
site is the Done/cleanup arm (`0x50`) at `0x801E5F64`, right after the
`0x50 -> 0x51` advance - so the next art draws
its arrows from a clean state. It reads the active actor
(`_DAT_8007bd24 + 0x13` indexes the actor-pointer table `DAT_801C9370`), then
gates on **what** was staged: the actor's last-staged action id `+0x1D9`. For a
party slot (index `< 3`) the re-arm runs only while `+0x1D9 < 0x10` - i.e. the
staged id is a plain direction (`0x0C..=0x0F`), not a materialized art or
starter (`>= 0x10`). For a monster (index `>= 3`) it resolves the materialized
art record (`+0x4C`) instead and bails when the record's `+0x87` flag byte is
set. When the gate passes it walks all seven actor slots, clearing each slot's
`+0x21C` latch (only when it holds `1`) and writing `+0x21D = 8` - restoring the
per-actor **animation-rate scalar** to normal after an art's slow-motion arms
(`FUN_8004AD80`) dropped it to `4` / `2` / `0`. It then clears the **battle
context's** `+0x243` byte (`ctx[+0x243] = 0`, `0x801E94F8`, off the pointer
re-loaded from `_DAT_8007BD24`) - the marker state `0x3C` sets, not an actor
field.

Nothing later overwrites `+0x21D` with an arm cost: the gauge builder
`FUN_801D388C` never touches that byte, and the per-command `+0x74` cost it
reads lands in `ctx[0x14 + slot]` (`0x801D3B3C`) with the icon width as
`cost - 6`. The `+0x1D9 < 0x10` gate is the same
direction-vs-materialized-art split the action queue uses (see
[art-data.md § Action Constants](../formats/art-data.md#action-constants)).

## Status limb gating

A **Rot** (or similar limb-disable) status grays individual command arrows and
refuses their input. The gauge-input arm `FUN_801D0748` state `0x50`
(`overlay_battle_action_801d0748.txt:3311-3360`) reads the active actor's
`+0x16E` status halfword; the gray-draw pass and the input gate agree
bit-for-bit:

| `+0x16E` bit | Arrow grayed (draw pos) | Blocks command |
|---|---|---|
| `0x08` (limb 0) | LEFT (`0xb3 - w/2, 0x42`) | Left `0x8000` / dir 0 |
| `0x10` (limb 1) | RIGHT (`0xe5 + w/2, 0x42`) | Right `0x2000` / dir 3 |
| `0x20` (limb 2) | UP (`0xcc, 0x22`) **and** DOWN (`0xcc, 0x62`) | Up `0x1000` / dir 1 **and** Down `0x4000` / dir 2 |
| `0x1000` (**Curse**) | the whole MAGIC command (`FUN_801dbec4(0xf8, 0x42)`, `:3229-3230`) | Magic |

With all three limb bits set (`0x38`) the whole Arm command is skipped and
Attack is unusable (`801d0748:3226-3227,3277`; `801e295c:5452`). This pinned
map **replaces** the engine's earlier reconstructed Left/Right/Down arrow-gray:
the retail assignment is Left = `0x08`, Right = `0x10`, and Up + Down together =
`0x20` (a single bit grays two arrows), not one bit per arrow. Rot rolls exactly
one of these three bits (`1 << (rand%3 + 3)`); see
[battle-formulas.md § status application](battle-formulas.md#status-application-the-art--move-record-status-byte).

## Weapon classes and favored mapping

"Off-class" is decided by the equipped weapon's **class** versus the character's favored class. The class is legible from the static item-property records (`DAT_80074368 + id*12`, 12-byte stride): the record's description pointer (`+8`) is **shared per class**, and the description carries a `Best:<character>` token. Universal weapons (equip-mask `0b111`) partition cleanly:

| Class (description pointer) | "Best" character | Example universal weapons |
|---|---|---|
| knife / sword (`0x800128D4`) | Vahn | Survival Knife, Battle Knife, Short Sword |
| claw (`0x80012870`) | Noa | Nail Glove, Crimson Nails, Fighter Claw, Bloody Claw |
| club / axe (`0x8001280C`) | Gala | Survival Club, Red Club, Survival Axe, Battle Axe |

Character-specific weapons (equip-mask `0b001`/`0b010`/`0b100`, e.g. Ra-Seru Blade / Fangs / Club) are locked to one owner by the [equip-character mask](../formats/equipment-table.md) and are always favored for that owner. The **Astral Sword** (`0xBA`) has its own description pointer (`0x80011710`), matches no character, and always takes the maximum penalty.

So: favored mapping is **knife/sword → Vahn, claw → Noa, club/axe → Gala**.

## Execution path

Once a combo is committed, it is replayed by the **Arms execution resolver `FUN_801EC3E4`** (overlay `0898`), which is **called from `SCUS_942.54` at `0x800478A0`** (`jal 0x801EC3E4`) - the arts execution driver is the static side, which is why the resolver has no caller inside the overlay. The resolver advances the input cursor (`actor + 0x1F4`) one step per recorded command and dispatches per-command sub-handlers through the jump table `PTR_801CF4B4[(actor + 0x1D9) - 0xC]`. These sub-handlers read the equipped weapon again to fold it into the damage calculation. This execution-time weapon read is **distinct** from the gauge-build cost above.

### The execution-time weapon fold

The dispatch is bounded at **six arms** (`(command - 0x0C) < 6`, i.e. commands `0x0C..=0x11`), and the head admission gate is a *different* band read from a *different* place: it tests the caller's command-record byte with `(cmd - 0x0C) < 0x14` (`0x0C..=0x1F`). A command in `0x12..=0x1F` is therefore admitted and then folds nothing.

Each arm resolves one or more of the character record's five equipment slots (`+0x196..+0x19B`) through the two-hop lookup item property record `DAT_80074368 + id*0xC` byte `+1` → equipment stat row `DAT_80074F68 + row*8` byte `+1` (the **attack** bonus), and adds it into the actor's **ATK working** halfword `+0x158`:

| command | equipment slots | fold into `+0x158` |
|---|---|---|
| `0x0C` | 2 | `atk[2] >> 1` |
| `0x0D` | 3 | `atk[3] >> 1` |
| `0x0E` / `0x0F` | 4 | `atk[4] >> 1` |
| `0x10` | none | nothing |
| `0x11` | 0,1,2,3,4 | `(sum of all five) >> 1` |

`0x0E` and `0x0F` share a jump-table arm (slots `[2]` and `[3]` hold the same target), and `0x10`'s slot is the same address the bounds check bails to - a live table entry that folds nothing. Retail applies no empty-slot test and no `kind == 1` item-class guard here, matching the battle-load seeder's behaviour.

This is the counterpart to the battle-load asymmetry recorded in [`battle-formulas.md`](battle-formulas.md): the seeder `FUN_80053CB8` folds the equipment table's UDF / LDF / SPD bytes and folds **neither** INT nor ATK, so a weapon's attack bonus never reaches the actor's ATK **base** (`+0x15A`). It reaches ATK **working** here instead, per committed command. The seeder's omission is correct, not a gap.

Ports: `legaia_engine_vm::battle_formulas::arms_command_equip_slots` / `arms_weapon_atk_fold` / `arms_resolver_admits`; the live loop seeds a party slot's `battle_attack` without the equipment sum and adds the halved slot per swing (`World::battle_equip_atk`). The player-facing formula this fold feeds is on [battle-formulas.md](battle-formulas.md#base-offense-value-base-atk-plus-half-of-one-equipment-slot).

## Who writes the cost

The cost is **not** computed by a runtime favored-class comparison. It is written once at battle load (the `game_mode 0x14 → 0x15` transition) as a **verbatim copy** out of the assembled battle-character buffer:

- The writer is `FUN_800557B8` (the per-command-struct copy routine in `SCUS_942.54`): a fixed 43-word block copy from the source `a1` to the runtime struct `a0` (`lw v0,(a1)` → `sw v0,(a0)`, the cost word at struct `+0x74` lands inside that block) followed by a variable-length tail whose length is `(src[0] * src[1] * 9 + 5) / 4`. There is **no arithmetic on the cost value** between load and store.
- It is called from the **battle character-assembly chain** (`FUN_80052770` → … → the call site at `0x80053330`; see [character-mesh assembly](../formats/character-mesh.md)), which splices the equipped item's section into the per-character battle buffer. Confirmed by a live write-watch on the cost field through a field→battle transition - the only write fires here, at battle load, with `pc = 0x80055810`.

So the arm cost originates in the **equipped weapon's section of the per-character [player battle file](../formats/battle-data-pack.md)** (extraction 863..866) and is carried verbatim into the runtime struct. The "off-class penalty" is therefore **per-(character, weapon) data baked into those files** - favored-class weapons simply carry a low arm cost in that character's file and off-class weapons a higher one - not a class comparison the engine performs. The same weapon yields different costs in different characters' files (a claw is cheap in Noa's file, expensive in Gala's).

## Disc location

Inside the [player battle file](../formats/battle-data-pack.md), the cost is in the weapon's section, reached through the section's **swing-action record**:

```
section (decoded)
  +0x04  u32 swing_rec_a   ; offset (within the section) to the swing/arm command record
  …
  swing_rec_a + 0x74       ; u8 arm cost  ← the weapon-specialty byte
```

The descriptor table keys sections by **equippable item id**, so each equippable weapon has its own section and its own swing record. Decoding all three player files (`asset battle-data-pack <file> --out`) and reading `section[+0x04] + 0x74` per weapon gives a clean, byte-exact picture - favored-class weapons carry `0x1E` (30), off-class weapons carry higher costs that scale with class distance:

| character (file) | favored class → `0x1E` | off-class → `0x2A` | far off-class → `0x36` |
|---|---|---|---|
| Vahn (863) | blade / knife / sword / fist | claw, axe | - |
| Noa (864) | claw / feral / fang (+ knife) | sword / blade | club / axe |
| Gala (865) | club / axe / mace | claw, knife | - |

The classes are finer than three families. Per weapon, reading every section of the three files (all other direction commands read `0x1E`):

| Weapon | Vahn | Noa | Gala |
|---|---|---|---|
| Survival Knife, Battle Knife | `0x1E` | `0x1E` | `0x2A` |
| Short Sword, Force Blade | `0x1E` | `0x2A` | `0x1E` |
| Beast Buster, Chaos Breaker | `0x1E` | - | `0x1E` |
| Nail Glove, Crimson Nails, Fighter Claw, Bloody Claw | `0x2A` | `0x1E` | `0x2A` |
| Survival Club, Red Club | `0x1E` | `0x36` | `0x1E` |
| Power Club, Survival Axe, Battle Axe, Great Axe | `0x2A` | `0x36` | `0x1E` |
| Astral Sword (`0xBA`) | `0x36` | - | - |
| character-locked gear (Ra-Seru weapons, Feral / Hard Beat / Heavy Strike, Holy / Golden Claw, Mace) | `0x1E` | `0x1E` | `0x1E` |

So Gala swings a Short Sword at the favored price but a knife off-class, Vahn swings the light clubs favored but the Power Club and every axe off-class, and Noa is the only character with a `0x36` tier on ordinary gear. The Astral Sword is not a code exception: it is simply Vahn's one `0x36` section, the same tier Noa gets from an axe.

Cross-checked against live RAM: Gala + Nail Glove reads `0x2A`, Gala + Ra-Seru Club reads `0x1E` - matching that file's `0x28` and `0x21` sections. The cost lives inside the section's **LZS-compressed** stream, so an editor decompresses the section, rewrites the byte at `swing_rec_a + 0x74`, recompresses, and writes back within the slot footprint.

### Reading it

`legaia_asset::battle_char_assembly::swing_command_costs(buf, pack, equipped)` returns the four costs for one equipped set, indexed in direction-command byte order (`Left, Right, Down, Up` = runtime action slots `0xC..=0xF`). It is the splice path, not a descriptor-id lookup: `select_sections` matches an equipped id positionally **inside its own section**, so the equipment index that re-prices a swing is whichever section the file keys the weapon under - and that differs per character.

Vahn's and Gala's files carry the weapons in section 2 (slot `0xC`, Left) with the Ra-Seru in section 3; Noa's file carries Ra-Seru Terra in section 2 and the weapons in **section 3**, so her weapon-priced command is slot `0xD` (**Right**). The character records agree: a retail save reads Vahn `[.., 0x1B Ra-Seru Blade, 0x09 Meta, ..]`, Gala `[.., 0x21 Ra-Seru Club, 0x19 Ozma, ..]`, Noa `[.., 0x11 Terra, 0x1F Ra-Seru Fangs, ..]` at `+0x196..`, which is why the [save-record](../formats/save-record.md) labels `+0x198` weapon / `+0x199` Ra-Seru hold for Vahn and Gala only.

An id placed at the wrong index silently falls through to the section default and every weapon reads `0x1E` - the failure mode to expect when a cost sweep comes back constant.

Every caller reads it here: the port's Arts input, the Muscle Dome's per-command cost, and the disc-gated pin in `crates/asset/tests/battle_data_pack_real.rs`. One byte prices one command, and the dome is a restricted normal battle, so a second reader would be a second answer.

## Confidence and open threads

**Confirmed** (live-pinned + byte-validated against the disc): the cost field `DAT_801C9360[char][0x0C] + 0x74`, its measured values, the case-`9` read and case-`0xB` AP spend in `FUN_801D388C`, the SCUS call site of the execution resolver, the **writer** (`FUN_800557B8`, verbatim copy from the LZS-decoded equipment section at battle load - no runtime penalty arithmetic), and the **disc location** of the cost byte (`section[+0x04]` swing record `+0x74` in the player battle files, tabulated above).

**Inferred**: the identification of the weapon-hand command as "the arm" (`0x0C` Left for Vahn and Gala, `0x0D` Right for Noa - the only command whose cost tracks the weapon; the live measurements above were taken on Gala and Vahn).

The weapon-specialty mechanic is therefore a fully editable data table: rewrite a character's favored-class arm costs up / another class's down to reassign their specialty. The [randomizer](../tooling/randomizer.md)'s `--weapon-specialty` does exactly this - it permutes the three favored families among the characters by rewriting these bytes (decompressing / re-compressing each touched section in place).

## Where the gauge pool comes from

The pool the costs above are spent against, `ctx + 0x6DC`, is **the acting
actor's AGL** - the same `actor + 0x154` gauge the enemy action budget spends.
`FUN_801D388C` seeds it straight off the actor-pointer table:

```text
801d38c8  addiu s4,v1,0x11        ; s4 = ctx + 0x11, so 0x2(s4) is ctx[0x13]
801d38e4  addiu s6,v1,0x6d6       ; s6 = ctx + 0x6D6, so 0x6(s6) is ctx + 0x6DC
...
801d4df8  lbu  v0,0x2(s4)         ; active actor index, ctx[0x13]
801d4e00  sll  v0,v0,0x2
801d4e04  addu v0,v0,v1           ; &DAT_801C9370[slot]
801d4e08  lw   v0,0x0(v0)         ; the live battle actor
801d4e10  lhu  v0,0x154(v0)       ; its AGL gauge
801d4e18  sh   v0,0x6(s6)         ; -> ctx + 0x6DC, the command-gauge pool
```

So the party command gauge and the [enemy AGL action
budget](battle-action.md#enemy-agl-action-budget-fun_801e9fd4) are **one
mechanic, not two**: both fill a per-turn pool from `actor + 0x154` and both
spend a per-action `+0x74` cost out of it. The party spends it on the direction
commands of an Arts input; a monster spends it on swing records it rolls. The
number of commands a turn admits is `AGL / cost` on either side, which is why a
retail party turn runs to two-to-four commands at the base cost `0x1E` (30) and
why a wider off-class arm (42) or the Astral Sword (54) buys fewer of them.

There is no alternate seed. All four `ctx + 0x6DC` stores in the builder
(`0x801D3A30`, `0x801D4E18`, `0x801D5068`, `0x801D5364`) read the acting actor's
AGL `+0x154`; the same value less 6 is *also* written out to `_DAT_80076D7E`
(`0x801D3A38`) for the readout, which is a destination, not a source. The
decompiled C renders that pair in the opposite order - read the disassembly.

## The port's input session

`legaia_engine_core::arts_command_input` is the retail flow: the Arts command
opens a per-press directional entry, each press appends its command byte to the
actor's buffer and debits that command's `+0x74` cost from the turn pool, and
entry ends either by itself once nothing is affordable or on the confirm mask
([`0x50` exits](#leaving-state-0x50)). The review screen's next press reaches
**Begin | Reselect** (`0x6E`).
The entered sequence resolves through the `legaia-art` matcher family - an
exact Miracle string replaces the whole queue, a recognised sequence ending on
a Super combination replaces the tail, and otherwise each named art contributes
its record's strikes with unmatched directions staying plain swings.

Costs come from the equipped set at scene entry
([above](#reading-it)) into `World::battle_swing_costs`; the pool seeds from
the actor's AGL. Sessions live at `World::battle_arts_input`, and
`World::arts_input_active()` is what a host's party status strip reads to park
itself, since retail moves the status plate off-screen for the whole session.

### What still diverges

| | retail | port |
|---|---|---|
| pool | actor AGL (`+0x154`) | actor AGL; `100` with no roster loaded |
| direction command | the `+0x74` byte | the `+0x74` byte, per equipped set |
| ending entry | auto-end, **or** confirm mask | same |
| cancel, buffer typed | clears the entry, refunds the pool | same |
| cancel, buffer empty | leaves to `0x78` / `0x28` | leaves to the command menu |
| art body | paid from **Spirit** `+0x170` | free - the swings are the whole price |
| target | pre-picked with the command | picked after Begin |

The first two rows are the gap this closes: the old `ap_gauge` model counted a
fitted `4 + level/10` pool and comped the swings entirely, so the
weapon-specialty mechanic was invisible to it. `ap_gauge` still backs the
Spirit-command path and the [AP override hook](#arts-ap-override-hook); it is
no longer what an Arts input spends.

Rows three to five were previously recorded here as port-only conveniences -
"retail auto-ends only", "retail cannot back out of the Arts command". Both
claims are **falsified**; see [Leaving state `0x50`](#leaving-state-0x50) for
the instructions. The port already matched retail on the confirm and on the
empty-buffer cancel; what it was actually missing, undisclosed, was the
typed-buffer cancel, and that is now wired.

The one real remaining gap is the un-closed half of the
[two-gauge split](#what-an-art-costs-in-ap): the port does not yet charge the
art body out of Spirit, so a turn's whole cost is its swings.

### Leaving state `0x50`

Three exits, two of them pad-driven. All three are in `FUN_801D0748`'s `0x50`
arm, and all three consult the committed count `ctx+0x19` (`0x8(s1)`).

**Auto-end.** `801d2054`..`801d2078` walks the four costs at `ctx+0x14`
against the pool, leaving `s0 = 0` when at least one is affordable and `s0 = 4`
when none is. `801d208c bne s0,zero,801d20ac` then writes `0x5A`.

**Confirm** - the configurable mask `_DAT_800846D0`, the same one the round
prompt and every menu in the game read:

```text
801d207c  lbu  v0,0x8(s1)        ; committed count; 0 skips to the cancel test
801d2084  beq  v0,zero,0x801d20e4
801d2094  lui  v0,0x8008
801d2098  lw   v0,0x46d0(v0)     ; _DAT_800846D0, the confirm mask
801d20a0  and  v0,s2,v0          ; s2 = the packed pad word built at 801d0b20
801d20ac  sb   v0,0x0(s3)        ; ctx+0x06 = 0x5A
```

**Cancel** - the sibling mask `_DAT_800846D4`, which forks on the same count:

```text
801d20ec  lw   v0,0x46d4(v0)     ; _DAT_800846D4, the cancel mask
801d20f4  and  v0,s2,v0
801d210c  lbu  v0,0x8(s1)
801d2114  bne  v0,zero,0x801d21a8 ; typed -> case 0x26, ctx+0x06 untouched
801d219c  li   v0,0x78            ; empty -> the attack-mode prompt
801d21a0  sb   v0,0x0(s3)         ; (0x28 instead when _DAT_800846C4 is set)
```

With a typed buffer the cancel is a **restart, not an exit**: case `0x26` of
`FUN_801D388C` wipes all sixteen queue bytes
(`801d52d4 sb zero,0x1df(v0)` under `801d52d8 sltiu v0,s3,0x10`), re-seeds the
pool from AGL (`801d535c lhu v0,0x154(v0)` -> `801d5364 sh v0,0x6(s6)`) and
zeros the count (`801d536c sb zero,0x8(s4)`). Nothing writes `ctx+0x06`, so the
flow stays in `0x50`.

Note the pad word these masks are tested against. `s2` is built at `801d0b20`
as `_DAT_8007B874 | _DAT_8007B938` - the **packed** layout, whose byte halves
are swapped against the raw BIOS word. So the entry's four direction tests at
`801d1e60`..`801d1f38` (`0x8000 / 0x1000 / 0x4000 / 0x2000`) are Left / Up /
Down / Right, not Square / Triangle / Cross / Circle. Reading them raw turns a
d-pad entry into a face-button one and makes the confirm mask look unreachable.

### Where a saved chain belongs

Retail has no "pick a saved art" list, so a saved chain never commits an art by
itself. Its retail role is to **preseed the entry**: `FUN_801DA34C` copies one
of the character record's two 16-byte arts-input strings (`+0x76F` / `+0x77F`)
into `actor[+0x1DF..]` when the entry opens, the pad then edits those bytes in
place, and `FUN_801DA59C` writes the result back after the action - so a chain
is a *remembered starting buffer*, not a shortcut past the input
([battle-action.md](battle-action.md#the-retail-queue-builder-fun_801eed1c-and-super-applier-fun_801ef9e4)
carries the byte-level walk; ported as
`legaia_engine_vm::battle_action::preseed_action_queue` / `save_action_queue`).

The port opens every entry empty. `World::saved_chains` stays live data - the
chain editor writes it, the save round-trip carries it, and the legacy
`LEGAIA_ARTS_SAVED_LIST=1` list still reads it - but nothing preseeds the input
from it. Wiring that is the open piece; what it needs first is whether the
preseeded presses arrive already paid for or re-debit the pool on the way in,
which no capture pins yet.

Because a turn now performs however many arts the pool paid for, the
**shout cue and the learn-on-use check are per art, not per turn** - see
[audio.md](audio.md#battle-arts-voice-shout-path-engine).

## What an art costs in AP

This is a **different AP** from the command-gauge cost above: the arm/command
width `+0x74` is spent out of the per-turn input budget, while an *art* is paid
for out of the caster's Spirit gauge `actor[+0x170]` - the same gauge the Spirit
command charges (see [`randomizer.md` § spirit AP](../tooling/randomizer.md)).
Conflating the two is the standing trap in this area.

**Retail stores no per-art AP cost.** The party arts queue-builder
`FUN_801EED1C` (PROT 0898, base `0x801CE818`, file `+0x20504`;
`see ghidra/scripts/funcs/overlay_battle_action_801eed1c.txt`) computes it. It
picks a multiplier into `t4` from three code immediates, keyed on how many art
rows it has already visited for this character (`[sp+0x40]`, zeroed at
`0x801EF300` and bumped once per row at `0x801EF844`):

| rows visited | multiplier | site |
|---|---|---|
| `0` | `0xB` (11) | `li t4,0xb` at `0x801EF328` |
| `1..3` | `0xA` (10) | `li t4,0xa` at `0x801EF32C` |
| `>= 4` | `6` | `li t4,0x6` at `0x801EF33C` |

halved by `srl t4,t4,0x1` at `0x801EF378` when the actor's `0x800` flag is set.
The cost is then `t4 x command_count`, twice: `mult t4,s1` / `mflo t7`
(`0x801EF40C`) produces the number the affordability gate compares against
Spirit, and `mult t4,v0` / `mflo a2` (`0x801EF474`) produces the number charged.

### Where the charge actually lands

The site-C debit (`subu v0,v0,a2` at `0x801EF498`) is **not** the spend. It is
undone by the builder's own tail (`Spirit += actor[+0x224]` at `0x801EF988`);
its purpose is to make a *chained* art's affordability gate account for what the
earlier arts in the same run already committed. The real spend is the
accumulator `actor[+0x224]`, subtracted once in the battle-action cleanup arm:

```text
801e5d60  lbu  a0,0x224(s3)     ; accumulated art cost
801e5d6c  sb   v1,0x224(s3)     ; +0x224 = 8 (the per-action accrual)
801e5d74  subu v0,v0,a0         ; gauge -= accumulated art cost   <- the spend
801e5d78  sh   v0,0x170(s3)
```

Anything that changes an art's cost therefore has to move the accumulator, not
just the in-builder debit.

### The menu number is a separate source

The AP the pause menu's arts list shows is the `+2` byte of the static
[arts-name table](../formats/art-data.md#arts-name-table-dat_80075ec4)
(`DAT_80075EC4 + n*0x14`), and **exactly one site in the whole image reads it**:
`lbu a0,0x2(s2)` at `0x801D4524` in the menu overlay's status-panel renderer
`FUN_801D33D8` (PROT 0899), which applies the same `0x800`-flag halving
(`sra a0,a0,0x1`) and hands the value to the 3-cell decimal drawer
`FUN_80034B78`. Retail keeps that byte consistent with the formula by hand: for
all 45 arts it equals `t4(rows visited) x command_count` exactly - including
Noa's, whose display indices skip `2` and `3` while her *visit* order does not,
which is why her index-4 Vulture Blade carries `10 x 5` and not `6 x 5`. So the
battle path and the menu have two independent sources that agree only by
authoring, and a mod that changes one must change the other.

## Arts AP override hook

The randomizer's **arts AP override** (`--arts-ap-grant` / `--arts-ap-cost`)
detours three sites of that flow so a configured art either is admitted at any
AP level and *adds* AP instead of paying, or is gated on and charged a flat cost
of the modder's choosing.

The art identity is register `s3` (the art-row cursor, `li s3,0xb` at
`0x801ef2ec`); the 0-based row is `s3 - 0x0B` (site B below,
`addiu a1,s3,-0xb`), which equals the art's arts-table display index (`0` =
Miracle Art). That row alone is shared across the three characters, so the
character comes from a second register the builder already holds:
`t6 = &DAT_8007BD10[slot]` (built by `addu t6,t9,t7` at `0x801EF30C` and read by
retail itself as `lbu v0,0x0(t6)` at `0x801EF340`), where
[`DAT_8007BD10[slot]`](battle.md) is the 1-based party-record id. The injected
routines replay that load, so the config index is
`(id - 1) * 32 + (s3 - 0x0B)` over a `4 x 32` `i8` table: `0` = retail,
`> 0` = grant that many AP (admit + no cost), `< 0` = charge `-value` AP. One
art per cell - **an override never moves another character's art**.

| Site | VA | Stock word | Role |
|---|---|---|---|
| A affordability guard | `0x801EF410` | `0x94A20170` (`lhu v0,0x170(a1)`) | a grant forces `v0 = 0x7FFF` so `slt v0,v0,t7` reads "affordable" (admit at 0 AP); a cost replaces `t7` with the configured value so the stock compare gates on it |
| B per-art index | `0x801EF438` | `0x2665FFF5` (`addiu a1,s3,-0xb`) | pins the config row `= s3 - 0x0B` (read-only build fingerprint, not detoured) |
| C AP debit + accrual | `0x801EF490` | `0x94620170` (`lhu v0,0x170(v1)`) | a grant *adds* AP (clamped at 100) and returns past the `+0x224` accrual (`0x801EF4A0..0x801EF4B4`) so the refund never double-counts it; a cost debits the gauge and accrues the same override into `+0x224` (the value the cleanup arm charges) and returns past both stock steps; a native art falls through to `subu v0,v0,a2` at `0x801EF498` |
| D end-of-turn refund | `0x801EF988` | `0x94620170` (`lhu v0,0x170(v1)`) | replays `Spirit += +0x224` and clamps it at 100 (retail leaves this unclamped, deferring to the `FUN_801E295C` state-`0x50` cap) |

A configured cost is **flat** - it replaces the product outright, so it does not
follow retail's `srl t4,t4,0x1` halving under the actor's `0x800` flag. The menu
renderer still halves what it *draws* in that state (its own `sra a0,a0,0x1`),
so an odd configured cost reads one lower there.

Alongside the code hook, each targeted art's menu `+2` byte is rewritten to
match: a cost writes the cost, a grant writes `0`. `FUN_80034B78` emits digit
sprites only (`u = digit*8`, `v = 0xD0`) and has no sign path, so `0` - a value
no retail art carries (the retail minimum is 18) and the smallest configurable
cost (`1`) cannot collide with - is the in-game marker for "this art pays you".
A literal `+`/`-` would need an extra sprite draw injected into 0899.

Placement: the battle overlay is packed (no dead space - the move-power window
`0x801F4E63..0x801F69D8` is the only large zero run and is runtime-indexed), so
the detour routines go into the verified-dead SCUS arenas
`shiny_seru::ARENA1_VA` (guard + debit) and `ARENA2_VA` (refund), with the
config table in the rodata gap `SCUS_GAP_VA`, all reached from the 0898 detours
by `j`. Those are the same bytes the
[shiny-Seru](../tooling/randomizer.md#shiny-seru) feature reuses, so **the arts
AP override and `--shiny-seru` are mutually exclusive** - enforced in the CLI and
the web patcher. All four site words plus the `t6` character read are
byte-verified against the extracted 0898 image; an unrecognized build is
refused, not corrupted.
Port: [`legaia_patcher::arts_ap_grant`](../../crates/patcher/src/arts_ap_grant.rs).

## If the Astral Sword is forced onto another character

A common follow-up question is whether the wide command would follow the
sword if a cheat or save edit placed it in Noa's or Gala's weapon slot. The
expected answer is no, for the same reason the cost is data rather than
logic: the value `0x36` exists in exactly one place on the disc, the `0xBA`
section of Vahn's file, and Noa's and Gala's files contain no section for
`0xBA`.

At battle load the section selector (`FUN_80052770` case 4, ported as
`select_sections`) matches the record's equipment byte against the ids in the
corresponding section and, when nothing matches, takes that section's id-0
default entry. Forcing `0xBA` into Noa's weapon byte therefore splices her
default weapon section: default mesh, default swing record, cost `0x1E`. She
swings at the favored price and does not visibly hold the sword, since the
model lives in the same missing section. The 97 attack still applies,
because attack is folded in at execution from the static equipment table,
which is keyed by item id alone. The equipment mask that restricts the sword
to Vahn is enforced only in the menu.

A modification that wants the penalty to travel with the sword must add an
`0xBA` section (or re-price the default record) in the other characters'
files. This follows from the disc layout and the traced selector; it has not
been confirmed with a live capture of an edited save. The
[patcher's equipment editor](../tooling/randomizer.md#equipment-editor-command-costs-and-equip-owners)
reports exactly these fall-through combinations when an owner edit creates
one, and exposes each section default's cost (`CHAR:default=COST` for the
weapon section, `raseru`, `feet` / `feet:up`) - the only in-place knob, since
the player files have no free space for a new section. The same editor
reprices the other three commands through their own sections: the Ra-Seru
arm's record and the footwear section's Down (`+0x04`) and Up (`+0x08`)
records, all `0x1E` in retail.

## Common misconceptions

| Claim | What the bytes say |
|---|---|
| "An off-class weapon doubles the arm command." | Off-class is 30 → 42 (×1.4); the far tier is 30 → 54 (×1.8). The "double" most likely counts penalties: the far penalty (+24) is twice the off-class penalty (+12). |
| "The game checks the weapon's class against the character." | No comparison exists; the value is authored data. The equipment stat table, item property table and accessory tables were each checked and none carries the cost. |
| "The cost is recalculated when equipment changes." | Changing equipment in the field alters only the equipment id bytes in the character record. The cost is read from the disc at the next battle load. |
| "The Astral Sword has a unique penalty." | Its value, 54, is the far-off-class tier Noa receives from any club or axe. |
| "The AP shown next to an art is what the bar charges." | Two different gauges - see [What an art costs in AP](#what-an-art-costs-in-ap). |

## Address reference

Every address on this page in one place (USA release, main-RAM virtual
addresses). The battle overlay (PROT 0898) is based at `0x801CE818`.

### Character records

Four `0x414`-byte records, contiguous, in roster order. These are the
records the save block is composed from and that the battle loader reads
equipment from ([save-record.md](../formats/save-record.md)).

| Character | Record base | Armor `+0x196` | Head `+0x197` | Index 2 `+0x198` | Index 3 `+0x199` | Legs `+0x19A` | Accessories `+0x19B..+0x19D` | Name `+0x2A7` |
|---|---|---|---|---|---|---|---|---|
| Vahn | `0x80084708` | `0x8008489E` | `0x8008489F` | `0x800848A0` weapon | `0x800848A1` Ra-Seru Meta | `0x800848A2` | `0x800848A3..A5` | `0x800849AF` |
| Noa | `0x80084B1C` | `0x80084CB2` | `0x80084CB3` | `0x80084CB4` Ra-Seru Terra | `0x80084CB5` weapon | `0x80084CB6` | `0x80084CB7..B9` | `0x80084DC3` |
| Gala | `0x80084F30` | `0x800850C6` | `0x800850C7` | `0x800850C8` weapon | `0x800850C9` Ra-Seru Ozma | `0x800850CA` | `0x800850CB..CD` | `0x800851D7` |
| Terra (slot 3) | `0x80085344` | same layout; the tail overlaps the story-flag bitmap at `0x80085600` | | | | | | |

Index 2 and index 3 hold the weapon and the Ra-Seru in the order the
character's player battle file expects ([which hand is priced](#reading-it)).
The cheat-database labels "weapon = `+0x198`" are Vahn's and Gala's layout.

### Battle globals and the runtime command record

| Address | Type | Meaning |
|---|---|---|
| `0x8007BD24` | u32 | Pointer to the battle context struct (`0x800EB654` in captured battles; `0` in the field) |
| `0x8007BD10` | u8[] | Per-seat character id, 1-based (1 Vahn, 2 Noa, 3 Gala) |
| `0x801C9370` | u32[8] | Battle-actor pointer table (party seats 0..2, monsters 3..) |
| `0x801C9360` | u32[3] | Per-party-member pointer to that member's command-record pointer array |
| `DAT_801C9360[char][cmd]` | u32 | Pointer to the runtime record of direction command `cmd` (`0x0C..0x0F`), indexed `cmd * 4` |
| `record + 0x74` | u8 | The AP cost / pennant width byte |
| `0x801F4B8C` | u8[4] | Command codes displayed on the bar: `0C 0F 0E 0D` |
| `0x801F4B94` | u8[4] | Icon base per command: `0D 10 11 0C`; `+2` when the equipment slot is empty |
| `0x800846D0` / `0x800846D4` | u16 | Configurable confirm / cancel pad masks the input state tests |

### Battle context fields (`ctx = *0x8007BD24`)

| Offset | Type | Meaning |
|---|---|---|
| `+0x06` | u8 | Command-menu flow byte (`0x50` Arts input, `0x5A` review, `0x6E` Begin / Reselect, `0x78` attack-mode prompt) |
| `+0x13` | u8 | Active actor slot; indexes `0x801C9370` |
| `+0x14..+0x17` | u8[4] | Per-command AP cost for the current input, copied from `record + 0x74` |
| `+0x19` | u8 | Number of commands committed in the current input |
| `+0x6DC` | u16 | The turn pool: seeded from the actor's AGL, debited per press |

### Battle actor fields (`actor = 0x801C9370[slot]`)

| Offset | Type | Meaning |
|---|---|---|
| `+0x154` / `+0x156` | u16 | AGL, current / base; the turn pool is seeded from `+0x154` |
| `+0x158` | u16 | Working ATK; the execution resolver folds half of one equipment slot's attack bonus in here per command (footwear for Up / Down, slot 2 / 3 for the arms, all five for an art) |
| `+0x16E` | u16 | Status word; bits `0x08` / `0x10` / `0x20` disable Left / Right / Up+Down |
| `+0x170` | u16 | Spirit gauge; where a named art's cost is charged (not the bar) |
| `+0x1D9` | u8 | Last staged action id (`< 0x10` = plain direction) |
| `+0x1DF..+0x1EE` | u8[16] | The committed command buffer the presses append to |
| `+0x1F4` | u8 | Execution cursor into the command buffer |
| `+0x224` | u8 | Accumulated art cost, subtracted from Spirit once in the cleanup arm |

### Functions

| Function | Image | Role |
|---|---|---|
| `FUN_801D388C` | battle overlay (0898) | Gauge build (case 9 / `0x2C`) reads `+0x74`, cost store at `0x801D3B3C`; press (case `0xB`) debits `ctx+0x6DC`; pool seeds at `0x801D3A30`, `0x801D4E18`, `0x801D5068`, `0x801D5364`; typed-buffer cancel (case `0x26`) at `0x801D52D4..0x801D536C` |
| `FUN_801D0748` | battle overlay (0898) | Command-menu state machine; state `0x50` is the Arts input (direction tests `0x801D1E60..0x801D1F38`, auto-end `0x801D2054..0x801D208C`, confirm `0x801D207C..0x801D20AC`, cancel `0x801D20EC..0x801D21A0`); pad word built at `0x801D0B20` |
| `FUN_800557B8` | SCUS | Swing-record copy at battle load; the single write to the cost field is at `0x80055810` |
| `FUN_80052770` | SCUS | Battle character assembly; case 4 selects equipment sections from the record's `+0x196..` bytes; calls into the copy chain at `0x80053330` |
| `FUN_80052FA0` | SCUS | Swing-splice half of the assembly: installs section 2/3/4 swing records into runtime slots `0x0C..0x0F` |
| `FUN_8001A55C` | SCUS | LZS decoder; fills the character buffer the copy above reads from |
| `FUN_801EC3E4` | battle overlay (0898) | Arms execution resolver; called from SCUS `0x800478A0`; folds equipment ATK into `actor+0x158` per command via jump table `PTR_801CF4B4` |
| `FUN_801EED1C` | battle overlay (0898) | Party arts queue builder; computes a named art's Spirit cost (`li t4` at `0x801EF328 / 0x801EF32C / 0x801EF33C`, halving at `0x801EF378`) |
| `FUN_801E9FD4` | battle overlay (0898) | Enemy action-queue filler; spends move `+0x74` costs from the monster's AGL |
| `FUN_801E93C8` | battle overlay (0898) | Gauge re-arm after an action completes (called from `0x801E5F64`) |
| `FUN_801D33D8` | menu overlay (0899) | Status-panel renderer; the one reader of the menu's per-art AP byte (`lbu a0,0x2(s2)` at `0x801D4524`) |

### Static tables in `SCUS_942.54`

| Address | Stride | Contents |
|---|---|---|
| `0x80074368` | 12 | Item property records; `+0` name pointer, `+1` equipment-row index, `+8` description pointer (shared per weapon class; the Astral Sword's is `0x80011710`) |
| `0x80074F68` | 8 | Equipment stat-bonus rows; `+1` attack bonus, `+6` equip-character mask, `+7` slot type |
| `0x80075EC4` | `0x14` | Arts-name table; `+2` is the menu's displayed art AP |
| `0x80084140` | `0x414` | Live game-state window the save block is composed from; `+0x5C8` = the first character record |

### Disc

| Location | Contents |
|---|---|
| PROT entry 863 / 864 / 865 | Player battle files for Vahn / Noa / Gala (extraction-index numbering) |
| weapon section `[+0x04] + 0x74` | The authored cost byte, inside the section's LZS stream |
| PROT entry 898 / 899 | Battle / menu overlay images; the cost store is at battle-overlay file offset `0x801D3B3C - 0x801CE818 = 0x5324` |

## See also

- [Art Data - Tactical Arts records](../formats/art-data.md) - the per-character art records and command-glyph strings.
- [Battle action state machine](battle-action.md) - `FUN_801E295C`, the layer that runs a committed action.
- [Battle-data pack](../formats/battle-data-pack.md) - the player battle files the per-command structs live in.
- [Equipment stat-bonus table](../formats/equipment-table.md) - the equip-character mask that locks character-specific weapons.
- [Move power table](../formats/move-power.md) - the per-move power/behaviour record used during execution.
