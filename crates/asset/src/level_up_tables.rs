//! Level-up curves from `SCUS_942.54`.
//!
//! The retail level-up applier `FUN_801E9504` (overlay-resident, called from the
//! battle reward resolver `FUN_8004E568` at `0x8004F34C`) reads three static
//! `SCUS_942.54` tables:
//!
//! - **`DAT_80076AF4`** — the per-level XP-delta table (u16 LE). The XP-to-next-
//!   level threshold for a character at `level` is the running sum of the first
//!   `level` deltas, scaled by the formula below.
//! - **`DAT_800769CC`** — three 0x62-byte (`= MAX_LEVEL − 1`) per-stat growth
//!   curves, indexed by level.
//! - **`DAT_80076918`** — a parameter block whose entries select which growth
//!   curve each stat reads.
//!
//! The XP threshold derivation is ported and validated ([`xp_thresholds_from_scus`]
//! reproduces the captured retail thresholds, e.g. L2 = 365, L3 = 730). The
//! growth curves + parameter block are parsed as raw bytes ([`growth_tables_from_scus`])
//! but the exact byte → per-stat-gain mapping is not yet applied — see
//! `docs/subsystems/level-up.md` § Stat gains.
//!
//! No `SCUS_942.54` bytes are committed; callers pass an image read from the
//! user's disc at runtime.
//!
//! PORT: FUN_801E9504 (XP-threshold derivation)

/// Max character level (levels run 1..=99).
pub const MAX_LEVEL: usize = 99;

/// RAM address of the per-level XP-delta table (u16 LE), `DAT_80076AF4`.
pub const XP_DELTA_VA: u32 = 0x8007_6AF4;
/// RAM address of the per-stat growth curves, `DAT_800769CC`.
pub const GROWTH_CURVES_VA: u32 = 0x8007_69CC;
/// RAM address of the growth parameter block, `DAT_80076918`.
pub const GROWTH_PARAM_VA: u32 = 0x8007_6918;

/// XP-threshold formula numerator (`FUN_801E9504`, level < [`XP_FORMULA_SWITCH_LEVEL`]).
pub const XP_SCALE_NUM: u64 = 9_999_999;
/// XP-threshold formula denominator (`0x140FE`).
pub const XP_SCALE_DEN: u64 = 0x1_40FE;
/// XP-threshold multiplier for `level >=` [`XP_FORMULA_SWITCH_LEVEL`] (`0x79`).
pub const XP_LATE_MULT: u64 = 0x79;
/// Level at/after which the formula switches from the scaled form to `× 0x79`.
pub const XP_FORMULA_SWITCH_LEVEL: usize = 0x11;

/// Per-stat growth-curve byte stride (`0x62 = MAX_LEVEL − 1`).
pub const GROWTH_ROW_STRIDE: usize = 0x62;
/// Number of distinct growth curves at [`GROWTH_CURVES_VA`].
pub const GROWTH_ROW_COUNT: usize = 3;
/// Size (bytes) of the parameter block at [`GROWTH_PARAM_VA`] (gap to the
/// growth curves).
pub const GROWTH_PARAM_LEN: usize = 0xB4;

/// PSX-EXE `t_addr` -> file-offset resolver. `SCUS_942.54` loads its data
/// segment at `t_addr` from file offset `0x800`. (Kept local, matching the
/// resolvers in [`crate::new_game`] / [`crate::item_names`].)
struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

/// Read a u16 LE at a virtual address.
fn read_u16(scus: &[u8], map: &ExeMap, va: u32) -> Option<u16> {
    let o = map.off(va)?;
    Some(u16::from_le_bytes(scus.get(o..o + 2)?.try_into().ok()?))
}

/// Apply the retail XP-threshold formula to a running delta-sum at `level`.
///
/// `cum` is `Σ DAT_80076AF4[0..level]`. Returns the total XP required to advance
/// from `level` to `level + 1`. Mirrors `FUN_801E9504` (`0x801E95D0`–`0x801E9624`):
/// integer `(cum × 9_999_999) / 0x140FE` for `level < 0x11`, else `cum × 0x79`.
pub fn xp_threshold_for(level: usize, cum: u64) -> u32 {
    let t = if level < XP_FORMULA_SWITCH_LEVEL {
        cum * XP_SCALE_NUM / XP_SCALE_DEN
    } else {
        cum * XP_LATE_MULT
    };
    t as u32
}

/// Parse the cumulative XP-to-next-level thresholds out of a `SCUS_942.54` image.
///
/// Returns `MAX_LEVEL − 1 = 98` entries where `table[level - 1]` is the total XP
/// required to reach `level + 1` (so `table[0]` = XP to reach L2). This is the
/// shape `engine_core::levelup::LevelUpTracker::with_xp_table` consumes.
///
/// `None` if the image isn't a `SCUS_942.54` (no `PS-X EXE` header / table out
/// of range).
pub fn xp_thresholds_from_scus(scus: &[u8]) -> Option<Vec<u32>> {
    let map = ExeMap::parse(scus)?;
    let mut thresholds = Vec::with_capacity(MAX_LEVEL - 1);
    let mut cum: u64 = 0;
    for i in 0..(MAX_LEVEL - 1) {
        let delta = read_u16(scus, &map, XP_DELTA_VA + (i as u32) * 2)? as u64;
        cum += delta;
        let level = i + 1; // threshold(level) = total XP to reach level + 1
        thresholds.push(xp_threshold_for(level, cum));
    }
    Some(thresholds)
}

/// Raw per-level stat-growth tables (`DAT_800769CC` curves + `DAT_80076918`
/// parameter block).
///
/// The byte → per-stat-gain mapping is not yet decoded into engine stat gains;
/// this exposes the raw data so a future port can apply it. See
/// `docs/subsystems/level-up.md` § Stat gains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrowthTables {
    /// The [`GROWTH_ROW_COUNT`] growth curves, each [`GROWTH_ROW_STRIDE`] bytes
    /// (indexed by level).
    pub curves: Vec<Vec<u8>>,
    /// The raw parameter block ([`GROWTH_PARAM_LEN`] bytes) that selects which
    /// curve each stat reads.
    pub param: Vec<u8>,
}

/// Parse the raw growth curves + parameter block out of a `SCUS_942.54` image.
pub fn growth_tables_from_scus(scus: &[u8]) -> Option<GrowthTables> {
    let map = ExeMap::parse(scus)?;
    let curves_base = map.off(GROWTH_CURVES_VA)?;
    let mut curves = Vec::with_capacity(GROWTH_ROW_COUNT);
    for r in 0..GROWTH_ROW_COUNT {
        let o = curves_base + r * GROWTH_ROW_STRIDE;
        curves.push(scus.get(o..o + GROWTH_ROW_STRIDE)?.to_vec());
    }
    let param_base = map.off(GROWTH_PARAM_VA)?;
    let param = scus
        .get(param_base..param_base + GROWTH_PARAM_LEN)?
        .to_vec();
    Some(GrowthTables { curves, param })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal synthetic `SCUS_942.54` with a known XP-delta table so the
    /// formula + offset math are exercised without disc data.
    fn synth_scus(deltas: &[u16]) -> Vec<u8> {
        const T_ADDR: u32 = 0x8001_0000;
        let xp_off = (XP_DELTA_VA - T_ADDR) as usize + 0x800;
        let total = xp_off + deltas.len() * 2 + 16;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        buf[0x1C..0x20].copy_from_slice(&((total - 0x800) as u32).to_le_bytes());
        for (i, &d) in deltas.iter().enumerate() {
            let o = xp_off + i * 2;
            buf[o..o + 2].copy_from_slice(&d.to_le_bytes());
        }
        buf
    }

    #[test]
    fn formula_reproduces_captured_thresholds() {
        // The first deltas of the real DAT_80076AF4 ramp: 1, 2, 3, ...
        // cum(1)=1 -> 121, cum(2)=3 -> 365, cum(3)=6 -> 730 (captured retail).
        assert_eq!(xp_threshold_for(1, 1), 121);
        assert_eq!(xp_threshold_for(2, 3), 365);
        assert_eq!(xp_threshold_for(3, 6), 730);
        // Late-level branch (level >= 0x11) switches to × 0x79.
        assert_eq!(xp_threshold_for(0x11, 100), 100 * 0x79);
    }

    #[test]
    fn thresholds_from_synth_scus() {
        let deltas: Vec<u16> = (0..(MAX_LEVEL - 1) as u16).map(|_| 0).collect();
        let mut deltas = deltas;
        deltas[0] = 1;
        deltas[1] = 2;
        deltas[2] = 3;
        let scus = synth_scus(&deltas);
        let t = xp_thresholds_from_scus(&scus).unwrap();
        assert_eq!(t.len(), MAX_LEVEL - 1);
        assert_eq!(&t[0..3], &[121, 365, 730]);
    }

    #[test]
    fn non_scus_returns_none() {
        assert!(xp_thresholds_from_scus(b"not an exe").is_none());
        assert!(growth_tables_from_scus(&[0u8; 16]).is_none());
    }
}
