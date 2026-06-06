# Battle formulas

Damage, MP-cost, and stat-cap math used by the [battle action state machine](battle-action.md). Lives in the battle overlay (`0898`); the central damage-application primitive is `FUN_800402F4`.

## Damage application primitive - `FUN_800402F4`

`ghidra/scripts/funcs/800402f4.txt` (7,904 bytes / 1,976 instructions, no static caller in `SCUS_942.54` - the battle overlay calls it indirectly).

Signature, after Ghidra type promotion:

```c
void FUN_800402F4(byte selector, byte sub_index, byte target_slot, uint flags);
```

The function is a **selector dispatch**: `switch(selector) { case 0..0x83 ... }`. Each case is one "damage / status / stat-modify kind." The `target_slot` is an index into the [8-slot battle actor pointer table](battle.md) at `0x801C9370`.

### Local stat-window setup (`selector` agnostic)

Before the switch, the function fills four local 8-pointer arrays (one entry per actor slot). Each array holds a pointer to one specific halfword inside the actor record:

| Local | Actor offset | Meaning |
|---|---|---|
| `local_b0[i]` | `+0x14C` | Current HP |
| `local_90[i]` | `+0x14E` | HP working/base (paired with current) |
| `local_70[i]` | `+0x150` | Current MP |
| `local_50[i]` | `+0x152` | MP working/base (paired) |

(Release setup at lines 2014-2017; the debug branch at 2027-2030 aliases the same windows over the per-character record at `0x80084708 + slot*0x414`, offsets `+0x104..+0x10A`.)

In the **debug** branch (when the condition at `0x80040314` selects), the loop iterates 7 times over the 8-slot pointer table; in the **release** branch it iterates the 3-slot party-only path using the character-record stride `0x414` from base `0x80084708`.

Cited in `ghidra/scripts/funcs/800402f4.txt` lines 22-87 (release setup), 1986-2034 (decomp).

## Actor stat block + monster record mapping

The per-actor stat block runs `+0x14C..+0x16A`, each stat stored as a **pair** of adjacent halfwords (the lower offset is the working value the formulas read; `+2` is the base used to restore after a buff wears off). For enemies, `FUN_80054CB0` (`ghidra/scripts/funcs/80054cb0.txt`, lines 629-699) copies the [monster stat record](battle.md) field-by-field into this block:

| Record offset | Actor pair | Stat | How named |
|---|---|---|---|
| `+0x0C` | `+0x14C/+0x14E` (+`+0x172`) | HP | direct |
| `+0x10` | `+0x150/+0x152` (+`+0x174`) | MP | direct |
| `+0x0E` | `+0x154/+0x156` | **SP** | spirit/action gauge - AI spell-selection budget; spirit-charge source |
| `+0x12` | `+0x158/+0x15A` | **ATK** | attacker's offense in the damage routine |
| `+0x14` | `+0x15C/+0x15E` | **DEF↑** (high/upper) | defender defense, branch A |
| `+0x16` | `+0x160/+0x162` | **DEF↓** (low/lower) | defender defense, branch B |
| `+0x18` | `+0x168/+0x16A` | **AGL** | accuracy/evasion seed (selector 9) |
| `+0x1A` | `+0x164/+0x166` | **SPD** | turn-order initiative seed |

`stat4` (`+0x18`) is also rescaled into the accuracy/evasion pair at copy time (`+0x168/+0x16A = stat4 + stat4/4` or `+ stat4/8` under the `_DAT_8007BD24+0x287` difficulty flag).

**SPD** (`+0x164`): `overlay_0897_801e23ec` seeds each actor's per-turn initiative key from it: `+0x16C = speed + (rand() % (speed/2 + 1)) + 1`. It has a dedicated "Speed Up" buff (selector 7 sub 1) and is reset to its base each round (`FUN_80053CB8`: `+0x164 = +0x166`). Distinct from AGL, which governs the hit/dodge roll rather than turn order. The next-actor selector `recompute_battle_order` (`FUN_801daba4`) reads the seeded `+0x16C` keys: it picks the living actor with the highest key (random tiebreak via `rand % tie_count`), zeroing dead actors' keys first. Ported as `World::next_combatant_by_initiative`; see [turn order in battle.md](battle.md#auto-resolve-vs-player-driven).

**SP** (`+0x154` current / `+0x156` base): a per-round spirit/action gauge. The enemy-AI spell picker (`overlay_0898_801e9fd4`) spends it - it deducts each candidate spell's cost byte (`spell_entry +0x74`) from `+0x154` and only queues spells it can still afford. Each round `FUN_801D88CC` resets `+0x154` to its base (`+0x156`), or, when the actor is spirit-charged (`+0x1DE == 4`), sets it to `(base*7/5)+8` capped at `0x120` (the same shape as the [spirit-damage formula](#spirit-damage-formula)). The damage popup (`_DAT_80076D7E`) reads `+0x154`. This corroborates the HP/MP/SP-triplet reading of `+0x14C..+0x156` in [battle.md](battle.md).

### Spell list (`record +0x4C`)

`record +0x4A` (u8) is the spell count; `record +0x4C` is an array of that many u32 **block-relative offsets**, each pointing at a spell entry inside the same decoded monster block. The battle loader `FUN_800542C8` (`ghidra/scripts/funcs/800542c8.txt`, lines 633-658) fixes every offset to an absolute pointer at battle init - `record[+0x4C + i*4] += block_base` - exactly like `name_offset`; it also fixes each entry's `+0x04`/`+0x08` sub-pointers and initialises a `+0x88` self-pointer to `entry+0x8C`.

Each spell entry's head:

- **`+0x00` (u8) — spell/action id**, which doubles as a category selector. `FUN_80054CB0` (lines 700-727) treats ids `2,3,4,5,0x0B` as **elemental resist/affinity** markers and writes the matching spell's index into actor `+0x1EF..+0x1F3`. The AI picker treats ids `0x0C..=0x1F` as **offensive castable spells** (`*entry - 0xC < 0x14`) and `0x23` (`'#'`) as a special category.
- **`+0x74` (u8) — SP cost**. The picker only rolls a spell when `cost != 0xFF` and current SP (`+0x154`) `>= cost`, then subtracts it (lines 2219-2252 of `overlay_0898_801e9fd4`).

Real-data sanity check: Gimard (id 10, SP 60) has 9 slots - the affinity prefix `0,1,2,4,5,0x0B` (cost 0), two castable spells `0x0D @ 28` and `0x0F @ 32` (both `<= 60`), and the `0x23` special. Hornet (id 61, SP 88) has `0x0C @ 88` and `0x13 @ 88`. Across every populated record the decoded list length equals the declared count and no offset escapes the block. The `legaia_asset::monster_archive::MonsterRecord::spells` field (`MonsterSpell { id, sp_cost, offset }`, with `is_castable()`) exposes this; the [enemy table](../../site/_content/monsters.html) renders the castable set with SP cost.

### Physical attack damage - `overlay_battle_action_801ec3e4`

Lines 2716-2826. The raw hit value is built from the **attacker's ATK** (`actor[+0x158]`) and reduced by the **defender's defense** - the routine reads `actor[+0x15C]` (DEF↑) when the attack's move index satisfies `(move - 0xC) % 10 < 5`, else `actor[+0x160]` (DEF↓):

```c
atk = attacker[+0x158];                                  // stat1 = ATK
def = ((move - 0xC) % 10 < 5) ? target[+0x15C]           // DEF↑ (stat2)
                              : target[+0x160];           // DEF↓ (stat3)
raw   = (atk + rand() % (atk/8 + 1)) * armor_factor>>4 + … ;
guard = def + rand() % (def/8 + 1) + … ;
// damage applied when raw exceeds guard, scaled by the difference
```

This is the binding that names ATK / DEF↑ / DEF↓; the `legaia_asset::monster_archive` accessors (`attack()` / `defense_high()` / `defense_low()`) and `engine-core`'s `monster_def_from_record` follow it.

### Selector 0 - basic damage (Attack / item / generic spell)

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

1. This is the HP **applicator**, not the attack-vs-defense calc: the base value is `actor[+0x14E] - actor[+0x14C]` (HP working minus current) *within the target actor* - i.e. the pending HP delta. The atk/def hit value is computed earlier in `overlay_battle_action_801ec3e4` (see above) and staged into the actor before this selector applies it.
2. Result is capped at `DAT_8007655C[sub_index]` for party slots 0..2. The cap table is 6 halfwords (twelve bytes) and represents per-character damage caps.
3. The capped value is then handed to a downstream applicator (the case body keeps writing it back into the actor record at `+0x14C`).

### Selector 9 - accuracy / evasion roll

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

`+0x168` is the **accuracy/evasion** halfword in the actor record (one stat field shared by both rolls - caster's at attacker actor, target's at defender actor). The roll is `rand % (caster + target)` so the **hit probability** is `caster / (caster + target)`. Standard JRPG-flat-roll model.

**Engine wiring.** `battle_formulas::accuracy_roll` ports this roll, and the live battle loop applies it per strike in both the arts/spell strike resolver and the basic-attack path (`engine-core::world::battle`). Each actor's `+0x168` value lives in the World-side `battle_accuracy` / `battle_evasion` arrays: party slots are seeded from each character's AGL-derived `acc`/`eva` (via `compute_battle_stats` in `seed_party_battle_stats`), monster slots from `MonsterDef::accuracy`/`evasion` (both = the monster's AGL) at battle setup. The roll engages **only when the attacker's accuracy is seeded** (`acc != 0`); an unseeded attacker auto-hits and consumes no RNG, so disc-free / synthetic battles keep their always-land behaviour and bit-identical RNG streams. Because both accuracy and evasion derive from AGL with the same scaling, the retail `+0x168 = AGL + AGL/4` rescale is ratio-preserving and not separately applied.

### Stat-buff selectors (1..7)

These cases multiply the actor stat block by `6/5` (decompiles to `0x4cccccccd >> 0x22` then `+ uVar13/5`, clamped to `0xFFFF`) - the +20% stat-up animations for buff spells. The earlier "one distinct stat per halfword across `+0x158..+0x16A`" reading was wrong: the actor stores each stat as a **pair of adjacent halfwords** (working + base, both seeded to the same value by `FUN_80054CB0`), so a buff touches two halfwords per stat. See the [actor stat block mapping](#actor-stat-block--monster-record-mapping) below.

**Engine wiring.** `battle_formulas::buff_ramp` ports the `×6/5`-clamped ramp, and the live battle loop applies it for **stat-up** buffs: `World::apply_battle_buff` routes a positive-magnitude `Buff` outcome through `ramp_buff_scalar`, which ramps the live per-slot scalar (`battle_attack` / `battle_magic` / `battle_defense`) by +20% of its current value and records the exact `u16` delta for precise revert on expiry (a refresh reverts the old delta first, so the ramp re-applies from the base with no compounding). Buffs consume **no RNG**, so determinism oracles are unaffected. **Debuffs** (negative magnitude) keep the saturating additive model because retail's debuff scaling factor is not yet pinned — the engine does not fabricate one. Accuracy / Evasion / Speed have no live-loop scalar, so a buff on them only runs the turn timer.

Selector 7's `param_2` sub-index picks which stat group to buff (lines 2473-2574 of `800402f4.txt`):

| `param_2` | Actor pairs raised | Stat(s) |
|---|---|---|
| 3 | `+0x158/+0x15A` | ATK |
| 2 | `+0x15C/+0x15E` and `+0x160/+0x162` | both defense facets (DEF↑ + DEF↓) |
| 1 | `+0x164/+0x166` | `stat5` (role open) |
| 4 | `+0x164/+0x166` + `+0x15C/+0x15E` | `stat5` + DEF↑ |

The single "Defense Up" buff (sub 2) raising **both** `+0x15C` and `+0x160` together is what confirms those two are the two facets of one defense, not separate stats.

## Victory spoils (rewards)

The post-battle EXP / gold / drop are inline in each monster record at
`+0x44..+0x49` (the global archive head; see
[`legaia_asset::monster_archive`](../formats/battle-data-pack.md) and
[battle.md](battle.md)). The spoils function `FUN_8004E568` walks the dead
enemies through the per-enemy **record-pointer table at `0x801C9348`** (populated
by the loader `FUN_800542C8`) and computes:

| Record | Field | Formula |
|---|---|---|
| `+0x44` u16 | base gold | `Σ (gold >> 1)` over dead enemies, `* 1.25` if a living party member has ability bit `0x10000`, then total halved. Lone enemy: `floor((gold >> 1) / 2)`. |
| `+0x46` u16 | base EXP | `Σ (exp)` then `* 3/4` (`v - v>>2`), split evenly among living party members. |
| `+0x48` u8 | drop item id | `0` = no drop. |
| `+0x49` u8 | drop chance % | per dead enemy, `rand() % 100 < (chance + bonus)` grants the item (added to the win banner at actor `+0xA9` and to inventory via `FUN_800421D4`). |

Gold commits to party gold `0x8008459C` (clamp `99,999,999`); EXP commits via
the generic `FUN_80026018` (accumulator `0x80084440` → party XP bank
`0x800845A4`, clamp `9,999,999`), which the minigames share. Runtime-confirmed:
the Gimard fight (`+0x44`=60) credited exactly `+15` gold
(`60>>1=30`, `30-(30>>1)=15`) via a write-watchpoint on `0x8008459C`. Drop ids
cross-check against `legaia-gamedata` (Gimard `+0x48`=119 @ 10% drops Healing
Leaf).

The gold/EXP scaling ports to pure kernels (`battle_formulas::victory_gold_per_monster`
/ `victory_gold_finalize` / `victory_exp_per_member`) the engine's
`World::apply_battle_loot` / `apply_battle_xp` call — so the credited reward is
the scaled amount, not the raw record sum. The +25% gold bonus reads the living
party members' `+0xF4` ability bit `0x10000`; the per-battle no-gold flag
(`_DAT_8007BAC0`, certain scripted fights) is the one remaining unmodelled gold
gate.

## Spirit damage formula

From [battle-action.md state `0x3E` and `0x46`](battle-action.md):

```text
damage = ((target_HP * 7) / 5) + 8;     // 1.4 × target HP + 8
damage = min(damage, 0x120);            // cap 1: 288 hit-points
// or cap 2 (smaller spirit arts): min(damage, 100);
```

This is hard-coded per Spirit super-art and bypasses `FUN_800402F4`. The `_DAT_80076D7E` damage popup is written directly with the result before the state machine calls `func_0x800402F4` in state `0x3F`. The spirit pre-application formula is the one place the engine has to reproduce a non-obvious arithmetic; everything else is selector-dispatch driven.

## Summon-magic damage roll - `FUN_801dd0ac`

A player Seru-magic *damage* summon does **not** go through `FUN_800402F4`'s
selector dispatch and has no static per-spell power scalar (see
[spell-table.md](../formats/spell-table.md#per-spell-damage-power-is-not-static-data--it-is-caster-state-derived)).
Its HP delta is built from live battle stats in three stages — all byte-traced
from `overlay_battle_action_801dd0ac.txt` and the two helpers it calls:

```c
// Stage 1 - rolls (FUN_801dd0ac, summon branch attacker_slot == 7)
atk = rand() % (summon.AGL + 1) + summon.HP + caster.AGL*2;
def = rand() % ((tgt.AGL >> 1) + 1) + (tgt.HP >> 8)
    + (tgt.DEFa >> 4) + (tgt.DEFb >> 4) + tgt.AGL*2;

// Stage 2 - scale (FUN_801dd864)
atk = atk * affinity[atk_elem*8 + def_elem] / 100;   // 8x8 matrix @ 0x801F53E8
if (summon.status & 1) atk = atk*9/10;  if (summon.status & 2) atk = atk*7/10;
if (tgt.guard == 4)    def <<= 1;
if (tgt.status & 1)    def = def*9/10;   if (tgt.status & 2)   def = def*7/10;
atk += atk * (magic_power_byte - 1) >> 3;             // SC + 0x729, summon only
// FUN_801dd0ac re-rolls a weak attacker:
if (def + summon.HP > atk) atk = def + rand() % ((summon.AGL >> 1) + 1) + summon.HP;

// Stage 3 - finish (FUN_801ddb30): resistance bits, rand%9+8 floor, 9999 cap,
//   spirit-gauge fill, damage popup, MP drain, per-element stat debuffs.
damage = atk - def;
```

Recovery summons skip the roll entirely and heal `(magic_power_byte << 5) + 0xE0`,
clamped to `maxHP - curHP`.

### Arts / physical branch (`attacker_slot != 7`)

The **same** kernel `FUN_801dd0ac` also resolves every melee / Tactical-Art /
enemy-special-attack hit. It is the twin of the summon branch with two
differences: the attacker roll is seeded by the **static per-move power scalar**
from the 26-byte-stride move-power table at `0x801F4F5C` (parsed off the disc as
[`legaia_asset::move_power`], see [move-power.md](../formats/move-power.md)), and
it draws two `rand()`s for the attacker roll plus two for the bonus (five total,
vs the summon branch's three). The defender roll and the `FUN_801dd864` scale /
`FUN_801ddb30` finisher are shared; the scale's per-character magic-power arm is
summon-only (`param_1 == 7`), so arts hits scale by affinity + status only.

```c
// Stage 1 - rolls (FUN_801dd0ac, arts/physical branch). power = (i16)move_power[id].+0
atk = rand() % ((power >> 2) + 1) + rand() % ((atk.AGL >> 1) + 1)
    + (atk.HP >> 8) + power + atk.AGL*2;
def = /* identical to the summon-branch defender roll above */;

// Stage 2 - scale (FUN_801dd864): affinity + status only (no magic-power arm).
// Stage 2c - FUN_801dd0ac re-rolls a weak attacker, this time off the power scalar:
if (atk < def + (power >> 1) + (atk.AGL >> 1))
    atk = def + (power >> 1) + rand() % ((power >> 3) + 1)
        + (atk.AGL >> 1) + rand() % ((atk.AGL >> 3) + 1);
```

The bounded, state-free arithmetic of stages 1 + 2 ports to pure kernels for
**both** branches (see the mirror table below); stage 3 (`FUN_801ddb30`, 889
instructions) reads ~20 battle globals and mutates live battle state, so it stays
the coupled tail of the live battle context rather than a pure kernel. Dumps:
`overlay_battle_action_801dd0ac.txt` / `_801dd864.txt` / `_801ddb30.txt`; see the
[`FUN_801DD0AC` / `FUN_801DD864` / `FUN_801DDB30` rows](../reference/functions.md).

**Engine wiring.** The arts/physical kernel is wired into the live loop for
**monster special-attacks**: the move-power table loads from PROT 0898 onto
`World::move_power` (the engine wrapper `move_power::MovePowerCatalog`), and when
a monster's chosen move id resolves to a power record, `cast_spell_on_slots`
overrides the cast's damage magnitude with `arts_physical_predamage` seeded by
that move's power (`World::enemy_move_predamage`, `engine-core::world::battle`).
The stat bridge reads live actor fields faithfully — AGL from `battle_accuracy`
(`+0x168`), HP from `battle.hp`, the two defender defense terms from the
`battle_defense_split` (UDF/LDF) pair — and takes the five `rand()` draws in
retail call order. The `FUN_801dd864` scale currently passes neutral affinity
(100) and no status/guard; the override engages **only when the move-power table
is installed**, so disc-free / synthetic battles keep the MP-scaled placeholder
magnitude with a bit-identical RNG stream. (A party member's Tactical Art does
**not** route through this table — the move-power table is special-attack-only
[its id→index map leaves the basic-attack / art id bands `0x08..=0x11` /
`0x16..=0x18` unmapped, pinned by a live capture], so a character's art takes its
power from the per-strike art-record power byte instead; only `apply_basic_attack`'s
flat `art_strike_damage_default` for a no-art generic hit is a stand-in. The
summon-branch live roll is its own remaining thread.)

### Element-affinity matrix (`FUN_801dd864`, `0x801F53E8`)

The scale stage's affinity byte comes from an 8×8 matrix, indexed
`matrix[attacker_element][defender_element]` (the disasm computes `def_elem +
atk_elem*8` — **row = attacker, column = defender**). The matrix and the
per-character element table that feeds it are static battle-action-overlay data,
now parsed off the disc by [`legaia_asset::element_affinity`] (PROT 0898; matrix
at file `0x26BD0`, char table at `0x26C68`, same link base `0x801CE818` as the
move-power table; CLI `asset element-affinity <0898.BIN>`).

The retail values are a small nudge rather than the classic ×0/×2 weakness
table: the same-element diagonal is `0x60` = 96 (a slight self-resist), reciprocal
opposite-element pairs (`earth↔wind`, `water↔fire`, `light↔dark`) carry `0x68` =
104, everything else is `0x64` = 100. The neutral element (id 7) has an all-100
row + column, and the thunder row (id 4) is special (attacks every element at 102,
takes 98 from dark). The element ids 2/3/4 (fire/wind/thunder) and 7 (neutral) are
byte-pinned; 0/1/5/6 (earth/water/light/dark) are inferred from the reciprocal
pairs + the spell-table element vocabulary.

`FUN_801dd864` resolves each side's element id by actor kind: a **party member**
(slot `< 3`) looks its element up in the per-character table by **1-based** char
id (`CHARACTER_ELEMENTS[char_id]` at `0x801F5480`: Vahn=fire, Noa=wind,
Gala=thunder, Terra=wind); an **enemy** (slot `>= 3`) reads `actor[+0x1d]`. The
enemy element comes from the monster record's **`+0x1D`** byte
([`legaia_asset::monster_archive::MonsterRecord::element`]) — pinned by
correlating that byte against the curated enemy elements across the whole roster
(the four party-table ids reproduce exactly; water/earth/light/dark corroborate)
and by the byte taking *only* values `0..=7` across every populated record.

**Engine wiring.** The matrix + per-character table load from the same PROT 0898
overlay as the move-power table (`World::element_affinity`), and the monster
special-attack path scales by `matrix[enemy_element][party_member_element]`
(`World::enemy_affinity_pct` → `enemy_move_predamage`): the enemy element from
`MonsterDef::element`, the defender from the active party member's element (the
engine models `char_id == party slot`, so a defender at actor slot *s* is char id
*s+1*). The multiply happens after all the rolls, so it never perturbs the RNG
stream, and it's gated on the affinity table being installed (disc-free /
synthetic battles keep the neutral 100% multiplier, bit-identical). The
player-driven **summon** roll is the remaining stand-in here; a party member's
Tactical Art is *not* a move-power case (it uses the art-record power byte — see
the note under the arts/physical kernel above).

## MP cost & ability-bit modifiers

From battle-action.md state `0x28` (Magic / Item - cast begin):

```text
base_mp_cost = spell_table[spell_id].mp_cost;       // entry +3 from spell record
if (character_record.ability_bits & 0x20) {         // "MP-half": shave 50%
    mp_cost = base_mp_cost - (base_mp_cost >> 1);
} else if (character_record.ability_bits & 0x10) {  // "MP-quarter": shave 25%
    mp_cost = base_mp_cost - (base_mp_cost >> 2);
} else {
    mp_cost = base_mp_cost;
}
actor.mp -= mp_cost;
```

The modifier **subtracts a right-shifted copy** of the cost; it is not a
floor-divide. Two consequences: Half rounds *up* on odd costs (`7 → 4`), and
"MP-quarter" (`0x10`) shaves only a **quarter off** (pay 3/4: `40 → 30`), it
does not make the cost a quarter. When both bits are set, **`0x20` (Half) wins**
— the `0x20` test (`andi 0x20; bne`) short-circuits before the `0x10` test is
reached. Dump-confirmed at `FUN_801E295C` `0x801E3D0C` (state `0x28`); the same
block recurs in state `0x3C` at `0x801E4568`. Ported verbatim in
`battle_formulas::mp_cost_after_ability_bits` + `MpCostModifier::from_ability_flags`.

`spell_table` is the static `SCUS_942.54` table at `DAT_800754C8` (stats) / `DAT_800754D0` (name pointers) — 12-byte stride, `+3` = MP cost. See [spell-table.md](../formats/spell-table.md) for the full record layout + the pinned player Seru-magic block (`0x81..=0x8b`).

`character_record.ability_bits` is the 4-byte field at `+0xF4` of the per-character record (record stride `0x414`, base `0x80084708`). See [battle.md](battle.md#character-record-layout).

The character record is documented to have stat fields at `+0x100..+0x110` and an ability-flag bitfield with at least 16 distinct bits in use (the 0x10 / 0x20 / 0x100 / 0x200 quarter / half / HP-cap / MP-cap split is confirmed; the rest of the bit assignments need a spreadsheet of "which character has which natural ability flag set" which is straightforward but hasn't been compiled).

## RNG primitive

`FUN_80056798()` is the in-game RNG. It's the standard PsyQ `rand()` pattern (same shape as the libc `rand()` PsyQ provides - 32-bit LCG with multiplier `1103515245` and increment `12345`, return value `(seed >> 16) & 0x7FFF`). The cliff-notes:

```c
int FUN_80056798(void) {
    DAT_8007AE5C = DAT_8007AE5C * 1103515245 + 12345;
    return (DAT_8007AE5C >> 16) & 0x7FFF;
}
```

(Confirmed: see `ghidra/scripts/funcs/80056798.txt`.) Range: 0..32767. For damage variance the battle code typically uses `roll % (cap)` so distribution skew at small cap values is fine.

The seed (`DAT_8007AE5C`) initialises from the boot timer - for deterministic playback the engine must seed it from the same source, otherwise replay tests will diverge.

## Engine-side mirror - `engine-vm::battle_formulas`

The clean-room Rust module `crates/engine-vm/src/battle_formulas.rs` ports the formulas above as pure functions. It's deliberately *not* trying to reproduce `FUN_800402F4`'s entire selector-dispatch - that lives in `engine-vm::battle_action` next to the state machine.

| Function | Provenance |
|---|---|
| `spirit_damage` | battle-action.md state 0x3E / 0x46 |
| `mp_cost_after_ability_bits` | battle-action.md state 0x28 |
| `accuracy_roll` | this doc, selector 9 above |
| `psyq_rand_step` | `ghidra/scripts/funcs/80056798.txt` |
| `damage_cap_for_party_slot` | this doc, selector 0 above (`DAT_8007655C` table) |
| `summon_attacker_roll` / `summon_defender_roll` / `summon_bonus_roll` / `summon_predamage` | this doc, summon-roll stages 1+2 (`FUN_801dd0ac` summon branch) |
| `arts_attacker_roll` / `arts_bonus_roll` / `arts_physical_predamage` | this doc, arts/physical-roll stages 1+2 (`FUN_801dd0ac` non-summon branch, seeded by the `0x801F4F5C` move-power table) |
| `apply_element_affinity` / `apply_status_weaken` / `apply_magic_power` | this doc, summon-roll scale stage (`FUN_801dd864`) |
| `heal_summon_amount` | this doc, recovery-summon closed form |
| `victory_gold_per_monster` / `victory_gold_finalize` / `victory_exp_per_member` | this doc, victory-spoils gold/EXP scaling (`FUN_8004E568`) |

The unit tests there pin the documented formulas as fixtures - a future runtime trace can then add comparison cases without touching the formula bodies.

## What's still open

- **Selector dispatch for selectors `0x10..=0x83`.** The cases beyond status / buff / damage handle stat-up animations, status-clear, queue-end markers, and the multi-target item slot used by Smelly Glove etc. They're mostly read-only stat ramps that don't affect game balance, so leaving them un-decoded is fine for a first port.
- The monster record is now fully decoded: all six stat halfwords (see [actor stat block mapping](#actor-stat-block--monster-record-mapping)), the reward fields (see [victory spoils](#victory-spoils-rewards)), and the spell-offset list (see [spell list](#spell-list-record-0x4c)). No record fields remain open. The spell entries' interior layout beyond the id (`+0x00`) and SP cost (`+0x74`) - the `+0x04`/`+0x08` effect-script sub-pointers - is the same attack-effect geometry as the monster's own `+0x04` data and is left undecoded.
- **Ability-bit catalogue.** The ability bitfield at `+0xF4` of the character record has at least the documented MP-half / MP-quarter / HP-cap / MP-cap bits in use, plus the impact-step modifier (`0x10` / `0x20`) on attack actions. The full per-character mapping comes out of save-data (the 0x414 record's `+0xF4..+0xF8` is one row in the save schema's character block) - a few new-game saves with different early-game characters resolve it.

## See also

**Reference** —
[Battle scene](battle.md) ·
[Battle action SM](battle-action.md) ·
[Level-up](level-up.md) ·
[Game-data tables](../reference/gamedata.md)
