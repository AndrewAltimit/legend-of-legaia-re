//! Seru-magic **side-effect table** (battle-action overlay, PROT 0898, runtime
//! VA `0x801F6870`) and the roster-side susceptibility kernel built on it.
//!
//! Every player Seru-magic cast whose summon module calls the side-effect
//! stager `FUN_801F3D3C` carries a secondary effect keyed on the **summon
//! creature's element** (the slot-7 record's `+0x1D`, the same byte the
//! affinity scale reads) and scaled by the caster's **magic level** for that
//! spell (character record `+0x161` array, the parallel of the `+0x13D` id
//! list): a stat debuff on the target for the six damaging elements, a cure
//! class for light. The stager selects an 8-byte record out of this table at
//! `[element][band]` (`sll v0,v0,0x5` / `(level - 3) >> 1` at
//! `0x801F4420..0x801F4440`), writes byte `0` to `0x801F6960` and the word at
//! `+4` (a banner-string pointer) to `0x800775B4`; the damage finisher
//! `FUN_801DDB30` then reads `0x801F6960` as a **percent** in its per-element
//! `switch` (`0x801DE60C..`) and subtracts `stat * pct / 100` from the
//! target's stat halfwords on every hit.
//!
//! ```text
//! element  kind            band 0 (lv 3-4)  band 1 (5-6)  band 2 (7-8)  band 3 (9)
//! 0 earth  DEF down        5%               10%           15%           20%
//! 1 water  AGL down        5%               10%           15%           20%
//! 2 fire   ATK down        5%               10%           15%           20%
//! 3 wind   SPD down        5%               10%           15%           20%
//! 4 thunder INT down       5%               10%           15%           20%
//! 5 light  cure class      1                2             3             4
//! 6 dark   MP down         5%               10%           15%           20%
//! 7 neutral (no row - the bytes past the table are code)
//! ```
//!
//! Levels `1..=2` stage nothing at all (`slti`/early return at the head of
//! the stager) - a low-level spell has no side effect and prints no banner.
//!
//! ## Which enemies the debuff can touch
//!
//! There is **no per-monster immunity table** anywhere in the record or the
//! overlays (the monster record's `+0x24..+0x43` tail is zero across the
//! roster). What reads as immunity is structural, and it depends on the
//! **scripted-fight flag** `ctx[+0x287]` (bit `0x80` of `DAT_8007BD60`,
//! raised by a formation row whose header byte is non-zero - the boss / story
//! rows - see `docs/formats/encounter.md`):
//!
//! - **Flag clear (random encounters).** No resist roll and, for earth / fire
//!   / wind / thunder / dark, no compare gate: the debuff lands on every hit
//!   and stacks. Only water (AGL) compares the target's AGL base halfword
//!   against the record and so lands once per battle.
//! - **Flag set (scripted fights).** First an 80% suppression roll unless the
//!   spell is light or the affinity `matrix[summon][first enemy]` is
//!   `>= 101` (thunder against the four base elements, or the opposite
//!   element). Then the stager compares the target's **base** stat halfword
//!   against the raw record value and stages nothing when they differ. The
//!   battle loader has already applied the scripted-fight boost profile to
//!   ATK (`x5/4`), UDF/LDF (`x2`) and INT (`x9/8`) - so those three debuffs
//!   can never land (unless the boost added nothing: ATK `< 4`, INT `< 8`,
//!   UDF `0`), while SPD and AGL (unboosted) land exactly once and MP (the
//!   compare reads the untouched base half) lands on every hit.
//!
//! That asymmetry is what a player sees as "this boss shrugs off ATK-down but
//! SPD-down works": [`Susceptibility::for_record`] computes it per record.
//!
//! Provenance: `ghidra/scripts/funcs/overlay_muscle_dome_801f3d3c.txt` (the
//! stager; the file is named for a capture whose extraction window runs into
//! the 0898 bytes - the entry is 0898's own code, see the note in
//! `crate::battle_camera_table`), `overlay_battle_action_801ddb30.txt` (the
//! finisher switch), `80054cb0.txt` (the boost profiles), and the two
//! save-state pins in `docs/subsystems/battle-formulas.md`.

use serde::Serialize;

/// CDNAME / PROT index of the battle-action overlay holding the table.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize = 898;

/// The battle-action overlay's link/load base (`VA − file_offset`).
pub const OVERLAY_LINK_BASE: u32 = 0x801C_E818;

/// Runtime VA of the `[element][band]` record table.
pub const SIDE_EFFECT_TABLE_VA: u32 = 0x801F_6870;

/// Raw PROT 0898 file offset of the table (= `VA − OVERLAY_LINK_BASE`).
pub const SIDE_EFFECT_TABLE_FILE_OFFSET: usize = 0x28058;

/// Element rows the table carries (`0..=6`; element `7` has no row).
pub const SIDE_EFFECT_ELEMENTS: usize = 7;

/// Level bands per element (`(level - 3) >> 1` for levels `3..=9`).
pub const SIDE_EFFECT_BANDS: usize = 4;

/// Bytes per `[element][band]` record: `u8 amount`, 3 pad, `u32 banner_ptr`.
pub const SIDE_EFFECT_RECORD_STRIDE: usize = 8;

/// Lowest magic level that stages a side effect at all.
pub const MIN_LEVEL: u8 = 3;

/// Affinity percent at or above which a scripted fight skips the 80%
/// suppression roll (`0x65` = 101).
pub const RESIST_BYPASS_MIN_PCT: u8 = 0x65;

/// Level band for a magic level: `(level - 3) >> 1`, clamped to the table.
/// `None` below [`MIN_LEVEL`].
pub fn level_band(level: u8) -> Option<usize> {
    if level < MIN_LEVEL {
        return None;
    }
    Some((((level - MIN_LEVEL) >> 1) as usize).min(SIDE_EFFECT_BANDS - 1))
}

/// What one summon element does to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectKind {
    /// Earth: both defence facets (UDF + LDF, working + base halfwords).
    DefDown,
    /// Water: the AGL action-gauge **base** halfword (`+0x156`).
    AglDown,
    /// Fire: ATK (working + base).
    AtkDown,
    /// Wind: SPD (working + base).
    SpdDown,
    /// Thunder: INT (working + base).
    IntDown,
    /// Light: a cure class `1..=4` for party targets - not a debuff.
    Cure,
    /// Dark: current MP (`+0x150`).
    MpDown,
    /// Neutral: no row, no effect.
    None,
}

impl SideEffectKind {
    /// The kind a summon element byte (`0..=7`) selects.
    pub fn for_element(element: u8) -> SideEffectKind {
        match element {
            0 => SideEffectKind::DefDown,
            1 => SideEffectKind::AglDown,
            2 => SideEffectKind::AtkDown,
            3 => SideEffectKind::SpdDown,
            4 => SideEffectKind::IntDown,
            5 => SideEffectKind::Cure,
            6 => SideEffectKind::MpDown,
            _ => SideEffectKind::None,
        }
    }

    /// The summon element that selects this kind, or `None` for
    /// [`SideEffectKind::None`].
    pub fn element(self) -> Option<u8> {
        match self {
            SideEffectKind::DefDown => Some(0),
            SideEffectKind::AglDown => Some(1),
            SideEffectKind::AtkDown => Some(2),
            SideEffectKind::SpdDown => Some(3),
            SideEffectKind::IntDown => Some(4),
            SideEffectKind::Cure => Some(5),
            SideEffectKind::MpDown => Some(6),
            SideEffectKind::None => None,
        }
    }

    /// The six stat debuffs, in the table's element order.
    pub const DEBUFFS: [SideEffectKind; 6] = [
        SideEffectKind::DefDown,
        SideEffectKind::AglDown,
        SideEffectKind::AtkDown,
        SideEffectKind::SpdDown,
        SideEffectKind::IntDown,
        SideEffectKind::MpDown,
    ];

    /// Short stat label (`"DEF"`, `"AGL"`, ...), `"cure"` for light.
    pub fn label(self) -> &'static str {
        match self {
            SideEffectKind::DefDown => "DEF",
            SideEffectKind::AglDown => "AGL",
            SideEffectKind::AtkDown => "ATK",
            SideEffectKind::SpdDown => "SPD",
            SideEffectKind::IntDown => "INT",
            SideEffectKind::Cure => "cure",
            SideEffectKind::MpDown => "MP",
            SideEffectKind::None => "-",
        }
    }
}

/// One `[element][band]` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SideEffectRecord {
    /// Byte `0`: the debuff percent (`5/10/15/20`) or, on the light row, the
    /// cure class (`1..=4`).
    pub amount: u8,
    /// Word `+4`: runtime VA of the banner string the reader half
    /// (`FUN_801F3C34`) / the stager installs at `0x800775B4`.
    pub banner_va: u32,
}

/// The parsed table: seven element rows of four level bands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SeruSideEffectTable {
    rows: [[SideEffectRecord; SIDE_EFFECT_BANDS]; SIDE_EFFECT_ELEMENTS],
}

impl SeruSideEffectTable {
    /// Parse the table out of the raw PROT 0898 entry bytes. `None` when the
    /// buffer is too short to be that overlay.
    pub fn parse(prot_0898: &[u8]) -> Option<SeruSideEffectTable> {
        let end = SIDE_EFFECT_TABLE_FILE_OFFSET
            + SIDE_EFFECT_ELEMENTS * SIDE_EFFECT_BANDS * SIDE_EFFECT_RECORD_STRIDE;
        if prot_0898.len() < end {
            return None;
        }
        let mut rows = [[SideEffectRecord {
            amount: 0,
            banner_va: 0,
        }; SIDE_EFFECT_BANDS]; SIDE_EFFECT_ELEMENTS];
        for (e, row) in rows.iter_mut().enumerate() {
            for (b, rec) in row.iter_mut().enumerate() {
                let o = SIDE_EFFECT_TABLE_FILE_OFFSET
                    + e * SIDE_EFFECT_BANDS * SIDE_EFFECT_RECORD_STRIDE
                    + b * SIDE_EFFECT_RECORD_STRIDE;
                rec.amount = prot_0898[o];
                rec.banner_va = u32::from_le_bytes([
                    prot_0898[o + 4],
                    prot_0898[o + 5],
                    prot_0898[o + 6],
                    prot_0898[o + 7],
                ]);
            }
        }
        Some(SeruSideEffectTable { rows })
    }

    /// The record a summon element (`0..=6`) and magic level select, or
    /// `None` below [`MIN_LEVEL`] / for element `7`.
    pub fn record(&self, element: u8, level: u8) -> Option<SideEffectRecord> {
        let band = level_band(level)?;
        self.rows.get(element as usize).map(|r| r[band])
    }

    /// The debuff percent (or cure class) for an element and level; `0`
    /// when nothing is staged.
    pub fn amount(&self, element: u8, level: u8) -> u8 {
        self.record(element, level).map(|r| r.amount).unwrap_or(0)
    }

    /// All seven rows in element order.
    pub fn rows(&self) -> &[[SideEffectRecord; SIDE_EFFECT_BANDS]; SIDE_EFFECT_ELEMENTS] {
        &self.rows
    }
}

/// The retail percent ladder by band - what every damaging row of the
/// shipped table carries. A parser-free consumer (the site's explainer, a
/// synthetic battle) can use this without the overlay bytes.
pub const RETAIL_PERCENT_BY_BAND: [u8; SIDE_EFFECT_BANDS] = [5, 10, 15, 20];

/// The retail cure-class ladder by band on the light row.
pub const RETAIL_CURE_CLASS_BY_BAND: [u8; SIDE_EFFECT_BANDS] = [1, 2, 3, 4];

/// How often one debuff can land on one enemy in one fight class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Susceptibility {
    /// Lands on every hit (and stacks).
    EveryHit,
    /// Lands once per battle - the stager's base-vs-record compare passes
    /// the first time and never again.
    Once,
    /// Never lands - the loader's boost already moved the base halfword off
    /// the record value, so the compare fails before the first cast.
    Never,
}

impl Susceptibility {
    /// Per-debuff verdicts for one monster record, in
    /// [`SideEffectKind::DEBUFFS`] order (DEF, AGL, ATK, SPD, INT, MP).
    ///
    /// `stats` is the raw record block in `MonsterRecord::stats` order
    /// (`[AGL, ATK, UDF, LDF, INT, SPD]`); `scripted` is the fight class
    /// (`ctx[+0x287]` set). The 80% suppression roll a scripted fight runs
    /// first is orthogonal to these verdicts - see
    /// [`resist_roll_bypassed`].
    ///
    /// The scripted arms mirror the stager's compare against the boost
    /// profile `FUN_80054CB0` installs when the flag is set: a boosted stat
    /// whose boost term is zero still equals the record, so a tiny stat keeps
    /// a one-shot window.
    pub fn for_record(stats: &[u16; 6], scripted: bool) -> [Susceptibility; 6] {
        let [agl, atk, udf, _ldf, int, _spd] = *stats;
        let _ = agl;
        if !scripted {
            // Flag clear: no compare for the five `+0x287`-gated arms; water
            // (AGL) compares unconditionally and moves the base it compares.
            return [
                Susceptibility::EveryHit, // DEF
                Susceptibility::Once,     // AGL
                Susceptibility::EveryHit, // ATK
                Susceptibility::EveryHit, // SPD
                Susceptibility::EveryHit, // INT
                Susceptibility::EveryHit, // MP
            ];
        }
        // Flag set: base halfword vs record. The scripted boost is
        // UDF x2 / ATK += ATK>>2 / INT += INT>>3; SPD, AGL and MP are copied
        // unchanged.
        let once_if = |unchanged: bool| {
            if unchanged {
                Susceptibility::Once
            } else {
                Susceptibility::Never
            }
        };
        [
            once_if(udf == 0),        // DEF: x2 leaves only 0 unchanged
            Susceptibility::Once,     // AGL: unboosted base, moved by the hit
            once_if(atk >> 2 == 0),   // ATK: += ATK>>2
            Susceptibility::Once,     // SPD: unboosted base, moved by the hit
            once_if(int >> 3 == 0),   // INT: += INT>>3
            Susceptibility::EveryHit, // MP: compare reads the base half, the hit moves the current half
        ]
    }
}

/// Whether a scripted fight's 80% suppression roll is skipped for a summon
/// element against the fight's first enemy element, given the affinity
/// percent `matrix[summon][enemy]` (`crate::element_affinity`). Light
/// (element `5`) always skips it; otherwise the percent must reach
/// [`RESIST_BYPASS_MIN_PCT`].
pub fn resist_roll_bypassed(summon_element: u8, affinity_pct: u8) -> bool {
    summon_element == 5 || affinity_pct >= RESIST_BYPASS_MIN_PCT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_overlay() -> Vec<u8> {
        let mut buf = vec![0u8; SIDE_EFFECT_TABLE_FILE_OFFSET + 7 * 4 * 8];
        for e in 0..7usize {
            for b in 0..4usize {
                let o = SIDE_EFFECT_TABLE_FILE_OFFSET + e * 32 + b * 8;
                buf[o] = if e == 5 {
                    RETAIL_CURE_CLASS_BY_BAND[b]
                } else {
                    RETAIL_PERCENT_BY_BAND[b]
                };
                let va = 0x801C_F000u32 + (e as u32) * 0x100 + (b as u32) * 0x24;
                buf[o + 4..o + 8].copy_from_slice(&va.to_le_bytes());
            }
        }
        buf
    }

    #[test]
    fn bands_pair_levels_and_gate_below_three() {
        assert_eq!(level_band(0), None);
        assert_eq!(level_band(2), None);
        assert_eq!(level_band(3), Some(0));
        assert_eq!(level_band(4), Some(0));
        assert_eq!(level_band(5), Some(1));
        assert_eq!(level_band(8), Some(2));
        assert_eq!(level_band(9), Some(3));
        // A level past 9 cannot occur; it stays inside the row.
        assert_eq!(level_band(12), Some(3));
    }

    #[test]
    fn parse_reads_amount_and_banner_per_record() {
        let t = SeruSideEffectTable::parse(&synthetic_overlay()).expect("parses");
        assert_eq!(t.amount(2, 3), 5);
        assert_eq!(t.amount(2, 6), 10);
        assert_eq!(t.amount(3, 9), 20);
        assert_eq!(t.amount(5, 7), 3);
        assert_eq!(t.amount(6, 1), 0);
        assert_eq!(t.amount(7, 9), 0);
        assert_eq!(
            t.record(1, 5).unwrap().banner_va,
            0x801C_F000 + 0x100 + 0x24
        );
        assert!(SeruSideEffectTable::parse(&[0u8; 16]).is_none());
    }

    #[test]
    fn kinds_follow_the_element_byte() {
        assert_eq!(SideEffectKind::for_element(0), SideEffectKind::DefDown);
        assert_eq!(SideEffectKind::for_element(6), SideEffectKind::MpDown);
        assert_eq!(SideEffectKind::for_element(7), SideEffectKind::None);
        for k in SideEffectKind::DEBUFFS {
            assert_eq!(SideEffectKind::for_element(k.element().unwrap()), k);
        }
    }

    #[test]
    fn random_encounters_land_everything_but_agl_stacks() {
        let v = Susceptibility::for_record(&[128, 288, 222, 200, 220, 146], false);
        assert_eq!(
            v,
            [
                Susceptibility::EveryHit,
                Susceptibility::Once,
                Susceptibility::EveryHit,
                Susceptibility::EveryHit,
                Susceptibility::EveryHit,
                Susceptibility::EveryHit,
            ]
        );
    }

    #[test]
    fn scripted_fights_shrug_off_the_boosted_stats() {
        // Gaza: every boosted stat moves, so DEF / ATK / INT never land.
        let v = Susceptibility::for_record(&[128, 288, 222, 200, 220, 146], true);
        assert_eq!(
            v,
            [
                Susceptibility::Never,
                Susceptibility::Once,
                Susceptibility::Never,
                Susceptibility::Once,
                Susceptibility::Never,
                Susceptibility::EveryHit,
            ]
        );
    }

    #[test]
    fn a_stat_the_boost_cannot_move_keeps_a_one_shot_window() {
        // ATK 3 (ATK>>2 == 0), INT 7 (INT>>3 == 0), UDF 0.
        let v = Susceptibility::for_record(&[10, 3, 0, 5, 7, 20], true);
        assert_eq!(v[0], Susceptibility::Once);
        assert_eq!(v[2], Susceptibility::Once);
        assert_eq!(v[4], Susceptibility::Once);
        // One notch higher and each is boosted off the record.
        let v = Susceptibility::for_record(&[10, 4, 1, 5, 8, 20], true);
        assert_eq!(v[0], Susceptibility::Never);
        assert_eq!(v[2], Susceptibility::Never);
        assert_eq!(v[4], Susceptibility::Never);
    }

    #[test]
    fn resist_bypass_needs_101_or_light() {
        assert!(resist_roll_bypassed(5, 0));
        assert!(resist_roll_bypassed(4, 102));
        assert!(resist_roll_bypassed(2, 104));
        assert!(!resist_roll_bypassed(2, 100));
        assert!(!resist_roll_bypassed(2, 96));
    }
}
