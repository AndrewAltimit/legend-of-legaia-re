use super::{ImpactStatusProc, enemy_impact_status_proc};
use legaia_art::record::EnemyEffect;

/// `FUN_801E09F8`'s selector ladder covers exactly `3` / `4` / `5`. The
/// Curse arm (`selector 6`, a 1-in-4 `ori 0x1000`) exists only in the
/// sibling arts/melee kernel `FUN_801EC3E4`, so an enemy **special** cannot
/// Curse. Driven with an all-zero RNG, i.e. every probability gate passes -
/// so a `None` here is the ladder's shape, not a failed roll.
#[test]
fn ladder_covers_three_four_five_only() {
    let mut rng = || 0u32;
    assert_eq!(enemy_impact_status_proc(0, true, 0, &mut rng), None);
    assert_eq!(enemy_impact_status_proc(1, true, 0, &mut rng), None);
    assert_eq!(enemy_impact_status_proc(2, true, 0, &mut rng), None);
    assert_eq!(
        enemy_impact_status_proc(3, true, 0, &mut rng),
        Some(ImpactStatusProc {
            effect: EnemyEffect::Other(3),
            rot_limb: None
        })
    );
    assert_eq!(
        enemy_impact_status_proc(4, true, 0, &mut rng),
        Some(ImpactStatusProc {
            effect: EnemyEffect::Other(4),
            rot_limb: None
        })
    );
    assert_eq!(
        enemy_impact_status_proc(5, true, 0, &mut rng),
        Some(ImpactStatusProc {
            effect: EnemyEffect::Other(5),
            rot_limb: Some(0)
        })
    );
    assert_eq!(
        enemy_impact_status_proc(6, true, 0, &mut rng),
        None,
        "no Curse arm in FUN_801E09F8 - that arm is FUN_801EC3E4's"
    );
    assert_eq!(enemy_impact_status_proc(7, true, 0, &mut rng), None);
}

/// The DoT arms gate on `rand & 7 == 0`: exactly one of any eight
/// consecutive RNG values passes, and each arm draws exactly one value.
#[test]
fn dot_arms_gate_on_rand_and_seven() {
    for selector in [3u8, 4] {
        let mut draws = 0usize;
        let landed = (0u32..8)
            .filter(|&v| {
                let mut rng = || {
                    draws += 1;
                    v
                };
                enemy_impact_status_proc(selector, true, 0, &mut rng).is_some()
            })
            .count();
        assert_eq!(
            landed, 1,
            "selector {selector} passes on exactly `rand & 7 == 0`"
        );
        assert_eq!(draws, 8, "one draw per call, no more");
    }
}

/// The Rot arm's limb is `rand % 3`, and the guard bitfield is read
/// *before* the draw - a guarded (or non-party) target consumes no RNG at
/// all, which is what keeps the shared cursor faithful.
#[test]
fn rot_arm_rolls_a_limb_and_guards_draw_nothing() {
    for (v, limb) in [(0u32, 0u8), (1, 1), (2, 2), (3, 0), (7, 1)] {
        let mut rng = || v;
        assert_eq!(
            enemy_impact_status_proc(5, true, 0, &mut rng)
                .and_then(|p| p.rot_limb)
                .unwrap(),
            limb
        );
    }
    for bits in [super::ROT_GUARD_BIT, super::MASTER_GUARD_BIT] {
        let mut draws = 0usize;
        let mut rng = || {
            draws += 1;
            0u32
        };
        assert_eq!(enemy_impact_status_proc(5, true, bits, &mut rng), None);
        assert_eq!(draws, 0, "a guarded target draws no RNG");
    }
    let mut draws = 0usize;
    let mut rng = || {
        draws += 1;
        0u32
    };
    assert_eq!(
        enemy_impact_status_proc(5, false, 0, &mut rng),
        None,
        "selector 5 on a monster target does nothing (retail's `sltiu a1,0x3`)"
    );
    assert_eq!(draws, 0, "a non-party target draws no RNG");
}
