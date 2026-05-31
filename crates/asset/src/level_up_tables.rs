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
//! reproduces the captured retail thresholds, e.g. L2 = 365, L3 = 730).
//!
//! The growth tables are parsed both raw ([`growth_tables_from_scus`]) and
//! structured ([`GrowthTables::char_params`]). The parameter block is a
//! per-character record (stride [`GROWTH_PARAM_STRIDE`], one per Vahn / Noa /
//! Gala) of [`GROWTH_STAT_COUNT`] contiguous 6-byte sub-records
//! `{u16 start, u16 max, u8 jitter, u8 row}` — `start` is the character's base
//! (level-1) stat, validated against the new-game starting template
//! ([`crate::new_game`]): **Gala matches the template on all 8 stats**, Vahn/Noa
//! on HP/MP/AGL. `max` is the level-99 ceiling and `row` selects one of the
//! [`GROWTH_ROW_COUNT`] progression curves.
//!
//! The applier's exact per-level gain arithmetic is decoded
//! ([`GrowthTables::level_gain_core`]) but does **not yet reconcile** with the
//! captured multi-level deltas (overshoots ~4.3..4.8x); the engine therefore
//! does not drive level-up from it yet — see `docs/subsystems/level-up.md`
//! § Stat gains.
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
/// growth curves) — `GROWTH_CHAR_COUNT × GROWTH_PARAM_STRIDE`.
pub const GROWTH_PARAM_LEN: usize = 0xB4;

/// Number of party characters with a growth-param record at [`GROWTH_PARAM_VA`]
/// (Vahn / Noa / Gala). The 4th roster slot is never grown by `FUN_801E9504`.
pub const GROWTH_CHAR_COUNT: usize = 3;
/// Per-character growth-param record stride (`0x3C`; `GROWTH_PARAM_LEN = 3 × 0x3C`).
pub const GROWTH_PARAM_STRIDE: usize = 0x3C;
/// Stats per character growth-param record (HP, MP, then six battle stats).
pub const GROWTH_STAT_COUNT: usize = 8;
/// Size of one per-stat growth-param sub-record (`{u16 start, u16 max, u8 jitter, u8 row}`).
pub const GROWTH_SUBRECORD_SIZE: usize = 6;
/// Divisor in `FUN_801E9504`'s per-level gain term: the `0x6F74AE27 >> (32+12)`
/// signed-magic divide reduces to integer division by `0x24C0` (= 9408).
pub const GROWTH_GAIN_DIVISOR: u32 = 0x24C0;

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
/// Use [`Self::char_params`] for the structured per-character view. The exact
/// byte → gain arithmetic ([`Self::level_gain_core`]) is decoded but not yet
/// reconciled with captured deltas — see `docs/subsystems/level-up.md` § Stat
/// gains.
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

/// One per-stat growth-param sub-record from `DAT_80076918` (`FUN_801E9504`).
///
/// Decoded from the contiguous 6-byte layout `{u16 start, u16 max, u8 jitter,
/// u8 row}`. `start` is the character's base (level-1) value for the stat —
/// validated against the new-game starting template ([`crate::new_game`]):
/// Gala's record matches the template on **all 8** stats, Vahn/Noa on HP/MP/AGL
/// (their late-join templates are lightly retuned).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatGrowthParam {
    /// Base (level-1) stat value (matches the new-game template stat).
    pub start: u16,
    /// Level-99 ceiling.
    pub max: u16,
    /// Jitter half-range: the gain term adds `rand() % (2*jitter + 1) - jitter`.
    pub jitter: u8,
    /// Curve-row selector (`0..`[`GROWTH_ROW_COUNT`]) into [`GROWTH_CURVES_VA`].
    pub row: u8,
}

/// The [`GROWTH_STAT_COUNT`]-stat growth-param record for one character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CharGrowthParams {
    /// Per-stat sub-records in record order (HP, MP, then the six battle stats).
    pub stats: [StatGrowthParam; GROWTH_STAT_COUNT],
}

impl GrowthTables {
    /// Decode the [`CharGrowthParams`] for party `slot` (0 = Vahn, 1 = Noa,
    /// 2 = Gala). `None` if `slot >=` [`GROWTH_CHAR_COUNT`] or the block is short.
    pub fn char_params(&self, slot: usize) -> Option<CharGrowthParams> {
        if slot >= GROWTH_CHAR_COUNT {
            return None;
        }
        let base = slot * GROWTH_PARAM_STRIDE;
        let mut stats = [StatGrowthParam::default(); GROWTH_STAT_COUNT];
        for (s, out) in stats.iter_mut().enumerate() {
            let o = base + s * GROWTH_SUBRECORD_SIZE;
            let b = self.param.get(o..o + GROWTH_SUBRECORD_SIZE)?;
            *out = StatGrowthParam {
                start: u16::from_le_bytes([b[0], b[1]]),
                max: u16::from_le_bytes([b[2], b[3]]),
                jitter: b[4],
                row: b[5],
            };
        }
        Some(CharGrowthParams { stats })
    }

    /// The deterministic (jitter-free) per-level gain term `FUN_801E9504`
    /// computes for one stat leveling up **from** `level` (curve indexed by
    /// `level - 1`): `max(1, (max - start) × curve[row][level-1] / 0x24C0)`.
    ///
    /// This is the literal arithmetic of the applier (disassembly
    /// `0x801E97B0..0x801E97F8`: `(max-start) × byte`, the `0x6F74AE27` magic
    /// divide by `0x24C0`, then `+ jitter - jitter_half` floored at 1).
    ///
    /// **Caveat — not yet reconciled.** Summed across the captured multi-level
    /// jumps this OVERSHOOTS the observed per-stat deltas by a *non-constant*
    /// ~4.3..4.8x (Gala HP +44 observed vs core-sum ~188 = 4.27x; Noa HP +32 vs
    /// ~154 = 4.81x), so a factor in the model is still unresolved — the
    /// multi-level captures conflate too much, and the reconciliation needs a
    /// single-level RNG-pinned save pair. Exposed for that investigation; the
    /// engine does **not** drive level-up from it. See
    /// `docs/subsystems/level-up.md` § Stat gains.
    pub fn level_gain_core(&self, p: &StatGrowthParam, level: usize) -> Option<u32> {
        if !(1..MAX_LEVEL).contains(&level) {
            return None;
        }
        let byte = *self.curves.get(p.row as usize)?.get(level - 1)? as u32;
        let span = p.max.saturating_sub(p.start) as u32;
        Some((span * byte / GROWTH_GAIN_DIVISOR).max(1))
    }
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

    #[test]
    fn char_params_decode_contiguous_subrecords() {
        // One char record: 8 stats × 6 bytes {start(u16), max(u16), jitter, row}.
        let mut param = vec![0u8; GROWTH_PARAM_LEN];
        // stat 0: start=180, max=5000, jitter=4, row=0 (Vahn HP shape).
        param[0..6].copy_from_slice(&[0xB4, 0x00, 0x88, 0x13, 0x04, 0x00]);
        // stat 1: start=20, max=900, jitter=1, row=2.
        param[6..12].copy_from_slice(&[0x14, 0x00, 0x84, 0x03, 0x01, 0x02]);
        let g = GrowthTables {
            curves: vec![vec![0u8; GROWTH_ROW_STRIDE]; GROWTH_ROW_COUNT],
            param,
        };
        let cp = g.char_params(0).unwrap();
        assert_eq!(
            cp.stats[0],
            StatGrowthParam {
                start: 180,
                max: 5000,
                jitter: 4,
                row: 0
            }
        );
        assert_eq!(
            cp.stats[1],
            StatGrowthParam {
                start: 20,
                max: 900,
                jitter: 1,
                row: 2
            }
        );
        // Out-of-range slot rejected.
        assert!(g.char_params(GROWTH_CHAR_COUNT).is_none());
    }

    #[test]
    fn level_gain_core_matches_disassembled_arithmetic() {
        let mut curves = vec![vec![0u8; GROWTH_ROW_STRIDE]; GROWTH_ROW_COUNT];
        curves[0][1] = 82; // curve[row0][level-1] for level 2
        let g = GrowthTables {
            curves,
            param: vec![0u8; GROWTH_PARAM_LEN],
        };
        let p = StatGrowthParam {
            start: 150,
            max: 4500,
            jitter: 4,
            row: 0,
        };
        // (4500-150) * 82 / 0x24C0 = 356700 / 9408 = 37.
        assert_eq!(g.level_gain_core(&p, 2), Some(37));
        // Floored at 1 when the span × byte term is zero.
        let z = StatGrowthParam {
            start: 100,
            max: 100,
            jitter: 0,
            row: 0,
        };
        assert_eq!(g.level_gain_core(&z, 2), Some(1));
        // No growth past the level cap.
        assert_eq!(g.level_gain_core(&p, MAX_LEVEL), None);
    }
}
