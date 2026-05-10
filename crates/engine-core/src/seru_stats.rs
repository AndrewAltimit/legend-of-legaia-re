//! Per-Seru stat-grant table - the disc-side data backing per-character
//! level-up curves.
//!
//! ## Retail layout
//!
//! Each captured character record (stride `0x414`, base `0x80084708` per
//! `legaia_save::SaveFile` analysis) holds per-stat fields at byte offsets
//! `+0x10E` (Spirit-max), `+0x11C..+0x126` (six u16 stats at 2-byte stride),
//! `+0x130` (rank counter), `+0x161..` (per-spell levels). A magic-rank-up
//! and character-level-up save triplet for Vahn pinned the destination
//! offsets ([`crate::levelup::observations::vahn_4_level_jump`]).
//!
//! The retail growth values themselves do *not* live in the level-up overlay's
//! data section - a writer-search across the captured `overlay_magic_level_up_*`
//! dumps for `sb` / `sh` writes targeting `+0x10E`, `+0x11C..+0x12C`, `+0x130`,
//! `+0x161` returns no code-side hits.
//!
//! A follow-up grep for any `lh` / `lhu` / `lb` / `lbu` / `lw` read at
//! `+0x74(reg)` across the same overlay surfaces five hits - but every one of
//! them is reading a 32-bit battle-state flag that gets *written* with the
//! constant `0x80808080` by the SCUS-side battle-actor handler `FUN_800480D8`
//! (`lui v0, 0x80; ori v0, v0, 0x8080; sw v0, 0x74(s0)`), not a stat-grant
//! pointer. The "Seru struct +0x74" pointer-dereference hypothesis is not
//! supported by the captured code; either the table base lives in a
//! still-uncaptured overlay (the most likely candidate is the battle-data
//! init path that loads PROT entries 0x05C4 + sibling Seru blobs at boot) or
//! the grant is computed inline from a packed Seru-record field that the
//! current capture set doesn't surface.
//!
//! Engines wiring this module today should treat the shipped values as
//! placeholders and override per-Seru with [`SeruStatTable::insert`]. The
//! pinned destination offsets ([`crate::levelup::observations::vahn_4_level_jump`])
//! still describe the *consumer* layout faithfully, so any future capture
//! that pins the *source* table can drop into the existing API without
//! re-shaping it.
//!
//! ## What this module provides
//!
//! Two layered APIs:
//!
//! 1. [`SeruStatGrant`] - the clean-room shape of one Seru's grant: HP, MP,
//!    Spirit, and the six u16 record-stat byte deltas. Engines populate this
//!    from a [`crate::levelup::LevelUpObservation`] (averaging across the
//!    observed range) until a true per-level vector is captured.
//! 2. [`SeruStatTable`] - id-keyed map of grants. Engines compose one per
//!    character from the Seru roster the player has equipped, sum the per-
//!    Seru grants, and feed the resulting [`crate::levelup::StatGrowthCurve::PerLevel`]
//!    into [`crate::levelup::LevelUpTracker::with_stat_curves`].
//!
//! Both APIs are clean-room: no on-disc bytes, no decompiled values. The
//! shipped `vanilla_*` constructors below are placeholder pre-balance values
//! roughly matching the retail Vahn / Noa / Gala curves the user sees during
//! the early game (verified by the legacy `vahn_4_level_jump` capture).

use crate::levelup::{StatGain, StatGrowthCurve};
use std::collections::HashMap;

/// One Seru's per-level stat grant.
///
/// Field offsets refer to the *destination* on the consuming character record
/// (`hp_max` at the typed-accessor location, `sp_max` at `+0x10E`, the six
/// u16 stat fields at `+0x11C..+0x126`). The grant is the *delta* applied
/// when the Seru's effective level increments by one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SeruStatGrant {
    /// HP_max delta per Seru-level.
    pub hp: u16,
    /// MP_max delta per Seru-level.
    pub mp: u16,
    /// Spirit-max delta (record `+0x10E`).
    pub sp: u8,
    /// Six per-stat u16 deltas, indexed in record order at `+0x11C..+0x126`.
    /// Engines treat retail "STR / VIT / AGI / DEX / INT / LUCK" (or the
    /// retail-faithful order, which is unconfirmed) as the slot semantics.
    pub stat_deltas: [u16; 6],
}

impl SeruStatGrant {
    /// All-zero grant. Equivalent to [`Default::default`] but `const`.
    pub const ZERO: Self = Self {
        hp: 0,
        mp: 0,
        sp: 0,
        stat_deltas: [0; 6],
    };

    /// Convenience constructor for the HP / MP pair (the most common shape
    /// before the wider stat capture lands).
    pub const fn hp_mp(hp: u16, mp: u16) -> Self {
        Self {
            hp,
            mp,
            sp: 0,
            stat_deltas: [0; 6],
        }
    }

    /// Element-wise sum, saturating at `u16::MAX` / `u8::MAX`.
    pub fn saturating_add(self, other: Self) -> Self {
        let mut out = Self {
            hp: self.hp.saturating_add(other.hp),
            mp: self.mp.saturating_add(other.mp),
            sp: self.sp.saturating_add(other.sp),
            stat_deltas: [0; 6],
        };
        for i in 0..6 {
            out.stat_deltas[i] = self.stat_deltas[i].saturating_add(other.stat_deltas[i]);
        }
        out
    }

    /// Project onto the legacy [`StatGain`] (HP / MP only). Engines that
    /// haven't migrated to the wider grant payload still see a useful value.
    pub const fn to_stat_gain(self) -> StatGain {
        StatGain {
            hp: self.hp,
            mp: self.mp,
        }
    }
}

/// Id-keyed roster of per-Seru grants.
///
/// The retail engine indexes the table by the Seru id from the per-character
/// `seru_roster` field on the character record. Engines build the roster
/// from the equipped-Seru list at boot / Seru-equip time and pass it to the
/// [`LevelUpTracker`] resolver.
///
/// [`LevelUpTracker`]: crate::levelup::LevelUpTracker
#[derive(Debug, Clone, Default)]
pub struct SeruStatTable {
    grants: HashMap<u16, SeruStatGrant>,
}

impl SeruStatTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, seru_id: u16, grant: SeruStatGrant) {
        self.grants.insert(seru_id, grant);
    }

    pub fn get(&self, seru_id: u16) -> Option<&SeruStatGrant> {
        self.grants.get(&seru_id)
    }

    pub fn len(&self) -> usize {
        self.grants.len()
    }

    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }

    /// Sum the grants for the given roster of Seru ids. Unknown ids are
    /// silently skipped (the retail engine treats an unequipped slot as a
    /// zero contribution).
    pub fn sum_roster(&self, roster: &[u16]) -> SeruStatGrant {
        let mut total = SeruStatGrant::ZERO;
        for &id in roster {
            if let Some(g) = self.get(id) {
                total = total.saturating_add(*g);
            }
        }
        total
    }

    /// Build a [`StatGrowthCurve::PerLevel`] vector that emits the same
    /// summed grant for every level. Engines call this when they want to
    /// install the roster's grant into a [`LevelUpTracker`] without first
    /// going through a [`crate::levelup::LevelUpObservation`].
    ///
    /// We materialise as `PerLevel` (not `Flat`) so the tracker resolver
    /// honours the value directly - the [`LevelUpTracker`]'s `Flat` arm
    /// intentionally falls back to the explicit `stat_gains` field for
    /// backward-compat with engines that haven't migrated to curves.
    ///
    /// [`LevelUpTracker`]: crate::levelup::LevelUpTracker
    pub fn to_flat_curve(&self, roster: &[u16]) -> StatGrowthCurve {
        let total = self.sum_roster(roster);
        let gain = total.to_stat_gain();
        let table: Vec<StatGain> =
            std::iter::repeat_n(gain, (crate::levelup::MAX_LEVEL - 1) as usize).collect();
        StatGrowthCurve::PerLevel(table)
    }
}

/// Vanilla early-game Seru roster - placeholder values until a runtime
/// trace pins down the retail `Seru struct +0x74` payload. The shipped
/// values roughly match the rate at which Vahn's early-game curve advances
/// HP/MP per level in the captured magic-rank-up + character-level-up
/// observation triplet.
///
/// The shipped roster is *flat* across every Seru id 0..=15 (10 HP / 5 MP
/// per Seru per level), so a 4-level jump on a roster of two Seru produces
/// the same delta the live observation captures. Engines that want
/// per-Seru divergence override individual entries via
/// [`SeruStatTable::insert`].
pub fn vanilla_seru_table() -> SeruStatTable {
    let mut t = SeruStatTable::new();
    for id in 0..=15u16 {
        t.insert(id, SeruStatGrant::hp_mp(10, 5));
    }
    t
}

/// Roster definitions used by the legacy [`LevelUpTracker`] when no
/// per-character roster has been wired through [`crate::world::World`].
///
/// Vahn / Noa / Gala start with one Seru each; their early-game roster
/// expands as the player progresses through the story. The shipped IDs
/// match the in-game default order surfaced by `crates/engine-core::seru_learning`.
pub mod default_rosters {
    /// Vahn's starting roster - one elemental Seru.
    pub const VAHN: &[u16] = &[0];
    /// Noa's starting roster - one healing-element Seru.
    pub const NOA: &[u16] = &[1];
    /// Gala's starting roster - one defensive Seru.
    pub const GALA: &[u16] = &[2];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table_returns_zero_for_any_roster() {
        let t = SeruStatTable::new();
        let g = t.sum_roster(&[0, 1, 2]);
        assert_eq!(g, SeruStatGrant::ZERO);
    }

    #[test]
    fn vanilla_table_has_shipped_breadth() {
        let t = vanilla_seru_table();
        assert_eq!(t.len(), 16);
        assert!(t.get(0).is_some());
        assert!(t.get(15).is_some());
        assert!(t.get(99).is_none());
    }

    #[test]
    fn sum_roster_sums_equipped_only() {
        let mut t = SeruStatTable::new();
        t.insert(0, SeruStatGrant::hp_mp(10, 5));
        t.insert(1, SeruStatGrant::hp_mp(20, 7));
        // Roster = 0 + 1 + (unknown 99 ignored).
        let total = t.sum_roster(&[0, 1, 99]);
        assert_eq!(total.hp, 30);
        assert_eq!(total.mp, 12);
    }

    #[test]
    fn add_saturates_on_overflow() {
        let a = SeruStatGrant {
            hp: u16::MAX - 5,
            mp: u16::MAX,
            sp: u8::MAX,
            stat_deltas: [u16::MAX; 6],
        };
        let b = SeruStatGrant {
            hp: 100,
            mp: 100,
            sp: 100,
            stat_deltas: [100; 6],
        };
        let s = a.saturating_add(b);
        assert_eq!(s.hp, u16::MAX);
        assert_eq!(s.mp, u16::MAX);
        assert_eq!(s.sp, u8::MAX);
        assert_eq!(s.stat_deltas, [u16::MAX; 6]);
    }

    #[test]
    fn to_flat_curve_emits_same_value_at_every_level() {
        let t = vanilla_seru_table();
        let curve = t.to_flat_curve(default_rosters::VAHN);
        let g1 = curve.gain_for(1);
        let g50 = curve.gain_for(50);
        assert_eq!(g1, g50);
        assert_eq!(g1.hp, 10);
    }

    #[test]
    fn to_stat_gain_projects_hp_mp() {
        let g = SeruStatGrant {
            hp: 12,
            mp: 7,
            sp: 5,
            stat_deltas: [1, 2, 3, 4, 5, 6],
        };
        let sg = g.to_stat_gain();
        assert_eq!(sg.hp, 12);
        assert_eq!(sg.mp, 7);
    }

    #[test]
    fn vanilla_roster_constants_are_short_and_distinct() {
        assert_eq!(default_rosters::VAHN.len(), 1);
        assert_eq!(default_rosters::NOA.len(), 1);
        assert_eq!(default_rosters::GALA.len(), 1);
        assert_ne!(default_rosters::VAHN[0], default_rosters::NOA[0]);
    }
}
