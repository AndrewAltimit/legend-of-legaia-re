//! Noa dance-minigame **step chart** (overlay VA `0x801D509C`).
//!
//! The hit-judge [`FUN_801d1960`] reads the chart symbol for the current beat as
//! `*(byte*)(DAT_801d581c/0x119 + lane*0x20 - 0x7fe2af64)` - the constant
//! `-0x7fe2af64` is `0x801D509C` (the chart base), `DAT_801d581c/0x119` is the
//! **beat index** (beat-clock / period `0x119`), and `lane` is the difficulty
//! row `DAT_801d544c[player] / 1000`. So the chart is a 2-D byte grid
//! `chart[lane * 0x20 + beat]`:
//!
//! * **rows** = difficulty lanes. The groove gauge `DAT_801d544c` clamps to
//!   `[0, 2999]`, so `gauge / 1000` selects row `0`, `1`, or `2`: exactly
//!   [`DANCE_CHART_ROWS`] rows. Higher rows are denser (more steps per bar) and
//!   score more (`FUN_801d1af4`: `+= (lane+1) * k`).
//! * **columns** = [`BEATS_PER_ROW`] (`0x20`) beat slots per row.
//! * **cell** = a direction symbol: `0` = no step, `1`/`2`/`3` = a judged
//!   direction (`FUN_801d4040` maps `1 → 0x80`, `2 → 0x20`, `3 → 0x10` pad bits).
//!
//! ## Provenance - baked overlay data
//!
//! The chart is **baked into the dance overlay's static image** (PROT entry
//! **0980**, base [`DANCE_OVERLAY_BASE_VA`]) at file offset
//! [`DANCE_CHART_FILE_OFFSET`] - not loaded per song into `.bss`. Verified by
//! the region being non-zero in the as-loaded overlay extract and reproducible
//! from the user's `PROT.DAT` (disc-gated `dance_chart_real`). No Sony bytes are
//! committed - the chart decodes from the user's disc.

/// CDNAME / PROT index of the dance minigame overlay.
pub const DANCE_OVERLAY_PROT_INDEX: usize = 980;

/// Load base of the dance overlay (the shared slot-A minigame base).
pub const DANCE_OVERLAY_BASE_VA: u32 = 0x801C_E818;

/// Runtime VA of the step-chart grid base (`0x801D509C`, the `-0x7fe2af64`
/// constant in `FUN_801d1960`).
pub const DANCE_CHART_VA: u32 = 0x801D_509C;

/// File offset of the step chart within the as-loaded overlay image.
pub const DANCE_CHART_FILE_OFFSET: usize = (DANCE_CHART_VA - DANCE_OVERLAY_BASE_VA) as usize;

/// Beat slots per difficulty row (`lane * 0x20 + beat` index math).
pub const BEATS_PER_ROW: usize = 0x20;

/// Difficulty rows: `DAT_801d544c / 1000` over the gauge's `[0, 2999]` clamp.
pub const DANCE_CHART_ROWS: usize = 3;

/// Runtime VA of the **sequence-bonus value table** `DAT_801d41a4` - the points
/// a completed direction sequence awards, indexed
/// `[dancer_kind * 4 + lane]` (`FUN_801d1960`:
/// `DAT_801d41a4 + lane*4 + DAT_801d540c[player]*0x10`, where `DAT_801d540c` is
/// the dancer's **kind** as stamped by the spawner `FUN_801d0190`).
pub const DANCE_BONUS_VA: u32 = 0x801D_41A4;

/// Runtime VA of the **AI triangle schedule** `DAT_801d41e4` - per dancer kind,
/// the number of 4-beat combo slots that must pass before that dancer spends its
/// next "groovy move" (`FUN_801d1820`:
/// `DAT_801d41e4 + tri_cursor*4 + kind*0x40 <= DAT_801d578c[player]`).
pub const DANCE_TRIANGLE_SCHEDULE_VA: u32 = 0x801D_41E4;

/// Dancer kinds the two tables are rowed by (0 = Noa / the human, 1..3 = the
/// competitor dancers; the Disco King (kind 4) never scores).
pub const DANCE_SKILL_ROWS: usize = 4;

/// Bonus-table lanes per kind row (`kind * 0x10` stride / 4-byte words).
pub const DANCE_BONUS_LANES: usize = 4;

/// Triangle-schedule slots per kind row (`kind * 0x40` stride / 4-byte words).
pub const DANCE_SCHEDULE_SLOTS: usize = 16;

/// The decoded step chart: [`DANCE_CHART_ROWS`] rows × [`BEATS_PER_ROW`] beats of
/// direction symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanceChart {
    /// `rows[lane][beat]` = direction symbol (`0` none, `1`/`2`/`3` judged).
    pub rows: Vec<[u8; BEATS_PER_ROW]>,
}

impl DanceChart {
    /// The symbol at `(lane, beat)`, or `None` if out of range.
    pub fn symbol(&self, lane: usize, beat: usize) -> Option<u8> {
        self.rows.get(lane)?.get(beat).copied()
    }

    /// Count of non-zero steps in a row (row density - higher rows are denser).
    pub fn step_count(&self, lane: usize) -> usize {
        self.rows
            .get(lane)
            .map(|r| r.iter().filter(|&&b| b != 0).count())
            .unwrap_or(0)
    }
}

/// Parse the dance step chart out of the as-loaded dance overlay image (PROT
/// entry [`DANCE_OVERLAY_PROT_INDEX`]). Returns `None` if the buffer is too
/// short, or if any cell is not a valid direction symbol (`0..=3`).
pub fn parse(overlay: &[u8]) -> Option<DanceChart> {
    parse_at(overlay, DANCE_CHART_FILE_OFFSET, DANCE_CHART_ROWS)
}

/// Parse `rows` rows of [`BEATS_PER_ROW`] symbols starting at file offset `off`.
pub fn parse_at(overlay: &[u8], off: usize, rows: usize) -> Option<DanceChart> {
    let need = off + rows * BEATS_PER_ROW;
    if overlay.len() < need {
        return None;
    }
    let mut out = Vec::with_capacity(rows);
    for r in 0..rows {
        let base = off + r * BEATS_PER_ROW;
        let mut row = [0u8; BEATS_PER_ROW];
        row.copy_from_slice(&overlay[base..base + BEATS_PER_ROW]);
        if row.iter().any(|&b| b > 3) {
            return None; // not a direction-symbol grid
        }
        out.push(row);
    }
    Some(DanceChart { rows: out })
}

/// The two per-dancer-kind scoring tables that sit just ahead of the chart in
/// the same overlay rodata block.
///
/// Both are rowed by the dancer's **kind** (`DAT_801d540c[slot]`, stamped from
/// the spawn table by `FUN_801d0190`), so they are simultaneously the human's
/// scoring table (kind 0 = Noa) and each AI dancer's - the competitors score
/// through the *same* award routine, only off their own kind's row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DanceScoreTables {
    /// `bonus[kind][lane]` - points a completed direction sequence awards
    /// (`DAT_801d41a4`). Retail rows are `k, 2k, 3k` - the `(lane + 1)` scaling
    /// is baked into the table, not applied by the code.
    pub bonus: Vec<[i32; DANCE_BONUS_LANES]>,
    /// `schedule[kind][n]` - combo slots the dancer must bank before spending
    /// its `n`-th triangle (`DAT_801d41e4`). A huge value means "never".
    pub schedule: Vec<[i32; DANCE_SCHEDULE_SLOTS]>,
}

impl DanceScoreTables {
    /// Sequence-bonus points for `kind` on difficulty `lane` (`0` out of range).
    pub fn bonus(&self, kind: usize, lane: usize) -> i32 {
        self.bonus
            .get(kind)
            .and_then(|r| r.get(lane))
            .copied()
            .unwrap_or(0)
    }

    /// Combo slots `kind` banks before spending its `n`-th triangle. Out of
    /// range (or an exhausted schedule) reports [`i32::MAX`] = never.
    pub fn schedule(&self, kind: usize, n: usize) -> i32 {
        self.schedule
            .get(kind)
            .and_then(|r| r.get(n))
            .copied()
            .unwrap_or(i32::MAX)
    }
}

/// Parse [`DanceScoreTables`] out of the as-loaded dance overlay image (PROT
/// [`DANCE_OVERLAY_PROT_INDEX`]). `None` when the buffer is too short.
pub fn parse_tables(overlay: &[u8]) -> Option<DanceScoreTables> {
    let word = |off: usize| -> Option<i32> {
        Some(i32::from_le_bytes(
            overlay.get(off..off + 4)?.try_into().ok()?,
        ))
    };
    let bonus_base = (DANCE_BONUS_VA - DANCE_OVERLAY_BASE_VA) as usize;
    let sched_base = (DANCE_TRIANGLE_SCHEDULE_VA - DANCE_OVERLAY_BASE_VA) as usize;
    let mut bonus = Vec::with_capacity(DANCE_SKILL_ROWS);
    let mut schedule = Vec::with_capacity(DANCE_SKILL_ROWS);
    for k in 0..DANCE_SKILL_ROWS {
        let mut row = [0i32; DANCE_BONUS_LANES];
        for (lane, cell) in row.iter_mut().enumerate() {
            *cell = word(bonus_base + (k * DANCE_BONUS_LANES + lane) * 4)?;
        }
        bonus.push(row);
        let mut srow = [0i32; DANCE_SCHEDULE_SLOTS];
        for (n, cell) in srow.iter_mut().enumerate() {
            *cell = word(sched_base + (k * DANCE_SCHEDULE_SLOTS + n) * 4)?;
        }
        schedule.push(srow);
    }
    Some(DanceScoreTables { bonus, schedule })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_offset_and_shape() {
        assert_eq!(DANCE_CHART_FILE_OFFSET, 0x6884);
        assert_eq!(BEATS_PER_ROW, 0x20);
        assert_eq!(DANCE_CHART_ROWS, 3);
        // The two scoring tables sit in the same rodata block, one row-stride
        // apart (`0x10` words of bonus per kind, then the schedule).
        assert_eq!(DANCE_BONUS_VA - DANCE_OVERLAY_BASE_VA, 0x598C);
        assert_eq!(DANCE_TRIANGLE_SCHEDULE_VA - DANCE_BONUS_VA, 0x40);
    }

    #[test]
    fn tables_parse_and_index_by_kind() {
        let sched_off = (DANCE_TRIANGLE_SCHEDULE_VA - DANCE_OVERLAY_BASE_VA) as usize;
        let mut buf = vec![0u8; sched_off + DANCE_SKILL_ROWS * DANCE_SCHEDULE_SLOTS * 4];
        let bonus_off = (DANCE_BONUS_VA - DANCE_OVERLAY_BASE_VA) as usize;
        // kind 1, lanes 0..2 = 12 / 24 / 36 (a retail-shaped `k, 2k, 3k` row).
        for (lane, v) in [12i32, 24, 36].into_iter().enumerate() {
            let o = bonus_off + (DANCE_BONUS_LANES + lane) * 4; // kind 1 = row stride
            buf[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        // kind 1's first triangle fires after 8 banked combo slots.
        let o = sched_off + DANCE_SCHEDULE_SLOTS * 4;
        buf[o..o + 4].copy_from_slice(&8i32.to_le_bytes());

        let t = parse_tables(&buf).expect("parses");
        assert_eq!(t.bonus(1, 0), 12);
        assert_eq!(t.bonus(1, 2), 36);
        assert_eq!(t.bonus(0, 0), 0);
        assert_eq!(t.schedule(1, 0), 8);
        assert_eq!(t.schedule(1, 1), 0);
        // Out of range = never.
        assert_eq!(t.schedule(9, 0), i32::MAX);
        assert_eq!(t.bonus(9, 0), 0);
    }

    #[test]
    fn parse_grid_and_density() {
        let off = 0x10;
        let mut buf = vec![0u8; off + 3 * BEATS_PER_ROW];
        // row 0: a step on beats 3, 7. row 1: beats 2,3,10,11. row 2: dense.
        buf[off + 3] = 1;
        buf[off + 7] = 2;
        for &b in &[2usize, 3, 10, 11] {
            buf[off + BEATS_PER_ROW + b] = 1;
        }
        for b in 0..8 {
            buf[off + 2 * BEATS_PER_ROW + b] = 1;
        }
        let chart = parse_at(&buf, off, 3).expect("parses");
        assert_eq!(chart.symbol(0, 3), Some(1));
        assert_eq!(chart.symbol(0, 7), Some(2));
        assert_eq!(chart.step_count(0), 2);
        assert_eq!(chart.step_count(1), 4);
        assert_eq!(chart.step_count(2), 8);
        assert_eq!(chart.symbol(3, 0), None); // out-of-range lane
    }

    #[test]
    fn rejects_non_symbol_bytes() {
        let mut buf = vec![0u8; BEATS_PER_ROW * 3];
        buf[5] = 0x80; // not a 0..=3 direction symbol
        assert!(parse_at(&buf, 0, 3).is_none());
    }
}
