//! Round scoring: the combo / health bonus tables and the per-round score.

use super::*;

/// The two per-round score-bonus tables the overlay carries as rodata: the
/// combo-bonus table `&DAT_801d70c4` (20 `i32` slots) and the health-bonus
/// table `&DAT_801d711c` (`i16` slots). They are Sony bytes with no parser in
/// `legaia_minigame_formats::baka_opponents`, so a host that wants the score rows
/// populated reads them out of the overlay image itself; with no tables the
/// rows stay at zero, which is what [`BakaFight`] does by default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BakaScoreTables {
    pub combo_bonus: Vec<i32>,
    pub health_bonus: Vec<i16>,
}

/// Runtime VA of the combo-bonus table `&DAT_801D70C4`.
pub const COMBO_BONUS_TABLE_VA: u32 = 0x801D_70C4;
/// Runtime VA of the health-bonus table `&DAT_801D711C`.
pub const HEALTH_BONUS_TABLE_VA: u32 = 0x801D_711C;
/// Load base of the Baka Fighter overlay (PROT 0976).
pub const BAKA_OVERLAY_BASE_VA: u32 = 0x801C_E818;
/// Rows in the health-bonus table: the index is `hp / `
/// [`BAKA_HEALTH_BONUS_DIVISOR`], and `hp` tops out at [`HP_START`], so
/// `0xC80 / 0x140 = 10` is the last reachable row.
pub const HEALTH_BONUS_ROWS: usize = 11;

impl BakaScoreTables {
    /// Read both bonus tables out of a loaded Baka Fighter overlay image.
    ///
    /// `overlay_0976` is the entry at [`BAKA_OVERLAY_BASE_VA`]. Returns `None`
    /// when the image is too short to hold either table, which is what keeps
    /// the fixed VAs honest on an entry that is not this one.
    ///
    /// The lengths are the index spaces their consumers can produce:
    /// [`BAKA_COMBO_MAX`]` + 1` combo rows and [`HEALTH_BONUS_ROWS`] health
    /// rows. Reading further would walk into the neighbouring rodata, which is
    /// how a table with no terminator gets over-read.
    pub fn from_overlay(overlay_0976: &[u8]) -> Option<Self> {
        let at = |va: u32| -> Option<usize> {
            let off = va.checked_sub(BAKA_OVERLAY_BASE_VA)? as usize;
            (off < overlay_0976.len()).then_some(off)
        };
        let combo_at = at(COMBO_BONUS_TABLE_VA)?;
        let health_at = at(HEALTH_BONUS_TABLE_VA)?;
        let combo_rows = (BAKA_COMBO_MAX + 1) as usize;
        let mut combo_bonus = Vec::with_capacity(combo_rows);
        for i in 0..combo_rows {
            let b = overlay_0976.get(combo_at + i * 4..combo_at + i * 4 + 4)?;
            combo_bonus.push(i32::from_le_bytes(b.try_into().ok()?));
        }
        let mut health_bonus = Vec::with_capacity(HEALTH_BONUS_ROWS);
        for i in 0..HEALTH_BONUS_ROWS {
            let b = overlay_0976.get(health_at + i * 2..health_at + i * 2 + 2)?;
            health_bonus.push(i16::from_le_bytes(b.try_into().ok()?));
        }
        Some(Self {
            combo_bonus,
            health_bonus,
        })
    }
}

/// The combo count is clamped to this before it indexes the combo-bonus table
/// (`FUN_801d2a28`: `if (0x13 < combo) combo = 0x13`).
pub const BAKA_COMBO_MAX: i32 = 0x13;

/// A round that ends with the winner still at full HP ([`HP_START`]) pays this
/// flat perfect-clear bonus instead of a health-scaled one
/// (`FUN_801d2a28`: `if (DAT_801dbfc4 == 0xc80) bonus += 0xc350`).
pub const BAKA_PERFECT_BONUS: i32 = 50_000;

/// The end-of-round HP is divided by this to index the health-bonus table
/// (`FUN_801d2a28`: `DAT_801dbfc4 / 0x140`). [`HP_START`] (`0xc80`) / `0x140`
/// is `10`, so the top table slot is reachable only via the perfect path.
pub const BAKA_HEALTH_BONUS_DIVISOR: i32 = 0x140;

/// The per-round score increment a completed round contributes to the two
/// score rows the end-of-match tally later drains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BakaRoundScore {
    /// Added to the combo-score row (`DAT_801dbed8`).
    pub combo_gain: i32,
    /// Added to the bonus row (`DAT_801dbedc`).
    pub bonus_gain: i32,
}

/// Clamp a raw combo count to the combo-bonus table index space.
///
/// PORT: FUN_801d2a28 (`0x801d2a34..0x801d2a40`). Retail keeps the count when
/// it is below `0x14` and otherwise pins it to [`BAKA_COMBO_MAX`]; the compare
/// is signed, so a (never-produced) negative count passes through unclamped,
/// exactly as the `slti` does.
pub fn baka_combo_index(combo: i32) -> i32 {
    if combo < BAKA_COMBO_MAX + 1 {
        combo
    } else {
        BAKA_COMBO_MAX
    }
}

/// Resolve the two score-row increments a finished round contributes.
///
/// PORT: FUN_801d2a28 (per-round score accumulation).
///
/// The combo input is [`BakaFight::max_combo`], the running maximum
/// `DAT_801dbec8` that the HUD renderer latches each frame from
/// `DAT_801dc094`. That address resolves: the per-fighter hits-taken array is
/// `&DAT_801dbfec[slot * 0x2a]` (an `int` index, so `0xa8` bytes per slot) and
/// `0x801dbfec + 0xa8 == 0x801dc094`, i.e. **slot 1's** consecutive-hits-taken
/// counter - how long a streak the player is currently landing. The two bonus
/// tables are still overlay rodata with no parser, so [`BakaFight`] keeps them
/// optional ([`BakaScoreTables`]): absent, every lookup misses and the rows
/// stay at zero. The retail routine reads
/// the running maximum combo (`DAT_801dbec8`) and the winner's remaining HP
/// (`DAT_801dbfc4`) and folds two increments into the running score rows the
/// end-of-match tally ([`BakaTally`]) later drains:
///
/// - the **combo** row gains `combo_bonus[clamp(combo)]`, indexed by
///   [`baka_combo_index`] into the overlay combo-bonus table
///   (`&DAT_801d70c4`, 20 `i32` slots);
/// - the **bonus** row gains [`BAKA_PERFECT_BONUS`] when the round ended at
///   full HP ([`HP_START`]), else `health_bonus[hp / `[`BAKA_HEALTH_BONUS_DIVISOR`]`]`
///   indexed into the overlay health-bonus table (`&DAT_801d711c`, `i16`
///   slots). The HP divide is the retail signed `/0x140`.
///
/// The two tables are disc data (`FUN_801d2a28`'s overlay), so they are passed
/// in by the caller rather than baked here; the caller supplies the slices it
/// parsed from the Baka Fighter overlay. Out-of-range indices are treated as a
/// zero contribution, which cannot happen with the retail table sizes but
/// keeps the kernel total.
pub fn baka_round_score(
    combo: i32,
    combo_bonus: &[i32],
    end_hp: i32,
    health_bonus: &[i16],
) -> BakaRoundScore {
    let combo_gain = combo_bonus
        .get(baka_combo_index(combo).max(0) as usize)
        .copied()
        .unwrap_or(0);

    let bonus_gain = if end_hp == HP_START {
        BAKA_PERFECT_BONUS
    } else {
        let idx = end_hp / BAKA_HEALTH_BONUS_DIVISOR;
        health_bonus
            .get(idx.max(0) as usize)
            .map(|&v| v as i32)
            .unwrap_or(0)
    };

    BakaRoundScore {
        combo_gain,
        bonus_gain,
    }
}
