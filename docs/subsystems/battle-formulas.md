# Battle formulas

Damage, MP-cost, and stat-cap math used by the [battle action state machine](battle-action.md). Lives in the battle overlay (`0898`); the central damage-application primitive is `FUN_800402F4`.

## Damage application primitive — `FUN_800402F4`

`ghidra/scripts/funcs/800402f4.txt` (7,904 bytes / 1,976 instructions, no static caller in `SCUS_942.54` — the battle overlay calls it indirectly).

Signature, after Ghidra type promotion:

```c
void FUN_800402F4(byte selector, byte sub_index, byte target_slot, uint flags);
```

The function is a **selector dispatch**: `switch(selector) { case 0..0x83 ... }`. Each case is one "damage / status / stat-modify kind." The `target_slot` is an index into the [8-slot battle actor pointer table](battle.md) at `0x801C9370`.

### Local stat-window setup (`selector` agnostic)

Before the switch, the function fills four local 8-pointer arrays (one entry per actor slot). Each array holds a pointer to one specific halfword inside the actor record:

| Local | Actor offset | Meaning |
|---|---|---|
| `local_90[i]` | `+0x14C` | Current HP |
| `local_30[i]` | `+0x14E` | (paired with HP — likely a saved/working copy) |
| `local_50[i]` | `+0x150` | Current MP |
| `local_70[i]` | `+0x152` | (paired with MP) |

In the **debug** branch (when the condition at `0x80040314` selects), the loop iterates 7 times over the 8-slot pointer table; in the **release** branch it iterates the 3-slot party-only path using the character-record stride `0x414` from base `0x80084708`.

Cited in `ghidra/scripts/funcs/800402f4.txt` lines 22-87 (release setup), 1986-2034 (decomp).

### Selector 0 — basic damage (Attack / item / generic spell)

Lines 2037-2043:

```c
iVar4 = (uint)*(ushort *)local_90[target_slot]   // attacker's HP slot? see note
       - (uint)*(ushort *)local_b0[target_slot]; // defender's HP slot
uVar13 = (ushort)iVar4;
if (sub_index < 3) {                              // party-side cap
    if ((int)(uint)*(ushort *)(&DAT_8007655C + sub_index * 2) < iVar4 * 0x10000 >> 0x10) {
        uVar13 = *(ushort *)(&DAT_8007655C + sub_index * 2);
    }
}
```

Reads:

1. The base hit value is the difference between two stat slots whose pointer-table indices come from the per-actor stat layout (the function aliases `local_90` and `local_b0` over the same 0x14C..+0x162 range with different starting offsets).
2. Result is capped at `DAT_8007655C[sub_index]` for party slots 0..2. The cap table is 6 halfwords (twelve bytes) and represents per-character damage caps.
3. The capped value is then handed to a downstream applicator (the case body keeps writing it back into the actor record at `+0x14C`).

### Selector 9 — accuracy / evasion roll

Lines 2730-2774. The pattern:

```c
iVar4 = FUN_80056798();   // RNG (PsyQ-shape rand, see audio.md)
roll = iVar4 % (caster_accuracy_at_+0x168 + target_evasion_at_+0x168);
if (target_evasion < roll) {
    target.status |= 4;            // mark hit
    if (target.action_category == 1 && target.cooldown != 0) {
        FUN_800421d4(item_id, 1);  // consume queued item
    }
    target.action_category = 0;    // cancel queued action
}
```

`+0x168` is the **accuracy/evasion** halfword in the actor record (one stat field shared by both rolls — caster's at attacker actor, target's at defender actor). The roll is `rand % (caster + target)` so the **hit probability** is `caster / (caster + target)`. Standard JRPG-flat-roll model.

### Stat-buff selectors (1..7)

These cases multiply `actor_record + 0x158..+0x16A` by `6/5` (decompiles to the constant `0x4cccccccd >> 0x22` shift pattern) per visible step, with a clamp to `0xFFFF`. They are the +20% stat-up animations for buff spells.

Hit one halfword per call:

| Offset | Semantic (inferred from buff order) |
|---|---|
| `+0x158` | ATK |
| `+0x15A` | DEF |
| `+0x15C` | (one of MAG / SPR / AGL — order not yet pinned) |
| `+0x15E` | |
| `+0x160` | |
| `+0x162` | |
| `+0x168` | accuracy/evasion (see selector 9) |
| `+0x16A` | (paired with accuracy) |

The stat semantics need a runtime trace to disambiguate — capturing actor record bytes before/after a known buff (e.g. Power Up) would resolve the pairing.

## Spirit damage formula

From [battle-action.md state `0x3E` and `0x46`](battle-action.md):

```text
damage = ((target_HP * 7) / 5) + 8;     // 1.4 × target HP + 8
damage = min(damage, 0x120);            // cap 1: 288 hit-points
// or cap 2 (smaller spirit arts): min(damage, 100);
```

This is hard-coded per Spirit super-art and bypasses `FUN_800402F4`. The `_DAT_80076D7E` damage popup is written directly with the result before the state machine calls `func_0x800402F4` in state `0x3F`. The spirit pre-application formula is the one place the engine has to reproduce a non-obvious arithmetic; everything else is selector-dispatch driven.

## MP cost & ability-bit modifiers

From battle-action.md state `0x28` (Magic / Item — cast begin):

```text
base_mp_cost = spell_table[spell_id].mp_cost;       // entry +3 from spell record
if (character_record.ability_bits & 0x20) {         // "MP-half"
    mp_cost = base_mp_cost / 2;
} else if (character_record.ability_bits & 0x10) {  // "MP-quarter"
    mp_cost = base_mp_cost / 4;
} else {
    mp_cost = base_mp_cost;
}
actor.mp -= mp_cost;
```

`character_record.ability_bits` is the 4-byte field at `+0xF4` of the per-character record (record stride `0x414`, base `0x80084708`). See [battle.md](battle.md#character-record-stride).

The character record is documented to have stat fields at `+0x100..+0x110` and an ability-flag bitfield with at least 16 distinct bits in use (the 0x10 / 0x20 / 0x100 / 0x200 quarter / half / HP-cap / MP-cap split is confirmed; the rest of the bit assignments need a spreadsheet of "which character has which natural ability flag set" which is straightforward but hasn't been compiled).

## RNG primitive

`FUN_80056798()` is the in-game RNG. It's the standard PsyQ `rand()` pattern (same shape as the libc `rand()` PsyQ provides — 32-bit LCG with multiplier `1103515245` and increment `12345`, return value `(seed >> 16) & 0x7FFF`). The cliff-notes:

```c
int FUN_80056798(void) {
    DAT_8007AE5C = DAT_8007AE5C * 1103515245 + 12345;
    return (DAT_8007AE5C >> 16) & 0x7FFF;
}
```

(Confirmed: see `ghidra/scripts/funcs/80056798.txt`.) Range: 0..32767. For damage variance the battle code typically uses `roll % (cap)` so distribution skew at small cap values is fine.

The seed (`DAT_8007AE5C`) initialises from the boot timer — for deterministic playback the engine must seed it from the same source, otherwise replay tests will diverge.

## Engine-side mirror — `engine-vm::battle_formulas`

The clean-room Rust module `crates/engine-vm/src/battle_formulas.rs` ports the formulas above as pure functions. It's deliberately *not* trying to reproduce `FUN_800402F4`'s entire selector-dispatch — that lives in `engine-vm::battle_action` next to the state machine.

| Function | Provenance |
|---|---|
| `spirit_damage` | battle-action.md state 0x3E / 0x46 |
| `mp_cost_after_ability_bits` | battle-action.md state 0x28 |
| `accuracy_roll` | this doc, selector 9 above |
| `psyq_rand_step` | `ghidra/scripts/funcs/80056798.txt` |
| `damage_cap_for_party_slot` | this doc, selector 0 above (`DAT_8007655C` table) |

The unit tests there pin the documented formulas as fixtures — a future runtime trace can then add comparison cases without touching the formula bodies.

## What's still open

- **Selector dispatch for selectors `0x10..=0x83`.** The cases beyond status / buff / damage handle stat-up animations, status-clear, queue-end markers, and the multi-target item slot used by Smelly Glove etc. They're mostly read-only stat ramps that don't affect game balance, so leaving them un-decoded is fine for a first port.
- **Stat-field semantics inside the actor record at `+0x158..+0x16A`.** The buff order in selectors 1..7 implies an ordering, but mapping each halfword to ATK / DEF / MAG / SPR / AGL / LUCK requires capturing actor records before / after each buff spell. Mednafen save state diff is the unblock.
- **Ability-bit catalogue.** The ability bitfield at `+0xF4` of the character record has at least the documented MP-half / MP-quarter / HP-cap / MP-cap bits in use, plus the impact-step modifier (`0x10` / `0x20`) on attack actions. The full per-character mapping comes out of save-data (the 0x414 record's `+0xF4..+0xF8` is one row in the save schema's character block) — a few new-game saves with different early-game characters resolve it.
