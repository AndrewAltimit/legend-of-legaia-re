# Arts command gauge — weapon-specialty arm width

When a character's turn opens the Arts command input, the battle UI draws an **action gauge**: a fixed pool of AP (Action Points) the player spends by inputting directional commands (High / Low / and the two **arm** swings). Each command consumes a per-command AP cost. The cost of the **arm** command is **not constant** — it depends on the class of the equipped weapon relative to the character's favored class. Equip a weapon outside your class and the arm command costs more AP, so fewer commands fit in the gauge; the Astral Sword costs the most of all. This is the engine side of the "weapon specialty" mechanic.

The popular description is "an off-class weapon **doubles** the arm command." The byte-level behaviour is a base cost plus an **escalating class penalty**, not a flat ×2 — see [the measured values](#measured-arm-cost).

## Contents

- [Where the cost lives](#where-the-cost-lives)
- [Measured arm cost](#measured-arm-cost)
- [How the gauge consumes it](#how-the-gauge-consumes-it)
- [Weapon classes and favored mapping](#weapon-classes-and-favored-mapping)
- [Execution path](#execution-path)
- [Confidence and open threads](#confidence-and-open-threads)
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

The other commands (`0x0D`/`0x0E`/`0x0F`) stay at `0x1E` (30) in every case. So the model is a **base `0x1E` plus a class penalty**: `+0x0C` for an off-class weapon, `+0x18` for the Astral Sword. The Astral penalty is twice the off-class penalty, which is where the "double" shorthand comes from — but the off-class case itself is `+0x0C` over base, i.e. ×1.4, not ×2.

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

Because a higher `+0x74` widens the gauge slot (`bVar3 - 6`) **and** drains more of the AP pool, an off-class arm both *looks* wider and lets fewer total commands fit — the visible "longer arm input."

> A separate `+2` in the same case (`icon = DAT_801F4B94[i] + 2`, gated on an *empty* equip slot, `equip[cmd] == 0`) is an empty-slot icon tweak, **not** the class penalty — a fully-equipped off-class character still shows the widened arm via the `+0x74` cost above.

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

Once a combo is committed, it is replayed by the **Arms execution resolver `FUN_801EC3E4`** (overlay `0898`), which is **called from `SCUS_942.54` at `0x800478A0`** (`jal 0x801EC3E4`) — the arts execution driver is the static side, which is why the resolver has no caller inside the overlay. The resolver advances the input cursor (`actor + 0x1F4`) one step per recorded command and dispatches per-command sub-handlers through the jump table `PTR_801CF4B4[(actor + 0x1D9) - 0xC]`. These sub-handlers read the equipped weapon again (e.g. `0x801ECC00`: weapon id → item subtype `DAT_80074369` → equip record `DAT_80074F68`) to fold the weapon into the damage / effect calculation. This execution-time weapon read is **distinct** from the gauge-build cost above.

## Confidence and open threads

**Confirmed** (live-pinned by reading the field across the off-class / favored / Astral states, plus the decompiled gauge builder): the cost field `DAT_801C9360[char][0x0C] + 0x74`, its measured values, the case-`9` read and case-`0xB` AP spend in `FUN_801D388C`, and the SCUS call site of the execution resolver.

**Inferred**: the identification of command `0x0C` as "the arm" (it is the only command whose cost tracks the weapon); the penalty structure (`base + 0x0C` off-class / `+ 0x18` Astral) from three data points.

**Open**: where `+0x74` is **written** — the code that adds the off-class penalty by comparing the equipped weapon's class to the character's favored class. The field is recomputed from the equipped weapon when arts input initializes (it differs for the same character across weapons), so a write-watch on the now-known address across an arts-menu (re)entry pins the comparison and the favored-class source. It is also unresolved whether the base `0x74` value is disc-resident in the player battle files ([863..866](../formats/battle-data-pack.md)) with the penalty added at runtime, or computed wholesale. Locating the writer is the prerequisite for a faithful weapon-specialty randomizer.

## See also

- [Art Data — Tactical Arts records](../formats/art-data.md) — the per-character art records and command-glyph strings.
- [Battle action state machine](battle-action.md) — `FUN_801E295C`, the layer that runs a committed action.
- [Battle-data pack](../formats/battle-data-pack.md) — the player battle files the per-command structs live in.
- [Equipment stat-bonus table](../formats/equipment-table.md) — the equip-character mask that locks character-specific weapons.
- [Move power table](../formats/move-power.md) — the per-move power/behaviour record used during execution.
