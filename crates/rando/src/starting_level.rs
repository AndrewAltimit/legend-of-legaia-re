//! Starting-level randomization: begin a New Game with the lead character
//! already at a chosen level instead of level 1.
//!
//! A vanilla New Game seeds Vahn at level 1. The seed routine `FUN_800560B4`
//! initialises these progression fields per roster record (offsets relative to the
//! `0x414`-byte record at `0x80084708 + n*0x414`, pinned from live captures + the
//! cheat database — see [`legaia_asset::new_game`]):
//!
//! - **`+0x0`** — current cumulative experience (the "Max Exp" cheat target). Left
//!   `0` at a New Game. **The displayed combat level is derived from this value**, so
//!   it is the field that actually sets the level.
//! - **`+0x4`** — the *next-level* XP threshold (what the status screen labels
//!   "next"). Seeded per character to its level-1→2 threshold (Vahn `121`, Noa `102`,
//!   Gala `140`). This is the literal at
//!   [`legaia_asset::new_game::STARTING_XP_SEED_VA`].
//! - **`+0x130`** — the magic-rank counter (it ticks `+1` per level-up *event*, not
//!   per level — across a captured 4-level jump it rose by only `+1`). NOT the level;
//!   the randomizer leaves it alone. (`+0x100` stays zero and is unrelated.)
//!
//! Stats come from the starting-party template
//! ([`legaia_asset::new_game::PARTY_TEMPLATE_VA`], level-1 values), copied into the
//! live record by the same routine.
//!
//! An earlier version of this randomizer mistook `+0x4` for cumulative XP and wrote
//! a level-`N` XP value *there* — so the value showed up as the "next level" readout
//! while the experience cell `+0x0` stayed `0`, leaving the derived level at `1`
//! (only the template stats, written correctly, looked like level `N`). This version
//! makes the start coherent at level `N` with same-size in-place `SCUS_942.54` edits:
//!
//! 1. **Current experience** — seed slot 0's `+0x0` to an in-band level-`N` value
//!    (the midpoint of `reach(N)..reach(N+1)`, so it is unambiguously inside level
//!    `N`'s band whichever comparison the level derivation uses). This is what makes
//!    the displayed level `N`.
//! 2. **Next threshold** — set `+0x4` to `reach(N+1)`, the real disc XP threshold to
//!    advance out of level `N`, so the "next" readout is correct and a single battle
//!    can't trigger a runaway level-up cascade.
//! 3. **Stats** — overwrite slot 0's eight `u16` template stats with the level-`N`
//!    values, accumulating the deterministic (jitter-free) per-level growth gains
//!    from `FUN_801E9504`'s curves ([`legaia_asset::level_up_tables`]) on top of the
//!    level-1 template.
//!
//! Both XP values are single `addiu` 16-bit immediates, which caps the level at
//! [`MAX_STARTING_LEVEL`] (where `reach(N+1)` still fits a positive `imm16`).
//!
//! Only party slot 0 (the character present at a New Game) is seeded; the
//! repurposed stores cost slots 1 and 3 (Noa / Terra) their seeded `+0x4` threshold,
//! but those characters re-scale when they join, so the loss is never observed.

use anyhow::{Context, Result, bail};
use legaia_asset::level_up_tables::{growth_tables_from_scus, xp_thresholds_from_scus};
use legaia_asset::new_game::{STAT_COUNT, StartingParty};

/// Default starting level the toggle seeds when enabled without an explicit
/// value. A modest head start that skips the earliest grind without
/// trivializing the opening.
pub const DEFAULT_STARTING_LEVEL: u8 = 10;

/// Lowest level the toggle accepts. Level 1 is the vanilla start (a no-op), so
/// the randomizer only acts from level 2 up.
pub const MIN_STARTING_LEVEL: u8 = 2;

/// Highest level the toggle accepts. The XP seeds are single `addiu` immediates
/// (16-bit), and the largest one written — the next-level threshold `reach(N+1)`
/// — stays within a positive `imm16` (`<= 0x7FFF`) through level 14 (`reach(15)`
/// = 32370); beyond that the literal would need a second instruction the
/// surrounding code has no room for.
pub const MAX_STARTING_LEVEL: u8 = 14;

/// A resolved starting-level plan: the XP literals to seed and the level-`N`
/// stats to write into slot 0's template, in template order
/// (`hp, mp, agl, atk, udf, ldf, spd, int`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartingLevelPlan {
    /// The chosen level (`MIN_STARTING_LEVEL..=MAX_STARTING_LEVEL`).
    pub level: u8,
    /// In-band cumulative experience for level `N` (the midpoint of
    /// `reach(N)..reach(N+1)`) — written to the record's current-experience cell
    /// `+0x0`, the field the displayed level derives from (fits a positive `imm16`).
    pub current_xp: u16,
    /// Cumulative XP to reach level `N+1` — the next-level threshold written to
    /// the record's `+0x4` cell (fits a positive `imm16`).
    pub next_threshold: u16,
    /// The level-`N` stats for slot 0, in template order.
    pub stats: [u16; STAT_COUNT],
}

/// `true` when `level` requests a non-vanilla starting level the randomizer can
/// seed (i.e. `MIN_STARTING_LEVEL..=MAX_STARTING_LEVEL`). Level 0 or 1 is the
/// vanilla start, so callers treat it as "off".
pub fn is_active(level: u8) -> bool {
    (MIN_STARTING_LEVEL..=MAX_STARTING_LEVEL).contains(&level)
}

/// Build the [`StartingLevelPlan`] for `level` from a `SCUS_942.54` image.
///
/// Reads the level-1 starting-party template, the per-level XP thresholds, and
/// the stat-growth curves straight out of the executable (no committed Sony
/// bytes), then computes the level-`N` XP seed and stats for party slot 0.
/// Errors if `level` is out of range or the image is missing the tables.
pub fn plan(scus: &[u8], level: u8) -> Result<StartingLevelPlan> {
    if !is_active(level) {
        bail!(
            "starting level {level} out of range ({}..={})",
            MIN_STARTING_LEVEL,
            MAX_STARTING_LEVEL
        );
    }
    let n = level as usize;

    // Level-N XP band from the real disc thresholds. `thresholds[k]` is the total
    // XP to reach level `k + 2`, so the XP to reach level `m` is `thresholds[m - 2]`.
    // The displayed level derives from the record's current experience (+0x0): seed
    // it to the midpoint of the band so it is unambiguously inside level N (above
    // `reach(N)`, below `reach(N+1)`) regardless of the exact `<`/`<=` comparison the
    // derivation uses; seed `+0x4` to `reach(N+1)` so the "next" readout is right and
    // experience stays below the threshold (no spurious level-up on the first frame).
    let thresholds =
        xp_thresholds_from_scus(scus).context("read XP thresholds from SCUS_942.54")?;
    let reach_n = thresholds
        .get(n - 2)
        .copied()
        .with_context(|| format!("XP threshold for level {level} out of range"))?;
    let reach_n1 = thresholds
        .get(n - 1)
        .copied()
        .with_context(|| format!("XP threshold for level {} out of range", level + 1))?;
    let as_imm16 = |xp: u32, what: &str| -> Result<u16> {
        u16::try_from(xp)
            .ok()
            .filter(|&v| v <= 0x7FFF)
            .with_context(|| {
                format!("level {level} {what} {xp} does not fit a positive 16-bit immediate")
            })
    };
    // Midpoint sits strictly between the two thresholds (they are distinct +
    // increasing), so it is unambiguously inside level N's band.
    let current_xp = as_imm16(
        reach_n + (reach_n1 - reach_n) / 2,
        "current-experience seed",
    )?;
    let next_threshold = as_imm16(reach_n1, "next-level threshold")?;

    // Level-N stats: accumulate the deterministic per-level growth gains on top
    // of the level-1 template. Growth slot 0 = Vahn; its 8 sub-records are in the
    // same order as the template stats (hp, mp, agl, atk, udf, ldf, spd, int).
    let template = StartingParty::from_scus(scus)
        .context("decode starting-party template from SCUS_942.54")?;
    let vahn = template
        .member(0)
        .context("starting-party template has no slot 0")?;
    let base = [
        vahn.hp_max,
        vahn.mp_max,
        vahn.agl,
        vahn.atk,
        vahn.udf,
        vahn.ldf,
        vahn.spd,
        vahn.intel,
    ];
    let growth =
        growth_tables_from_scus(scus).context("read stat-growth tables from SCUS_942.54")?;
    let params = growth
        .char_params(0)
        .context("growth tables have no slot 0 (Vahn)")?;

    let mut stats = [0u16; STAT_COUNT];
    for (s, out) in stats.iter_mut().enumerate() {
        let p = &params.stats[s];
        let mut val = base[s] as u32;
        // Gains applied leveling from L to L+1, for L = 1..N-1 (reaching level N).
        for l in 1..n {
            val += growth.level_gain_core(p, l).unwrap_or(1);
        }
        // Never exceed the stat's level-99 ceiling (or the u16 range).
        *out = val.min(p.max as u32).min(u16::MAX as u32) as u16;
    }

    Ok(StartingLevelPlan {
        level,
        current_xp,
        next_threshold,
        stats,
    })
}

/// Encode the next-level threshold into the 4 bytes of the `addiu $v0, $zero, imm`
/// literal at [`legaia_asset::new_game::STARTING_XP_SEED_VA`] (the `+0x4` seed). The
/// instruction is `0x2402_0000 | imm16`; the whole word is returned so the patcher
/// writes a complete instruction.
pub fn next_threshold_instruction(next_threshold: u16) -> [u8; 4] {
    (0x2402_0000u32 | next_threshold as u32).to_le_bytes()
}

/// Encode the current-experience preload `addiu $t0, $zero, imm` written at
/// [`legaia_asset::new_game::CURRENT_XP_PRELOAD_VA`] (repurposing the slot-3 / Terra
/// `+0x4` store). `$t0` then holds the value the [`current_xp_store_instruction`]
/// writes to slot 0's `+0x0` cumulative-experience cell. `$t0` = register 8, so the
/// opcode/rt nibble is `0x2408`.
pub fn current_xp_preload_instruction(current_xp: u16) -> [u8; 4] {
    (0x2408_0000u32 | current_xp as u32).to_le_bytes()
}

/// The fixed `sw $t0, 0x5c8($s0)` instruction written at
/// [`legaia_asset::new_game::CURRENT_XP_STORE_VA`] — stores the preloaded experience
/// value (`$t0`) to party slot 0's `+0x0` cumulative-experience cell (replacing the
/// vanilla slot-1 / Noa `addiu $v0, $zero, 0x66` threshold literal). `$s0` is the SC
/// base, so slot-0 record `+0x0` is at `$s0 + 0x5c8`.
pub fn current_xp_store_instruction() -> [u8; 4] {
    0xAE08_05C8u32.to_le_bytes()
}

/// Encode the loop's level literal `addiu $v0, $zero, (1 << 8) | level` written at
/// [`legaia_asset::new_game::LEVEL_SEED_VA`]. Packed so the [`level_store_instruction`]
/// `sh` sets the record's displayed-level cell `+0x130 = level` (low byte) and the
/// magic-rank cell `+0x131 = 1` (high byte) in one halfword store.
pub fn level_literal_instruction(level: u8) -> [u8; 4] {
    (0x2402_0000u32 | 0x0100u32 | level as u32).to_le_bytes()
}

/// The fixed `sh $v0, 0x6f8($s0)` instruction written at
/// [`legaia_asset::new_game::LEVEL_STORE_VA`] — stores the packed `[level, 1]`
/// halfword to the record's `+0x130`/`+0x131` cells (replacing the vanilla
/// `sb $v0, 0x6f9($s0)`).
pub fn level_store_instruction() -> [u8; 4] {
    0xA602_06F8u32.to_le_bytes()
}

/// A `nop`, written at [`legaia_asset::new_game::LEVEL_STORE_REDUNDANT_VA`] — the
/// vanilla second level store, made redundant by the `sh` above.
pub fn nop_instruction() -> [u8; 4] {
    [0; 4]
}

/// Encode the level-`N` stats into the 16 bytes of slot 0's template stat block
/// (`STAT_COUNT` little-endian `u16`s, in template order).
pub fn stat_block(stats: &[u16; STAT_COUNT]) -> [u8; STAT_COUNT * 2] {
    let mut out = [0u8; STAT_COUNT * 2];
    for (i, s) in stats.iter().enumerate() {
        out[i * 2..i * 2 + 2].copy_from_slice(&s.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_predicate() {
        assert!(!is_active(0));
        assert!(!is_active(1));
        assert!(is_active(MIN_STARTING_LEVEL));
        assert!(is_active(DEFAULT_STARTING_LEVEL));
        assert!(is_active(MAX_STARTING_LEVEL));
        assert!(!is_active(MAX_STARTING_LEVEL + 1));
    }

    #[test]
    fn next_threshold_instruction_keeps_the_opcode() {
        // addiu $v0, $zero, 0x258d  (a representative level-10-ish threshold)
        assert_eq!(
            next_threshold_instruction(0x258d),
            0x2402_258du32.to_le_bytes()
        );
        // The opcode/rt nibble is preserved for any immediate.
        for imm in [0u16, 1, 0x79, 0x2bbb, 0x7fff] {
            let w = u32::from_le_bytes(next_threshold_instruction(imm));
            assert_eq!(w >> 16, 0x2402, "addiu $v0, $zero stays intact");
            assert_eq!(w & 0xffff, imm as u32);
        }
    }

    #[test]
    fn current_xp_preload_targets_t0() {
        // addiu $t0, $zero, imm  (rt = register 8 = $t0).
        for imm in [0u16, 1, 0x2589, 0x7fff] {
            let w = u32::from_le_bytes(current_xp_preload_instruction(imm));
            assert_eq!(w >> 16, 0x2408, "addiu $t0, $zero stays intact");
            assert_eq!(w & 0xffff, imm as u32);
        }
    }

    #[test]
    fn current_xp_store_encoding() {
        // sw $t0, 0x5c8($s0): opcode 0x2b, base 16 ($s0), rt 8 ($t0), record +0x0.
        let sw = u32::from_le_bytes(current_xp_store_instruction());
        assert_eq!(sw >> 26, 0x2b, "sw opcode");
        assert_eq!((sw >> 21) & 0x1f, 16, "base = $s0");
        assert_eq!((sw >> 16) & 0x1f, 8, "rt = $t0");
        assert_eq!(sw & 0xffff, 0x5c8, "record +0x0");
    }

    #[test]
    fn stat_block_is_little_endian_in_order() {
        let stats = [0x0102u16, 0x0304, 0, 0, 0, 0, 0, 0x0a0b];
        let b = stat_block(&stats);
        assert_eq!(&b[0..2], &[0x02, 0x01]);
        assert_eq!(&b[2..4], &[0x04, 0x03]);
        assert_eq!(&b[14..16], &[0x0b, 0x0a]);
    }

    #[test]
    fn plan_rejects_out_of_range_levels() {
        // No SCUS needed: the range guard fires first.
        assert!(plan(&[], 1).is_err());
        assert!(plan(&[], MAX_STARTING_LEVEL + 1).is_err());
    }
}
