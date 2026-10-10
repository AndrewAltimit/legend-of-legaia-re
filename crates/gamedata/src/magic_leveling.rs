//! Magic-leveling tables (per-spell XP curve + damage scaling).
//!
//! Spells level up with use: each cast awards `0..=12` xp (single-target)
//! or `0..=4` per target (multi-target), scaled by what fraction of the
//! target's max HP the cast inflicted. Hitting a level threshold below
//! permanently upgrades the spell. Source: Meth962 v1.10 "Magic Levels
//! & Experience" section.

/// Cumulative XP required to *reach* level `N` (N = 2..=9).
///
/// `XP_TO_LEVEL[0]` = cost of Lv1 -> Lv2 cumulative threshold (= 18).
/// Spells start at Lv1 with 0 xp.
pub const XP_TO_LEVEL: [u32; 8] = [18, 51, 93, 145, 209, 289, 393, 537];

/// Damage / heal scaling multiplier per spell level.
///
/// Lv1 = 1.0 (base). Lv9 = 2.0 (+100%). Stored as basis points
/// (1/100 of a percent) for exact integer math: 10000 = 100%.
///
/// Lv1 entry (10000 = no bonus) is included so the index matches
/// the spell level directly (`SCALING_BP[level - 1]`).
pub const SCALING_BP: [u32; 9] = [
    10000, // Lv1: base 100%
    11248, // Lv2: +12.48%
    12499, // Lv3: +24.99%
    13750, // Lv4: +37.50%
    15000, // Lv5: +50.00%
    16250, // Lv6: +62.50%
    17500, // Lv7: +75.00%
    18750, // Lv8: +87.50%
    20000, // Lv9: +100%
];

/// Vera per-level heal amount (HP restored on cast).
pub const VERA_HEAL: [u32; 9] = [256, 288, 320, 352, 384, 416, 448, 480, 512];

/// Orb per-level heal amount (AoE).
pub const ORB_HEAL: [u32; 9] = [512, 576, 640, 704, 768, 832, 896, 960, 1024];

/// Spoon per-level heal amount (AoE, post-Sol).
pub const SPOON_HEAL: [u32; 9] = [1024, 1152, 1280, 1408, 1536, 1664, 1792, 1920, 2048];

/// Nighto per-level base success chance (%) for its Confuse / Death
/// status roll. Confuse:Death ratio is fixed at 8:1 across all levels.
pub const NIGHTO_HIT_PCT: [u32; 9] = [50, 53, 56, 60, 64, 69, 75, 82, 90];

/// Single-target damage XP award table.
///
/// Returns 0..=12 xp depending on what fraction of the target's max
/// HP the cast inflicted. Buckets are 8.5% / 17.5% / 25% / 33.5% /
/// 42% / 50.5% / 57.5% / 66% / 74.5% / 83% / 91.5% / 100% (with the
/// last bucket also covering kill-shots on partially-damaged
/// targets, for Gimard).
pub fn xp_single_target(hp_fraction_pct: u32) -> u32 {
    const THRESHOLDS: [(u32, u32); 13] = [
        (100, 12),
        (92, 11), // >= 91.5
        (83, 10),
        (75, 9), // >= 74.5
        (66, 8),
        (58, 7), // >= 57.5
        (51, 6), // >= 50.5
        (42, 5),
        (34, 4), // >= 33.5
        (25, 3),
        (18, 2), // >= 17.5
        (9, 1),  // >= 8.5
        (0, 0),
    ];
    for &(threshold, xp) in &THRESHOLDS {
        if hp_fraction_pct >= threshold {
            return xp;
        }
    }
    0
}

/// Multi-target damage XP award (per enemy hit).
pub fn xp_multi_target(hp_fraction_pct: u32) -> u32 {
    match hp_fraction_pct {
        100.. => 4,
        75..=99 => 3, // >= 74.5
        51..=74 => 2, // >= 50.5
        25..=50 => 1, // >= 24.99
        _ => 0,
    }
}
