//! The Mystic Shield: the up-check and its per-frame break tick.

use super::*;

impl World {
    /// Whether Cort's **Mystic Shield** is up - retail's `_DAT_8007BD84`,
    /// the effect handle PROT 0940's `0xAC` arm 4 stores (`0x801F7678`) and
    /// the engine mirrors as [`crate::monster_ai::MonsterAiState::flag_bd84`].
    /// While it is non-null the damage finisher halves every hit on an enemy
    /// defender (`FUN_801DDB30`, `0x801DDB98..0x801DDBC8`).
    pub(in crate::world) fn mystic_shield_up(&self) -> bool {
        self.battle.monster_ai_state.flag_bd84 != 0
    }

    /// Break Cort's Mystic Shield once he is worn to half HP - the arm of
    /// the per-frame actor pass `FUN_8004CE2C` at `0x8004D534..0x8004D668`.
    ///
    /// Retail's gates, in order: the first formation id `gp[+0x9F4]`
    /// (`0x8007BD0C`) is `0xB4`, the shield word is non-null, and the first
    /// monster seat's HP `+0x14C` has fallen to `+0x14E >> 1` or below
    /// (`srl` / `sltu` at `0x8004D5CC..0x8004D5D4`). Then it restores the
    /// seat's reaction run the shield's arm 4 blanked (`+0x1EF = 3`,
    /// `+0x1F0 = 2`, `+0x1F1 = 4`, `+0x1F2 = 5`), retires the shield effect
    /// (`+0x10 |= 8`) and clears the word (`sw zero` at `0x8004D658`) - so
    /// the finisher's halve lifts and the `0xB4` pick's Evil Seru Magic arm,
    /// gated on the word being null, opens.
    ///
    /// Not ported: the shield effect's own pose and colour writes
    /// (`+0x56` / `+0x72` / `+0x14..+0x1B`, presentation of an effect the
    /// engine does not spawn) and the break's cue `0x10D`.
    ///
    /// PORT: FUN_8004CE2C (the Mystic Shield break arm, `0x8004D534..0x8004D668`)
    pub(in crate::world) fn tick_mystic_shield_break(&mut self) {
        const CORT_FIRST_FORM: u16 = 0xB4;
        if !self.mystic_shield_up() {
            return;
        }
        let seat = self.party.party_count as usize;
        let Some(a) = self.actors.get_mut(seat) else {
            return;
        };
        if a.battle_monster_id != Some(CORT_FIRST_FORM) {
            return;
        }
        if a.battle.hp > a.battle.max_hp >> 1 {
            return;
        }
        for (off, clip) in [(0x1EF, 3u8), (0x1F0, 2), (0x1F1, 4), (0x1F2, 5)] {
            if let Some(p) = a.battle.params.get_mut(off - 0x1DF) {
                *p = clip;
            }
        }
        self.battle.monster_ai_state.flag_bd84 = 0;
    }
}
