//! The progression half of a pack: its XP curve, the EXP and gold
//! multipliers, enemy level scaling and level-up rewards, as kernels over
//! plain numbers.

use legaia_expansion_format::progression::{
    EnemyScaling, LevelReward, MAX_LEVEL, ProgressionSection, ScalingScope, XP_CAP, XpCurve,
};

use crate::battle::scale_permille;

/// A multiplier in thousandths, `1000` when unset. A non-positive or
/// non-finite one reads as `1000` (the validator reports it).
pub fn permille(mul: Option<f64>) -> u32 {
    match mul {
        Some(m) if m.is_finite() && m > 0.0 => (m * 1000.0).round().min(u32::MAX as f64) as u32,
        _ => 1000,
    }
}

/// The cumulative-XP table `curve` makes of the `retail` one (`table[i]` =
/// XP to reach level `i + 2`). Every entry is held to `1..=XP_CAP` and the
/// table rises strictly, so a level is always one more step away.
pub fn xp_table(curve: &XpCurve, retail: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = match curve {
        XpCurve::Retail => return retail.to_vec(),
        XpCurve::Scaled { mul } => {
            let p = permille(Some(*mul));
            retail
                .iter()
                .map(|&x| scale_permille(x, p, XP_CAP))
                .collect()
        }
        XpCurve::Banded { bands } => {
            let mut acc = 0u64;
            let mut prev = 0u32;
            retail
                .iter()
                .enumerate()
                .map(|(i, &x)| {
                    let level = i as u8 + 2;
                    let step = x.saturating_sub(prev);
                    prev = x;
                    let p = bands
                        .iter()
                        .filter(|b| b.from_level <= level)
                        .max_by_key(|b| b.from_level)
                        .map_or(1000, |b| permille(Some(b.mul)));
                    acc += u64::from(scale_permille(step, p, u32::MAX));
                    acc.min(u64::from(XP_CAP)) as u32
                })
                .collect()
        }
        XpCurve::Explicit { levels } => levels.clone(),
    };
    out.truncate(usize::from(MAX_LEVEL) - 1);
    let mut floor = 0u32;
    for x in &mut out {
        *x = (*x).clamp(floor + 1, XP_CAP);
        floor = *x;
    }
    out
}

/// The enemy stat multiplier (thousandths) for a party whose strongest
/// present member is `party_level`; `1000` below the scaling's
/// `from_level`.
pub fn enemy_scaling_permille(s: &EnemyScaling, party_level: u8) -> u32 {
    let over = u32::from(party_level.saturating_sub(s.from_level));
    let per = permille(Some(s.per_level)).min(1_000_000);
    let per = if s.per_level > 0.0 { per } else { 0 };
    let max = permille(Some(s.max_mul)).max(1000);
    (1000 + over.saturating_mul(per)).min(max)
}

/// Whether the scaling reaches a fight: always for a pack formation, and for
/// a retail one only under [`ScalingScope::All`].
pub fn scaling_applies(s: &EnemyScaling, pack_formation: bool) -> bool {
    pack_formation || s.scope == ScalingScope::All
}

/// The items `rewards` grant roster slot `char_slot` for levelling from
/// `old` to `new`, as `(item, count)`.
pub fn level_rewards(
    rewards: &[LevelReward],
    char_slot: u8,
    old: u8,
    new: u8,
) -> Vec<(legaia_expansion_format::items::ItemRef, u8)> {
    rewards
        .iter()
        .filter(|r| r.level > old && r.level <= new)
        .filter(|r| r.character.is_none_or(|c| c == char_slot))
        .flat_map(|r| r.items.iter())
        .filter(|i| i.item.is_item())
        .map(|i| (i.item.clone(), i.count))
        .collect()
}

/// The progression-side runtime state of an active pack. The default is the
/// identity: no saved table, both multipliers `1000`.
#[derive(Debug, Clone)]
pub struct ProgressionRuntime {
    /// The XP table the tracker held before the pack's was installed,
    /// restored on deactivation.
    pub saved_xp_table: Option<Vec<u32>>,
    /// EXP multiplier, thousandths.
    pub xp_permille: u32,
    /// Gold multiplier, thousandths.
    pub gold_permille: u32,
}

impl Default for ProgressionRuntime {
    fn default() -> Self {
        Self {
            saved_xp_table: None,
            xp_permille: 1000,
            gold_permille: 1000,
        }
    }
}

impl ProgressionRuntime {
    /// The runtime for `section`.
    pub fn new(section: &ProgressionSection) -> Self {
        Self {
            saved_xp_table: None,
            xp_permille: permille(section.xp_mul),
            gold_permille: permille(section.gold_mul),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_expansion_format::battle::ItemReward;
    use legaia_expansion_format::progression::XpBand;

    fn retail() -> Vec<u32> {
        legaia_engine_battle::levelup::retail_xp_table()
    }

    #[test]
    fn the_retail_curve_is_the_identity() {
        assert_eq!(xp_table(&XpCurve::Retail, &retail()), retail());
    }

    #[test]
    fn a_scaled_curve_scales_every_level() {
        let t = xp_table(&XpCurve::Scaled { mul: 0.5 }, &retail());
        assert_eq!(t.len(), 98);
        assert_eq!(t[0], 61);
        assert_eq!(t[97], retail()[97].div_ceil(2));
    }

    #[test]
    fn a_banded_curve_keeps_retail_below_its_first_band() {
        let r = retail();
        let t = xp_table(
            &XpCurve::Banded {
                bands: vec![XpBand {
                    from_level: 60,
                    mul: 0.25,
                }],
            },
            &r,
        );
        // Level 59 (index 57) is retail's; level 60 on adds quarter steps.
        assert_eq!(&t[..58], &r[..58]);
        let step = |t: &[u32], i: usize| t[i] - t[i - 1];
        for i in 58..98 {
            let want = (step(&r, i) + 2) / 4;
            assert!(step(&t, i).abs_diff(want) <= 1, "level {}", i + 2);
        }
        assert!(t[97] < r[97] / 2, "level 99 is much nearer");
    }

    #[test]
    fn an_explicit_curve_is_held_rising_and_capped() {
        let t = xp_table(
            &XpCurve::Explicit {
                levels: vec![100, 100, 50, 20_000_000],
            },
            &retail(),
        );
        assert_eq!(t, [100, 101, 102, XP_CAP]);
    }

    #[test]
    fn enemy_scaling_ramps_from_its_level_and_caps() {
        let s = EnemyScaling {
            from_level: 40,
            per_level: 0.05,
            max_mul: 2.0,
            scope: ScalingScope::Pack,
            rewards: false,
        };
        assert_eq!(enemy_scaling_permille(&s, 10), 1000);
        assert_eq!(enemy_scaling_permille(&s, 40), 1000);
        assert_eq!(enemy_scaling_permille(&s, 50), 1500);
        assert_eq!(enemy_scaling_permille(&s, 99), 2000);
        assert!(scaling_applies(&s, true) && !scaling_applies(&s, false));
    }

    fn ids(v: Vec<(legaia_expansion_format::items::ItemRef, u8)>) -> Vec<(u8, u8)> {
        v.into_iter()
            .map(|(r, c)| (r.retail_id().unwrap(), c))
            .collect()
    }

    #[test]
    fn level_rewards_fire_once_on_the_crossing() {
        let item = |id| ItemReward {
            item: legaia_expansion_format::items::ItemRef::Retail(id),
            count: 1,
        };
        let rewards = vec![
            LevelReward {
                level: 50,
                items: vec![item(1)],
                character: None,
            },
            LevelReward {
                level: 60,
                items: vec![item(2)],
                character: Some(1),
            },
        ];
        assert_eq!(ids(level_rewards(&rewards, 0, 48, 52)), [(1, 1)]);
        assert_eq!(ids(level_rewards(&rewards, 0, 50, 52)), []);
        assert_eq!(ids(level_rewards(&rewards, 0, 49, 65)), [(1, 1)]);
        assert_eq!(ids(level_rewards(&rewards, 1, 49, 65)), [(1, 1), (2, 1)]);
    }

    #[test]
    fn multipliers_default_to_the_identity() {
        assert_eq!(permille(None), 1000);
        assert_eq!(permille(Some(-1.0)), 1000);
        assert_eq!(permille(Some(1.5)), 1500);
        let r = ProgressionRuntime::new(&ProgressionSection::default());
        assert_eq!((r.xp_permille, r.gold_permille), (1000, 1000));
    }
}

#[cfg(test)]
mod rebirth {
    use super::*;
    use crate::battle::CompiledBattle;
    use legaia_expansion_format::battle::BattleSection;

    const BATTLE: &str = include_str!("../../../packs/rebirth/battle.toml");
    const PROGRESSION: &str = include_str!("../../../packs/rebirth/progression.toml");

    /// The flagship pack's battle and progression files parse and compile
    /// with no problems.
    #[test]
    fn the_rebirth_sections_compile_clean() {
        let battle: BattleSection = toml::from_str(BATTLE).expect("battle.toml parses");
        let compiled = CompiledBattle::compile("rebirth", &battle).expect("battle.toml compiles");
        assert!(compiled.boss("hollow_sovereign").is_some());
        let prog: ProgressionSection =
            toml::from_str(PROGRESSION).expect("progression.toml parses");
        assert_eq!(prog.problems(), Vec::<String>::new());
    }

    /// Rebirth's curve keeps retail's to level 39 (the story's levels) and
    /// puts level 99 within a fraction of retail's total.
    #[test]
    fn the_rebirth_curve_shortens_the_endgame() {
        let prog: ProgressionSection = toml::from_str(PROGRESSION).unwrap();
        let retail = legaia_engine_battle::levelup::retail_xp_table();
        let t = xp_table(&prog.xp_curve, &retail);
        assert_eq!(&t[..38], &retail[..38]);
        let (r60, r99, t99) = (retail[57], retail[97], t[97]);
        assert!(
            t99 - r60 < (r99 - r60) / 3 + 1,
            "60..99 costs under a third"
        );
    }
}
