//! Retail cumulative-XP threshold table (and the test placeholder table).
//!
//! Extracted verbatim from `levelup.rs`.

#[cfg(test)]
use super::MAX_LEVEL;

/// Cumulative XP thresholds for levels 2..=`MAX_LEVEL`.
///
/// `table[i]` = total XP required to reach level `i + 2` (from level 1).
/// Prefix-summed from the 98 u16 values below.
///
/// **These values are FABRICATED, not retail XP.** They are a 98-entry slice
/// of the GTE sin LUT (`sin[0x408..0x46A]`) that an earlier pass mis-read as an
/// XP table after an off-by-`0x800` confusion (file `0x6123C` vs virtual
/// `0x80070A3C`); the bytes are rotation-LUT data the `RotMatrixX/Y/Z`
/// (`0x800461A4 / 0x8004629C / 0x8004638C`) and cutscene-camera (`FUN_8001CF50`)
/// builders consume, not levelling data.
///
/// The real retail curve is the static-SCUS per-level u16 delta table
/// `DAT_80076AF4`, read by the overlay level-up applier `FUN_801E9504` (called
/// from `FUN_8004E568` at `0x8004F34C`): the running sum to the current level is
/// scaled `(sum × 9_999_999) / 0x140FE` for `level < 0x11` (else `sum × 0x79`)
/// and compared `≤ record cumulative XP`. The engine installs that real curve at
/// boot (`legaia_asset::level_up_tables::xp_thresholds_from_scus` →
/// `BootSession`); this placeholder is the fallback when no `SCUS_942.54` is
/// reachable (disc-less tests), retained only so the tracker has a curve shape.
///
/// [`LevelUpTracker::default`] uses this table. See
/// [`docs/subsystems/level-up.md`](https://github.com/altimit-mii/legend-of-legaia-re/blob/main/docs/subsystems/level-up.md#xp-table)
/// for the full write-up.
pub fn retail_xp_table() -> Vec<u32> {
    // 98 u16 values that are a sin-LUT slice (SCUS file offset 0x61A3C / virtual
    // 0x80070A3C = sin LUT base 0x80070A2C + 0x10), NOT retail XP. Placeholder
    // only - the real curve is DAT_80076AF4 + formula (see docstring).
    const INCREMENTS: [u16; 98] = [
        50, 56, 62, 69, 75, 81, 87, 94, 100, 106, 113, 119, 125, 131, 138, 144, 150, 157, 163, 169,
        175, 182, 188, 194, 200, 207, 213, 219, 226, 232, 238, 244, 251, 257, 263, 269, 276, 282,
        288, 295, 301, 307, 313, 320, 326, 332, 338, 345, 351, 357, 363, 370, 376, 382, 388, 395,
        401, 407, 413, 420, 426, 432, 438, 445, 451, 457, 463, 470, 476, 482, 488, 495, 501, 507,
        513, 520, 526, 532, 538, 545, 551, 557, 563, 569, 576, 582, 588, 594, 601, 607, 613, 619,
        625, 632, 638, 644, 650, 656,
    ];
    let mut cumulative = Vec::with_capacity(INCREMENTS.len());
    let mut total: u32 = 0;
    for &inc in &INCREMENTS {
        total += u32::from(inc);
        cumulative.push(total);
    }
    cumulative
}

/// Geometric `100 × n²` approximation - used only in unit tests that need
/// fixed threshold values independent of the retail data.
#[cfg(test)]
pub fn placeholder_xp_table() -> Vec<u32> {
    (1u32..MAX_LEVEL as u32).map(|n| 100 * n * n).collect()
}
