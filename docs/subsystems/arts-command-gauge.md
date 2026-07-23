# Arts command gauge - weapon-specialty arm width

When a character's turn opens the Arts command input, the battle UI draws an **action gauge**: a fixed pool of AP (Action Points) the player spends by inputting directional commands (High / Low / and the two **arm** swings). Each command consumes a per-command AP cost. The cost of the **arm** command is **not constant** - it depends on the class of the equipped weapon relative to the character's favored class. Equip a weapon outside your class and the arm command costs more AP, so fewer commands fit in the gauge; the Astral Sword costs the most of all. This is the engine side of the "weapon specialty" mechanic.

The popular description is "an off-class weapon **doubles** the arm command." The byte-level behaviour is a base cost plus an **escalating class penalty**, not a flat ×2 - see [the measured values](#measured-arm-cost).

## Contents

- [Where the cost lives](#where-the-cost-lives)
- [Measured arm cost](#measured-arm-cost)
- [How the gauge consumes it](#how-the-gauge-consumes-it)
- [Status limb gating](#status-limb-gating)
- [Weapon classes and favored mapping](#weapon-classes-and-favored-mapping)
- [Execution path](#execution-path)
- [Who writes the cost](#who-writes-the-cost)
- [Disc location](#disc-location)
- [Confidence and open threads](#confidence-and-open-threads)
- [Arts AP-grant hook](#arts-ap-grant-hook)
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

The same case-`9`/`0xB` machinery also deals and spends the **Muscle Dome hand**: a dome card is one of the four direction commands (`0xC..=0xF`, the deck table `DAT_801f4b8c`), its cost is this same `+0x74` byte, and the commit debits the same `ctx+0x6DC` pool - see [`minigame-muscle-dome.md`](minigame-muscle-dome.md#hand-deck-decoded).

The **enemy analogue** is the AGL action-budget in `FUN_801E9FD4`: a monster fills its per-turn action queue by rolling candidate moves and paying each move's `+0x74` cost out of the per-round AGL gauge (`actor[+0x154]`), the same "wider cost = fewer commands" mechanic on the AI side - see [`battle-action.md` § Enemy AGL action-budget](battle-action.md#enemy-agl-action-budget-fun_801e9fd4).

> A separate `+2` in the same case (`icon = DAT_801F4B94[i] + 2`, gated on an *empty* equip slot, `equip[cmd] == 0`) is an empty-slot icon tweak, **not** the class penalty - a fully-equipped off-class character still shows the widened arm via the `+0x74` cost above.

## Status limb gating

A **Rot** (or similar limb-disable) status grays individual command arrows and
refuses their input. The gauge-input arm `FUN_801D0748` state `0x50`
(`overlay_battle_action_801d0748.txt:3311-3360`) reads the active actor's
`+0x16E` status halfword; the gray-draw pass and the input gate agree
bit-for-bit:

| `+0x16E` bit | Arrow grayed (draw pos) | Blocks command |
|---|---|---|
| `0x08` (limb 0) | LEFT (`0xb3 - w/2, 0x42`) | Square `0x8000` / dir 0 |
| `0x10` (limb 1) | RIGHT (`0xe5 + w/2, 0x42`) | Circle `0x2000` / dir 3 |
| `0x20` (limb 2) | UP (`0xcc, 0x22`) **and** DOWN (`0xcc, 0x62`) | Triangle `0x1000` / dir 1 **and** Cross `0x4000` / dir 2 |
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

Ports: `legaia_engine_vm::battle_formulas::arms_command_equip_slots` / `arms_weapon_atk_fold` / `arms_resolver_admits`.

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

Cross-checked against live RAM: Gala + Nail Glove reads `0x2A`, Gala + Ra-Seru Club reads `0x1E` - matching that file's `0x28` and `0x21` sections. The cost lives inside the section's **LZS-compressed** stream, so an editor decompresses the section, rewrites the byte at `swing_rec_a + 0x74`, recompresses, and writes back within the slot footprint.

## Confidence and open threads

**Confirmed** (live-pinned + byte-validated against the disc): the cost field `DAT_801C9360[char][0x0C] + 0x74`, its measured values, the case-`9` read and case-`0xB` AP spend in `FUN_801D388C`, the SCUS call site of the execution resolver, the **writer** (`FUN_800557B8`, verbatim copy from the LZS-decoded equipment section at battle load - no runtime penalty arithmetic), and the **disc location** of the cost byte (`section[+0x04]` swing record `+0x74` in the player battle files, tabulated above).

**Inferred**: the identification of command `0x0C` as "the arm" (it is the only command whose cost tracks the weapon).

The weapon-specialty mechanic is therefore a fully editable data table: rewrite a character's favored-class arm costs up / another class's down to reassign their specialty. The [randomizer](../tooling/randomizer.md)'s `--weapon-specialty` does exactly this - it permutes the three favored families among the characters by rewriting these bytes (decompressing / re-compressing each touched section in place).

## Arts AP-grant hook

The gauge cost above is spent inside the **party arts queue-builder**
`FUN_801EED1C` (PROT 0898, base `0x801CE818`, file `+0x20504`;
`see ghidra/scripts/funcs/overlay_battle_action_801eed1c.txt`), which runs only
for a party slot (`< 3`; monster AI uses `FUN_801E7320`). When a committed
directional run fully matches one of the character's art records, the builder
gates it on the caster's Spirit/AP (`actor[+0x170]`), debits the cost, and
accrues it into the spent accumulator (`actor[+0x224]`) that the same function
refunds at end of turn. The randomizer's **arts AP-grant** hook
(`--arts-ap-grant`) detours three sites of that flow so a configured art is
admitted at any AP level and *adds* AP instead of paying.

The art identity is register `s3` (the art-row cursor, `li s3,0xb` at
`0x801ef2ec`); the 0-based row is `s3 - 0x0B` (site B below,
`addiu a1,s3,-0xb`), which equals the art's arts-table display index (`0` =
Miracle Art). A 26-entry `i8` config table indexed by that row (art rows
`0x0B..=0x24`) drives the grant: `0` = retail, `> 0` = grant that many AP,
admit + no cost. The row is a **shared** index across the three characters (the
table is indexed by the row cursor, not by character), so a grant applies to
every character's art at that row.

| Site | VA | Stock word | Role |
|---|---|---|---|
| A affordability guard | `0x801EF410` | `0x94A20170` (`lhu v0,0x170(a1)`) | bypassed for a grant art so `slt v0,v0,t7` reads "affordable" (admit at 0 AP) |
| B per-art index | `0x801EF438` | `0x2665FFF5` (`addiu a1,s3,-0xb`) | pins the config index `= s3 - 0x0B` (read-only build fingerprint, not detoured) |
| C AP debit + accrual | `0x801EF490` | `0x94620170` (`lhu v0,0x170(v1)`) | a grant art *adds* AP (clamped at 100), stores, and returns past the `+0x224` accrual (`0x801EF4A0..0x801EF4B4`) so the refund never double-counts it; a native art falls through to the stock `subu v0,v0,a2` at `0x801EF498` |
| D end-of-turn refund | `0x801EF988` | `0x94620170` (`lhu v0,0x170(v1)`) | replays `Spirit += +0x224` and clamps it at 100 (retail leaves this unclamped, deferring to the `FUN_801E295C` state-`0x50` cap) |

Placement: the battle overlay is packed (no dead space - the move-power window
`0x801F4E63..0x801F69D8` is the only large zero run and is runtime-indexed), so
the three detour routines + the config table are injected into a verified-dead
SCUS arena (`shiny_seru::ARENA1_VA = 0x8007AE00..0x8007AF00`), reached from the
0898 detours by `j`. Those bytes are the same ones the [shiny-Seru](../tooling/randomizer.md#shiny-seru)
feature reuses, so **arts AP-grant and `--shiny-seru` are mutually exclusive** -
enforced in the CLI and the web patcher. All four site words are byte-verified
against the extracted 0898 image; an unrecognized build is refused, not
corrupted. Port: [`legaia_patcher::arts_ap_grant`](../../crates/patcher/src/arts_ap_grant.rs).

## See also

- [Art Data - Tactical Arts records](../formats/art-data.md) - the per-character art records and command-glyph strings.
- [Battle action state machine](battle-action.md) - `FUN_801E295C`, the layer that runs a committed action.
- [Battle-data pack](../formats/battle-data-pack.md) - the player battle files the per-command structs live in.
- [Equipment stat-bonus table](../formats/equipment-table.md) - the equip-character mask that locks character-specific weapons.
- [Move power table](../formats/move-power.md) - the per-move power/behaviour record used during execution.
