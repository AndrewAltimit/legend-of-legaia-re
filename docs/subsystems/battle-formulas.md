# Battle formulas

Damage, MP-cost, and stat-cap math used by the [battle action state machine](battle-action.md). Lives in the battle overlay (`0898`); the central damage-application primitive is `FUN_800402F4`.

## Contents

- [Damage application primitive - `FUN_800402F4`](#damage-application-primitive---fun_800402f4)
- [Actor stat block + monster record mapping](#actor-stat-block--monster-record-mapping) - [spell list](#spell-list-record-0x4c) · [physical attack damage](#physical-attack-damage---overlay_battle_action_801ec3e4) · [selector 0](#selector-0---basic-damage-attack--item--generic-spell) · [selector 9 (accuracy)](#selector-9---accuracy--evasion-roll) · [stat-buff selectors](#stat-buff-selectors-17)
- [Victory spoils (rewards)](#victory-spoils-rewards) · [spirit damage formula](#spirit-damage-formula) · [run / escape roll](#run--escape-roll---fun_801e791c)
- [Per-round status DoT ticker - `FUN_801E752C`](#per-round-status-dot-ticker---fun_801e752c) · [status application byte map](#status-application-the-art--move-record-status-byte)
- [Summon-magic damage roll - `FUN_801dd0ac`](#summon-magic-damage-roll---fun_801dd0ac) - [arts / physical branch](#arts--physical-branch-attacker_slot--7) · [element-affinity matrix](#element-affinity-matrix-fun_801dd864-0x801f53e8)
- [Summon spell XP + magic level-up](#summon-spell-xp--magic-level-up)
- [MP cost & ability-bit modifiers](#mp-cost--ability-bit-modifiers) · [RNG primitive](#rng-primitive)
- [Engine-side mirror - `engine-vm::battle_formulas`](#engine-side-mirror---engine-vmbattle_formulas) · [what's still open](#whats-still-open)

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
| `+0x0E` | `+0x154/+0x156` | **AGL** | agility / action gauge - spent per action, reset each round; the "Power Up" buff raises it ("agility increased!") |
| `+0x12` | `+0x158/+0x15A` | **ATK** | attacker's offense in the damage routine |
| `+0x14` | `+0x15C/+0x15E` | **UDF** (high/upper) | defender defense, branch A |
| `+0x16` | `+0x160/+0x162` | **LDF** (low/lower) | defender defense, branch B |
| `+0x18` | `+0x168/+0x16A` | **INT** | magical damage / magic defense (summon/arts kernel) + accuracy/evasion seed (selector 9); the bestiary INT column |
| `+0x1A` | `+0x164/+0x166` | **SPD** | turn-order initiative seed |

> **Stat names** match the game's own labels (the "Power Up" buff prints *"agility increased!"* and bumps `+0x0E`) and the fan bestiaries; the curated `enemies.toml` `agl` / `int` columns byte-match `+0x0E` / `+0x18` (see `gamedata/tests/enemy_stats_vs_disc`). Earlier drafts of this doc swapped two of them - what was labeled "SP/spirit" is **AGL** (`+0x0E`), and what was labeled "AGL" is **INT** (`+0x18`). The `+0x168` actor slot ("accuracy" below) is therefore the monster's INT; party members seed `+0x168` from their AGL-derived accuracy instead (an engine model). Per Meth962, INT "affects your magical damage and defense against other magical spells" - which the summon/arts kernel (`FUN_801dd0ac`) bears out: attacker INT is a damage term, defender INT a mitigation term.

**Battle-load stat boost.** The record halfwords above are *not* the values the fight uses. After the plain copy, `FUN_80054CB0` boosts four combat stats, picking one of two profiles by the battle-context flag `_DAT_8007BD24 + 0x287` (= `(*(u8*)0x8007BD60 >> 5) & 4`, bit 7 of a per-battle flags byte set by `FUN_800513F0`):

- **gate-set profile (B):** `ATK += ATK>>2` (×5/4), `UDF × 2`, `LDF × 2`, `INT += INT>>3` (×9/8).
- **gate-clear profile (A):** `UDF`/`LDF += (x>>1)+(x>>2)` (×7/4), `INT += INT>>2` (×5/4), ATK unchanged.

HP/MP/AGL/SPD are copied unchanged in both. Both profiles boost - the raw record always understates the fight. A live international-retail capture reproduces profile **B** byte-for-byte (Gaza Sim-Seru id 166: raw ATK 288 / UDF 222 / LDF 200 / INT 220 → in-battle 360 / 444 / 400 / 247), which is also what the curated `enemies.toml` holds and what `MonsterRecord::battle_stats()` returns. This cross-region difficulty difference was first surfaced by **Zetopheonix**; the [enemy table](../../site/_content/monsters.html) shows the boosted stats by default with a raw-record toggle.

**SPD** (`+0x164`): `overlay_0897_801e23ec` seeds each actor's per-turn initiative key from it: `+0x16C = speed + (rand() % (speed/2 + 1)) + 1`. It has a dedicated "Speed Up" buff (selector 7 sub 1) and is reset to its base each round (`FUN_80053CB8`: `+0x164 = +0x166`). Distinct from INT, which governs the hit/dodge roll rather than turn order, and from AGL, which is the per-round action gauge. The next-actor selector `recompute_battle_order` (`FUN_801daba4`) reads the seeded `+0x16C` keys: it picks the living actor with the highest key (random tiebreak via `rand % tie_count`), zeroing dead actors' keys first. Ported as `World::next_combatant_by_initiative`; see [turn order in battle.md](battle.md#auto-resolve-vs-player-driven).

**AGL** (`+0x154` current / `+0x156` base): the per-round agility / action gauge. Every action draws it down; the enemy-AI action picker (`overlay_0898_801e9fd4`) deducts each candidate action's `+0x74` cost from `+0x154` and only queues actions it can still afford. Each round `FUN_801D88CC` resets `+0x154` to its base (`+0x156`), or, when spirit-charged (`+0x1DE == 4`), to `(base*7/5)+8` capped at `0x120` (the same shape as the [spirit-damage formula](#spirit-damage-formula)). Live-RAM confirmed by Zetopheonix: the "Power Up" buff prints *"agility increased!"* and raises this cur/base pair. The damage popup (`_DAT_80076D7E`) reads `+0x154`; this is the HP/MP/AGL triplet at `+0x14C..+0x156` in [battle.md](battle.md).

### Spell list (`record +0x4C`)

`record +0x4A` (u8) is the spell count; `record +0x4C` is an array of that many u32 **block-relative offsets**, each pointing at a spell entry inside the same decoded monster block. The battle loader `FUN_800542C8` (`ghidra/scripts/funcs/800542c8.txt`, lines 633-658) fixes every offset to an absolute pointer at battle init - `record[+0x4C + i*4] += block_base` - exactly like `name_offset`; it also initialises a `+0x88` self-pointer to `entry+0x8C` and resolves each entry's `+0x04`/`+0x08` **effect indices** (see below).

Each entry's `+0x04`/`+0x08` are **1-based indices** (`0` = none), *not* direct pointers, into the per-block **effect-offset table** that immediately follows the spell-offset array (table word base `magic_count + 0x13`). The loader resolves index → offset → pointer: `entry[+0x04] = block[(index + magic_count + 0x12)*4] + block_base`. The resolved offset lands on a short per-spell effect/animation descriptor (observed head `[00, a, b, b, len, 00 00 00, u32, …]`) - a small fixed record, **not** a TMD and **not** "the same geometry as the monster's own `+0x04`". Decoded from disc by `legaia_asset::monster_archive` (`MonsterSpell::effect_offset` / `aux_offset`); 289 of 1811 spell entries carry an effect index, 24 an aux index.
The descriptor's interior field semantics are still open (its runtime consumer is the cast/effect path, not the AI picker).

Each spell entry's head:

- **`+0x00` (u8) - spell/action id**, which doubles as a category selector. `FUN_80054CB0` (lines 700-727) treats ids `2,3,4,5,0x0B` as **elemental resist/affinity** markers and writes the matching spell's index into actor `+0x1EF..+0x1F3`. The AI picker treats ids `0x0C..=0x1F` as **offensive castable spells** (`*entry - 0xC < 0x14`) and `0x23` (`'#'`) as a special category.
- **`+0x74` (u8) - AGL (action) cost**. The picker only rolls a spell when `cost != 0xFF` and current AGL (`+0x154`) `>= cost`, then subtracts it (lines 2219-2252 of `overlay_0898_801e9fd4`).

Real-data sanity check: Gimard (id 10, AGL 60) has 9 slots - the affinity prefix `0,1,2,4,5,0x0B` (cost 0), two castable spells `0x0D @ 28` and `0x0F @ 32` (both `<= 60`), and the `0x23` special. Hornet (id 61, AGL 88) has `0x0C @ 88` and `0x13 @ 88`. Across every populated record the decoded list length equals the declared count and no offset escapes the block. The `legaia_asset::monster_archive::MonsterRecord::spells` field (`MonsterSpell { id, agl_cost, offset, effect_offset, aux_offset }`, with `is_castable()`) exposes this; the [enemy table](../../site/_content/monsters.html) renders the castable set with AGL cost.

### Physical attack damage - `overlay_battle_action_801ec3e4`

Lines 2716-2826. The raw hit value is built from the **attacker's ATK** (`actor[+0x158]`) and reduced by the **defender's defense** - the routine reads `actor[+0x15C]` (UDF) when the attack's move index satisfies `(move - 0xC) % 10 < 5`, else `actor[+0x160]` (LDF):

```c
atk = attacker[+0x158];                                  // stat1 = ATK
def = ((move - 0xC) % 10 < 5) ? target[+0x15C]           // UDF (stat2)
                              : target[+0x160];           // LDF (stat3)
raw   = (atk + rand() % (atk/8 + 1)) * armor_factor>>4 + … ;
guard = def + rand() % (def/8 + 1) + … ;
// damage applied when raw exceeds guard, scaled by the difference
```

This is the binding that names ATK / UDF / LDF; the `legaia_asset::monster_archive` accessors (`attack()` / `defense_high()` / `defense_low()`) and `engine-core`'s `monster_def_from_record` follow it.

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

**Engine wiring.** `battle_formulas::accuracy_roll` ports this roll, and the live battle loop applies it per strike in both the arts/spell strike resolver and the basic-attack path (`engine-core::world::battle`). Each actor's `+0x168` value lives in the World-side `battle_accuracy` / `battle_evasion` arrays: party slots are seeded from each character's AGL-derived `acc`/`eva` (via `compute_battle_stats` in `seed_party_battle_stats`), monster slots from `MonsterDef::accuracy`/`evasion` (both = the monster's INT, record `+0x18`) at battle setup. The roll engages **only when the attacker's accuracy is seeded** (`acc != 0`); an unseeded attacker auto-hits and consumes no RNG, so disc-free / synthetic battles keep their always-land behaviour and bit-identical RNG streams.
For party members, both accuracy and evasion derive from the character's AGL with the same scaling, so the retail `+0x168 = AGL + AGL/4` rescale is ratio-preserving and not separately applied. (For monsters, `+0x168` is loaded directly from record `+0x18` = INT.)

### Stat-buff selectors (1..7)

These cases multiply the actor stat block by `6/5` (decompiles to `0x4cccccccd >> 0x22` then `+ uVar13/5`, clamped to `0xFFFF`) - the +20% stat-up animations for buff spells. The earlier "one distinct stat per halfword across `+0x158..+0x16A`" reading was wrong: the actor stores each stat as a **pair of adjacent halfwords** (working + base, both seeded to the same value by `FUN_80054CB0`), so a buff touches two halfwords per stat. See the [actor stat block mapping](#actor-stat-block--monster-record-mapping) below.

**Engine wiring.** `battle_formulas::buff_ramp` ports the `×6/5`-clamped ramp, and the live battle loop applies it for **stat-up** buffs: `World::apply_battle_buff` routes a positive-magnitude `Buff` outcome through `ramp_buff_scalar`, which ramps the live per-slot scalar (`battle_attack` / `battle_magic` / `battle_defense`) by +20% of its current value and records the exact `u16` delta for precise revert on expiry (a refresh reverts the old delta first, so the ramp re-applies from the base with no compounding). Buffs consume **no RNG**, so determinism oracles are unaffected. **Debuffs** (negative magnitude) keep the saturating additive model because retail's debuff scaling factor is not yet pinned - the engine does not fabricate one. Accuracy / Evasion / Speed have no live-loop scalar,
so a buff on them only runs the turn timer.

Class 7's `param_2` sub-index picks which stat group to buff (`800402f4.txt`
`case 7`, lines 2473-2639). This is the **one-battle stat-buff Elixir** path -
the consumer is the item-use apply handler, and each item's `param_2` is its
descriptor `tier` byte (`legaia_asset::item_effect`):

| `param_2` (= tier) | Actor pairs raised | Stat(s) | Item |
|---|---|---|---|
| 1 | `+0x164/+0x166` | **SPD** | Speed Elixir |
| 2 | `+0x15C/+0x15E` and `+0x160/+0x162` | both defence facets (UDF + LDF) | Shield Elixir |
| 3 | `+0x158/+0x15A` | **ATK** | Power Elixir |
| 4 | `+0x164/+0x166` + the two DEF pairs + `+0x158/+0x15A` + `+0x168/+0x16A` | **all** (SPD + DEF + ATK + INT) | Wonder Elixir |

(This corrects an earlier reading that labelled `param_2 == 1` "`stat5` (role
open)" and `param_2 == 4` "`stat5` + UDF" - the `+0x164` field is **SPD** (Speed
Elixir confirms it), and `param_2 == 4` raises every battle stat (Wonder Elixir,
the full `case 7` arm at lines 2555-2638), not just two.)

The single "Defense Up" buff (sub 2) raising **both** `+0x15C` and `+0x160`
together is what confirms those two are the two facets of one defense, not
separate stats.

The sibling **permanent** stat-up classes are in the same handler. **Class 6**
(field-use, the *Water* line: Life / Power / Guardian / Swift / Wisdom / Magic
Water + the all-stats Honey / Miracle Water) adds a flat increment to the
**character record** (`0x80084708 + slot*0x414`), `tier` selecting the stat;
**class 5** (Fury Boost) sets the action-gauge flag. Full taxonomy +
disc-pinned item ids in [`item-effect-table.md`](../formats/item-effect-table.md#stat-up--buff-items-class-567);
parser `legaia_asset::item_effect::stat_item_effect` / `ItemEffectTable::stat_effect`.

## Victory spoils (rewards)

The post-battle EXP / gold / drop are inline in each monster record at
`+0x44..+0x49` (the global archive head; see
[`legaia_asset::monster_archive`](../formats/monster-animation.md) and
[battle.md](battle.md)). The spoils function `FUN_8004E568` walks the dead
enemies through the per-enemy **record-pointer table at `0x801C9348`** (populated
by the loader `FUN_800542C8`) and computes:

| Record | Field | Formula |
|---|---|---|
| `+0x44` u16 | base gold | `Σ (gold >> 1)` over dead enemies, `* 1.25` if a living party member has ability bit `0x10000`, then total halved. Lone enemy: `floor((gold >> 1) / 2)`. |
| `+0x46` u16 | base EXP | `Σ (exp)` then `* 3/4` (`v - v>>2`), split evenly among living party members. |
| `+0x48` u8 | drop item id | `0` = no drop. |
| `+0x49` u8 | drop chance % | per dead enemy, `rand() % 100 < (chance + bonus)` grants the item (added to the win banner at actor `+0xA9` and to inventory via `FUN_800421D4`). |

Gold commits to party gold `0x8008459C` (clamp `99,999,999`); EXP is divided
among the living members inside `FUN_8004E568` itself (`divu` by the alive
count) and applied per member via the level-up applier `FUN_801E9504`. (The
earlier "EXP commits via the generic `FUN_80026018`" reading is wrong:
`FUN_80026018` is the mode-24 **minigame exit / return-warp** handler and its
`_DAT_800845A4 += _DAT_80084440` commit is the **casino-coin** bank - no
battle-path caller exists in the dump corpus; see
[`script-vm.md § 0x3E WARP`](script-vm.md#0x3e-warp-mode-24-minigame-door-warp).)
Runtime-confirmed:
the Gimard fight (`+0x44`=60) credited exactly `+15` gold
(`60>>1=30`, `30-(30>>1)=15`) via a write-watchpoint on `0x8008459C`. Drop ids
cross-check against `legaia-gamedata` (Gimard `+0x48`=119 @ 10% drops Healing
Leaf).

The gold/EXP scaling ports to pure kernels (`battle_formulas::victory_gold_per_monster`
/ `victory_gold_finalize` / `victory_exp_per_member`) the engine's
`World::apply_battle_loot` / `apply_battle_xp` call - so the credited reward is
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

## Run / escape roll - `FUN_801E791C`

The flee decision battle-action state `0x64` requests:

```text
party_score = Σ_party  (SPD*3)>>1 + (maxHP - curHP)>>4
enemy_score = Σ_enemy   SPD      + (maxHP - curHP)>>5
roll_p = rand() % party_score ;  roll_e = rand() % enemy_score
Escape Boost (ability bit 52):  roll_p += roll_p >> 1
Great Escape (bit 55):          roll_p = roll_e            // forced tie
caught iff  roll_p < roll_e  or  ctx[+0x287] != 0          // strict <
```

Missing HP raises *both* sides' scores (a hurt party escapes more easily, a hurt
enemy pursues harder) and the party's SPD is weighted 1.5x the enemies'. Full
decode - the outcome pointer, the accessory-bit fold over living wearers, the
forced-flee battle flag and the success-side flee staging - lives in
[battle-action.md § the escape roll](battle-action.md#the-escape-roll-fun_801e791c).

## Per-round status DoT ticker - `FUN_801E752C`

`ghidra/scripts/funcs/overlay_battle_action_801e752c.txt` (760 bytes / 190
instructions). Called once per battle round by the round driver `FUN_801D0748`
(state `0x14`, immediately after `FUN_801D88CC` / `FUN_801DA780`), gated on
`ctx[+0x28A] != 0` - the round counter, incremented by the SM at end-of-round
(`0x801E67E8`) and by the scripted `case 0xFF` phase action - so no DoT lands
before the first round has completed. Walks all 7 actor slots; for each living
actor (`+0x14C != 0`) it reads the `+0x16E` status halfword:

```c
if (status & 2) {                       // Toxic (strong DoT) - tested FIRST
    dmg = max_hp >> 4;                  //   +0x14E / 16
    if (cur_hp <= dmg) dmg = cur_hp - 1;//   never lethal: leaves 1 HP
    if (dmg > 0x100)   dmg = 0x100;     //   cap 256
} else if (status & 1) {                // Venom (weak DoT) - shadowed by Toxic
    dmg = max_hp >> 5;                  //   +0x14E / 32
    if (cur_hp <= dmg) dmg = cur_hp - 1;
    if (dmg > 0x80)    dmg = 0x80;      //   cap 128
}
cur_hp -= dmg;                          // +0x14C and the +0x172 mirror
```

Reads:

1. **Both arms key on max HP** (`+0x14E`), not current HP, so the drain does
   not taper; Toxic is exactly 2x Venom.
2. **The never-kill clamp precedes the cap**, so a low-HP actor's tick is
   `cur_hp - 1` even when that exceeds what the raw fraction would give after
   mitigation. There is no 1-damage floor - a tiny `max_hp` ticks 0 and draws
   no damage popup (the popup ring at `ctx[+0x83C]`/`+0x318` is only pushed
   for `dmg != 0`).
3. **Toxic suppresses Venom** - the bits are an if/else pair, so with both set
   only the strong arm ticks.
4. Under the `ctx[+0x287]` config flag, a **monster's** (slot >= 3) DoT bit is
   cleared after one tick (`status &= ~bit`) - party DoTs persist regardless.
   Not yet modelled by the engine.
5. The same walk pays the per-round accessory recoveries for party slots:
   char `+0xF8` bit `0x20` (passive `0x25` HP After / Life Grail) →
   `FUN_800402F4(0, 0, slot)`, bit `0x40` (`0x26` MP After / Magic Grail) →
   `FUN_800402F4(2, 2, slot)`.

The two DoT bits also scale combat rolls: `FUN_801DD864` (and the inline twin
in `overlay_battle_action_801ec3e4` lines 2800-2808) multiplies an afflicted
actor's outgoing roll *and* its guard roll by `9/10` for bit 1 (Venom) and
`7/10` for bit 2 (Toxic) - already ported as
`battle_formulas::apply_status_weaken`.

### Status application (the art / move record status byte)

The two pinned hit resolvers - `overlay_battle_action_801ec3e4` (~line 3099,
physical strike, art record `+0x7A`) and `overlay_battle_action_801e09f8`
(~line 1416, monster special attack, effect-block `+0x0A`) - apply the same
byte map onto the target's `+0x16E`:

| byte | `+0x16E` effect | chance | guard |
|---|---|---|---|
| `1`, `2` | visual only here (`actor+0x21F` marker + tint word `+4`); the mechanical arm for these bytes is not in the dumped corpus | - | - |
| `3` | `\|= 1` (**Venom**) | `rand & 7 == 0` (1/8) | - |
| `4` | `\|= 2` (**Toxic**) | `rand & 7 == 0` (1/8) | - |
| `5` | `\|= 1 << (rand%3 + 3)` (**Rot** - disables one random strike command; bits `8`/`0x10`/`0x20` gray the matching arrow in the command menu, `== 0x38` blocks Attack entirely) | always (party target) | char `+0xF4` bit 24 (passive `0x18` Rot Guard) or bit 28 (`0x1C` Master Guard) nullifies |
| `6` | `\|= 0x1000` (**Curse** - the magic block the menu + AI affordability checks read) | `rand & 3 == 0` (1/4) | - |

**Stone = `+0x16E` bit `0x04`** - capture-pinned from a before/after pair
around an enemy Glare cast (the petrify lands as `+0x16E: 0 → 4` with HP
untouched; the victim's queued action category at `+0x1DE` clears, and the
`+0x220` flag near the lingering-status visual marker drops). Bit `0x04` is
exactly the hole the applier byte map above leaves unassigned; the petrify
applier itself (Glare's effect path) is not in the dumped corpus.

Note this conflicts with the engine's inherited byte naming (`4` = Sleep, `5` =
Confuse, from external notes - see `legaia_engine_vm::status_effects`); the
remap is held open until a capture pins what bytes `1`/`2` do mechanically.

## Summon-magic damage roll - `FUN_801dd0ac`

A player Seru-magic *damage* summon does **not** go through `FUN_800402F4`'s
selector dispatch and has no static per-spell power scalar (see
[spell-table.md](../formats/spell-table.md#per-spell-damage-power-is-not-static-data--it-is-caster-state-derived)).
Its HP delta is built from live battle stats in three stages - all byte-traced
from `overlay_battle_action_801dd0ac.txt` and the two helpers it calls. In the
pseudocode below `INT` is the actor's `+0x168` stat (for monsters that is
record `+0x18`, the bestiary INT column; for the party caster it is the
character's `+0x168` accuracy line) - **not** the AGL action gauge (`+0x0E`):

```c
// Stage 1 - rolls (FUN_801dd0ac, summon branch attacker_slot == 7)
atk = rand() % (summon.INT + 1) + summon.HP + caster.INT*2;
def = rand() % ((tgt.INT >> 1) + 1) + (tgt.HP >> 8)
    + (tgt.DEFa >> 4) + (tgt.DEFb >> 4) + tgt.INT*2;

// Stage 2 - scale (FUN_801dd864)
atk = atk * affinity[atk_elem*8 + def_elem] / 100;   // 8x8 matrix @ 0x801F53E8
if (summon.status & 1) atk = atk*9/10;  if (summon.status & 2) atk = atk*7/10;
if (tgt.guard == 4)    def <<= 1;
if (tgt.status & 1)    def = def*9/10;   if (tgt.status & 2)   def = def*7/10;
atk += atk * (magic_power_byte - 1) >> 3;             // SC + 0x729, summon only
// FUN_801dd0ac re-rolls a weak attacker:
if (def + summon.HP > atk) atk = def + rand() % ((summon.INT >> 1) + 1) + summon.HP;

// Stage 3 - finish (FUN_801ddb30): equipment elemental-resistance halving,
//   guard halve, rand%9+8 no-damage floor, summon power-% scale, 9999 cap,
//   spirit-gauge fill, damage popup, MP drain, per-element stat debuffs.
damage = atk - def;
```

The finisher works on `over = atk - def` (the damage above the base) and rewrites
it through six closed-form stages: (1) **party-defender elemental resistance** -
if the defender's equipment sets the resist bit for the attacker's element,
`over >>= 1` (the absorb bit `0x10` instead routes to a `over*3>>2` 3/4 scale).
The resist words are the first two words of the character record's
accessory-passive **ability bitfield** (`+0xF4`/`+0xF8`, aggregator
`FUN_80042558`), and every flag is passive index `0x1D + element` read through
the word boundary: the elemental-guard passives sit contiguously at
`0x1D..=0x23` (Earth, Water, Fire, Wind, Thunder, Light, Dark - the element-id
order), so elements 0..=2 test `+0xF4` bits 29..31 and elements 3..=6 test
`+0xF8` bits 0..3; the absorb gate `+0xF8 & 0x10` is All Guard (`0x24`, Rainbow
Jewel), and the two "spirit gain up" bits below are AP Boost 1/2
(`0x28`/`0x29`). See
[accessory-passive-table.md](../formats/accessory-passive-table.md);
(2) **enemy-defender halve** (`_DAT_8007bd84`); (3) **guard halve** (defender
`+0x1de == 4`); (4) the **no-damage floor** `over = rand()%9 + 8` when mitigation
zeroed it; (5) the **summon power-% scale** (`attacker_slot == 7`): `over =
over * pct / 100` with `pct = table[(caster_char_id - 1) * 8 + summon_element]`
from the per-caster table at `0x801F5468` (PROT 0898 file `0x26C50`, the 24
bytes before the per-character element table; parsed as
`legaia_asset::element_affinity::ElementAffinity::summon_power`). Each caster
summons their own element at 100% and their opposed element weakest - Vahn
fire 100 / water 40, Noa wind 100 / earth 40, Gala thunder 100 / dark 60, the
rest 70–95 (`asset element-affinity` prints the rows); (6) the **9999 cap**. The defender's spirit gauge then fills by `pct = max(1,
over*100/maxHP)` plus the two "spirit gain up" equipment bits (`+0xF8 & 0x200`
→ `pct>>2`, `& 0x100` → `pct/10`), clamped to 100.

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
//   atk.INT = the attacker's +0x168 stat (record +0x18 for monsters)
atk = rand() % ((power >> 2) + 1) + rand() % ((atk.INT >> 1) + 1)
    + (atk.HP >> 8) + power + atk.INT*2;
def = /* identical to the summon-branch defender roll above */;

// Stage 2 - scale (FUN_801dd864): affinity + status only (no magic-power arm).
// Stage 2c - FUN_801dd0ac re-rolls a weak attacker, this time off the power scalar:
if (atk < def + (power >> 1) + (atk.INT >> 1))
    atk = def + (power >> 1) + rand() % ((power >> 3) + 1)
        + (atk.INT >> 1) + rand() % ((atk.INT >> 3) + 1);
```

The bounded, state-free arithmetic of stages 1 + 2 ports to pure kernels for
**both** branches (see the mirror table below). Stage 3 (`FUN_801ddb30`, 889
instructions) splits: its **closed-form finalisation arithmetic** now ports too -
`battle_formulas::damage_finish` (the six damage-rewrite stages above) and
`spirit_gauge_fill` (the gauge accrual), both with hand-checked unit tests. The
engine can route the live basic-attack damage through `damage_finish` behind the
`World::use_damage_finish` gate (the `--damage-finish` play-window flag): the raw
roll feeds the finisher so the 9999 cap and the `rand()%9+8` no-damage floor
apply. The **defender resist inputs are live**: `World::defender_resist` reads
the two resist words off the occupying character's rebuilt ability bitfield
(`refresh_party_ability_bits`), so an equipped elemental-guard accessory halves
a matching-element monster special, All Guard applies the 3/4 scale, and the AP
Boost bits accelerate the wearer's spirit-gauge fill - the monster
special-attack path (`enemy_move_predamage`) runs the closed-form finisher
stages (resist ladder vs the monster record element `+0x1D`, guard halve,
floor, cap) on every hit. The finisher
draws its one RNG only when a hit zeroes out, so the no-gear RNG call-count is
unchanged. The
finisher's remaining tail - the damage-popup accumulator (`_DAT_8007bd14`), the
`DAT_801f6980` AI revenge table, the MP drain, and the per-element stat-debuff
`switch` (keyed on the attacker element at `DAT_801c9358+0x1d`) - reads/writes
~20 battle globals and stays in the live battle context. Dumps:
`overlay_battle_action_801dd0ac.txt` / `_801dd864.txt` / `_801ddb30.txt`; see the
[`FUN_801DD0AC` / `FUN_801DD864` / `FUN_801DDB30` rows](../reference/functions.md).

**Engine wiring.** The arts/physical kernel is wired into the live loop for
**monster special-attacks**: the move-power table loads from PROT 0898 onto
`World::move_power` (the engine wrapper `move_power::MovePowerCatalog`), and when
a monster's chosen move id resolves to a power record, `cast_spell_on_slots`
overrides the cast's damage magnitude with `arts_physical_predamage` seeded by
that move's power (`World::enemy_move_predamage`, `engine-core::world::battle`).
The stat bridge reads live actor fields faithfully - INT (the `+0x168` stat,
record `+0x18`) from `battle_accuracy`, HP from `battle.hp`, the two defender defense terms from the
`battle_defense_split` (UDF/LDF) pair - and takes the `rand()` draws in retail
call order: attacker ×2 + defender ×1 up front, then the bonus pair **lazily**
(only when the bonus arm fires, via `arts_physical_predamage_lazy`), so the
shared RNG cursor advances by three or five draws exactly as `FUN_801dd0ac` does.
The `FUN_801dd864` scale supplies the real enemy→party element affinity
(`World::enemy_affinity_pct`, `matrix[enemy_element][party_member_element]`,
neutral 100 when the affinity table isn't installed) with status/guard still
defaulted; the override engages **only when the move-power table
is installed**, so disc-free / synthetic battles keep the MP-scaled placeholder
magnitude with a bit-identical RNG stream. (A party member's Tactical Art does
**not** route through this table - the move-power table is special-attack-only
[its id→index map leaves the basic-attack / art id bands `0x08..=0x11` /
`0x16..=0x18` unmapped, pinned by a live capture], so a character's art takes its
power from the per-strike art-record power byte instead; only `apply_basic_attack`'s
flat `art_strike_damage_default` for a no-art generic hit is a stand-in.)

**The summon branch is wired the same way for player Seru-magic casts**
(`World::player_summon_predamage`): when the monster catalog resolves the
spell's namesake summon creature, `cast_spell_on_slots` replaces the MP-scaled
placeholder with `summon_predamage_lazy` seeded faithfully - summon-body
HP/INT from the creature's `battle_data` record (the stats the loader installs
on the freshly-spawned slot-7 actor; INT = record `+0x18`), the caster's `battle_accuracy` (`+0x168`)
doubled, the affinity percent inside the roll, and the caster's per-spell
**magic-power byte** searched the way `FUN_801dd864` does (the character
record's 32-entry spell-id list at `+0x13D` with parallel level bytes at
`+0x161`, live `0x80084845`/`0x80084869`; identity `1` when the roster doesn't
carry the spell). The closed-form `FUN_801ddb30` finisher stages then apply -
the lazily-drawn `rand()%9+8` floor, the per-caster summon power-percent
(`0x801F5468`), and the 9999 cap. RNG draws follow retail call order:
attacker + defender eager, the bonus arm and the floor lazy, so the cursor
advances by two to four draws exactly as `FUN_801dd0ac`/`FUN_801ddb30` do.
Gating mirrors the arts path: an unresolved creature (disc-free / synthetic
battles) keeps the placeholder magnitude and an untouched RNG stream.

### Element-affinity matrix (`FUN_801dd864`, `0x801F53E8`)

The scale stage's affinity byte comes from an 8×8 matrix, indexed
`matrix[attacker_element][defender_element]` (the disasm computes `def_elem +
atk_elem*8` - **row = attacker, column = defender**). The matrix and the
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

`FUN_801dd864` resolves each side's element id **by the actor's battle slot, not
the spell**: a **party member** (slot `< 3`) looks its element up in the
per-character table by **1-based** char id (`CHARACTER_ELEMENTS[char_id]` at
`0x801F5480`: Vahn=fire, Noa=wind, Gala=thunder, Terra=wind); any **other slot**
(`>= 3`, which is both enemies *and* the slot-7 summon body) reads the element
**directly from the monster-archive record `+0x1d`** - `FUN_801dd864` indexes the
per-enemy **record-pointer table** `0x801C9348` (NOT the live-actor table
`0x801C9370`) by `slot - 3` and does `lbu …,0x1d(record)` (dump
`overlay_battle_action_801dd864.txt` `0x801dd8c4`/`0x801dd8dc`). There is **no**
copy of the element into a live-actor field (unlike the `+0x0E..+0x1A` stats,
which `FUN_80054CB0` *does* copy into `+0x14C..`).

This **resolves the player-cast element question**: a player Seru-magic cast
attacks *as the summoned creature* - it rolls through the summon path
(`FUN_801dd0ac` `param_2 == 7`) and `FUN_801dd864` is called with the attacker as
slot 7, so the attacker element is the **summon body's `+0x1d`** (the namesake
creature's monster element - Gimard's creature, etc.), **not** the caster
character's element and **not** the spell's own `SpellElement` (the spell element
is never read here). The matrix index is the raw `0..=7` element byte, so there is
no separate `SpellElement → index` mapping. A party member's *non-summon* attack
(slot `< 3`) instead uses that member's character-table element. The
enemy element comes from the monster record's **`+0x1D`** byte
([`legaia_asset::monster_archive::MonsterRecord::element`]) - now **pinned by the
`FUN_801dd864` disasm directly** (the record-direct `lbu …,0x1d(record)` read
above), which supersedes the earlier curated-element *correlation* argument as
the mechanism: the affinity scale reaches `MonsterRecord::element` through the
same `0x801C9348` record-pointer table the victory-spoils path uses, so the byte
is consumed by the live game exactly as the parser exposes it. (The correlation
still corroborates the *id labelling* - the four party-table ids reproduce
exactly, water/earth/light/dark corroborate, and the byte takes only `0..=7`
across every populated record.)

**Engine wiring.** The matrix + per-character table load from the same PROT 0898
overlay as the move-power table (`World::element_affinity`), and the monster
special-attack path scales by `matrix[enemy_element][party_member_element]`
(`World::enemy_affinity_pct` → `enemy_move_predamage`): the enemy element from
`MonsterDef::element`, the defender from the active party member's element (the
engine models `char_id == party slot`, so a defender at actor slot *s* is char id
*s+1*). The scale is applied *inside* the roll (`arts_physical_predamage_lazy`),
before the conditional bonus-arm threshold - matching retail's scale→bonus order
(`FUN_801dd864` scale precedes the `FUN_801dd0ac` second arm) - so a non-neutral
affinity can change whether the lazy bonus pair is drawn. The gating is what's
invariant: an uninstalled table resolves to the neutral 100% multiplier (no
scaling), reproducing the no-affinity baseline bit-identically, so disc-free /
synthetic battles keep an unchanged magnitude *and* RNG stream.

The **player→enemy** direction is **also wired** - the same matrix the other way
round, `matrix[summon-creature element][target element]` (attacker = the summon
body's `+0x1d`, defender = the target monster's `+0x1d`). `cast_spell_on_slots`
applies it for a player Seru-magic cast through `World::cast_affinity_pct`: the
attacker element resolves off the summon **creature** - the spell's display name
matched to its namesake `battle_data` record (`World::summon_attacker_element`,
the engine-side equivalent of resolving slot 7's `+0x1d`), *not* the casting
character's element - and the defender element resolves by slot
(`World::battle_slot_element`: party member → per-character table, enemy / summon
body → monster record `+0x1d`). When the catalog resolves the creature, the
percent feeds the **faithful summon roll** (`World::player_summon_predamage`,
see the summon-branch wiring above), applied *inside* the roll before the
bonus-arm threshold exactly like the enemy direction; when only the affinity
tables are present but the creature isn't resolvable, the cast falls back to
the placeholder magnitude with the percent applied post-roll (RNG untouched).
A party member's Tactical Art is *not* a move-power case (it uses the
art-record power byte - see the note under the arts/physical kernel above) and
does not route through this cast path.

## Summon spell XP + magic level-up

Casting Seru magic trains the spell itself. The character record carries a
per-spell-slot u32 **XP array at `+0x8`** (parallel to the spell-id list at
`+0x13D` and the level bytes at `+0x161`), and two retail pieces drive it:

**Accrual - the `FUN_801ddb30` tail** (`overlay_battle_action_801ddb30.txt:1037..1084`,
summon attacker `param_1 == 7` only). Per finisher call (= per hit), with
`damage = *atk - *def` (the final committed delta) against the defender's live
HP (`+0x14C`) and max HP (`+0x14E`), keyed on the summon's target byte
(`+0x1DD`: `< 8` single-target, `8`/`9` group):

```text
if (target_hp < 2)            gain = 0;                       // both branches gate
else if (damage < target_hp)  gain = damage * (single ? 12 : 4) / target_max_hp;
else                          gain = single ? 12 : 4;          // killing hit: flat
xp[spell_slot] += gain;
```

Gates: the per-battle no-reward flag `_DAT_8007BAC0` (the same scripted-fight
flag as the gold gate above) and an unidentified skip `_DAT_8007BDB8`. The
heal-spell arms of `FUN_800402F4` (case-0 tiers 3/4/5: spell ids `0x83`/`0x89`)
accrue into the same array inline.

**Level-up - `FUN_801E70BC`** (`overlay_battle_action_801e70bc.txt`), fired
once per cast at summon return (state `0x36`): finds the cast spell id
(`actor[+0x1DF]`) in the record's id list (search bound `0x20`), then

```text
mult      = (id in {0x86,0x88,0x8D,0x99,0x9B,0xA0}) ? 3 : 2;
threshold = (u16_table[level - 1] * mult) >> 1;     // table at SCUS 0x8007656C
if (level < 9 && threshold < xp)  level += 1;        // strict compare, cap 9
```

The threshold table is 8 ascending u16 steps (levels 1..=8; level 9 is the
cap). The leveled `+0x161` byte is exactly the **magic-power** input of the
next cast's scale stage (`FUN_801dd864`, `apply_magic_power` above) - so the
loop is cast → XP → level → stronger cast.

Engine: kernels `battle_formulas::summon_spell_xp_gain` /
`summon_magic_levels_up`; threshold loader
`engine-core::magic_xp::thresholds_from_scus` (decoded off the user's
`SCUS_942.54`, disc-gated `magic_xp_disc`); live wiring
`World::cast_spell_on_slots` → `World::accrue_summon_spell_xp` (XP persists in
the record's `+0x8` bytes, so it round-trips through saves). The engine
narrows "summon attacker" to the Seru-magic id block its summon path covers
(`0x81..=0x8B`); the evolved-spell ids above that block accrue nothing until
the summon coverage widens.

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
- the `0x20` test (`andi 0x20; bne`) short-circuits before the `0x10` test is
reached. Dump-confirmed at `FUN_801E295C` `0x801E3D0C` (state `0x28`); the same
block recurs in state `0x3C` at `0x801E4568`. Ported verbatim in
`battle_formulas::mp_cost_after_ability_bits` + `MpCostModifier::from_ability_flags`.

`spell_table` is the static `SCUS_942.54` table at `DAT_800754C8` (stats) / `DAT_800754D0` (name pointers) - 12-byte stride, `+3` = MP cost. See [spell-table.md](../formats/spell-table.md) for the full record layout + the pinned player Seru-magic block (`0x81..=0x8b`).

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
| `damage_finish` / `spirit_gauge_fill` (+ `DamageFinish` / `DefenderResist`) | this doc, finisher closed-form stages (`FUN_801ddb30`) |
| `summon_spell_xp_gain` / `summon_magic_levels_up` (+ `summon_magic_level_threshold`) | this doc, [summon spell XP + magic level-up](#summon-spell-xp--magic-level-up) (`FUN_801ddb30` tail / `FUN_801E70BC`) |
| `heal_summon_amount` | this doc, recovery-summon closed form |
| `victory_gold_per_monster` / `victory_gold_finalize` / `victory_exp_per_member` | this doc, victory-spoils gold/EXP scaling (`FUN_8004E568`) |
| `escape_roll` / `escape_party_score` / `escape_enemy_score` (+ `EscapeFlags`) | this doc, [run / escape roll](#run--escape-roll---fun_801e791c) (`FUN_801E791C`) |
| `status_effects::toxic_tick_damage` / `venom_tick_damage` (module `engine-vm::status_effects`) | this doc, [per-round status DoT ticker](#per-round-status-dot-ticker---fun_801e752c) (`FUN_801E752C`) |

The unit tests there pin the documented formulas as fixtures - a future runtime trace can then add comparison cases without touching the formula bodies.

## What's still open

- **Selector dispatch for selectors `0x10..=0x83`.** The cases beyond status / buff / damage handle stat-up animations, status-clear, queue-end markers, and the multi-target item slot used by Smelly Glove etc. They're mostly read-only stat ramps that don't affect game balance, so leaving them un-decoded is fine for a first port.
- The monster record is now fully decoded: all six stat halfwords (see [actor stat block mapping](#actor-stat-block--monster-record-mapping)), the reward fields (see [victory spoils](#victory-spoils-rewards)), and the spell-offset list (see [spell list](#spell-list-record-0x4c)). No record fields remain open. The spell entries' `+0x04`/`+0x08` **effect indices** now resolve through the per-block effect-offset table to the per-spell effect descriptor (`MonsterSpell::effect_offset` / `aux_offset`; see [spell list](#spell-list-record-0x4c)) - these are indices into a table, not direct sub-pointers, and the target is a small fixed descriptor, not TMD geometry. What stays open is only that descriptor's **interior field semantics** (its runtime consumer is the cast/effect path).
- **Ability-bit catalogue.** The ability bitfield at `+0xF4` of the character record has at least the documented MP-half / MP-quarter / HP-cap / MP-cap bits in use, plus the impact-step modifier (`0x10` / `0x20`) on attack actions. The full per-character mapping comes out of save-data (the 0x414 record's `+0xF4..+0xF8` is one row in the save schema's character block) - a few new-game saves with different early-game characters resolve it.

## See also

**Reference** -
[Battle scene](battle.md) ·
[Battle action SM](battle-action.md) ·
[Level-up](level-up.md) ·
[Game-data tables](../reference/gamedata.md)
