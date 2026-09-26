//! Victory spoils (`FUN_8004E568` gold + EXP) and summon-magic spell XP /
//! level-up arithmetic. Split out of `battle_formulas.rs`.

// ---------------------------------------------------------------------------
// FUN_8004E568 - victory spoils (gold + EXP reward arithmetic)
// ---------------------------------------------------------------------------
//
// The post-battle reward resolver `FUN_8004E568`
// (`ghidra/scripts/funcs/8004e568.txt`) builds the gold and EXP awards from the
// dead enemies' record fields (`+0x44` gold, `+0x46` EXP). Both are scaled - the
// engine must not credit the raw record sums. Pinned arithmetic (decompiled
// block at `8004e568.txt:411..461`):
//
//   gold: acc = Σ (enemy_gold >> 1) over dead enemies;
//         if a living party member carries ability bit 0x10000: acc += acc >> 2;  // +25%
//         credited = acc - (acc >> 1);                                            // halve
//   exp:  per_member = ceil((Σ enemy_exp - (Σ enemy_exp >> 2)) / alive_count);    // ×3/4
//
// The gold path is runtime-confirmed: the lone-enemy Gimard fight (record gold
// 60) credited exactly +15 (`60>>1 = 30`, `30 - (30>>1) = 15`) via a
// write-watchpoint on the party purse `0x8008459C`.
//
// Region note: this is the NTSC-U (`SCUS_942.54`) chain. The JP original
// (`SCPS_100.59`) and the three PAL executables (`SCES_019.44/.45/.46`) run the same accumulate + Golden Book
// steps but have neither the second halving nor the 3/4 EXP cut - a lone enemy
// pays `gold >> 1` and the party splits the whole EXP. The monster records are
// byte-identical across discs, so the difference is code-only. See
// `docs/subsystems/battle-formulas.md` (Regional difference).

/// One dead enemy's contribution to the victory gold accumulator
/// (`FUN_8004E568`, `8004e568.txt:413`): `enemy_gold >> 1` (record `+0x44`).
/// Sum this over every dead enemy, then pass the total to
/// [`victory_gold_finalize`].
pub fn victory_gold_per_monster(enemy_gold: u16) -> u32 {
    (enemy_gold >> 1) as u32
}

/// Finalize the accumulated victory gold (`FUN_8004E568`, `8004e568.txt:435/440`):
/// apply the optional +25% "extra gold" bonus when a living party member carries
/// ability bit `0x10000` (`acc += acc >> 2`), then halve the total (`acc - (acc
/// >> 1)`). With `more_gold == false` and a lone enemy this is the
/// runtime-confirmed Gimard chain `60 -> 30 -> 15` (`floor((gold >> 1) / 2)`).
/// The party-purse cap (`99,999,999`) is applied by the caller, not here.
pub fn victory_gold_finalize(accumulated: u32, more_gold: bool) -> u32 {
    let acc = if more_gold {
        accumulated.saturating_add(accumulated >> 2)
    } else {
        accumulated
    };
    acc - (acc >> 1)
}

/// Per-member EXP from a won battle (`FUN_8004E568`, `8004e568.txt:461`): the
/// summed enemy EXP (record `+0x46`) is scaled by 3/4 (`v - (v >> 2)`) then
/// **ceiling**-divided among the `alive` living, EXP-eligible party members
/// (`(scaled + alive - 1) / alive`). Returns 0 when `alive == 0`.
pub fn victory_exp_per_member(exp_sum: u32, alive: u32) -> u32 {
    if alive == 0 {
        return 0;
    }
    let scaled = exp_sum - (exp_sum >> 2);
    scaled.div_ceil(alive)
}

/// The "Items Up" drop bonus: a living party member whose record `+0xF8`
/// word carries bit `0x20000` adds this many points to every monster's drop
/// chance (`li s4,0x1e` at `0x8004F464`, carried as `s7`).
pub const VICTORY_DROP_ITEMS_UP_BONUS: u32 = 0x1E;

/// One enemy seat as the victory drop roll sees it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VictoryDropSeat {
    /// Record `+0x48` - the drop item id, `0` for none.
    pub item: u8,
    /// Record `+0x49` - the drop chance in percent.
    pub chance_pct: u8,
    /// Actor `+0x227` non-zero - the capture takedown bumps it, and a seat
    /// that carries it cannot supply the drop.
    pub captured: bool,
}

/// The victory drop roll of `FUN_8004E568` (`0x8004F478..0x8004F5A0`).
///
/// Returns the one item id retail offers (`0` = no drop). The caller still
/// owes the held-count check (`FUN_80042F4C(item) == 99` skips the grant,
/// `0x8004F5A8..0x8004F5BC`) and the bag add.
///
/// ```text
/// item = 0; best = 0
/// if no_reward_word == 0:                          // _DAT_8007BAC0, 0x8004F488
///   for seat in monsters:                          // record table 0x801C9348
///     best = max(best, seat.chance)                // 0x8004F4C8
///     if rand() % 100 < seat.chance + bonus        // 0x8004F4D8..0x8004F520
///        and seat.actor[+0x227] == 0:              // 0x8004F53C
///       item = seat.item                           // last winner wins; a zero id clears
/// if best < 100 and bonus == 0:                    // 0x8004F570 / 0x8004F57C
///   if rand() & 3 != 0: item = 0                   // 0x8004F584..0x8004F598
/// ```
///
/// One `rand()` per seat is drawn whether or not the seat can drop anything,
/// and the trailing 1-in-4 draw is taken even in a no-reward battle. `rand`
/// is the BIOS `rand()` stream - a shaped `0..=0x7FFF` value.
///
// REF: FUN_8004E568 (`0x8004F3D8..0x8004F5A0`: the victory drop roll; the
// routine's `PORT:` tag is the module-level one in `battle_formulas.rs`)
pub fn victory_drop_roll(
    seats: &[VictoryDropSeat],
    items_up: bool,
    no_reward: bool,
    mut rand: impl FnMut() -> u32,
) -> u8 {
    let bonus = if items_up {
        VICTORY_DROP_ITEMS_UP_BONUS
    } else {
        0
    };
    let mut item = 0u8;
    let mut best = 0u8;
    if !no_reward {
        for seat in seats {
            best = best.max(seat.chance_pct);
            // `rand` is non-negative, so retail's signed `% 100` (the
            // `0x51EB851F` reciprocal) and this unsigned one agree.
            let r = rand() % 100;
            if r < u32::from(seat.chance_pct) + bonus && !seat.captured {
                item = seat.item;
            }
        }
    }
    if best < 100 && bonus == 0 && rand() & 3 != 0 {
        item = 0;
    }
    item
}

// ---------------------------------------------------------------------------
// Summon-magic spell XP + level-up (FUN_801DDB30 tail / FUN_801E70BC)
// ---------------------------------------------------------------------------
//
// Casting Seru magic trains the spell itself. Two coupled retail pieces:
//
// - The damage finisher `FUN_801ddb30` ends with a spell-XP accrual tail that
//   only runs for the summon attacker (`param_1 == 7`): it finds the cast
//   spell id (`caster_actor + 0x1DF`) in the caster's character-record
//   spell-id list (record `+0x13D`, search bound `0x20`) and adds a
//   damage-proportional gain into the parallel per-spell u32 XP array at
//   record `+0x8` (`overlay_battle_action_801ddb30.txt:1037..1084`).
// - After the summon returns (state `0x36` of `FUN_801E295C`), `FUN_801e70bc`
//   re-finds the slot, reads the spell-level byte (`+0x161` array) and the
//   accrued XP, and levels the spell up when the XP clears a threshold from
//   the static SCUS table at `0x8007656C`
//   (`overlay_battle_action_801e70bc.txt`).
//
// The leveled byte is the **magic-power** stage input of the next cast
// (`FUN_801dd864` reads the same `+0x161` byte - see [`apply_magic_power`]),
// so the loop is: cast → XP → level → stronger cast.
//
// Unmodelled retail gates (documented, intentionally not reproduced): the
// per-battle no-reward flag `_DAT_8007BAC0` (scripted fights skip the accrual,
// same flag battle-formulas.md notes as the unmodelled gold gate) and the
// unidentified accrual skip `_DAT_8007BDB8`.

/// One target's spell-XP gain from a summon hit - PORT: FUN_801ddb30
/// (spell-XP accrual tail, `attacker_slot == 7` only; decompiled block
/// `overlay_battle_action_801ddb30.txt:1049..1084`).
///
/// `damage` is the finisher's final damage delta (`*param_3 - *param_4`),
/// `target_hp`/`target_max_hp` are the defender's live and max HP (actor
/// `+0x14C`/`+0x14E`), `group_target` mirrors the summon actor's target byte
/// (`+0x1DD`): `false` for a single-target cast (`< 8`), `true` for a
/// group-target cast (`8`/`9`).
///
/// Retail arithmetic, exactly:
///
/// - a target with fewer than 2 HP grants nothing (both branches gate on
///   `target_hp >= 2`);
/// - non-killing hit (`damage < target_hp`): gain = `damage * 12 /
///   target_max_hp` single-target, `damage * 4 / target_max_hp` group;
/// - killing hit (`damage >= target_hp`): flat `12` single-target, `4` group.
///
/// A zero `target_max_hp` divides-by-zero in retail (`trap(0x1c00)`); the
/// engine returns 0 instead.
pub fn summon_spell_xp_gain(
    damage: u32,
    target_hp: u16,
    target_max_hp: u16,
    group_target: bool,
) -> u32 {
    if target_hp < 2 {
        return 0;
    }
    let unit: u32 = if group_target { 4 } else { 12 };
    if damage < target_hp as u32 {
        if target_max_hp == 0 {
            return 0;
        }
        (damage * unit) / target_max_hp as u32
    } else {
        unit
    }
}

/// The six spell ids whose level-up threshold is scaled ×1.5 - the explicit
/// `switch` cases of `FUN_801e70bc` (`iVar1 = 3` instead of `2`, halved into
/// `(threshold * mult) >> 1`).
pub const SUMMON_XP_TRIPLE_THRESHOLD_IDS: [u8; 6] = [0x86, 0x88, 0x8D, 0x99, 0x9B, 0xA0];

/// The spell-XP total a spell at `level` must **exceed** to level up -
/// PORT: FUN_801e70bc (battle overlay 0898,
/// `overlay_battle_action_801e70bc.txt`).
///
/// `table` is the static SCUS u16 threshold table at `0x8007656C`, indexed
/// `[level - 1]` (8 ascending entries for levels 1..=8; level 9 is the cap).
/// The retail comparison is `((table[level-1] * mult) >> 1) < xp` with
/// `mult = 3` for the [`SUMMON_XP_TRIPLE_THRESHOLD_IDS`] and `2` otherwise
/// (so the default multiplier is the raw table value - the same compare the
/// heal-spell inline copy in `FUN_800402F4` case-0 tier-4 uses).
///
/// Returns `None` when no level-up is possible: level already at the cap
/// (`level >= 9`, the retail pre-increment guard), level `0` (retail would
/// read `table[-1]`; the engine guards), or `table` too short.
pub fn summon_magic_level_threshold(spell_id: u8, level: u8, table: &[u16]) -> Option<u32> {
    if level == 0 || level >= 9 {
        return None;
    }
    let base = *table.get((level - 1) as usize)? as u32;
    let mult: u32 = if SUMMON_XP_TRIPLE_THRESHOLD_IDS.contains(&spell_id) {
        3
    } else {
        2
    };
    Some((base * mult) >> 1)
}

/// `true` when a spell at `level` with accrued `xp` levels up - the
/// strict-greater compare of `FUN_801e70bc` (`threshold < xp`). The caller
/// applies the level increment (`level += 1`, cap 9) and the UI banner.
/// REF: FUN_801e70bc
pub fn summon_magic_levels_up(spell_id: u8, level: u8, xp: u32, table: &[u16]) -> bool {
    match summon_magic_level_threshold(spell_id, level, table) {
        Some(threshold) => threshold < xp,
        None => false,
    }
}
