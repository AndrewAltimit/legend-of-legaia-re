//! Seru-magic **side-effect stager** (`FUN_801F3D3C`, PROT 0898 own code,
//! file `0x25524`) and the damage finisher's **per-element debuff switch**
//! (`FUN_801DDB30`, `0x801DE60C..0x801DE8EC`) - the pair that gives every
//! levelled Seru spell its secondary effect and that makes a boss look
//! "immune" to ATK-down while SPD-down still lands.
//!
//! The table both halves index is [`legaia_asset::seru_side_effect`]
//! (`0x801F6870`, `[element][band]`, percent `5/10/15/20` by magic-level band);
//! the full mechanism is written up in
//! `docs/subsystems/battle-formulas.md` § "Seru-magic side-effects".
//!
//! ## The stager, read from the disassembly
//!
//! ```text
//!   actor  = *(0x801C9370 + ctx[+0x13]*4)         // the caster
//!   i      = index of actor[+0x1DF] in char[+0x13D..+0x15D]   (0x20 scan)
//!   level  = char[+0x161 + i]
//!   if level < 3: return                          // 0x801F3DA0
//!   sum_el = (*0x801C9358)[+0x1D]                 // summon record element
//!   if ctx[+0x287] && sum_el != 5 && rand() % 5 != 0:
//!       if affinity[sum_el][(*0x801C9348)[+0x1D]] < 0x65: return   // suppressed
//!   switch sum_el:                                // 0x801F3EB4 jump table
//!     0,2,3,4,6: if ctx[+0x287] and target is an enemy slot:
//!                  compare target base halfword vs record field; differ -> return
//!     1:         same compare, unconditionally (AGL base +0x156 vs record +0x0E)
//!     5,7:       no compare
//!   *0x800775B4 = table[sum_el][band].banner     // 0x801F4444..
//!   *0x801F6960 = table[sum_el][band].amount     // the finisher's percent
//!   *0x801F6964 = 0xB4
//!   FUN_801D8DE8(0x66, 0)                         // the "Magic effect" banner
//! ```
//!
//! The compare operands per element (target actor halfword vs raw record):
//!
//! | element | actor base | record | boosted by `FUN_80054CB0` when flagged |
//! |---|---|---|---|
//! | 0 earth | `+0x15E` UDF base | `+0x14` | `x2` |
//! | 1 water | `+0x156` AGL base | `+0x0E` | no |
//! | 2 fire | `+0x15A` ATK base | `+0x12` | `+= ATK>>2` |
//! | 3 wind | `+0x166` SPD base | `+0x1A` | no |
//! | 4 thunder | `+0x16A` INT base | `+0x18` | `+= INT>>3` |
//! | 6 dark | `+0x152` MP base | `+0x10` | no |
//!
//! A single-enemy target (`actor[+0x1DD]` in `3..=6`) is compared itself; a
//! group target (`> 7`) compares the **first living** enemy slot; a party or
//! all-party target (`< 3`, `== 8`) skips the compare.
//!
//! ## The finisher switch
//!
//! `FUN_801DDB30` runs once per hit. On the summon path (`attacker_slot ==
//! 7`) its tail switches on the same summon element and subtracts
//! `stat * pct / 100` (truncating) from the target: both halfwords of the
//! four DEF words (earth), of ATK (fire), SPD (wind), INT (thunder); the AGL
//! **base** only (water); the MP **current** only (dark). A zero percent
//! (nothing staged) subtracts zero, which is how a suppressed cast stays
//! inert without a second gate.
//!
//! ## Wiring
//!
//! NOT WIRED into the live battle loop. What the host lacks, on both sides:
//!
//! - `World` keeps **one live scalar per stat** (`battle_attack`,
//!   `battle_defense_split`, `battle_accuracy`) that buffs write in place -
//!   there is no base halfword to compare against the record, which is the
//!   whole gate. It also keeps no live SPD or AGL scalar for a monster
//!   (`MonsterDef` is read-only at battle time), so two of the six debuffs
//!   have nothing to land on.
//! - The scripted-fight flag (`FormationDef::per_battle_flags`) reaches only
//!   the battle-intro transition; `World` does not retain it, and the enemy
//!   stat seed takes the scripted boost profile in every fight.
//!
//! The kernels below are pure and tested; the pieces above are the ready
//! work named in `docs/reference/open-rev-eng-threads.md`.

use legaia_asset::seru_side_effect::{
    MIN_LEVEL, RESIST_BYPASS_MIN_PCT, SeruSideEffectTable, SideEffectKind, level_band,
};

// REF: FUN_801d8de8 (the banner printer the stager fires with id 0x66)
// REF: FUN_80054cb0 (the boost profiles the compare gate runs against)
// REF: FUN_80056798 (the BIOS rand() the suppression roll draws)

/// The base-halfword / raw-record pair the stager compares for one stat.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatCompare {
    /// The target actor's **base** halfword for the stat.
    pub base: u16,
    /// The raw monster-record field.
    pub record: u16,
}

impl StatCompare {
    /// `true` when the stager's `bne` finds them equal.
    pub fn unchanged(self) -> bool {
        self.base == self.record
    }
}

/// The compare operands of the enemy the stager reads, in table element
/// order of the debuffs it can gate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EnemyCompare {
    /// Earth: UDF base (`+0x15E`) vs record `+0x14`.
    pub udf: StatCompare,
    /// Water: AGL base (`+0x156`) vs record `+0x0E`.
    pub agl: StatCompare,
    /// Fire: ATK base (`+0x15A`) vs record `+0x12`.
    pub atk: StatCompare,
    /// Wind: SPD base (`+0x166`) vs record `+0x1A`.
    pub spd: StatCompare,
    /// Thunder: INT base (`+0x16A`) vs record `+0x18`.
    pub int: StatCompare,
    /// Dark: MP base (`+0x152`) vs record `+0x10`.
    pub mp: StatCompare,
}

/// Who the cast is aimed at, as the stager resolves `actor[+0x1DD]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagerTarget {
    /// A party seat (`< 3`) or the all-party shape (`== 8`): no compare.
    Party,
    /// One enemy seat, or the first living enemy of a group cast.
    Enemy(EnemyCompare),
    /// A group cast with no living enemy: the loop falls through to the
    /// installer tail without comparing.
    NoLivingEnemy,
}

/// Everything the stager reads besides the caster's spell list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagerInputs {
    /// The caster's magic level for the cast spell (`char[+0x161 + i]`).
    pub level: u8,
    /// The summon creature's record element (`(*0x801C9358)[+0x1D]`).
    pub summon_element: u8,
    /// `ctx[+0x287] != 0` - the scripted-fight flag.
    pub scripted: bool,
    /// `affinity[summon_element][first enemy element]` in percent.
    pub affinity_pct: u8,
    /// The cast's target.
    pub target: StagerTarget,
}

/// What the stager decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagerOutcome {
    /// Level `< 3`: nothing staged, no banner of any kind.
    LevelTooLow,
    /// The scripted-fight suppression roll failed.
    Suppressed,
    /// The target's base halfword already differs from the record.
    AlreadyMoved,
    /// Staged: the finisher will apply `amount` percent of `kind` per hit
    /// (or the cure class, for light).
    Staged {
        kind: SideEffectKind,
        amount: u8,
        band: usize,
    },
}

/// Run the stager for one cast. `rand` is drawn **only** when the scripted
/// suppression roll runs (flag set, non-light summon), matching retail's
/// single `FUN_80056798` call on that path; pass the shared BIOS-rand
/// mirror so the stream stays in step.
///
/// PORT: FUN_801f3d3c
pub fn stage(
    table: &SeruSideEffectTable,
    inp: &StagerInputs,
    rand: impl FnOnce() -> i32,
) -> StagerOutcome {
    let Some(band) = level_band(inp.level) else {
        return StagerOutcome::LevelTooLow;
    };
    if inp.scripted && inp.summon_element != 5 {
        let r = rand();
        if r % 5 != 0 && inp.affinity_pct < RESIST_BYPASS_MIN_PCT {
            return StagerOutcome::Suppressed;
        }
    }
    let kind = SideEffectKind::for_element(inp.summon_element);
    if let StagerTarget::Enemy(cmp) = inp.target {
        let pair = match kind {
            SideEffectKind::DefDown if inp.scripted => Some(cmp.udf),
            SideEffectKind::AtkDown if inp.scripted => Some(cmp.atk),
            SideEffectKind::SpdDown if inp.scripted => Some(cmp.spd),
            SideEffectKind::IntDown if inp.scripted => Some(cmp.int),
            SideEffectKind::MpDown if inp.scripted => Some(cmp.mp),
            SideEffectKind::AglDown => Some(cmp.agl),
            _ => None,
        };
        if let Some(pair) = pair
            && !pair.unchanged()
        {
            return StagerOutcome::AlreadyMoved;
        }
    }
    let amount = table.amount(inp.summon_element, inp.level);
    StagerOutcome::Staged { kind, amount, band }
}

/// Whether the return-from-fade pass (`FUN_801F3C34`) prints the
/// "No effect." banner: the spell was levelled enough to carry an effect and
/// nothing is pending.
///
/// PORT: FUN_801f3c34 (the level gate + latch read; the message emit is
/// `crate::move_no_effect_guard::queued_magic_message`)
pub fn no_effect_banner_fires(level: u8, outcome: StagerOutcome) -> bool {
    level >= MIN_LEVEL && !matches!(outcome, StagerOutcome::Staged { .. })
}

/// The target stat halfwords the finisher switch writes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TargetStats {
    /// `+0x15C` / `+0x15E`.
    pub udf: (u16, u16),
    /// `+0x160` / `+0x162`.
    pub ldf: (u16, u16),
    /// `+0x158` / `+0x15A`.
    pub atk: (u16, u16),
    /// `+0x164` / `+0x166`.
    pub spd: (u16, u16),
    /// `+0x168` / `+0x16A`.
    pub int: (u16, u16),
    /// `+0x156` - the AGL base only.
    pub agl_base: u16,
    /// `+0x150` - the MP current only.
    pub mp: u16,
}

/// `stat - (stat * pct) / 100` in the finisher's truncating integer shape.
fn shave(stat: u16, pct: u8) -> u16 {
    let cut = (u32::from(stat) * u32::from(pct)) / 100;
    stat.wrapping_sub(cut as u16)
}

fn shave_pair(pair: &mut (u16, u16), pct: u8) {
    pair.0 = shave(pair.0, pct);
    pair.1 = shave(pair.1, pct);
}

/// Apply one hit's debuff to the target: the finisher's per-element switch
/// with `pct` = the staged amount (`*0x801F6960`). A cure or neutral kind, or
/// a zero percent, changes nothing.
///
/// PORT: FUN_801ddb30 (`0x801DE60C..0x801DE8EC`, the summon-path stat-debuff
/// switch)
pub fn apply_hit(kind: SideEffectKind, pct: u8, stats: &mut TargetStats) {
    if pct == 0 {
        return;
    }
    match kind {
        SideEffectKind::DefDown => {
            shave_pair(&mut stats.udf, pct);
            shave_pair(&mut stats.ldf, pct);
        }
        SideEffectKind::AglDown => stats.agl_base = shave(stats.agl_base, pct),
        SideEffectKind::AtkDown => shave_pair(&mut stats.atk, pct),
        SideEffectKind::SpdDown => shave_pair(&mut stats.spd, pct),
        SideEffectKind::IntDown => shave_pair(&mut stats.int, pct),
        SideEffectKind::MpDown => stats.mp = shave(stats.mp, pct),
        SideEffectKind::Cure | SideEffectKind::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::seru_side_effect::{
        RETAIL_CURE_CLASS_BY_BAND, RETAIL_PERCENT_BY_BAND, SIDE_EFFECT_TABLE_FILE_OFFSET,
    };

    fn table() -> SeruSideEffectTable {
        let mut buf = vec![0u8; SIDE_EFFECT_TABLE_FILE_OFFSET + 7 * 4 * 8];
        for e in 0..7usize {
            for b in 0..4usize {
                let o = SIDE_EFFECT_TABLE_FILE_OFFSET + e * 32 + b * 8;
                buf[o] = if e == 5 {
                    RETAIL_CURE_CLASS_BY_BAND[b]
                } else {
                    RETAIL_PERCENT_BY_BAND[b]
                };
            }
        }
        SeruSideEffectTable::parse(&buf).unwrap()
    }

    fn equal(v: u16) -> StatCompare {
        StatCompare { base: v, record: v }
    }

    /// Gaza as the scripted boost installs him: UDF / ATK / INT moved off
    /// the record, SPD / AGL / MP still equal.
    fn gaza_boosted() -> EnemyCompare {
        EnemyCompare {
            udf: StatCompare {
                base: 444,
                record: 222,
            },
            agl: equal(128),
            atk: StatCompare {
                base: 360,
                record: 288,
            },
            spd: equal(146),
            int: StatCompare {
                base: 247,
                record: 220,
            },
            mp: equal(1200),
        }
    }

    fn inputs(level: u8, element: u8, scripted: bool, target: StagerTarget) -> StagerInputs {
        StagerInputs {
            level,
            summon_element: element,
            scripted,
            affinity_pct: 100,
            target,
        }
    }

    fn no_rand() -> i32 {
        panic!("the roll must not draw on this path")
    }

    #[test]
    fn a_low_level_spell_stages_nothing_and_draws_nothing() {
        let t = table();
        for lvl in 0..MIN_LEVEL {
            let inp = inputs(lvl, 2, true, StagerTarget::Enemy(gaza_boosted()));
            assert_eq!(stage(&t, &inp, no_rand), StagerOutcome::LevelTooLow);
            assert!(!no_effect_banner_fires(lvl, StagerOutcome::LevelTooLow));
        }
    }

    #[test]
    fn a_random_encounter_stages_every_element_without_a_roll() {
        let t = table();
        let cmp = gaza_boosted();
        for (el, kind) in [
            (0u8, SideEffectKind::DefDown),
            (2, SideEffectKind::AtkDown),
            (3, SideEffectKind::SpdDown),
            (4, SideEffectKind::IntDown),
            (6, SideEffectKind::MpDown),
        ] {
            let inp = inputs(5, el, false, StagerTarget::Enemy(cmp));
            assert_eq!(
                stage(&t, &inp, no_rand),
                StagerOutcome::Staged {
                    kind,
                    amount: 10,
                    band: 1
                }
            );
        }
    }

    #[test]
    fn water_compares_the_agl_base_in_every_fight() {
        let t = table();
        let mut cmp = gaza_boosted();
        let inp = inputs(3, 1, false, StagerTarget::Enemy(cmp));
        assert!(matches!(
            stage(&t, &inp, no_rand),
            StagerOutcome::Staged { .. }
        ));
        cmp.agl.base = 121;
        let inp = inputs(3, 1, false, StagerTarget::Enemy(cmp));
        assert_eq!(stage(&t, &inp, no_rand), StagerOutcome::AlreadyMoved);
    }

    #[test]
    fn a_scripted_fight_shrugs_off_the_boosted_stats_and_takes_the_rest() {
        let t = table();
        let cmp = gaza_boosted();
        // Weak-to affinity so the suppression roll is bypassed after its draw.
        let mut inp = inputs(9, 2, true, StagerTarget::Enemy(cmp));
        inp.affinity_pct = 104;
        assert_eq!(stage(&t, &inp, || 1), StagerOutcome::AlreadyMoved);
        inp.summon_element = 0;
        assert_eq!(stage(&t, &inp, || 1), StagerOutcome::AlreadyMoved);
        inp.summon_element = 4;
        assert_eq!(stage(&t, &inp, || 1), StagerOutcome::AlreadyMoved);
        inp.summon_element = 3;
        assert_eq!(
            stage(&t, &inp, || 1),
            StagerOutcome::Staged {
                kind: SideEffectKind::SpdDown,
                amount: 20,
                band: 3
            }
        );
        inp.summon_element = 6;
        assert_eq!(
            stage(&t, &inp, || 1),
            StagerOutcome::Staged {
                kind: SideEffectKind::MpDown,
                amount: 20,
                band: 3
            }
        );
    }

    #[test]
    fn the_scripted_roll_suppresses_four_in_five_unless_weak_or_light() {
        let t = table();
        let inp = inputs(5, 3, true, StagerTarget::Enemy(gaza_boosted()));
        assert_eq!(stage(&t, &inp, || 11), StagerOutcome::Suppressed);
        assert!(matches!(
            stage(&t, &inp, || 10),
            StagerOutcome::Staged { .. }
        ));
        let mut weak = inp;
        weak.affinity_pct = RESIST_BYPASS_MIN_PCT;
        assert!(matches!(
            stage(&t, &weak, || 11),
            StagerOutcome::Staged { .. }
        ));
        // Light skips the roll entirely - no draw.
        let light = inputs(5, 5, true, StagerTarget::Party);
        assert_eq!(
            stage(&t, &light, no_rand),
            StagerOutcome::Staged {
                kind: SideEffectKind::Cure,
                amount: 2,
                band: 1
            }
        );
    }

    #[test]
    fn a_group_cast_with_no_living_enemy_falls_through_to_the_tail() {
        let t = table();
        let inp = inputs(3, 2, true, StagerTarget::NoLivingEnemy);
        let mut weak = inp;
        weak.affinity_pct = 104;
        assert!(matches!(
            stage(&t, &weak, || 1),
            StagerOutcome::Staged { .. }
        ));
    }

    #[test]
    fn the_no_effect_banner_needs_a_levelled_spell_and_nothing_staged() {
        assert!(no_effect_banner_fires(3, StagerOutcome::Suppressed));
        assert!(no_effect_banner_fires(9, StagerOutcome::AlreadyMoved));
        assert!(!no_effect_banner_fires(2, StagerOutcome::LevelTooLow));
        assert!(!no_effect_banner_fires(
            5,
            StagerOutcome::Staged {
                kind: SideEffectKind::AtkDown,
                amount: 10,
                band: 1
            }
        ));
    }

    #[test]
    fn the_finisher_shaves_the_right_halfwords() {
        let mut s = TargetStats {
            udf: (444, 444),
            ldf: (400, 400),
            atk: (360, 360),
            spd: (146, 146),
            int: (247, 247),
            agl_base: 128,
            mp: 1200,
        };
        apply_hit(SideEffectKind::DefDown, 20, &mut s);
        assert_eq!(s.udf, (356, 356));
        assert_eq!(s.ldf, (320, 320));
        apply_hit(SideEffectKind::AtkDown, 5, &mut s);
        assert_eq!(s.atk, (342, 342));
        apply_hit(SideEffectKind::SpdDown, 15, &mut s);
        assert_eq!(s.spd, (125, 125));
        apply_hit(SideEffectKind::IntDown, 10, &mut s);
        assert_eq!(s.int, (223, 223));
        apply_hit(SideEffectKind::AglDown, 10, &mut s);
        assert_eq!(s.agl_base, 116);
        apply_hit(SideEffectKind::MpDown, 20, &mut s);
        assert_eq!(s.mp, 960);
        // Cure / neutral / zero percent touch nothing.
        let before = s;
        apply_hit(SideEffectKind::Cure, 20, &mut s);
        apply_hit(SideEffectKind::None, 20, &mut s);
        apply_hit(SideEffectKind::AtkDown, 0, &mut s);
        assert_eq!(s, before);
    }

    #[test]
    fn repeated_hits_stack_multiplicatively() {
        let mut s = TargetStats {
            atk: (100, 100),
            ..Default::default()
        };
        apply_hit(SideEffectKind::AtkDown, 10, &mut s);
        apply_hit(SideEffectKind::AtkDown, 10, &mut s);
        assert_eq!(s.atk, (81, 81));
    }
}
