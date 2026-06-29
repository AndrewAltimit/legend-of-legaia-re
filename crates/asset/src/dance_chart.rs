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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_offset_and_shape() {
        assert_eq!(DANCE_CHART_FILE_OFFSET, 0x6884);
        assert_eq!(BEATS_PER_ROW, 0x20);
        assert_eq!(DANCE_CHART_ROWS, 3);
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
